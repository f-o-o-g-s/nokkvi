//! Wrap-around neighbor stepping over a fixed option set — the single home
//! of the position + fallback-anchor + modular-step algorithm shared by view
//! sort modes ([`crate::types::sort_mode::SortMode::cycle`]), queue sort
//! cycling, and the Trawl tray's keyboard value cycling.

/// The neighbor of `current` in `all`, wrapping at both ends. An absent
/// `current` anchors at index 0, so the step still returns a valid element
/// rather than panicking (unreachable for exhaustive const option arrays).
///
/// # Panics
///
/// Panics if `all` is empty — every caller cycles a non-empty option set.
pub fn cycle_wrapping<T: Copy + PartialEq>(all: &[T], current: T, forward: bool) -> T {
    let idx = all.iter().position(|v| *v == current).unwrap_or(0);
    let len = all.len();
    if forward {
        all[(idx + 1) % len]
    } else {
        all[(idx + len - 1) % len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::trawl::{TrawlBlend, TrawlMinLength};

    #[test]
    fn wraps_at_both_ends() {
        let all = &TrawlBlend::ALL;
        assert_eq!(cycle_wrapping(all, all[0], true), all[1]);
        assert_eq!(
            cycle_wrapping(all, all[all.len() - 1], true),
            all[0],
            "forward wrap"
        );
        assert_eq!(
            cycle_wrapping(all, all[0], false),
            all[all.len() - 1],
            "backward wrap"
        );

        // A pick_list-typed array behaves identically (generic path).
        let lengths = &TrawlMinLength::ALL;
        assert_eq!(cycle_wrapping(lengths, lengths[1], false), lengths[0]);
    }

    #[test]
    fn absent_current_anchors_at_index_zero() {
        // 2 is not in the set: the anchor falls back to index 0, so the
        // step lands on a neighbor of the first element — never a panic.
        let all = &[10_u8, 20, 30];
        assert_eq!(cycle_wrapping(all, 2, true), 20);
        assert_eq!(cycle_wrapping(all, 2, false), 30);
    }
}
