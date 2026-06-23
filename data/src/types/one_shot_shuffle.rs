//! One-shot shuffle directive for play actions.
//!
//! A [`OneShotShuffle`] permutes a resolved `Vec<Song>` ONCE, immediately before
//! it is handed to the queue; playback then proceeds linearly through that
//! permutation. This is deliberately distinct from the persistent shuffle MODE
//! (`queue.shuffle`): a one-shot shuffle NEVER writes the mode flag, so the
//! player-bar shuffle indicator stays off. It is the queue-contents analogue of
//! MPD's `shuffle` command, where the persistent mode flag is MPD's `random`.

use rand::seq::SliceRandom;

use crate::types::song::Song;

/// How a play action should permute its resolved track list before queueing.
///
/// Applied at the single `dispatch` chokepoint, after resolution and before the
/// queue verb runs. A `Copy`, iced-free directive that never touches the
/// persistent shuffle mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OneShotShuffle {
    /// Keep the resolved order (today's behavior).
    #[default]
    None,
    /// Uniform Fisher-Yates over the whole list.
    Full,
    /// Uniform Fisher-Yates over `songs[1..]`, pinning index 0. The caller is
    /// responsible for having rotated the anchor track to the front first.
    AnchorFirst,
}

impl OneShotShuffle {
    /// Permute `songs` in place using the thread-local RNG (production path).
    pub fn apply(self, songs: &mut [Song]) {
        self.apply_with(songs, &mut rand::rng());
    }

    /// Permute `songs` in place using the supplied RNG.
    ///
    /// Mirrors the Fisher-Yates usage in `services/queue/order.rs`. The seedable
    /// `rng` parameter lets tests assert deterministic permutations.
    pub fn apply_with<R: rand::Rng + ?Sized>(self, songs: &mut [Song], rng: &mut R) {
        match self {
            OneShotShuffle::None => {}
            OneShotShuffle::Full => songs.shuffle(rng),
            OneShotShuffle::AnchorFirst => {
                // The caller has already rotated the anchor to index 0; shuffle
                // only the tail so index 0 stays pinned.
                if songs.len() > 1 {
                    songs[1..].shuffle(rng);
                }
            }
        }
    }

    /// Whether this directive actually permutes the list (anything but
    /// [`OneShotShuffle::None`]). Used to force play-from-top under shuffle.
    pub fn shuffles(self) -> bool {
        !matches!(self, OneShotShuffle::None)
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    fn make(n: usize) -> Vec<Song> {
        (0..n)
            .map(|i| Song::test_default(&format!("s{i}"), &format!("Song {i}")))
            .collect()
    }

    fn ids(songs: &[Song]) -> Vec<String> {
        songs.iter().map(|s| s.id.clone()).collect()
    }

    fn sorted_ids(songs: &[Song]) -> Vec<String> {
        let mut v = ids(songs);
        v.sort();
        v
    }

    #[test]
    fn default_is_none() {
        assert_eq!(OneShotShuffle::default(), OneShotShuffle::None);
    }

    #[test]
    fn shuffles_is_true_for_permuting_variants() {
        assert!(!OneShotShuffle::None.shuffles());
        assert!(OneShotShuffle::Full.shuffles());
        assert!(OneShotShuffle::AnchorFirst.shuffles());
    }

    #[test]
    fn none_is_identity() {
        let mut songs = make(8);
        let before = ids(&songs);
        OneShotShuffle::None.apply_with(&mut songs, &mut StdRng::seed_from_u64(1));
        assert_eq!(ids(&songs), before);
    }

    #[test]
    fn full_preserves_multiset() {
        let mut songs = make(64);
        let before = sorted_ids(&songs);
        OneShotShuffle::Full.apply_with(&mut songs, &mut StdRng::seed_from_u64(7));
        assert_eq!(sorted_ids(&songs), before);
        assert_eq!(songs.len(), 64);
    }

    #[test]
    fn full_actually_reorders() {
        // 64 distinct tracks: a real shuffle is ~never the identity (~1/64!).
        let mut songs = make(64);
        let before = ids(&songs);
        OneShotShuffle::Full.apply_with(&mut songs, &mut StdRng::seed_from_u64(7));
        assert_ne!(ids(&songs), before, "Full must permute the list");
    }

    #[test]
    fn anchor_first_pins_index_0() {
        let mut songs = make(64);
        let anchor = songs[0].id.clone();
        OneShotShuffle::AnchorFirst.apply_with(&mut songs, &mut StdRng::seed_from_u64(3));
        assert_eq!(songs[0].id, anchor, "AnchorFirst must keep index 0 fixed");
    }

    #[test]
    fn anchor_first_reorders_tail() {
        let mut songs = make(64);
        let tail_before = ids(&songs)[1..].to_vec();
        OneShotShuffle::AnchorFirst.apply_with(&mut songs, &mut StdRng::seed_from_u64(3));
        let tail_after = ids(&songs)[1..].to_vec();
        assert_ne!(
            tail_after, tail_before,
            "AnchorFirst must permute songs[1..]"
        );
    }

    #[test]
    fn anchor_first_preserves_multiset() {
        let mut songs = make(64);
        let before = sorted_ids(&songs);
        OneShotShuffle::AnchorFirst.apply_with(&mut songs, &mut StdRng::seed_from_u64(9));
        assert_eq!(sorted_ids(&songs), before);
    }

    #[test]
    fn empty_and_single_are_noops() {
        for variant in [
            OneShotShuffle::None,
            OneShotShuffle::Full,
            OneShotShuffle::AnchorFirst,
        ] {
            let mut empty: Vec<Song> = Vec::new();
            variant.apply_with(&mut empty, &mut StdRng::seed_from_u64(0));
            assert!(empty.is_empty());

            let mut one = make(1);
            variant.apply_with(&mut one, &mut StdRng::seed_from_u64(0));
            assert_eq!(ids(&one), vec!["s0".to_string()]);
        }
    }

    #[test]
    fn seeded_is_deterministic() {
        for variant in [OneShotShuffle::Full, OneShotShuffle::AnchorFirst] {
            let mut a = make(32);
            let mut b = make(32);
            variant.apply_with(&mut a, &mut StdRng::seed_from_u64(123));
            variant.apply_with(&mut b, &mut StdRng::seed_from_u64(123));
            assert_eq!(ids(&a), ids(&b), "same seed must yield same permutation");
        }
    }
}
