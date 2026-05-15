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
}
