//! Client-Side Search Filtering Utilities
//!
//! Provides reusable search/filter functionality for different data types.
//! Uses trait-based approach for type-safe, extensible filtering.
//!
//! ## Performance contract
//!
//! Implementors expose `matches_query`, which receives the query already
//! lowercased. Cached implementors (the big library types) hold a precomputed
//! `searchable_lower` field built once at construction time and check
//! `cached.contains(query_lower)`. Small uncached types (e.g. radio stations)
//! may compute on the fly. The hot filter path lowercases the query exactly
//! once and then defers to `matches_query`.

use std::borrow::Cow;

/// Trait for types that can be filtered by a search query.
///
/// `query_lower` is the user's query, already lowercased. Implementors that
/// hold a precomputed `searchable_lower` field check
/// `self.searchable_lower.contains(query_lower)`. Small uncached types may
/// build a temporary lowercase string and check against it.
pub trait Searchable {
    fn matches_query(&self, query_lower: &str) -> bool;
}

/// Build a single space-joined, lowercased searchable string from a slice of
/// field references. Use at view-data construction time:
///
/// ```ignore
/// Self {
///     name: name.clone(),
///     artist: artist.clone(),
///     searchable_lower: build_searchable_lower(&[&name, &artist]),
///     // ...
/// }
/// ```
pub fn build_searchable_lower<S: AsRef<str>>(parts: &[S]) -> String {
    let estimated: usize = parts.iter().map(|p| p.as_ref().len() + 1).sum();
    let mut out = String::with_capacity(estimated);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        for c in part.as_ref().chars() {
            for lower in c.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out
}

/// Filter a collection of items based on a search query.
///
/// Lowercases the query once and then defers to `Searchable::matches_query`
/// per item. Returns `Cow::Borrowed` when the query is empty (zero-cost
/// passthrough), or `Cow::Owned` with the filtered results otherwise.
pub fn filter_items<'a, T>(items: &'a [T], query: &str) -> Cow<'a, [T]>
where
    T: Searchable + Clone,
{
    if query.trim().is_empty() {
        return Cow::Borrowed(items);
    }

    let query_lower = query.to_lowercase();

    Cow::Owned(
        items
            .iter()
            .filter(|item| item.matches_query(&query_lower))
            .cloned()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestItem {
        description: String,
        searchable_lower: String,
    }

    impl TestItem {
        fn new(name: &str, description: &str) -> Self {
            Self {
                description: description.to_string(),
                searchable_lower: build_searchable_lower(&[name, description]),
            }
        }
    }

    impl Searchable for TestItem {
        fn matches_query(&self, query_lower: &str) -> bool {
            self.searchable_lower.contains(query_lower)
        }
    }

    #[test]
    fn build_lower_joins_and_lowercases() {
        let s = build_searchable_lower(&["The Beatles", "ROCK BAND"]);
        assert_eq!(s, "the beatles rock band");
    }

    #[test]
    fn build_lower_handles_unicode_case_folding() {
        let s = build_searchable_lower(&["BÉATLES"]);
        assert_eq!(s, "béatles");
    }

    #[test]
    fn test_filter_items_empty_query() {
        let items = vec![TestItem::new("Item 1", "Description 1")];
        let result = filter_items(&items, "");
        assert_eq!(result.len(), 1);
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn test_filter_items_case_insensitive() {
        let items = vec![TestItem::new("Beatles", "Rock band")];
        let result = filter_items(&items, "BEATLES");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_items_substring_match() {
        let items = vec![
            TestItem::new("The Beatles", "Rock"),
            TestItem::new("The Rolling Stones", "Rock"),
        ];
        let result = filter_items(&items, "beatles");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_items_multiple_fields() {
        let items = vec![TestItem::new("Album 1", "Beatles")];
        let result = filter_items(&items, "beatles");
        assert_eq!(result.len(), 1);
    }

    /// 5,000-item filter correctness regression net (carried over from Phase 0).
    /// Locks down that the precomputed `searchable_lower` produces identical
    /// match results to the prior per-field lowercase loop.
    #[test]
    fn filter_alloc_count_smoke_5000_items() {
        let items: Vec<TestItem> = (0..5_000)
            .map(|i| {
                if i % 100 == 0 {
                    TestItem::new(&format!("Album {i}"), &format!("Beatles tribute {i}"))
                } else {
                    TestItem::new(&format!("Album {i}"), &format!("Filler description {i}"))
                }
            })
            .collect();

        let result = filter_items(&items, "BEATLES");
        assert_eq!(result.len(), 50);
        assert!(result.iter().all(|it| it.description.contains("Beatles")));

        let passthrough = filter_items(&items, "");
        assert_eq!(passthrough.len(), 5_000);
        assert!(matches!(passthrough, std::borrow::Cow::Borrowed(_)));

        let no_match = filter_items(&items, "Album 99999");
        assert_eq!(no_match.len(), 0);
    }
}
