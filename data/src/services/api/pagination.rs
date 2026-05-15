//! Shared pagination helpers for Navidrome `_start` / `_end` query parameters.
//!
//! Navidrome uses `_start` and `_end` (inclusive-exclusive index range) rather
//! than `_offset` / `_limit`. The "no practical limit" sentinel is 999_999 —
//! large enough that no realistic Navidrome library has ever exceeded it.
//!
//! Callers that paginate by page take `paged_range(offset, limit)` and push
//! the returned strings into their params vec. Callers that always want the
//! cap can push `("_end", NO_LIMIT_END_STR)` directly.

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
}
