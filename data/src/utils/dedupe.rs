//! Remove Duplicates — the one home of the "which rows are repeats" rule
//! behind the playlist editor's and the queue's de-dupe.
//!
//! A duplicate is a row whose song id already appeared. Two files of the same
//! recording carry different ids, so both stay.

use std::collections::HashSet;

/// The `entry_id`s of the rows a Remove Duplicates pass drops, in list order.
///
/// `rows` yields `(song id, entry_id)` in list order. Every song keeps its
/// first row, except that a `protected` row is always the kept copy of its
/// song: every other row of that song is dropped, earlier ones included. The
/// queue protects the row under the play cursor so the playing song never
/// loses its row. A `protected` id that names no row, or a row whose song
/// appears once, changes nothing.
///
/// Two passes over `rows` (one to find the protected row's song), O(n).
pub fn duplicate_entry_ids<'a, I>(rows: I, protected: Option<u64>) -> Vec<u64>
where
    I: Iterator<Item = (&'a str, u64)> + Clone,
{
    let mut seen: HashSet<&str> = HashSet::new();
    if let Some(protected_song) = protected.and_then(|p| {
        rows.clone()
            .find(|&(_, entry_id)| entry_id == p)
            .map(|(song_id, _)| song_id)
    }) {
        seen.insert(protected_song);
    }
    rows.filter(|&(song_id, entry_id)| Some(entry_id) != protected && !seen.insert(song_id))
        .map(|(_, entry_id)| entry_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// `songs` as rows with entry_ids `0..len`.
    fn dropped(songs: &[&str], protected: Option<u64>) -> Vec<u64> {
        duplicate_entry_ids(
            songs.iter().enumerate().map(|(i, s)| (*s, i as u64)),
            protected,
        )
    }

    #[test]
    fn empty_and_unique_lists_drop_nothing() {
        assert!(dropped(&[], None).is_empty());
        assert!(dropped(&["a", "b", "c"], None).is_empty());
        assert!(dropped(&["a", "b", "c"], Some(1)).is_empty());
    }

    #[test]
    fn later_copies_go_in_list_order() {
        // [a, b, a, c, b]: the second a (row 2) and the second b (row 4).
        assert_eq!(dropped(&["a", "b", "a", "c", "b"], None), [2, 4]);
    }

    #[test]
    fn a_protected_middle_copy_keeps_its_row() {
        // Three copies, the middle one protected: the first and third go.
        assert_eq!(dropped(&["a", "x", "a", "y", "a"], Some(2)), [0, 4]);
    }

    #[test]
    fn a_protected_row_without_siblings_or_unknown_acts_like_none() {
        let songs = ["a", "b", "a", "c", "b"];
        let unprotected = dropped(&songs, None);
        // Row 3 (c) has no sibling.
        assert_eq!(dropped(&songs, Some(3)), unprotected);
        // No row carries entry_id 99.
        assert_eq!(dropped(&songs, Some(99)), unprotected);
    }

    #[test]
    fn a_protected_later_copy_drops_the_first() {
        assert_eq!(dropped(&["a", "b", "a"], Some(2)), [0]);
    }

    proptest! {
        /// After dropping the result: every song id appears once, the drop
        /// list is in list order, no song vanishes, the protected row
        /// survives, and every other survivor is its song's first row.
        #[test]
        fn survivors_are_unique_and_complete(
            songs in proptest::collection::vec(0u8..6, 0..40),
            protected in proptest::option::of(0u64..45),
        ) {
            let ids: Vec<String> = songs.iter().map(|s| format!("s{s}")).collect();
            let rows = ids.iter().enumerate().map(|(i, s)| (s.as_str(), i as u64));
            let dropped = duplicate_entry_ids(rows.clone(), protected);

            prop_assert!(dropped.windows(2).all(|w| w[0] < w[1]));
            let dropped_set: HashSet<u64> = dropped.iter().copied().collect();
            let survivors: Vec<(&str, u64)> =
                rows.clone().filter(|(_, e)| !dropped_set.contains(e)).collect();

            let mut seen = HashSet::new();
            prop_assert!(survivors.iter().all(|(s, _)| seen.insert(*s)));
            let all: HashSet<&str> = ids.iter().map(String::as_str).collect();
            prop_assert_eq!(seen, all);

            let protected_song = protected
                .and_then(|p| ids.get(p as usize))
                .map(String::as_str);
            if let Some(p) = protected.filter(|&p| (p as usize) < ids.len()) {
                prop_assert!(!dropped_set.contains(&p));
            }
            for (song, entry_id) in survivors {
                if Some(song) != protected_song {
                    let first = ids.iter().position(|s| s == song).map(|i| i as u64);
                    prop_assert_eq!(first, Some(entry_id));
                }
            }
        }
    }
}
