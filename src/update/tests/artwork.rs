//! Tests for the version-aware artwork prefetch dedup gate (N17).
//!
//! Pins the invariant that a server-side cover change (a different
//! `updated_at`) makes an already-warmed `album_art` slot a genuine prefetch
//! MISS, so the passive mini-thumbnail path re-fetches the new cover instead
//! of serving the stale one for the rest of the session.

use std::collections::{HashMap, HashSet};

use crate::update::components::should_refetch;

/// Build a `(cached_ids, versions)` pair the way a prefetch tick would: the
/// version map is kept in lockstep with the `album_art` keys.
fn seeded() -> (Vec<String>, HashMap<String, Option<String>>) {
    let ids = vec!["al-1".to_string()];
    let mut versions = HashMap::new();
    versions.insert("al-1".to_string(), Some("v1".to_string()));
    (ids, versions)
}

#[test]
fn prefetch_dedup_treats_changed_updated_at_as_cache_miss() {
    let (ids, versions) = seeded();
    let cached: HashSet<&String> = ids.iter().collect();

    // Same version → already warm → NOT a refetch.
    assert!(
        !should_refetch(
            &cached,
            &versions,
            &"al-1".to_string(),
            &Some("v1".to_string())
        ),
        "unchanged cover must not re-fetch",
    );

    // Changed version → server cover changed → MUST refetch.
    assert!(
        should_refetch(
            &cached,
            &versions,
            &"al-1".to_string(),
            &Some("v2".to_string())
        ),
        "a changed updated_at must be treated as a cache miss",
    );
}

#[test]
fn prefetch_dedup_unknown_id_is_a_miss() {
    let (ids, versions) = seeded();
    let cached: HashSet<&String> = ids.iter().collect();

    assert!(
        should_refetch(
            &cached,
            &versions,
            &"al-unknown".to_string(),
            &Some("v1".to_string())
        ),
        "an id absent from album_art must always be a miss",
    );
}

#[test]
fn prefetch_dedup_evicted_handle_is_a_miss() {
    // Version recorded but the handle was evicted from album_art (capacity
    // skew): membership check, not just the version map, must catch it.
    let mut versions = HashMap::new();
    versions.insert("al-1".to_string(), Some("v1".to_string()));
    let cached: HashSet<&String> = HashSet::new(); // album_art is empty

    assert!(
        should_refetch(
            &cached,
            &versions,
            &"al-1".to_string(),
            &Some("v1".to_string())
        ),
        "a version hit whose album_art handle was evicted must re-fetch",
    );
}
