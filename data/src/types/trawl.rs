//! Trawl mix-builder domain types and the pure blend engine.
//!
//! A [`TrawlCrate`] holds mixed seeds — any [`BatchItem`] plus a weight and
//! display labels — together with the blend mode and minimum-length filter.
//! The crate is persistent root state in the UI; the modal is just its editor.
//!
//! Resolution happens in `LibraryOrchestrator::resolve_trawl`, which fetches
//! each seed's songs and hands the per-seed lists to [`blend_trawl`]. The
//! engine here is pure and rng-injectable (the `apply`/`apply_with` split from
//! [`crate::types::one_shot_shuffle`]) so tests are deterministic.

use std::collections::HashSet;

use rand::seq::SliceRandom;

use crate::types::{batch::BatchItem, song::Song};

/// Sample cap applied to artist and genre seeds before blending. Albums,
/// playlists and hand-picked songs go in whole — they are bounded and
/// intentional; a genre can be thousands of songs and would swamp the mix.
pub const TRAWL_SEED_SAMPLE_CAP: usize = 50;

/// Weight bounds for the Weighted blend (steppers clamp to this range).
pub const TRAWL_WEIGHT_MIN: u8 = 1;
pub const TRAWL_WEIGHT_MAX: u8 = 5;

/// How the crate's per-seed track lists merge into one queue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrawlBlend {
    /// Round-robin: one track per seed, then around again.
    #[default]
    Interleave,
    /// Round-robin with ratios: a seed takes `weight` tracks per pass.
    Weighted,
    /// Every seed's tracks pooled together and shuffled evenly.
    ShuffleAll,
}

impl TrawlBlend {
    /// Every variant, in display order (drift-anchored by tests).
    pub const ALL: [TrawlBlend; 3] = [
        TrawlBlend::Interleave,
        TrawlBlend::Weighted,
        TrawlBlend::ShuffleAll,
    ];

    /// Segmented-control label.
    pub fn label(self) -> &'static str {
        match self {
            TrawlBlend::Interleave => "Interleave",
            TrawlBlend::Weighted => "Weighted",
            TrawlBlend::ShuffleAll => "Shuffle all",
        }
    }

    /// One-line hint naming what observably changes under this blend.
    pub fn hint(self) -> &'static str {
        match self {
            TrawlBlend::Interleave => "One track per seed, then around again.",
            TrawlBlend::Weighted => "A weight-3 seed lands 3 tracks per pass.",
            TrawlBlend::ShuffleAll => "Every seed's tracks pooled and shuffled evenly.",
        }
    }
}

/// Minimum track length for songs pulled in by expanded seeds (albums,
/// artists, genres, playlists). Hand-picked song seeds always play, and a
/// song with an unknown duration (0) is never filtered — absence of evidence
/// is not shortness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrawlMinLength {
    /// No filtering.
    Off,
    /// Keep songs of 0:30 or longer.
    S30,
    /// Keep songs of 1:00 or longer (default — skits and interludes sit under this).
    #[default]
    S60,
    /// Keep songs of 1:30 or longer.
    S90,
    /// Keep songs of 2:00 or longer.
    S120,
}

impl TrawlMinLength {
    /// Every variant, in display order (drift-anchored by tests).
    pub const ALL: [TrawlMinLength; 5] = [
        TrawlMinLength::Off,
        TrawlMinLength::S30,
        TrawlMinLength::S60,
        TrawlMinLength::S90,
        TrawlMinLength::S120,
    ];

    /// Threshold in seconds; `None` = filter off.
    pub fn secs(self) -> Option<u32> {
        match self {
            TrawlMinLength::Off => None,
            TrawlMinLength::S30 => Some(30),
            TrawlMinLength::S60 => Some(60),
            TrawlMinLength::S90 => Some(90),
            TrawlMinLength::S120 => Some(120),
        }
    }

    /// Picker label — names exactly which songs survive.
    pub fn label(self) -> &'static str {
        match self {
            TrawlMinLength::Off => "No minimum",
            TrawlMinLength::S30 => "0:30 or longer",
            TrawlMinLength::S60 => "1:00 or longer",
            TrawlMinLength::S90 => "1:30 or longer",
            TrawlMinLength::S120 => "2:00 or longer",
        }
    }

    /// Bare `m:ss` threshold for error copy ("every song was under 1:00").
    pub fn threshold_label(self) -> &'static str {
        match self {
            TrawlMinLength::Off => "0:00",
            TrawlMinLength::S30 => "0:30",
            TrawlMinLength::S60 => "1:00",
            TrawlMinLength::S90 => "1:30",
            TrawlMinLength::S120 => "2:00",
        }
    }
}

impl std::fmt::Display for TrawlMinLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Maximum track length for songs pulled in by expanded seeds — keeps
/// 20-minute epics and DJ-mix rips out of a blend. Same honesty rule as the
/// minimum: a song with an unknown duration (0) is never filtered. Labels
/// are strict ("Under 8:00" = `< 480`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrawlMaxLength {
    /// No filtering.
    #[default]
    Off,
    /// Keep songs under 6:00.
    S360,
    /// Keep songs under 8:00.
    S480,
    /// Keep songs under 10:00.
    S600,
    /// Keep songs under 15:00.
    S900,
    /// Keep songs under 20:00.
    S1200,
}

impl TrawlMaxLength {
    /// Every variant, in display order (drift-anchored by tests).
    pub const ALL: [TrawlMaxLength; 6] = [
        TrawlMaxLength::Off,
        TrawlMaxLength::S360,
        TrawlMaxLength::S480,
        TrawlMaxLength::S600,
        TrawlMaxLength::S900,
        TrawlMaxLength::S1200,
    ];

    /// Threshold in seconds; `None` = filter off.
    pub fn secs(self) -> Option<u32> {
        match self {
            TrawlMaxLength::Off => None,
            TrawlMaxLength::S360 => Some(360),
            TrawlMaxLength::S480 => Some(480),
            TrawlMaxLength::S600 => Some(600),
            TrawlMaxLength::S900 => Some(900),
            TrawlMaxLength::S1200 => Some(1200),
        }
    }

    /// Picker label — names exactly which songs survive.
    pub fn label(self) -> &'static str {
        match self {
            TrawlMaxLength::Off => "No maximum",
            TrawlMaxLength::S360 => "Under 6:00",
            TrawlMaxLength::S480 => "Under 8:00",
            TrawlMaxLength::S600 => "Under 10:00",
            TrawlMaxLength::S900 => "Under 15:00",
            TrawlMaxLength::S1200 => "Under 20:00",
        }
    }

    /// Bare `m:ss` threshold for error copy ("every song was over 8:00").
    pub fn threshold_label(self) -> &'static str {
        match self {
            TrawlMaxLength::Off => "0:00",
            TrawlMaxLength::S360 => "6:00",
            TrawlMaxLength::S480 => "8:00",
            TrawlMaxLength::S600 => "10:00",
            TrawlMaxLength::S900 => "15:00",
            TrawlMaxLength::S1200 => "20:00",
        }
    }
}

impl std::fmt::Display for TrawlMaxLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Minimum user star rating for songs pulled in by expanded seeds. Unlike
/// the length filter, an UNRATED song is dropped when this is active: a
/// missing duration is unknown metadata, but an unrated song is one the
/// user hasn't vouched for — "4 stars and up" must not flood with unrated
/// tracks. Hand-picked song seeds are exempt, as ever.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrawlMinRating {
    /// No filtering.
    #[default]
    Off,
    /// Any rated song (drops only the unrated).
    R1,
    R2,
    R3,
    R4,
    /// Five-star songs only.
    R5,
}

impl TrawlMinRating {
    /// Every variant, in display order (drift-anchored by tests).
    pub const ALL: [TrawlMinRating; 6] = [
        TrawlMinRating::Off,
        TrawlMinRating::R1,
        TrawlMinRating::R2,
        TrawlMinRating::R3,
        TrawlMinRating::R4,
        TrawlMinRating::R5,
    ];

    /// Threshold in stars; `None` = filter off.
    pub fn stars(self) -> Option<u32> {
        match self {
            TrawlMinRating::Off => None,
            TrawlMinRating::R1 => Some(1),
            TrawlMinRating::R2 => Some(2),
            TrawlMinRating::R3 => Some(3),
            TrawlMinRating::R4 => Some(4),
            TrawlMinRating::R5 => Some(5),
        }
    }

    /// Picker label — names exactly which songs survive.
    pub fn label(self) -> &'static str {
        match self {
            TrawlMinRating::Off => "Any rating",
            TrawlMinRating::R1 => "1 star and up",
            TrawlMinRating::R2 => "2 stars and up",
            TrawlMinRating::R3 => "3 stars and up",
            TrawlMinRating::R4 => "4 stars and up",
            TrawlMinRating::R5 => "5 stars only",
        }
    }
}

impl std::fmt::Display for TrawlMinRating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Cap on the blended mix's total track count. Applied AFTER the blend so
/// the interleave/weighted character survives into the truncated head (and
/// Shuffle all becomes a uniform sample). `Off` = no cap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrawlMaxTracks {
    /// No cap — the crate's full blended output.
    #[default]
    Off,
    T25,
    T50,
    T100,
    T200,
}

impl TrawlMaxTracks {
    /// Every variant, in display order (drift-anchored by tests).
    pub const ALL: [TrawlMaxTracks; 5] = [
        TrawlMaxTracks::Off,
        TrawlMaxTracks::T25,
        TrawlMaxTracks::T50,
        TrawlMaxTracks::T100,
        TrawlMaxTracks::T200,
    ];

    /// The cap, `None` = unlimited.
    pub fn limit(self) -> Option<usize> {
        match self {
            TrawlMaxTracks::Off => None,
            TrawlMaxTracks::T25 => Some(25),
            TrawlMaxTracks::T50 => Some(50),
            TrawlMaxTracks::T100 => Some(100),
            TrawlMaxTracks::T200 => Some(200),
        }
    }

    /// Picker label — names the resulting queue size.
    pub fn label(self) -> &'static str {
        match self {
            TrawlMaxTracks::Off => "No limit",
            TrawlMaxTracks::T25 => "25 tracks",
            TrawlMaxTracks::T50 => "50 tracks",
            TrawlMaxTracks::T100 => "100 tracks",
            TrawlMaxTracks::T200 => "200 tracks",
        }
    }
}

impl std::fmt::Display for TrawlMaxTracks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The seed-kind half of a seed's identity key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrawlSeedKind {
    Song,
    Album,
    Artist,
    Genre,
    Playlist,
}

/// Identity key for a [`BatchItem`]: kind + id (genres are name-keyed, like
/// the batch pipeline itself).
pub fn batch_item_key(item: &BatchItem) -> (TrawlSeedKind, &str) {
    match item {
        BatchItem::Song(song) => (TrawlSeedKind::Song, song.id.as_str()),
        BatchItem::Album(id) => (TrawlSeedKind::Album, id.as_str()),
        BatchItem::Artist(id) => (TrawlSeedKind::Artist, id.as_str()),
        BatchItem::Genre(name) => (TrawlSeedKind::Genre, name.as_str()),
        BatchItem::Playlist(id) => (TrawlSeedKind::Playlist, id.as_str()),
    }
}

/// One crate entry: what to resolve, how loud it blends, and how the chip
/// reads. Labels are captured at add time so the UI never re-resolves them.
#[derive(Debug, Clone)]
pub struct TrawlSeed {
    pub item: BatchItem,
    /// Weighted-blend ratio, clamped to `TRAWL_WEIGHT_MIN..=TRAWL_WEIGHT_MAX`.
    pub weight: u8,
    /// Chip title (album/artist/genre/playlist name, or song title).
    pub label: String,
    /// Chip subtitle — the duplicate-name disambiguator (artist for songs and
    /// albums, "Artist"/"Genre"/"Playlist" style facts otherwise).
    pub sublabel: String,
}

impl TrawlSeed {
    pub fn new(item: BatchItem, label: impl Into<String>, sublabel: impl Into<String>) -> Self {
        Self {
            item,
            weight: TRAWL_WEIGHT_MIN,
            label: label.into(),
            sublabel: sublabel.into(),
        }
    }

    /// Identity key (kind + id/name).
    pub fn key(&self) -> (TrawlSeedKind, &str) {
        batch_item_key(&self.item)
    }
}

/// The persistent mix crate: seeds + blend + minimum length.
#[derive(Debug, Clone, Default)]
pub struct TrawlCrate {
    pub seeds: Vec<TrawlSeed>,
    pub blend: TrawlBlend,
    pub min_length: TrawlMinLength,
    pub max_length: TrawlMaxLength,
    pub min_rating: TrawlMinRating,
    pub max_tracks: TrawlMaxTracks,
}

impl TrawlCrate {
    pub fn is_empty(&self) -> bool {
        self.seeds.is_empty()
    }

    pub fn len(&self) -> usize {
        self.seeds.len()
    }

    /// Index of the seed matching `item`'s identity, if present.
    pub fn position(&self, item: &BatchItem) -> Option<usize> {
        let key = batch_item_key(item);
        self.seeds.iter().position(|s| s.key() == key)
    }

    pub fn contains(&self, item: &BatchItem) -> bool {
        self.position(item).is_some()
    }

    /// Add a seed unless its identity is already present. Returns whether it
    /// was added.
    pub fn add(&mut self, seed: TrawlSeed) -> bool {
        if self.contains(&seed.item) {
            return false;
        }
        self.seeds.push(seed);
        true
    }

    /// Add the seed, or remove the existing one with the same identity.
    /// Returns `true` when the seed ended up added.
    pub fn toggle(&mut self, seed: TrawlSeed) -> bool {
        if let Some(idx) = self.position(&seed.item) {
            self.seeds.remove(idx);
            false
        } else {
            self.seeds.push(seed);
            true
        }
    }

    pub fn remove_at(&mut self, index: usize) {
        if index < self.seeds.len() {
            self.seeds.remove(index);
        }
    }

    /// Drop every seed, keeping blend + min-length choices.
    pub fn clear_seeds(&mut self) {
        self.seeds.clear();
    }
}

/// Drop songs under the threshold. Songs with unknown duration (0) survive.
pub fn filter_min_length(songs: Vec<Song>, min: TrawlMinLength) -> Vec<Song> {
    match min.secs() {
        None => songs,
        Some(threshold) => songs
            .into_iter()
            .filter(|s| s.duration == 0 || s.duration >= threshold)
            .collect(),
    }
}

/// Drop songs at or over the threshold ("Under 8:00" is strict). Songs with
/// unknown duration (0) survive, like [`filter_min_length`].
pub fn filter_max_length(songs: Vec<Song>, max: TrawlMaxLength) -> Vec<Song> {
    match max.secs() {
        None => songs,
        Some(threshold) => songs
            .into_iter()
            .filter(|s| s.duration == 0 || s.duration < threshold)
            .collect(),
    }
}

/// Drop songs under the star threshold. Unrated songs are DROPPED when the
/// filter is active — deliberate contrast with [`filter_min_length`]'s
/// unknown-duration bypass (see [`TrawlMinRating`]).
pub fn filter_min_rating(songs: Vec<Song>, min: TrawlMinRating) -> Vec<Song> {
    match min.stars() {
        None => songs,
        Some(threshold) => songs
            .into_iter()
            .filter(|s| s.rating.unwrap_or(0) >= threshold)
            .collect(),
    }
}

/// Uniform random sample down to `cap` when the list is longer (shuffle-then-
/// truncate, the in-repo idiom). Shorter lists keep their order untouched.
pub fn sample_cap<R: rand::Rng + ?Sized>(songs: &mut Vec<Song>, cap: usize, rng: &mut R) {
    if songs.len() > cap {
        songs.shuffle(rng);
        songs.truncate(cap);
    }
}

/// Merge per-seed `(songs, weight)` lists into one queue under `blend`.
///
/// Dedupe is take-next-fresh: a seed skips songs already emitted by an
/// earlier take **without consuming its slot**, so weighted ratios stay
/// honest even when seeds overlap (an album seed plus its artist seed).
pub fn blend_trawl<R: rand::Rng + ?Sized>(
    lists: Vec<(Vec<Song>, u8)>,
    blend: TrawlBlend,
    rng: &mut R,
) -> Vec<Song> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Song> = Vec::new();

    match blend {
        TrawlBlend::Interleave | TrawlBlend::Weighted => {
            let takes: Vec<usize> = lists
                .iter()
                .map(|(_, weight)| match blend {
                    TrawlBlend::Weighted => usize::from((*weight).max(TRAWL_WEIGHT_MIN)),
                    TrawlBlend::Interleave | TrawlBlend::ShuffleAll => 1,
                })
                .collect();
            let mut iters: Vec<std::vec::IntoIter<Song>> = lists
                .into_iter()
                .map(|(songs, _)| songs.into_iter())
                .collect();
            let mut exhausted = vec![false; iters.len()];

            while exhausted.iter().any(|done| !done) {
                for (i, iter) in iters.iter_mut().enumerate() {
                    if exhausted[i] {
                        continue;
                    }
                    let mut taken = 0;
                    while taken < takes[i] {
                        match iter.next() {
                            Some(song) => {
                                // A duplicate is skipped without consuming the
                                // slot — the seed keeps digging for a fresh one.
                                if seen.insert(song.id.clone()) {
                                    out.push(song);
                                    taken += 1;
                                }
                            }
                            None => {
                                exhausted[i] = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        TrawlBlend::ShuffleAll => {
            for (songs, _) in lists {
                for song in songs {
                    if seen.insert(song.id.clone()) {
                        out.push(song);
                    }
                }
            }
            out.shuffle(rng);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    fn song(id: &str, dur: u32) -> Song {
        let mut s = Song::test_default(id, &format!("Song {id}"));
        s.duration = dur;
        s
    }

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter().map(|id| song(id, 180)).collect()
    }

    fn ids(list: &[Song]) -> Vec<String> {
        list.iter().map(|s| s.id.clone()).collect()
    }

    fn sorted_ids(list: &[Song]) -> Vec<String> {
        let mut v = ids(list);
        v.sort();
        v
    }

    fn seed(item: BatchItem) -> TrawlSeed {
        TrawlSeed::new(item, "label", "sublabel")
    }

    // ---- enums / labels -------------------------------------------------

    #[test]
    fn blend_defaults_and_anchor() {
        assert_eq!(TrawlBlend::default(), TrawlBlend::Interleave);
        assert_eq!(TrawlBlend::ALL.len(), 3);
        // Every variant appears in ALL exactly once.
        let mut seen = Vec::new();
        for b in TrawlBlend::ALL {
            // exhaustive: a new variant must be added to ALL or this match
            match b {
                TrawlBlend::Interleave | TrawlBlend::Weighted | TrawlBlend::ShuffleAll => {
                    assert!(!seen.contains(&b));
                    seen.push(b);
                }
            }
        }
        assert!(!TrawlBlend::Interleave.label().is_empty());
        assert!(!TrawlBlend::Weighted.hint().is_empty());
    }

    #[test]
    fn min_length_defaults_secs_and_labels() {
        assert_eq!(TrawlMinLength::default(), TrawlMinLength::S60);
        assert_eq!(TrawlMinLength::ALL.len(), 5);
        assert_eq!(TrawlMinLength::Off.secs(), None);
        assert_eq!(TrawlMinLength::S30.secs(), Some(30));
        assert_eq!(TrawlMinLength::S60.secs(), Some(60));
        assert_eq!(TrawlMinLength::S90.secs(), Some(90));
        assert_eq!(TrawlMinLength::S120.secs(), Some(120));
        assert_eq!(TrawlMinLength::Off.label(), "No minimum");
        assert_eq!(TrawlMinLength::S60.label(), "1:00 or longer");
        assert_eq!(TrawlMinLength::S60.threshold_label(), "1:00");
        assert_eq!(TrawlMinLength::S60.to_string(), "1:00 or longer");
    }

    // ---- crate helpers ---------------------------------------------------

    #[test]
    fn crate_add_dedupes_by_identity() {
        let mut c = TrawlCrate::default();
        assert!(c.add(seed(BatchItem::Album("al1".into()))));
        assert!(!c.add(seed(BatchItem::Album("al1".into()))), "dup add");
        assert!(
            c.add(seed(BatchItem::Artist("al1".into()))),
            "same id, other kind"
        );
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn crate_toggle_adds_then_removes() {
        let mut c = TrawlCrate::default();
        assert!(c.toggle(seed(BatchItem::Genre("Phonk".into()))));
        assert!(c.contains(&BatchItem::Genre("Phonk".into())));
        assert!(!c.toggle(seed(BatchItem::Genre("Phonk".into()))));
        assert!(c.is_empty());
    }

    #[test]
    fn crate_song_identity_uses_song_id() {
        let mut c = TrawlCrate::default();
        let s = song("s1", 200);
        assert!(c.add(seed(BatchItem::Song(Box::new(s.clone())))));
        assert!(c.contains(&BatchItem::Song(Box::new(s))));
    }

    #[test]
    fn crate_clear_seeds_keeps_settings() {
        let mut c = TrawlCrate {
            seeds: vec![seed(BatchItem::Album("a".into()))],
            blend: TrawlBlend::Weighted,
            min_length: TrawlMinLength::S120,
            max_length: TrawlMaxLength::default(),
            min_rating: TrawlMinRating::default(),
            max_tracks: TrawlMaxTracks::default(),
        };
        c.clear_seeds();
        assert!(c.is_empty());
        assert_eq!(c.blend, TrawlBlend::Weighted);
        assert_eq!(c.min_length, TrawlMinLength::S120);
    }

    #[test]
    fn crate_remove_at_out_of_bounds_is_noop() {
        let mut c = TrawlCrate::default();
        c.add(seed(BatchItem::Album("a".into())));
        c.remove_at(5);
        assert_eq!(c.len(), 1);
        c.remove_at(0);
        assert!(c.is_empty());
    }

    // ---- max-length filter -----------------------------------------------

    #[test]
    fn max_length_defaults_off_with_anchored_variants() {
        assert_eq!(TrawlMaxLength::default(), TrawlMaxLength::Off);
        assert_eq!(TrawlMaxLength::ALL.len(), 6);
        assert_eq!(TrawlMaxLength::Off.secs(), None);
        assert_eq!(TrawlMaxLength::S480.secs(), Some(480));
        assert_eq!(TrawlMaxLength::S480.label(), "Under 8:00");
        assert_eq!(TrawlMaxLength::S480.threshold_label(), "8:00");
        assert_eq!(TrawlMaxLength::Off.to_string(), "No maximum");
    }

    #[test]
    fn max_filter_off_keeps_everything() {
        let list = vec![song("a", 30), song("b", 0), song("c", 5000)];
        let out = filter_max_length(list, TrawlMaxLength::Off);
        assert_eq!(ids(&out), vec!["a", "b", "c"]);
    }

    #[test]
    fn max_filter_is_strictly_under() {
        let list = vec![song("under", 479), song("at", 480), song("over", 481)];
        let out = filter_max_length(list, TrawlMaxLength::S480);
        assert_eq!(ids(&out), vec!["under"], "'Under 8:00' drops 8:00 exactly");
    }

    #[test]
    fn max_filter_unknown_duration_bypasses() {
        let list = vec![song("unknown", 0), song("epic", 1500)];
        let out = filter_max_length(list, TrawlMaxLength::S360);
        assert_eq!(
            ids(&out),
            vec!["unknown"],
            "unknown is not evidence of length"
        );
    }

    // ---- min-rating filter ---------------------------------------------------

    #[test]
    fn min_rating_defaults_off_with_anchored_variants() {
        assert_eq!(TrawlMinRating::default(), TrawlMinRating::Off);
        assert_eq!(TrawlMinRating::ALL.len(), 6);
        assert_eq!(TrawlMinRating::Off.stars(), None);
        assert_eq!(TrawlMinRating::R1.stars(), Some(1));
        assert_eq!(TrawlMinRating::R5.stars(), Some(5));
        assert_eq!(TrawlMinRating::R4.label(), "4 stars and up");
        assert_eq!(TrawlMinRating::R5.to_string(), "5 stars only");
    }

    fn rated(id: &str, rating: Option<u32>) -> Song {
        let mut s = song(id, 180);
        s.rating = rating;
        s
    }

    #[test]
    fn rating_filter_off_keeps_everything_including_unrated() {
        let list = vec![rated("unrated", None), rated("one", Some(1))];
        let out = filter_min_rating(list, TrawlMinRating::Off);
        assert_eq!(ids(&out), vec!["unrated", "one"]);
    }

    #[test]
    fn rating_filter_keeps_threshold_and_above_drops_below() {
        let list = vec![
            rated("two", Some(2)),
            rated("three", Some(3)),
            rated("five", Some(5)),
        ];
        let out = filter_min_rating(list, TrawlMinRating::R3);
        assert_eq!(ids(&out), vec!["three", "five"], "at-threshold survives");
    }

    #[test]
    fn rating_filter_drops_unrated_when_active() {
        let list = vec![
            rated("unrated", None),
            rated("zero", Some(0)),
            rated("one", Some(1)),
        ];
        let out = filter_min_rating(list, TrawlMinRating::R1);
        assert_eq!(
            ids(&out),
            vec!["one"],
            "unrated is not vouched for — dropped (deliberate contrast with duration-0)"
        );
    }

    // ---- max-tracks cap -----------------------------------------------------

    #[test]
    fn max_tracks_defaults_off_with_anchored_variants() {
        assert_eq!(TrawlMaxTracks::default(), TrawlMaxTracks::Off);
        assert_eq!(TrawlMaxTracks::ALL.len(), 5);
        assert_eq!(TrawlMaxTracks::Off.limit(), None);
        assert_eq!(TrawlMaxTracks::T50.limit(), Some(50));
        assert_eq!(TrawlMaxTracks::T50.label(), "50 tracks");
        assert_eq!(TrawlMaxTracks::Off.to_string(), "No limit");
    }

    #[test]
    fn crate_clear_seeds_keeps_max_tracks() {
        let mut c = TrawlCrate {
            seeds: vec![seed(BatchItem::Album("a".into()))],
            max_tracks: TrawlMaxTracks::T25,
            ..TrawlCrate::default()
        };
        c.clear_seeds();
        assert_eq!(c.max_tracks, TrawlMaxTracks::T25);
    }

    // ---- min-length filter ------------------------------------------------

    #[test]
    fn filter_off_keeps_everything() {
        let list = vec![song("a", 10), song("b", 0), song("c", 500)];
        let out = filter_min_length(list, TrawlMinLength::Off);
        assert_eq!(ids(&out), vec!["a", "b", "c"]);
    }

    #[test]
    fn filter_drops_short_keeps_threshold_and_over() {
        let list = vec![song("under", 59), song("at", 60), song("over", 61)];
        let out = filter_min_length(list, TrawlMinLength::S60);
        assert_eq!(
            ids(&out),
            vec!["at", "over"],
            "under-threshold dropped, >= kept"
        );
    }

    #[test]
    fn filter_unknown_duration_bypasses() {
        let list = vec![song("unknown", 0), song("short", 30)];
        let out = filter_min_length(list, TrawlMinLength::S120);
        assert_eq!(
            ids(&out),
            vec!["unknown"],
            "duration 0 is not evidence of shortness"
        );
    }

    // ---- sample cap --------------------------------------------------------

    #[test]
    fn sample_under_cap_is_identity_in_order() {
        let mut list = songs(&["a", "b", "c"]);
        sample_cap(&mut list, 50, &mut StdRng::seed_from_u64(1));
        assert_eq!(ids(&list), vec!["a", "b", "c"]);
    }

    #[test]
    fn sample_over_cap_truncates_to_cap_subset() {
        let all: Vec<String> = (0..200).map(|i| format!("s{i}")).collect();
        let all_refs: Vec<&str> = all.iter().map(String::as_str).collect();
        let mut list = songs(&all_refs);
        sample_cap(&mut list, 50, &mut StdRng::seed_from_u64(7));
        assert_eq!(list.len(), 50);
        let mut unique = sorted_ids(&list);
        unique.dedup();
        assert_eq!(unique.len(), 50, "sample must not duplicate");
        for id in ids(&list) {
            assert!(all.contains(&id), "sample must come from the input");
        }
    }

    #[test]
    fn sample_is_deterministic_by_seed() {
        let all: Vec<String> = (0..100).map(|i| format!("s{i}")).collect();
        let all_refs: Vec<&str> = all.iter().map(String::as_str).collect();
        let mut a = songs(&all_refs);
        let mut b = songs(&all_refs);
        sample_cap(&mut a, 10, &mut StdRng::seed_from_u64(42));
        sample_cap(&mut b, 10, &mut StdRng::seed_from_u64(42));
        assert_eq!(a.len(), 10, "cap applied");
        assert_eq!(ids(&a), ids(&b));
    }

    // ---- blend: interleave --------------------------------------------------

    #[test]
    fn interleave_round_robins_in_crate_order() {
        let lists = vec![
            (songs(&["a1", "a2"]), 1),
            (songs(&["b1", "b2"]), 1),
            (songs(&["c1", "c2"]), 1),
        ];
        let out = blend_trawl(lists, TrawlBlend::Interleave, &mut StdRng::seed_from_u64(0));
        assert_eq!(ids(&out), vec!["a1", "b1", "c1", "a2", "b2", "c2"]);
    }

    #[test]
    fn interleave_continues_after_seed_exhausts() {
        let lists = vec![(songs(&["a1"]), 1), (songs(&["b1", "b2", "b3"]), 1)];
        let out = blend_trawl(lists, TrawlBlend::Interleave, &mut StdRng::seed_from_u64(0));
        assert_eq!(ids(&out), vec!["a1", "b1", "b2", "b3"]);
    }

    #[test]
    fn interleave_dupe_skip_does_not_consume_slot() {
        // b's first song IS a's first song; b must still land a fresh song
        // in the same pass (take-next-fresh).
        let lists = vec![
            (songs(&["shared", "a2"]), 1),
            (songs(&["shared", "b2", "b3"]), 1),
        ];
        let out = blend_trawl(lists, TrawlBlend::Interleave, &mut StdRng::seed_from_u64(0));
        assert_eq!(ids(&out), vec!["shared", "b2", "a2", "b3"]);
    }

    #[test]
    fn interleave_ignores_weights() {
        let lists = vec![(songs(&["a1", "a2"]), 5), (songs(&["b1", "b2"]), 1)];
        let out = blend_trawl(lists, TrawlBlend::Interleave, &mut StdRng::seed_from_u64(0));
        assert_eq!(ids(&out), vec!["a1", "b1", "a2", "b2"]);
    }

    // ---- blend: weighted ------------------------------------------------------

    #[test]
    fn weighted_takes_weight_per_pass() {
        let lists = vec![
            (songs(&["a1", "a2", "a3", "a4"]), 2),
            (songs(&["b1", "b2"]), 1),
        ];
        let out = blend_trawl(lists, TrawlBlend::Weighted, &mut StdRng::seed_from_u64(0));
        assert_eq!(ids(&out), vec!["a1", "a2", "b1", "a3", "a4", "b2"]);
    }

    #[test]
    fn weighted_zero_weight_is_clamped_to_one() {
        let lists = vec![(songs(&["a1", "a2"]), 0), (songs(&["b1"]), 1)];
        let out = blend_trawl(lists, TrawlBlend::Weighted, &mut StdRng::seed_from_u64(0));
        assert_eq!(ids(&out), vec!["a1", "b1", "a2"]);
    }

    // ---- blend: shuffle all -------------------------------------------------

    #[test]
    fn shuffle_all_preserves_deduped_multiset() {
        let lists = vec![
            (songs(&["a1", "shared", "a3"]), 1),
            (songs(&["b1", "shared"]), 3),
        ];
        let out = blend_trawl(lists, TrawlBlend::ShuffleAll, &mut StdRng::seed_from_u64(5));
        assert_eq!(sorted_ids(&out), vec!["a1", "a3", "b1", "shared"]);
    }

    #[test]
    fn shuffle_all_actually_reorders_and_is_deterministic() {
        let all: Vec<String> = (0..64).map(|i| format!("s{i}")).collect();
        let all_refs: Vec<&str> = all.iter().map(String::as_str).collect();
        let lists_a = vec![(songs(&all_refs), 1)];
        let lists_b = vec![(songs(&all_refs), 1)];
        let a = blend_trawl(
            lists_a,
            TrawlBlend::ShuffleAll,
            &mut StdRng::seed_from_u64(9),
        );
        let b = blend_trawl(
            lists_b,
            TrawlBlend::ShuffleAll,
            &mut StdRng::seed_from_u64(9),
        );
        assert_eq!(a.len(), 64, "nothing lost");
        assert_ne!(ids(&a), all, "must permute");
        assert_eq!(ids(&a), ids(&b), "same seed, same permutation");
    }

    // ---- blend: edges ----------------------------------------------------------

    #[test]
    fn blend_empty_input_is_empty() {
        for blend in TrawlBlend::ALL {
            let out = blend_trawl(Vec::new(), blend, &mut StdRng::seed_from_u64(0));
            assert!(out.is_empty());
            let out = blend_trawl(vec![(Vec::new(), 1)], blend, &mut StdRng::seed_from_u64(0));
            assert!(out.is_empty());
        }
    }

    proptest::proptest! {
        /// Every blend emits exactly the deduped union of its inputs — a
        /// permutation, never a loss or a duplicate.
        #[test]
        fn blend_output_is_deduped_union(
            sizes in proptest::collection::vec(0usize..12, 1..5),
            weights in proptest::collection::vec(0u8..7, 1..5),
            blend_idx in 0usize..3,
            seed_val in proptest::prelude::any::<u64>(),
            overlap in proptest::prelude::any::<bool>(),
        ) {
            let blend = TrawlBlend::ALL[blend_idx];
            let mut expected = std::collections::BTreeSet::new();
            let lists: Vec<(Vec<Song>, u8)> = sizes
                .iter()
                .enumerate()
                .map(|(li, &n)| {
                    let list: Vec<Song> = (0..n)
                        .map(|i| {
                            // With overlap on, every list shares ids s0..sn;
                            // otherwise ids are per-list unique.
                            let id = if overlap {
                                format!("s{i}")
                            } else {
                                format!("l{li}s{i}")
                            };
                            expected.insert(id.clone());
                            song(&id, 180)
                        })
                        .collect();
                    let w = weights.get(li).copied().unwrap_or(1);
                    (list, w)
                })
                .collect();
            let out = blend_trawl(lists, blend, &mut StdRng::seed_from_u64(seed_val));
            let got: std::collections::BTreeSet<String> =
                out.iter().map(|s| s.id.clone()).collect();
            proptest::prop_assert_eq!(out.len(), got.len(), "no duplicates");
            proptest::prop_assert_eq!(got, expected, "exactly the deduped union");
        }
    }
}
