//! Block reorder for a multi-row drag drop — the one home of the "move these
//! rows as a block above the target row" algorithm behind the queue's and
//! the playlist editor's optimistic batch drop.

/// Move the rows at `selected` (positions into `rows`) into one contiguous
/// block that lands directly above the row that sat at `target` before the
/// move.
///
/// - The moved rows keep their relative order, and so do the rest; the order
///   of `selected` itself doesn't matter.
/// - A `target` at or past `rows.len()` appends the block.
/// - A `target` inside the moved set lands the block where that row sat.
/// - A position past the end is skipped, and a repeated position moves its
///   row once.
///
/// One pass over `rows` that moves each row, never clones one (`T` needs no
/// `Clone`): O(n + k) for n rows and k selected positions, where a
/// remove/insert loop costs O(k·n).
pub fn move_block_before<T>(rows: &mut Vec<T>, selected: &[usize], target: usize) {
    let mut is_moved = vec![false; rows.len()];
    for &i in selected {
        if let Some(flag) = is_moved.get_mut(i) {
            *flag = true;
        }
    }

    // Unmoved rows above the target stay above the block; every other
    // unmoved row goes below it.
    let mut above = Vec::with_capacity(rows.len());
    let mut block = Vec::new();
    let mut below = Vec::new();
    for (i, (row, moved)) in std::mem::take(rows).into_iter().zip(is_moved).enumerate() {
        if moved {
            block.push(row);
        } else if i < target {
            above.push(row);
        } else {
            below.push(row);
        }
    }
    above.append(&mut block);
    above.append(&mut below);
    *rows = above;
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// The remove/insert algorithm the queue and the editor ran before the
    /// shared pass, kept as the reference: remove the selected rows in
    /// descending order, then insert them in ascending order at the target
    /// shifted back by the removed rows that sat above it.
    fn reference_remove_insert<T>(rows: &mut Vec<T>, selected: &[usize], target: usize) {
        let mut descending = selected.to_vec();
        descending.sort_unstable_by(|a, b| b.cmp(a));
        let raw_target = target.min(rows.len());
        let mut moved = Vec::new();
        for &i in &descending {
            if i < rows.len() {
                moved.push(rows.remove(i));
            }
        }
        moved.reverse();
        let removed_above = descending.iter().filter(|&&i| i < raw_target).count();
        let insert_at = raw_target.saturating_sub(removed_above).min(rows.len());
        for (offset, row) in moved.into_iter().enumerate() {
            rows.insert(insert_at + offset, row);
        }
    }

    fn moved(rows: &[&str], selected: &[usize], target: usize) -> Vec<String> {
        let mut rows: Vec<String> = rows.iter().map(|r| (*r).to_string()).collect();
        move_block_before(&mut rows, selected, target);
        rows
    }

    #[test]
    fn moved_rows_keep_their_relative_order() {
        // Selected out of order; the block still reads b, d.
        assert_eq!(
            moved(&["a", "b", "c", "d", "e"], &[3, 1], 5),
            ["a", "c", "e", "b", "d"]
        );
        assert_eq!(
            moved(&["a", "b", "c", "d", "e"], &[4, 0], 2),
            ["b", "a", "e", "c", "d"]
        );
    }

    #[test]
    fn target_past_the_end_appends() {
        assert_eq!(moved(&["a", "b", "c"], &[0], 3), ["b", "c", "a"]);
        assert_eq!(moved(&["a", "b", "c"], &[0], 99), ["b", "c", "a"]);
    }

    #[test]
    fn target_inside_the_moved_set_lands_where_it_sat() {
        // Target row c (index 2) is itself moved: the block lands where c sat,
        // below the unmoved a and above d.
        assert_eq!(
            moved(&["a", "b", "c", "d", "e"], &[1, 2, 4], 2),
            ["a", "b", "c", "e", "d"]
        );
    }

    #[test]
    fn positions_past_the_end_are_skipped() {
        assert_eq!(moved(&["a", "b", "c"], &[7, 0], 2), ["b", "a", "c"]);
        assert_eq!(moved(&["a", "b", "c"], &[7], 0), ["a", "b", "c"]);
    }

    #[test]
    fn a_repeated_position_moves_one_row() {
        assert_eq!(
            moved(&["a", "b", "c", "d"], &[0, 0, 0], 3),
            ["b", "c", "a", "d"]
        );
    }

    #[test]
    fn empty_rows_and_empty_selection_are_no_ops() {
        assert!(moved(&[], &[0, 1], 0).is_empty());
        assert_eq!(moved(&["a", "b"], &[], 0), ["a", "b"]);
    }

    /// `n` rows, a set of distinct in-range positions in arbitrary order, and
    /// a target anywhere from the first row to a little past the end.
    fn rows_selection_target() -> impl Strategy<Value = (usize, Vec<usize>, usize)> {
        (0usize..48).prop_flat_map(|n| {
            (
                Just(n),
                proptest::sample::subsequence((0..n).collect::<Vec<_>>(), 0..=n).prop_shuffle(),
                0..=n + 2,
            )
        })
    }

    proptest! {
        /// On distinct in-range positions (the only input the reference
        /// handles), the single pass and the remove/insert loop agree.
        #[test]
        fn matches_remove_insert_reference((n, selected, target) in rows_selection_target()) {
            let mut fast: Vec<usize> = (0..n).collect();
            let mut reference = fast.clone();
            move_block_before(&mut fast, &selected, target);
            reference_remove_insert(&mut reference, &selected, target);
            prop_assert_eq!(fast, reference);
        }

        /// Repeats and out-of-range positions behave as their deduplicated,
        /// in-range set.
        #[test]
        fn repeats_and_strays_reduce_to_the_distinct_set(
            n in 0usize..32,
            raw in proptest::collection::vec(0usize..40, 0..40),
            target in 0usize..40,
        ) {
            let mut distinct: Vec<usize> = raw.iter().copied().filter(|&i| i < n).collect();
            distinct.sort_unstable();
            distinct.dedup();

            let mut with_noise: Vec<usize> = (0..n).collect();
            let mut clean = with_noise.clone();
            move_block_before(&mut with_noise, &raw, target);
            reference_remove_insert(&mut clean, &distinct, target);
            prop_assert_eq!(with_noise, clean);
        }
    }
}
