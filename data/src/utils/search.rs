//! Client-Side Search Filtering Utilities
//!
//! Provides reusable search/filter functionality for different data types.
//! Uses trait-based approach for type-safe, extensible filtering.

use std::borrow::Cow;

/// Trait for types that can be searched/filtered
///
/// Implement this trait to define which fields should be searchable
/// for a given data type.
pub trait Searchable {
    /// Returns a list of searchable field values for this item.
    /// Each string in the returned Vec will be checked for matches.
    fn searchable_fields(&self) -> Vec<&str>;
}

/// Filter a collection of items based on a search query.
///
/// Performs case-insensitive substring matching across all searchable fields.
/// Returns `Cow::Borrowed` when the query is empty (zero-cost passthrough),
/// or `Cow::Owned` with the filtered results when actively filtering.
///
/// # Arguments
/// * `items` - The collection to filter
/// * `query` - The search query string
///
/// # Returns
/// `Cow::Borrowed(items)` when query is empty, `Cow::Owned(filtered)` otherwise.
///
/// # Example
/// ```ignore
/// let filtered = filter_items(&albums, &search_query);
/// // Use as &[T] — Cow<[T]> derefs to &[T] transparently.
/// ```
pub fn filter_items<'a, T>(items: &'a [T], query: &str) -> Cow<'a, [T]>
where
    T: Searchable + Clone,
{
    // Empty query = borrow the original slice (zero allocations)
    if query.trim().is_empty() {
        return Cow::Borrowed(items);
    }

    let query_lower = query.to_lowercase();

    Cow::Owned(
        items
            .iter()
            .filter(|item| {
                // Check if any searchable field contains the query
                item.searchable_fields()
                    .iter()
                    .any(|field| field.to_lowercase().contains(&query_lower))
            })
            .cloned()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestItem {
        name: String,
        description: String,
    }

    impl Searchable for TestItem {
        fn searchable_fields(&self) -> Vec<&str> {
            vec![&self.name, &self.description]
        }
    }

    #[test]
    fn test_filter_items_empty_query() {
        let items = vec![TestItem {
            name: "Item 1".to_string(),
            description: "Description 1".to_string(),
        }];

        let result = filter_items(&items, "");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_items_case_insensitive() {
        let items = vec![TestItem {
            name: "Beatles".to_string(),
            description: "Rock band".to_string(),
        }];

        let result = filter_items(&items, "BEATLES");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_items_substring_match() {
        let items = vec![
            TestItem {
                name: "The Beatles".to_string(),
                description: "Rock".to_string(),
            },
            TestItem {
                name: "The Rolling Stones".to_string(),
                description: "Rock".to_string(),
            },
        ];

        let result = filter_items(&items, "beatles");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_items_multiple_fields() {
        let items = vec![TestItem {
            name: "Album 1".to_string(),
            description: "Beatles".to_string(),
        }];

        // Should match on description field
        let result = filter_items(&items, "beatles");
        assert_eq!(result.len(), 1);
    }

    /// Smoke test that locks down `filter_items` correctness on a 5,000-item
    /// synthetic library — the regression net for Phase 4A (lowercase-once
    /// rewrite) and Phase 4C (filter-result memoization).
    ///
    /// Asserts the filtered result contains exactly the items whose name or
    /// description match the query, case-insensitively. Phase 4 changes will
    /// keep this passing.
    #[test]
    fn filter_alloc_count_smoke_5000_items() {
        // Synthetic library: 5,000 items where every 100th has "BEATLES"
        // sprinkled into the description, so the matcher has 50 true hits.
        let items: Vec<TestItem> = (0..5_000)
            .map(|i| TestItem {
                name: format!("Album {i}"),
                description: if i % 100 == 0 {
                    format!("Beatles tribute {i}")
                } else {
                    format!("Filler description {i}")
                },
            })
            .collect();

        let result = filter_items(&items, "BEATLES");
        assert_eq!(result.len(), 50, "expected 50 matches in 5,000 items");
        assert!(
            result.iter().all(|it| it.description.contains("Beatles")),
            "every match must have 'Beatles' in description"
        );

        // Empty-query path stays zero-cost (Cow::Borrowed): same length, no clone.
        let passthrough = filter_items(&items, "");
        assert_eq!(passthrough.len(), 5_000);
        assert!(matches!(passthrough, std::borrow::Cow::Borrowed(_)));
    }
}
