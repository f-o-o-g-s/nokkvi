//! Instant mix: songs LIKE a few seeds, never the seeds themselves.
//!
//! Each seed walks a fallback ladder ([`seed_ladder`]) toward a per-seed
//! target ([`per_seed_target`]); every rung appends only songs the seed's
//! list doesn't hold yet, after the fixed filters
//! ([`filter_instant_mix`]: no known duration under 60 s, no 1-star song):
//!
//! 1. **Server mix** — `getSimilarSongs2(seed id)`. Since Navidrome 0.64 the
//!    id may be a song, album, artist, playlist or genre (tag) id and the
//!    server tops a short answer up from sampled seed tracks; an older server
//!    answers an error for a playlist or genre id.
//! 2. **Client seed mix** — only when rung 1 answered nothing (failed or
//!    empty) and the seed is not a song: what 0.64 does server-side, for
//!    older servers. Sample [`INSTANT_MIX_SAMPLED_TRACKS`] of the seed's own
//!    tracks, ask `getSimilarSongs2` about each, merge one per track around.
//! 3. **Same genre** — only while the list is under half the target:
//!    `getRandomSongs(genre NAME)`. The genre is the song seed's own, the
//!    genre seed's name, else the most common one among the seed's tracks.
//! 4. **The seed's own ground** — still under half: other songs by a song
//!    seed's artist, or the seed's own tracks, shuffled.
//!
//! Harbour sends song seeds only today; the ladder stays general over
//! [`BatchItem`] so an album, artist, genre or playlist seed works the same.
//! A failed request is an empty rung, except a rejected session
//! (`NokkviError::is_unauthorized`), which ends the whole mix with that
//! error. The network sits behind [`MixSource`], so the tests run the whole
//! ladder against an in-memory fake.

use std::{collections::HashSet, future::Future};

use anyhow::Result;
use futures::{StreamExt, TryStreamExt, stream};
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};

use crate::{
    backend::library_orchestrator::LibraryOrchestrator,
    services::api::{random::RandomApiService, similar::SimilarApiService},
    types::{
        batch::BatchItem,
        error::NokkviError,
        instant_mix::{
            INSTANT_MIX_CONCURRENT_SEEDS, INSTANT_MIX_MAX_REQUEST, INSTANT_MIX_SAMPLED_TRACKS,
            INSTANT_MIX_TRACKS, dominant_genre, filter_instant_mix, per_seed_target, request_count,
            top_up,
        },
        song::Song,
        trawl::{TrawlBlend, batch_item_key, blend_trawl, sample_cap},
    },
};

// A request never asks for more than `getRandomSongs` serves.
const _: () = assert!(INSTANT_MIX_MAX_REQUEST <= crate::services::api::random::MAX_RANDOM_SONGS);

/// Error for a mix asked for with no seeds.
pub const NO_SEEDS_ERROR: &str = "Nothing to build a mix from";

/// Error for a mix whose requests found no song that passed the filters.
pub const NOTHING_SIMILAR_ERROR: &str = "No similar songs found";

/// Error for a mix whose every song was already queued (Add to Queue).
pub const ALL_QUEUED_ERROR: &str = "Every song in the mix is already in the queue";

/// Where the ladder's songs come from. The server implementation is
/// [`ServerMixSource`]; tests use an in-memory fake.
pub(crate) trait MixSource: Sync {
    /// `getSimilarSongs2(id, count)`.
    fn similar(&self, id: &str, count: usize) -> impl Future<Output = Result<Vec<Song>>> + Send;

    /// `getRandomSongs(count, genre)`, `genre` being the genre NAME.
    fn random_in_genre(
        &self,
        genre: &str,
        count: usize,
    ) -> impl Future<Output = Result<Vec<Song>>> + Send;

    /// A seed's own tracks (the song itself, or the album, artist, genre or
    /// playlist).
    fn own_tracks(&self, seed: &BatchItem) -> impl Future<Output = Result<Vec<Song>>> + Send;

    /// Every song by `artist_id`.
    fn artist_tracks(&self, artist_id: &str) -> impl Future<Output = Result<Vec<Song>>> + Send;
}

/// [`MixSource`] over the live server: Subsonic `getSimilarSongs2` /
/// `getRandomSongs`, and the orchestrator's per-entity `resolve_*` for a
/// seed's own tracks.
pub(crate) struct ServerMixSource<'o, 'a> {
    pub(crate) orch: &'o LibraryOrchestrator<'a>,
    /// The active libraries, scoping the same-genre rung like Harbour's
    /// genre draw (empty = every library the user can read).
    pub(crate) library_ids: &'o [i32],
}

impl MixSource for ServerMixSource<'_, '_> {
    async fn similar(&self, id: &str, count: usize) -> Result<Vec<Song>> {
        let api = self
            .orch
            .auth()
            .build_subsonic_api(SimilarApiService::new)
            .await?;
        api.get_similar_songs(id, u32::try_from(count).unwrap_or(u32::MAX))
            .await
    }

    async fn random_in_genre(&self, genre: &str, count: usize) -> Result<Vec<Song>> {
        let api = self
            .orch
            .auth()
            .build_subsonic_api(RandomApiService::new)
            .await?;
        api.get_random_songs(count, Some(genre), self.library_ids)
            .await
    }

    async fn own_tracks(&self, seed: &BatchItem) -> Result<Vec<Song>> {
        match seed {
            BatchItem::Song(song) => Ok(vec![(**song).clone()]),
            BatchItem::Album(id) => self.orch.resolve_album(id).await,
            BatchItem::Artist(id) => self.orch.resolve_artist(id).await,
            BatchItem::Genre(genre) => self.orch.resolve_genre(genre).await,
            BatchItem::Playlist(id) => self.orch.resolve_playlist(id).await,
        }
    }

    async fn artist_tracks(&self, artist_id: &str) -> Result<Vec<Song>> {
        self.orch.resolve_artist(artist_id).await
    }
}

/// What a rung's songs must get past before they count.
struct MixGate<'a> {
    /// The song seeds' ids: the mix never holds a seed.
    seed_ids: HashSet<&'a str>,
    /// Ids the caller leaves out (the queue, for Add to Queue).
    exclude: &'a HashSet<String>,
}

/// One seed's ladder result, plus the tallies the empty-mix error reads.
#[derive(Debug, Default)]
struct SeedOutcome {
    songs: Vec<Song>,
    /// Songs the requests returned that were not seeds and passed the
    /// filters (whether or not `exclude` then dropped them).
    passed: usize,
    /// Requests that failed and read as empty rungs.
    failed_requests: usize,
}

impl SeedOutcome {
    /// A request's songs, or an empty rung when it failed. A rejected
    /// session is the one failure that ends the mix.
    fn answer(&mut self, result: Result<Vec<Song>>) -> Result<Vec<Song>> {
        match result {
            Ok(songs) => Ok(songs),
            Err(e) if NokkviError::is_unauthorized(&e) => Err(e),
            Err(e) => {
                self.failed_requests += 1;
                tracing::debug!("Instant mix request failed, reading it as empty: {e:#}");
                Ok(Vec::new())
            }
        }
    }

    /// Put a rung's songs through the gate (no seeds, the fixed filters, no
    /// excluded ids), then top the list up to `target` with what is left.
    fn offer(&mut self, gate: &MixGate<'_>, mut songs: Vec<Song>, target: usize) {
        songs.retain(|s| !gate.seed_ids.contains(s.id.as_str()));
        let mut kept = filter_instant_mix(songs);
        self.passed += kept.len();
        if !gate.exclude.is_empty() {
            kept.retain(|s| !gate.exclude.contains(&s.id));
        }
        top_up(&mut self.songs, kept, target);
    }
}

/// Whether a list still sits under half its target (Tempus's top-up rule).
fn under_half(len: usize, target: usize) -> bool {
    len.saturating_mul(2) < target
}

/// A seed's own tracks, fetched at most once per seed: the first call asks
/// the source (a failure caches as empty), later ones read the cache.
async fn own_tracks_once<'c, S: MixSource>(
    src: &S,
    seed: &BatchItem,
    cache: &'c mut Option<Vec<Song>>,
    outcome: &mut SeedOutcome,
) -> Result<&'c [Song]> {
    if cache.is_none() {
        let tracks = outcome.answer(src.own_tracks(seed).await)?;
        *cache = Some(tracks);
    }
    Ok(cache.as_deref().unwrap_or_default())
}

/// Walk one seed's ladder (module docs) toward `target` songs, asking each
/// request for `request` songs.
async fn seed_ladder<S: MixSource>(
    src: &S,
    seed: &BatchItem,
    gate: &MixGate<'_>,
    target: usize,
    request: usize,
    mut rng: StdRng,
) -> Result<SeedOutcome> {
    let mut outcome = SeedOutcome::default();
    let mut own: Option<Vec<Song>> = None;

    // Rung 1: the server's mix for the seed id (a genre's is its tag id).
    let answer = outcome.answer(src.similar(batch_item_key(seed).1, request).await)?;
    let server_answered = !answer.is_empty();
    outcome.offer(gate, answer, target);

    // Rung 2: the client-side seed mix, for servers before 0.64.
    let is_song = matches!(seed, BatchItem::Song(_));
    if !server_answered && !is_song {
        let mut sampled = own_tracks_once(src, seed, &mut own, &mut outcome)
            .await?
            .to_vec();
        sample_cap(&mut sampled, INSTANT_MIX_SAMPLED_TRACKS, &mut rng);
        let answers =
            futures::future::join_all(sampled.iter().map(|track| src.similar(&track.id, request)))
                .await;
        let mut per_track = Vec::with_capacity(answers.len());
        for answer in answers {
            per_track.push((outcome.answer(answer)?, 1));
        }
        let merged = blend_trawl(per_track, TrawlBlend::Interleave, &mut rng);
        outcome.offer(gate, merged, target);
    }

    // Rung 3: random songs from the seed's genre.
    if under_half(outcome.songs.len(), target) {
        let genre: Option<String> = match seed {
            BatchItem::Song(song) => song.genre.clone().filter(|g| !g.trim().is_empty()),
            BatchItem::Genre(genre) => Some(genre.name.clone()),
            BatchItem::Album(_) | BatchItem::Artist(_) | BatchItem::Playlist(_) => {
                dominant_genre(own_tracks_once(src, seed, &mut own, &mut outcome).await?)
                    .map(str::to_owned)
            }
        };
        if let Some(genre) = genre {
            let answer = outcome.answer(src.random_in_genre(&genre, request).await)?;
            outcome.offer(gate, answer, target);
        }
    }

    // Rung 4: the seed's own ground.
    if under_half(outcome.songs.len(), target) {
        let mut ground = match seed {
            BatchItem::Song(song) => match song.artist_id.as_deref().filter(|id| !id.is_empty()) {
                Some(artist_id) => outcome.answer(src.artist_tracks(artist_id).await)?,
                None => Vec::new(),
            },
            BatchItem::Album(_)
            | BatchItem::Artist(_)
            | BatchItem::Genre(_)
            | BatchItem::Playlist(_) => own_tracks_once(src, seed, &mut own, &mut outcome)
                .await?
                .to_vec(),
        };
        ground.shuffle(&mut rng);
        outcome.offer(gate, ground, target);
    }

    Ok(outcome)
}

/// Resolve an instant mix for `seeds`: every seed's ladder, at most
/// [`INSTANT_MIX_CONCURRENT_SEEDS`] at once, lists kept in seed order, then
/// merged one per seed around (take-next-fresh dedupe, like Trawl's
/// Interleave) and cut to [`INSTANT_MIX_TRACKS`]. The mix never holds a song
/// seed, nor an id in `exclude`. `rng` drives every sample and shuffle, so
/// a seeded one makes the mix deterministic.
pub(crate) async fn resolve_instant_mix<S: MixSource>(
    src: &S,
    seeds: &[BatchItem],
    exclude: &HashSet<String>,
    mut rng: StdRng,
) -> Result<Vec<Song>> {
    if seeds.is_empty() {
        return Err(anyhow::anyhow!(NO_SEEDS_ERROR));
    }
    let target = per_seed_target(seeds.len());
    let request = request_count(target);
    let gate = MixGate {
        seed_ids: seeds
            .iter()
            .filter_map(|seed| match seed {
                BatchItem::Song(song) => Some(song.id.as_str()),
                BatchItem::Album(_)
                | BatchItem::Artist(_)
                | BatchItem::Genre(_)
                | BatchItem::Playlist(_) => None,
            })
            .collect(),
        exclude,
    };
    let seed_rngs: Vec<StdRng> = seeds.iter().map(|_| rng.fork()).collect();

    // The ladders are built up front rather than in a `stream.map(closure)`:
    // a closure over borrowed seeds inside the stream type trips rustc's
    // higher-ranked `Send` check (rust-lang/rust#102211) once this future
    // reaches `shell_task`. Futures are lazy, so nothing runs until polled.
    let ladders: Vec<_> = seeds
        .iter()
        .zip(seed_rngs)
        .map(|(seed, seed_rng)| seed_ladder(src, seed, &gate, target, request, seed_rng))
        .collect();
    let outcomes: Vec<SeedOutcome> = stream::iter(ladders)
        .buffered(INSTANT_MIX_CONCURRENT_SEEDS)
        .try_collect()
        .await?;

    let empty_seeds = outcomes.iter().filter(|o| o.songs.is_empty()).count();
    let failed_requests: usize = outcomes.iter().map(|o| o.failed_requests).sum();
    if empty_seeds > 0 || failed_requests > 0 {
        tracing::warn!(
            "Instant mix: {empty_seeds} of {} seeds found no songs, {failed_requests} requests failed",
            seeds.len()
        );
    }
    let passed: usize = outcomes.iter().map(|o| o.passed).sum();

    let lists = outcomes.into_iter().map(|o| (o.songs, 1)).collect();
    let mut blended = blend_trawl(lists, TrawlBlend::Interleave, &mut rng);
    blended.truncate(INSTANT_MIX_TRACKS);
    if !blended.is_empty() {
        Ok(blended)
    } else if passed == 0 {
        Err(anyhow::anyhow!(NOTHING_SIMILAR_ERROR))
    } else {
        Err(anyhow::anyhow!(ALL_QUEUED_ERROR))
    }
}

/// A fresh, unpredictable rng for a production mix.
pub(crate) fn mix_rng() -> StdRng {
    StdRng::from_rng(&mut rand::rng())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use parking_lot::Mutex;

    use super::*;
    use crate::types::genre::GenreRef;

    /// How the fake answers one request.
    #[derive(Clone)]
    enum Answer {
        Songs(Vec<Song>),
        /// A plain failure (a pre-0.64 server's error, a timeout).
        Fail,
        /// A rejected session.
        Reject,
    }

    impl Answer {
        fn result(&self) -> Result<Vec<Song>> {
            match self {
                Answer::Songs(songs) => Ok(songs.clone()),
                Answer::Fail => Err(anyhow::anyhow!("server error")),
                Answer::Reject => Err(NokkviError::Unauthorized.into()),
            }
        }
    }

    /// In-memory [`MixSource`]: scripted answers by id (a missing id answers
    /// no songs), every call recorded as `"<kind>:<id>:<count>"`.
    #[derive(Default)]
    struct Fake {
        similar: HashMap<String, Answer>,
        random: HashMap<String, Answer>,
        own: HashMap<String, Answer>,
        artist: HashMap<String, Answer>,
        /// Per-id delay on `similar`, to make seeds finish out of order.
        delay: HashMap<String, u64>,
        calls: Mutex<Vec<String>>,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
    }

    impl Fake {
        fn answer(map: &HashMap<String, Answer>, id: &str) -> Result<Vec<Song>> {
            map.get(id).map_or_else(|| Ok(Vec::new()), Answer::result)
        }

        fn record(&self, call: String) {
            self.calls.lock().push(call);
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().clone()
        }

        fn count(&self, prefix: &str) -> usize {
            self.calls()
                .iter()
                .filter(|c| c.starts_with(prefix))
                .count()
        }
    }

    impl MixSource for Fake {
        fn similar(
            &self,
            id: &str,
            count: usize,
        ) -> impl Future<Output = Result<Vec<Song>>> + Send {
            self.record(format!("similar:{id}:{count}"));
            let result = Self::answer(&self.similar, id);
            let delay = self.delay.get(id).copied().unwrap_or(1);
            async move {
                let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_in_flight.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                result
            }
        }

        fn random_in_genre(
            &self,
            genre: &str,
            count: usize,
        ) -> impl Future<Output = Result<Vec<Song>>> + Send {
            self.record(format!("random:{genre}:{count}"));
            std::future::ready(Self::answer(&self.random, genre))
        }

        fn own_tracks(&self, seed: &BatchItem) -> impl Future<Output = Result<Vec<Song>>> + Send {
            let id = batch_item_key(seed).1;
            self.record(format!("own:{id}"));
            std::future::ready(Self::answer(&self.own, id))
        }

        fn artist_tracks(&self, artist_id: &str) -> impl Future<Output = Result<Vec<Song>>> + Send {
            self.record(format!("artist:{artist_id}"));
            std::future::ready(Self::answer(&self.artist, artist_id))
        }
    }

    fn song(id: &str) -> Song {
        let mut s = Song::test_default(id, &format!("Song {id}"));
        s.duration = 180;
        s
    }

    /// `n` songs `<prefix>0..<prefix>n`.
    fn run(prefix: &str, n: usize) -> Vec<Song> {
        (0..n).map(|i| song(&format!("{prefix}{i}"))).collect()
    }

    fn seed_song(id: &str, genre: Option<&str>, artist_id: Option<&str>) -> BatchItem {
        let mut s = song(id);
        s.genre = genre.map(str::to_owned);
        s.artist_id = artist_id.map(str::to_owned);
        BatchItem::Song(Box::new(s))
    }

    fn ids(list: &[Song]) -> Vec<String> {
        list.iter().map(|s| s.id.clone()).collect()
    }

    fn sorted_ids(list: &[Song]) -> Vec<String> {
        let mut v = ids(list);
        v.sort_unstable();
        v
    }

    async fn mix(fake: &Fake, seeds: &[BatchItem]) -> Result<Vec<Song>> {
        resolve_instant_mix(fake, seeds, &HashSet::new(), StdRng::seed_from_u64(7)).await
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_server_answer_skips_every_fallback_and_drops_the_seed() {
        let mut answer = vec![song("seed")];
        answer.extend(run("s", 60));
        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Songs(answer))]),
            ..Fake::default()
        };
        let out = mix(&fake, &[seed_song("seed", Some("Rock"), Some("ar1"))])
            .await
            .expect("mix");
        assert_eq!(out.len(), INSTANT_MIX_TRACKS);
        assert!(!ids(&out).contains(&"seed".to_owned()), "never the seed");
        assert_eq!(ids(&out)[0], "s0", "server order kept");
        assert_eq!(
            fake.calls(),
            vec!["similar:seed:100"],
            "one seed: target 50, asked for twice it"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_short_answer_tops_up_from_the_genre_then_the_artist() {
        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Songs(run("s", 10)))]),
            random: HashMap::from([("Rock".into(), Answer::Songs(run("g", 5)))]),
            artist: HashMap::from([("ar1".into(), Answer::Songs(run("a", 100)))]),
            ..Fake::default()
        };
        let out = mix(&fake, &[seed_song("seed", Some("Rock"), Some("ar1"))])
            .await
            .expect("mix");
        assert_eq!(
            fake.calls(),
            vec!["similar:seed:100", "random:Rock:100", "artist:ar1"]
        );
        assert_eq!(out.len(), 50, "the last rung fills up to the target");
        assert_eq!(
            ids(&out[..15]),
            [ids(&run("s", 10)), ids(&run("g", 5))].concat()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_fallbacks_stop_once_the_list_reaches_half_the_target() {
        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Songs(run("s", 10)))]),
            random: HashMap::from([("Rock".into(), Answer::Songs(run("g", 15)))]),
            artist: HashMap::from([("ar1".into(), Answer::Songs(run("a", 100)))]),
            ..Fake::default()
        };
        let out = mix(&fake, &[seed_song("seed", Some("Rock"), Some("ar1"))])
            .await
            .expect("mix");
        assert_eq!(out.len(), 25, "25 of 50 is no longer under half");
        assert_eq!(fake.count("artist:"), 0, "rung 4 never ran");

        // At half already after rung 1: neither fallback runs.
        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Songs(run("s", 25)))]),
            random: HashMap::from([("Rock".into(), Answer::Songs(run("g", 15)))]),
            ..Fake::default()
        };
        mix(&fake, &[seed_song("seed", Some("Rock"), None)])
            .await
            .expect("mix");
        assert_eq!(fake.calls(), vec!["similar:seed:100"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_song_seed_never_takes_the_client_seed_mix() {
        let fake = Fake {
            artist: HashMap::from([("ar1".into(), Answer::Songs(run("a", 3)))]),
            ..Fake::default()
        };
        let out = mix(&fake, &[seed_song("seed", None, Some("ar1"))])
            .await
            .expect("artist songs still make a mix");
        assert_eq!(sorted_ids(&out), ids(&run("a", 3)), "shuffled, all there");
        assert_eq!(
            fake.calls(),
            vec!["similar:seed:100", "artist:ar1"],
            "no own-track sampling for a song, no genre to draw from"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_album_with_no_server_mix_asks_about_its_own_tracks_once() {
        let mut tracks = run("t", 8);
        for t in &mut tracks {
            t.genre = Some("Jazz".into());
        }
        let mut similar = HashMap::from([("al1".into(), Answer::Fail)]);
        for (i, t) in tracks.iter().enumerate() {
            similar.insert(t.id.clone(), Answer::Songs(run(&format!("x{i}-"), 2)));
        }
        let fake = Fake {
            similar,
            own: HashMap::from([("al1".into(), Answer::Songs(tracks.clone()))]),
            random: HashMap::from([("Jazz".into(), Answer::Songs(run("g", 3)))]),
            ..Fake::default()
        };
        let out = mix(&fake, &[BatchItem::Album("al1".into())])
            .await
            .expect("mix");

        assert_eq!(fake.count("own:"), 1, "own tracks fetched at most once");
        assert_eq!(
            fake.count("similar:t"),
            INSTANT_MIX_SAMPLED_TRACKS,
            "five sampled tracks asked about"
        );
        assert!(
            fake.calls().contains(&"random:Jazz:100".to_owned()),
            "the dominant genre of its tracks: {:?}",
            fake.calls()
        );
        // 5 tracks x 2 similar = 10, + 3 genre = 13 < 25, so rung 4 adds the
        // album's own tracks.
        assert_eq!(out.len(), 13 + tracks.len());
        let first_five: HashSet<&str> = out[..5].iter().map(|s| &s.id[..2]).collect();
        assert_eq!(first_five.len(), 5, "rung 2 merges one per track around");
    }

    #[tokio::test(start_paused = true)]
    async fn an_answered_server_mix_skips_the_client_seed_mix() {
        let mut short = song("short");
        short.duration = 30;
        let fake = Fake {
            similar: HashMap::from([("al1".into(), Answer::Songs(vec![short]))]),
            own: HashMap::from([("al1".into(), Answer::Songs(run("t", 3)))]),
            ..Fake::default()
        };
        let out = mix(&fake, &[BatchItem::Album("al1".into())])
            .await
            .expect("mix");
        assert_eq!(
            fake.count("similar:t"),
            0,
            "the server answered, even if the filters then dropped it all"
        );
        assert_eq!(fake.count("own:"), 1);
        assert_eq!(ids(&out).len(), 3, "rung 4: the album's own tracks");
    }

    #[tokio::test(start_paused = true)]
    async fn a_genre_seed_asks_by_tag_id_and_draws_by_name() {
        let fake = Fake {
            random: HashMap::from([("Rock".into(), Answer::Songs(run("g", 30)))]),
            ..Fake::default()
        };
        let seed = BatchItem::Genre(GenreRef::new("tag-rock", "Rock"));
        mix(&fake, &[seed]).await.expect("mix");
        let calls = fake.calls();
        assert_eq!(calls[0], "similar:tag-rock:100");
        assert!(calls.contains(&"random:Rock:100".to_owned()), "{calls:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn every_rung_runs_the_fixed_filters() {
        let short = |id: &str| {
            let mut s = song(id);
            s.duration = 59;
            s
        };
        let one_star = |id: &str| {
            let mut s = song(id);
            s.rating = Some(1);
            s
        };
        let mut unknown = song("unknown");
        unknown.duration = 0;
        let fake = Fake {
            similar: HashMap::from([(
                "seed".into(),
                Answer::Songs(vec![short("s-short"), one_star("s-one"), song("s-ok")]),
            )]),
            random: HashMap::from([(
                "Rock".into(),
                Answer::Songs(vec![short("g-short"), one_star("g-one"), unknown]),
            )]),
            artist: HashMap::from([(
                "ar1".into(),
                Answer::Songs(vec![short("a-short"), one_star("a-one"), song("a-ok")]),
            )]),
            ..Fake::default()
        };
        let out = mix(&fake, &[seed_song("seed", Some("Rock"), Some("ar1"))])
            .await
            .expect("mix");
        let mut got = ids(&out);
        got.sort_unstable();
        assert_eq!(got, vec!["a-ok", "s-ok", "unknown"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_rung_reads_as_empty() {
        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Fail)]),
            random: HashMap::from([("Rock".into(), Answer::Fail)]),
            artist: HashMap::from([("ar1".into(), Answer::Songs(run("a", 4)))]),
            ..Fake::default()
        };
        let out = mix(&fake, &[seed_song("seed", Some("Rock"), Some("ar1"))])
            .await
            .expect("the last rung still answers");
        assert_eq!(sorted_ids(&out), ids(&run("a", 4)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejected_session_ends_the_whole_mix() {
        let fake = Fake {
            similar: HashMap::from([("one".into(), Answer::Songs(run("s", 50)))]),
            random: HashMap::from([("Rock".into(), Answer::Reject)]),
            ..Fake::default()
        };
        let err = mix(
            &fake,
            &[
                seed_song("one", None, None),
                seed_song("two", Some("Rock"), None),
            ],
        )
        .await
        .expect_err("a rejected session must end the mix");
        assert!(NokkviError::is_unauthorized(&err), "got: {err:#}");
    }

    #[tokio::test(start_paused = true)]
    async fn the_mix_never_holds_any_seed() {
        let fake = Fake {
            similar: HashMap::from([
                ("a".into(), Answer::Songs(vec![song("b"), song("x1")])),
                ("b".into(), Answer::Songs(vec![song("a"), song("y1")])),
            ]),
            ..Fake::default()
        };
        let out = mix(
            &fake,
            &[seed_song("a", None, None), seed_song("b", None, None)],
        )
        .await
        .expect("mix");
        assert_eq!(ids(&out), vec!["x1", "y1"]);
    }

    #[tokio::test(start_paused = true)]
    async fn excluded_songs_are_left_out_and_the_ladder_tops_up_past_them() {
        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Songs(run("s", 30)))]),
            random: HashMap::from([("Rock".into(), Answer::Songs(run("g", 40)))]),
            ..Fake::default()
        };
        let queued: HashSet<String> = (0..20).map(|i| format!("s{i}")).collect();
        let out = resolve_instant_mix(
            &fake,
            &[seed_song("seed", Some("Rock"), None)],
            &queued,
            StdRng::seed_from_u64(1),
        )
        .await
        .expect("mix");
        assert!(out.iter().all(|s| !queued.contains(&s.id)));
        assert_eq!(fake.count("random:"), 1, "10 fresh songs is under half");
        assert_eq!(out.len(), 50);
    }

    #[tokio::test(start_paused = true)]
    async fn an_empty_mix_names_its_cause() {
        let err = mix(&Fake::default(), &[]).await.expect_err("no seeds");
        assert_eq!(err.to_string(), NO_SEEDS_ERROR);

        let err = mix(
            &Fake::default(),
            &[seed_song("seed", Some("Rock"), Some("ar1"))],
        )
        .await
        .expect_err("nothing found");
        assert_eq!(err.to_string(), NOTHING_SIMILAR_ERROR);

        let mut short = song("short");
        short.duration = 10;
        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Songs(vec![short, song("seed")]))]),
            ..Fake::default()
        };
        let err = mix(&fake, &[seed_song("seed", None, None)])
            .await
            .expect_err("filtered away");
        assert_eq!(
            err.to_string(),
            NOTHING_SIMILAR_ERROR,
            "the seed is no find"
        );

        let fake = Fake {
            similar: HashMap::from([("seed".into(), Answer::Songs(run("s", 3)))]),
            ..Fake::default()
        };
        let queued: HashSet<String> = ids(&run("s", 3)).into_iter().collect();
        let err = resolve_instant_mix(
            &fake,
            &[seed_song("seed", None, None)],
            &queued,
            StdRng::seed_from_u64(0),
        )
        .await
        .expect_err("all queued");
        assert_eq!(err.to_string(), ALL_QUEUED_ERROR);
    }

    #[tokio::test(start_paused = true)]
    async fn seeds_merge_one_each_around_in_seed_order_and_cut_to_the_mix_size() {
        // Seed 0 answers last; its songs still lead.
        let fake = Fake {
            similar: HashMap::from([
                ("p".into(), Answer::Songs(run("p", 50))),
                ("q".into(), Answer::Songs(run("q", 50))),
            ]),
            delay: HashMap::from([("p".into(), 500), ("q".into(), 1)]),
            ..Fake::default()
        };
        let out = mix(
            &fake,
            &[seed_song("p", None, None), seed_song("q", None, None)],
        )
        .await
        .expect("mix");
        assert_eq!(out.len(), INSTANT_MIX_TRACKS);
        assert_eq!(ids(&out[..4]), vec!["p0", "q0", "p1", "q1"]);
    }

    #[tokio::test(start_paused = true)]
    async fn at_most_four_seeds_resolve_at_once() {
        let seeds: Vec<BatchItem> = (0..9)
            .map(|i| seed_song(&format!("seed{i}"), None, None))
            .collect();
        let similar = (0..9)
            .map(|i| {
                (
                    format!("seed{i}"),
                    Answer::Songs(run(&format!("m{i}-"), 20)),
                )
            })
            .collect();
        let fake = Fake {
            similar,
            ..Fake::default()
        };
        let out = mix(&fake, &seeds).await.expect("mix");
        assert_eq!(
            fake.max_in_flight.load(Ordering::SeqCst),
            INSTANT_MIX_CONCURRENT_SEEDS
        );
        assert!(
            fake.calls().iter().all(|c| c.ends_with(":40")),
            "nine seeds: target 20, asked for 40"
        );
        assert_eq!(out.len(), INSTANT_MIX_TRACKS);
        let heads: Vec<String> = out[..9].iter().map(|s| s.id.clone()).collect();
        let expected: Vec<String> = (0..9).map(|i| format!("m{i}-0")).collect();
        assert_eq!(heads, expected, "results in seed order");
    }
}
