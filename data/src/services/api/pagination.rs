//! Shared pagination helpers for Navidrome `_start` / `_end` query parameters.
//!
//! Navidrome uses `_start` and `_end` (inclusive-exclusive index range) rather
//! than `_offset` / `_limit`. The "no practical limit" sentinel is 999_999 —
//! large enough that no realistic Navidrome library has ever exceeded it.
//!
//! Callers that paginate by page take `paged_range(offset, limit)` and push
//! the returned strings into their params vec. Callers that always want the
//! cap can push `("_end", NO_LIMIT_END_STR)` directly. Callers that need
//! to fetch *every* row of a filtered set use `fetch_all_pages` instead,
//! which loops in `FULL_LOAD_PAGE_SIZE` chunks until exhausted.

use anyhow::Result;

/// "No practical limit" sentinel for `_end`. See module doc for the contract.
pub(crate) const NO_LIMIT_END: u32 = 999_999;

/// String form of `NO_LIMIT_END` for direct embedding in `params` vecs.
///
/// Kept in lockstep with `NO_LIMIT_END` by a test below.
pub(crate) const NO_LIMIT_END_STR: &str = "999999";

/// Materialized `_start` / `_end` query-string values for a paginated request.
///
/// The owner of the params vec holds this on the stack and pushes `(&str, &str)`
/// borrows into the params: this works around the lifetime juggling that
/// `format!`-on-the-fly otherwise needs.
pub(crate) struct PagedRange {
    pub start: String,
    pub end: String,
}

/// Build a `PagedRange` for a Navidrome `_start` / `_end` pair.
///
/// `offset` is the zero-based starting index. `limit` is the page size; when
/// `None`, the helper substitutes `NO_LIMIT_END` so the caller fetches "the
/// rest" (up to the sentinel cap). `offset + limit` is saturating so an
/// overflowing arithmetic cannot escape into the request.
pub(crate) fn paged_range(offset: u32, limit: Option<u32>) -> PagedRange {
    let limit = limit.unwrap_or(NO_LIMIT_END);
    let end = offset.saturating_add(limit);
    PagedRange {
        start: offset.to_string(),
        end: end.to_string(),
    }
}

/// Page size used by `fetch_all_pages` when paginating an unbounded fetch.
///
/// Chosen to balance round-trips vs. server stress: a 50k-song library
/// completes in ~10 round-trips at this size. Smaller pages would multiply
/// per-request overhead; larger pages would risk request-time / memory
/// spikes on slow networks.
pub(crate) const FULL_LOAD_PAGE_SIZE: u32 = 5_000;

/// Repeatedly call `fetch_page(start, end)` until a short page is returned
/// or the cumulative item count reaches the total reported by the first call.
///
/// Used by per-domain loaders that need "load every record" semantics where
/// the previous code hard-coded an arbitrary `_end=50000` ceiling.
///
/// * `page_size` — passed to each `fetch_page` invocation as `(start, start + page_size)`.
/// * `fetch_page` — async callable that, given `(start, end_exclusive)`,
///   returns `(page, total_count)`. `total_count` should be the
///   server-reported total (e.g. X-Total-Count); the helper uses the value
///   from the first call as a secondary loop terminator. The primary
///   terminator is a short page (a returned `page.len() < page_size`).
pub(crate) async fn fetch_all_pages<T, F, Fut>(
    page_size: u32,
    fetch_page: F,
) -> Result<(Vec<T>, usize)>
where
    F: Fn(u32, u32) -> Fut,
    Fut: std::future::Future<Output = Result<(Vec<T>, usize)>>,
{
    let mut all: Vec<T> = Vec::new();
    let mut offset: u32 = 0;
    let mut total: Option<usize> = None;
    loop {
        let end = offset.saturating_add(page_size);
        let (page, page_total) = fetch_page(offset, end).await?;
        let total_count = *total.get_or_insert(page_total);
        let page_len = page.len();
        all.extend(page);
        if page_len < page_size as usize || all.len() >= total_count {
            break;
        }
        offset = all.len() as u32;
    }
    Ok((all, total.unwrap_or(0)))
}

/// The `(sort, order)` every [`draw_random_row`] caller passes. Load-bearing and
/// therefore named once: the whole point of the count-probe draw is that the sort
/// is **stable**, so switching this to `"random"` would silently reinstate the
/// `resetSeededRandom` corruption described on that function. A test asserts it
/// does not map to a server-side random sort.
pub(crate) const RANDOM_DRAW_SORT: (&str, &str) = ("name", "ASC");

/// Draw ONE uniformly-random row from a paginated browse endpoint: probe the
/// table size with a 1-row page, then fetch the single row sitting at a random
/// offset. Two tiny requests, and — crucially — a **stable** sort throughout
/// ([`RANDOM_DRAW_SORT`]).
///
/// This is the client-side replacement for `_sort=random`, which is unusable for
/// a one-row draw: Navidrome re-seeds its per-`(table, user)` seeded-random
/// ordering for every query it maps to `Sort: "random"` arriving at `Offset == 0`
/// (`resetSeededRandom` in `persistence/sql_base_repository.go`), silently
/// re-permuting an in-progress "Random"-sort pagination in whichever browse view
/// the user left mid-scroll. A stable sort plus a random offset draws just as
/// uniformly (modulo the probe-row reuse noted below) and touches no shared
/// server state.
///
/// * `label` — names the draw in the two degradation warnings below.
/// * `fetch` — async callable taking `(offset, limit)` and returning
///   `(page, raw X-Total-Count)`. The RAW header matters for OBSERVABILITY, not
///   for the drawn row: a caller that coalesces a missing header to the page
///   length reports `total = 1` for the 1-row probe, and `random_range(0..1)` then
///   lands on the probe row — the same row this helper returns. The difference is
///   that the helper can *see* the missing header and warn, instead of a frozen
///   draw looking exactly like an unlucky streak.
///
/// `Ok(None)` only when the (library-scoped) table is empty. Both degradations —
/// a missing total and a failed offset page — fall back to the probe row rather
/// than losing the draw, and warn.
pub(crate) async fn draw_random_row<T, F, Fut>(label: &str, fetch: F) -> Result<Option<T>>
where
    F: Fn(usize, usize) -> Fut,
    Fut: std::future::Future<Output = Result<(Vec<T>, Option<u32>)>>,
{
    use rand::RngExt;

    let (first_page, total) = fetch(0, 1).await?;
    let Some(first) = first_page.into_iter().next() else {
        return Ok(None);
    };
    let Some(total) = total.filter(|t| *t > 0) else {
        tracing::warn!(
            "{label}: no X-Total-Count — table size unknown, draw degrades to the first row"
        );
        return Ok(Some(first));
    };
    let offset = rand::rng().random_range(0..total) as usize;
    if offset == 0 {
        return Ok(Some(first));
    }
    match fetch(offset, 1).await {
        // An empty offset page means a between-requests table shrink left the
        // offset past the end; the probe row is a fine draw in that racy sliver.
        Ok((page, _)) => match page.into_iter().next() {
            Some(row) => Ok(Some(row)),
            None => Ok(Some(first)),
        },
        Err(e) => {
            tracing::warn!("{label}: offset page failed ({e:#}), drawing the probe row instead");
            Ok(Some(first))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_limit_end_str_matches_numeric() {
        assert_eq!(NO_LIMIT_END.to_string(), NO_LIMIT_END_STR);
    }

    #[test]
    fn paged_range_none_limit_uses_no_limit_end() {
        let r = paged_range(0, None);
        assert_eq!(r.start, "0");
        assert_eq!(r.end, "999999");
    }

    #[test]
    fn paged_range_some_limit_offsets_correctly() {
        let r = paged_range(100, Some(50));
        assert_eq!(r.start, "100");
        assert_eq!(r.end, "150");
    }

    #[test]
    fn paged_range_offset_zero_some_limit() {
        let r = paged_range(0, Some(500));
        assert_eq!(r.start, "0");
        assert_eq!(r.end, "500");
    }

    #[test]
    fn paged_range_saturates_on_overflow() {
        let r = paged_range(u32::MAX - 10, Some(u32::MAX));
        assert_eq!(r.end, u32::MAX.to_string());
    }

    #[tokio::test]
    async fn fetch_all_pages_aggregates_until_short_page() {
        use std::sync::Mutex;
        let pages: Mutex<Vec<Vec<i32>>> = Mutex::new(vec![
            (1..=5000).collect(),
            (5001..=10000).collect(),
            vec![10001, 10002, 10003], // short page → terminator
        ]);
        let total = 10003;
        let (items, total_count) = fetch_all_pages(5000, |_start, _end| async {
            let mut p = pages.lock().expect("test pages lock");
            let page = if p.is_empty() {
                Vec::new()
            } else {
                p.remove(0)
            };
            Ok((page, total))
        })
        .await
        .expect("fetch_all_pages should not fail");
        assert_eq!(items.len(), 10003);
        assert_eq!(total_count, 10003);
    }

    #[tokio::test]
    async fn fetch_all_pages_stops_at_total_count() {
        use std::sync::Mutex;
        let pages: Mutex<Vec<Vec<i32>>> = Mutex::new(vec![
            (1..=5000).collect(),
            (5001..=8000).collect(), // 3000 items, exactly meeting total
        ]);
        let total = 8000;
        let (items, total_count) = fetch_all_pages(5000, |_, _| async {
            let mut p = pages.lock().expect("test pages lock");
            let page = if p.is_empty() {
                Vec::new()
            } else {
                p.remove(0)
            };
            Ok((page, total))
        })
        .await
        .expect("fetch_all_pages should not fail");
        assert_eq!(items.len(), 8000);
        assert_eq!(total_count, 8000);
    }

    #[tokio::test]
    async fn fetch_all_pages_loads_more_than_old_50000_cap() {
        // Regression test for the songs.rs 50000-cap truncation bug.
        // A library with 80_000 songs would have lost 30_000 under the old cap.
        use std::sync::Mutex;
        let pages: Mutex<Vec<Vec<u32>>> = Mutex::new(
            (0..16)
                .map(|i| (i * 5000..(i + 1) * 5000).collect())
                .collect(),
        );
        let total = 80_000;
        let (items, total_count) = fetch_all_pages(5_000, |_, _| async {
            let mut p = pages.lock().expect("test pages lock");
            let page = if p.is_empty() {
                Vec::new()
            } else {
                p.remove(0)
            };
            Ok((page, total))
        })
        .await
        .expect("fetch_all_pages should not fail");
        assert_eq!(items.len(), 80_000);
        assert_eq!(total_count, 80_000);
        assert!(
            items.len() > 50_000,
            "old code would have truncated at 50000"
        );
    }

    /// An empty table draws nothing — the caller renders its action-copy
    /// fallback rather than a bogus row.
    #[tokio::test]
    async fn draw_random_row_returns_none_for_an_empty_table() {
        let drawn: Option<i32> = draw_random_row("test", |_offset, _limit| async {
            Ok((Vec::new(), Some(0)))
        })
        .await
        .expect("an empty table is not an error");
        assert!(drawn.is_none());
    }

    /// The uniform path: the probe reports the true total, so the second fetch
    /// asks for a single row at a random offset inside `0..total`.
    ///
    /// Repeated, because the helper uses real randomness: a single draw lands on
    /// the `offset == 0` shortcut once every `TOTAL` runs and would then assert
    /// nothing about the two-request path. Over 32 draws that path is exercised
    /// with certainty for practical purposes, and the loop doubles as a spread
    /// check — a draw pinned to one row (the missing-header degradation) fails it.
    #[tokio::test]
    async fn draw_random_row_fetches_one_row_at_a_random_offset() {
        use std::{collections::HashSet, sync::Mutex};

        const TOTAL: u32 = 500;
        let mut distinct = HashSet::new();
        let mut saw_offset_page = false;

        for _ in 0..32 {
            let calls: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
            let drawn = draw_random_row("test", |offset, limit| {
                calls.lock().expect("test lock").push((offset, limit));
                async move { Ok((vec![offset as i32], Some(TOTAL))) }
            })
            .await
            .expect("draw must succeed")
            .expect("a non-empty table always draws a row");

            let calls = calls.lock().expect("test lock");
            assert_eq!(calls[0], (0, 1), "the probe is a 1-row page at offset 0");
            assert!(calls.len() <= 2, "at most two requests per draw");
            if let Some(&(offset, limit)) = calls.get(1) {
                saw_offset_page = true;
                assert_eq!(limit, 1, "the draw fetches exactly one row");
                assert!(
                    offset > 0 && (offset as u32) < TOTAL,
                    "the offset stays inside the reported total"
                );
                assert_eq!(drawn, offset as i32, "the drawn row is the offset row");
            } else {
                assert_eq!(drawn, 0, "offset 0 reuses the probe row, no second request");
            }
            distinct.insert(drawn);
        }

        assert!(
            saw_offset_page,
            "32 draws over a 500-row table must exercise the offset-page path"
        );
        assert!(
            distinct.len() > 1,
            "the draw must spread across the table, not pin to one row (got {distinct:?})"
        );
    }

    /// A missing `X-Total-Count` (a proxy stripping non-standard response
    /// headers) leaves the table size unknown. The draw must still produce the
    /// probe row rather than collapsing to `None` and deadening the feature.
    #[tokio::test]
    async fn draw_random_row_without_a_total_falls_back_to_the_probe_row() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let drawn = draw_random_row("test", |_offset, _limit| {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Ok((vec![7], None)) }
        })
        .await
        .expect("a missing header is a degradation, not an error");
        assert_eq!(drawn, Some(7));
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "no uniform offset to draw — the second request is skipped"
        );
    }

    /// The draw sort must never be a server-side random sort — that is the entire
    /// premise of the count-probe draw. Flipping [`RANDOM_DRAW_SORT`] to
    /// `"random"` would reinstate the `resetSeededRandom` corruption with every
    /// other test still green, so pin it here.
    #[test]
    fn random_draw_sort_is_not_a_server_random_sort() {
        use crate::services::api::sort::{self, SortDomain};

        let (sort_mode, order) = RANDOM_DRAW_SORT;
        assert_ne!(
            sort_mode, "random",
            "the count-probe draw requires a STABLE sort"
        );
        for domain in [SortDomain::Albums, SortDomain::Artists] {
            assert_ne!(
                sort::map_sort_mode(domain, sort_mode),
                "random",
                "{domain:?}: the draw sort must not reach the server as _sort=random"
            );
        }
        assert_eq!(order, "ASC", "a deterministic order keeps offsets stable");
    }

    /// A transient failure on the offset page must not throw away the probe row
    /// the helper already holds; losing the whole pick to one 500 would blank
    /// the row until the next load.
    #[tokio::test]
    async fn draw_random_row_survives_a_failed_offset_page() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let drawn = draw_random_row("test", |_offset, _limit| {
            let n = calls.fetch_add(1, Ordering::Relaxed);
            async move {
                if n == 0 {
                    Ok((vec![42], Some(1_000)))
                } else {
                    Err(anyhow::anyhow!("transient 500"))
                }
            }
        })
        .await
        .expect("a failed offset page degrades, it does not propagate");
        assert_eq!(drawn, Some(42), "the probe row is still a valid draw");
    }
}
