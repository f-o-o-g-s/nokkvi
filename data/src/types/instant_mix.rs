//! Pure helpers behind Instant mix: a mix of songs LIKE a few seed songs,
//! never the seeds themselves. Harbour picks the seeds from the pools it
//! already holds ([`pick_instant_mix_seeds`]); the per-seed fallback ladder
//! that finds the songs lives in `backend::instant_mix`. Everything here is
//! pure and rng-injectable so tests are deterministic.

use std::collections::HashSet;

use rand::seq::SliceRandom;

use crate::types::song::Song;

/// How many seed songs Harbour hands over for one mix.
pub const INSTANT_MIX_SEEDS: usize = 5;

/// Most songs one mix holds: the per-seed lists merge one per seed around,
/// then the merged list is cut here.
pub const INSTANT_MIX_TRACKS: usize = 50;

/// Fewest songs a seed aims for, so a five-seed mix still has depth when a
/// seed comes back short.
pub const INSTANT_MIX_MIN_SEED_TARGET: usize = 20;

/// The server is asked for this many times a seed's target: the length and
/// rating filters always run and would otherwise leave the list short.
pub const INSTANT_MIX_REQUEST_FACTOR: usize = 2;

/// Most songs one request asks for: Navidrome clamps `getSimilarSongs2`
/// (`maxSimilarSongs` in `core/external/provider_similarsongs.go`) and
/// `getRandomSongs` (`MAX_RANDOM_SONGS`) to 500.
pub const INSTANT_MIX_MAX_REQUEST: usize = 500;

/// How many of a seed's own tracks the client-side seed mix (ladder rung 2)
/// asks `getSimilarSongs2` about: Navidrome 0.64's own `maxSeeds`.
pub const INSTANT_MIX_SAMPLED_TRACKS: usize = 5;

/// How many seeds resolve at once. A similarity request can wait seconds on
/// an external agent (Last.fm, Deezer), so seeds overlap, but a mix must not
/// flood a small server.
pub const INSTANT_MIX_CONCURRENT_SEEDS: usize = 4;

/// Songs with a known duration under this many seconds stay out of the mix:
/// intros, skits and interludes polluted it. An unknown duration (0) passes,
/// as in Trawl: absence of evidence is not shortness.
pub const INSTANT_MIX_MIN_DURATION_SECS: u32 = 60;

/// Songs the user rated exactly this many stars stay out of the mix and are
/// never picked as seeds: one star is the user saying "not this".
pub const INSTANT_MIX_REJECTED_RATING: u32 = 1;

/// The song count each of `seeds` seeds aims for in a mix of `tracks`
/// songs: twice an even share of `tracks`, at least
/// [`INSTANT_MIX_MIN_SEED_TARGET`] and at most the whole mix, or the minimum
/// when the mix is smaller (an instant mix of 50 from one seed: 50, from
/// five: 20; an Autoplay batch of 10: 20). The double share, and the floor,
/// leave room for the merge's dedupe across seeds and Autoplay's artist cap.
pub fn per_seed_target(tracks: usize, seeds: usize) -> usize {
    tracks.div_ceil(seeds.max(1)).saturating_mul(2).clamp(
        INSTANT_MIX_MIN_SEED_TARGET,
        tracks.max(INSTANT_MIX_MIN_SEED_TARGET),
    )
}

/// The `count` / `size` a request sends for a seed aiming at `target`:
/// [`INSTANT_MIX_REQUEST_FACTOR`] times it, never over
/// [`INSTANT_MIX_MAX_REQUEST`].
pub fn request_count(target: usize) -> usize {
    target
        .saturating_mul(INSTANT_MIX_REQUEST_FACTOR)
        .min(INSTANT_MIX_MAX_REQUEST)
}

/// Whether a song is rated exactly [`INSTANT_MIX_REJECTED_RATING`] stars.
pub(crate) fn is_rejected(song: &Song) -> bool {
    song.rating == Some(INSTANT_MIX_REJECTED_RATING)
}

/// Whether `song` passes the mix's fixed filters: not under
/// [`INSTANT_MIX_MIN_DURATION_SECS`] (unknown duration passes) and not rated
/// exactly [`INSTANT_MIX_REJECTED_RATING`] star.
pub fn instant_mix_keeps(song: &Song) -> bool {
    (song.duration == 0 || song.duration >= INSTANT_MIX_MIN_DURATION_SECS) && !is_rejected(song)
}

/// Drop the songs [`instant_mix_keeps`] turns away, keeping order.
pub fn filter_instant_mix(mut songs: Vec<Song>) -> Vec<Song> {
    songs.retain(instant_mix_keeps);
    songs
}

/// Append the songs of `extra` that `list` doesn't hold yet, in order,
/// until `list` reaches `target`. A list already at or over the target is
/// left alone. Returns how many songs were added.
pub fn top_up(list: &mut Vec<Song>, extra: Vec<Song>, target: usize) -> usize {
    let mut seen: HashSet<String> = list.iter().map(|s| s.id.clone()).collect();
    let before = list.len();
    for song in extra {
        if list.len() >= target {
            break;
        }
        if seen.insert(song.id.clone()) {
            list.push(song);
        }
    }
    list.len() - before
}

/// The genre most of `songs` carry (blank genres don't count). A tie goes to
/// the genre that appears first, so the answer is stable for a given list.
pub fn dominant_genre(songs: &[Song]) -> Option<&str> {
    // (genre, count) in first-appearance order; seed lists are small.
    let mut tally: Vec<(&str, usize)> = Vec::new();
    for genre in songs
        .iter()
        .filter_map(|s| s.genre.as_deref())
        // Kept verbatim: `getRandomSongs?genre=` wants the server's own
        // spelling (gotchas/server-api.md "Genre identity").
        .filter(|g| !g.trim().is_empty())
    {
        match tally.iter_mut().find(|(name, _)| *name == genre) {
            Some((_, count)) => *count += 1,
            None => tally.push((genre, 1)),
        }
    }
    let mut best: Option<(&str, usize)> = None;
    for (genre, count) in tally {
        if best.is_none_or(|(_, top)| count > top) {
            best = Some((genre, count));
        }
    }
    best.map(|(genre, _)| genre)
}

/// The artist identity seeds spread over (and Autoplay's per-artist cap
/// counts by): the artist id, else the name.
pub(crate) fn artist_key(song: &Song) -> &str {
    song.artist_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .unwrap_or(&song.artist)
}

/// Up to `count` seed songs drawn at random from `pool`: one per artist
/// while the pool has enough artists, then (only when it has fewer artists
/// than `count`) more songs by the artists already picked. Never a song
/// rated exactly [`INSTANT_MIX_REJECTED_RATING`] star, never the same song
/// twice. An empty pool gives no seeds.
pub fn pick_instant_mix_seeds<R: rand::Rng + ?Sized>(
    pool: &[Song],
    count: usize,
    rng: &mut R,
) -> Vec<Song> {
    let mut ids = HashSet::new();
    let mut candidates: Vec<&Song> = pool
        .iter()
        .filter(|s| !is_rejected(s) && ids.insert(s.id.as_str()))
        .collect();
    candidates.shuffle(rng);

    let mut artists: HashSet<&str> = HashSet::new();
    let (mut picked, rest): (Vec<&Song>, Vec<&Song>) = candidates
        .into_iter()
        .partition(|s| artists.insert(artist_key(s)));
    picked.truncate(count);
    let missing = count.saturating_sub(picked.len());
    picked.extend(rest.into_iter().take(missing));
    picked.into_iter().cloned().collect()
}

/// [`pick_instant_mix_seeds`] drawn with a fresh thread rng, for a caller
/// with no rng of its own (the UI crate does not link `rand`).
pub fn draw_instant_mix_seeds(pool: &[Song], count: usize) -> Vec<Song> {
    pick_instant_mix_seeds(pool, count, &mut rand::rng())
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    fn song(id: &str) -> Song {
        let mut s = Song::test_default(id, &format!("Song {id}"));
        s.duration = 180;
        s
    }

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter().map(|id| song(id)).collect()
    }

    fn ids(list: &[Song]) -> Vec<&str> {
        list.iter().map(|s| s.id.as_str()).collect()
    }

    fn with_genre(id: &str, genre: Option<&str>) -> Song {
        let mut s = song(id);
        s.genre = genre.map(str::to_owned);
        s
    }

    fn by(id: &str, artist: &str) -> Song {
        let mut s = song(id);
        s.artist = artist.to_owned();
        s.artist_id = Some(format!("ar-{artist}"));
        s
    }

    #[test]
    fn per_seed_target_doubles_an_even_share_within_bounds() {
        let t = INSTANT_MIX_TRACKS;
        assert_eq!(per_seed_target(t, 1), 50, "one seed fills the whole mix");
        assert_eq!(per_seed_target(t, 2), 50);
        assert_eq!(per_seed_target(t, 3), 34, "2 * ceil(50 / 3)");
        assert_eq!(per_seed_target(t, 5), 20);
        assert_eq!(per_seed_target(t, 9), 20, "never under the minimum");
        assert_eq!(per_seed_target(t, 0), 50, "no division by zero");
    }

    #[test]
    fn per_seed_target_for_a_small_mix_stays_at_the_minimum() {
        assert_eq!(per_seed_target(10, 3), INSTANT_MIX_MIN_SEED_TARGET);
        assert_eq!(per_seed_target(10, 1), INSTANT_MIX_MIN_SEED_TARGET);
    }

    #[test]
    fn request_count_asks_twice_the_target_within_the_server_cap() {
        assert_eq!(request_count(20), 40);
        assert_eq!(request_count(50), 100);
        assert_eq!(request_count(400), INSTANT_MIX_MAX_REQUEST);
    }

    #[test]
    fn filters_drop_short_and_one_star_songs() {
        let mut short = song("short");
        short.duration = 59;
        let mut at = song("at");
        at.duration = 60;
        let mut unknown = song("unknown");
        unknown.duration = 0;
        let mut one = song("one");
        one.rating = Some(1);
        let mut two = song("two");
        two.rating = Some(2);
        let mut cleared = song("cleared");
        cleared.rating = Some(0);
        let out = filter_instant_mix(vec![short, at, unknown, one, two, cleared]);
        assert_eq!(ids(&out), vec!["at", "unknown", "two", "cleared"]);
    }

    #[test]
    fn top_up_appends_fresh_songs_up_to_the_target() {
        let mut list = songs(&["a", "b"]);
        let added = top_up(&mut list, songs(&["b", "c", "c", "d", "e"]), 4);
        assert_eq!(added, 2);
        assert_eq!(
            ids(&list),
            vec!["a", "b", "c", "d"],
            "dupes skipped, stops at 4"
        );
    }

    #[test]
    fn top_up_leaves_a_full_list_alone() {
        let mut list = songs(&["a", "b", "c"]);
        assert_eq!(top_up(&mut list, songs(&["d"]), 2), 0);
        assert_eq!(ids(&list), vec!["a", "b", "c"], "never truncates");
    }

    #[test]
    fn dominant_genre_counts_and_breaks_ties_by_first_appearance() {
        let list = vec![
            with_genre("1", Some("Jazz")),
            with_genre("2", Some("Rock")),
            with_genre("3", Some("Rock")),
            with_genre("4", None),
            with_genre("5", Some("")),
        ];
        assert_eq!(dominant_genre(&list), Some("Rock"));

        let tie = vec![with_genre("1", Some("Jazz")), with_genre("2", Some("Rock"))];
        assert_eq!(dominant_genre(&tie), Some("Jazz"));

        let none = vec![with_genre("1", None), with_genre("2", Some("  "))];
        assert_eq!(dominant_genre(&none), None);
        assert_eq!(dominant_genre(&[]), None);
    }

    #[test]
    fn seeds_spread_one_per_artist_when_the_pool_allows() {
        let pool = vec![
            by("a1", "A"),
            by("a2", "A"),
            by("a3", "A"),
            by("b1", "B"),
            by("b2", "B"),
            by("c1", "C"),
            by("d1", "D"),
            by("e1", "E"),
            by("f1", "F"),
        ];
        for seed in 0..32 {
            let out = pick_instant_mix_seeds(&pool, 5, &mut StdRng::seed_from_u64(seed));
            assert_eq!(out.len(), 5);
            let artists: HashSet<&str> = out.iter().map(|s| s.artist.as_str()).collect();
            assert_eq!(
                artists.len(),
                5,
                "six artists: no artist twice (seed {seed})"
            );
        }
    }

    #[test]
    fn seeds_repeat_an_artist_only_when_artists_run_out() {
        let pool = vec![by("a1", "A"), by("a2", "A"), by("a3", "A"), by("b1", "B")];
        let out = pick_instant_mix_seeds(&pool, 3, &mut StdRng::seed_from_u64(3));
        assert_eq!(out.len(), 3);
        assert!(
            out.iter().any(|s| s.id == "b1"),
            "the lone B song is picked"
        );
        let unique: HashSet<&str> = out.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(unique.len(), 3, "never the same song twice");
    }

    #[test]
    fn seeds_skip_one_star_songs_and_duplicates() {
        let mut rejected = by("x", "X");
        rejected.rating = Some(1);
        let pool = vec![rejected, by("a", "A"), by("a", "A"), by("b", "B")];
        let out = pick_instant_mix_seeds(&pool, 5, &mut StdRng::seed_from_u64(1));
        let mut got = ids(&out);
        got.sort_unstable();
        assert_eq!(got, vec!["a", "b"], "the pool holds two usable songs");
    }

    #[test]
    fn seeds_from_an_empty_pool_or_zero_count_are_empty() {
        assert!(pick_instant_mix_seeds(&[], 5, &mut StdRng::seed_from_u64(0)).is_empty());
        let pool = songs(&["a"]);
        assert!(pick_instant_mix_seeds(&pool, 0, &mut StdRng::seed_from_u64(0)).is_empty());
    }

    #[test]
    fn seeds_are_deterministic_by_rng_seed() {
        let pool: Vec<Song> = (0..40)
            .map(|i| by(&format!("s{i}"), &format!("{}", i % 8)))
            .collect();
        let a = pick_instant_mix_seeds(&pool, 5, &mut StdRng::seed_from_u64(9));
        let b = pick_instant_mix_seeds(&pool, 5, &mut StdRng::seed_from_u64(9));
        assert_eq!(ids(&a), ids(&b));
    }
}
