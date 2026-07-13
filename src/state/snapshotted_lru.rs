//! `SnapshottedLru<K, V>` — an LRU cache that maintains an automatically
//! refreshed read-only `HashMap` snapshot for `view()`-layer borrowing.
//!
//! Iced's `view()` is a pure `&self` function, but `LruCache::get` requires
//! `&mut self` (to bump recency). The pattern we want — render-pass code
//! borrowing a `HashMap<K, V>` from app state — is therefore incompatible
//! with a bare LRU. Historically `ArtworkState` solved this by carrying
//! parallel `LruCache<K, V>` + `HashMap<K, V>` fields and a manual
//! `refresh_*_snapshot()` method that callers had to invoke after every
//! `put()` / `pop()`. Forgetting that call left the view borrowing a stale
//! snapshot — a documented "silent" gotcha in `CLAUDE.md` and
//! `.claude/rules/gotchas.md`.
//!
//! This newtype collapses the pair so the pairing is structural: `put()` and
//! `pop()` refresh the snapshot inline, and the snapshot is exposed as a
//! `pub` field for direct borrowing. Read-only LRU operations (`peek`,
//! `iter`, `contains`, `len`) delegate straight to the inner cache without
//! touching recency or the snapshot.

use std::{collections::HashMap, hash::Hash, num::NonZeroUsize};

use lru::LruCache;

/// LRU cache that maintains a read-only `HashMap` snapshot for `view()`
/// borrowing.
///
/// Every mutation (`put` / `pop` / `get_touch`) refreshes the snapshot
/// automatically — callers can't forget the pairing. The snapshot is a `pub`
/// field rather than a method so existing field-style access patterns
/// (`state.cache.snapshot.get(&id)`, `&state.cache.snapshot`) work without
/// extra parentheses.
///
/// External code MUST NOT mutate `snapshot` directly; doing so would diverge
/// it from the LRU. Read-only access only.
pub struct SnapshottedLru<K, V> {
    lru: LruCache<K, V>,
    /// Read-only `HashMap` mirror of the LRU. Refreshed automatically on
    /// every mutating call. Borrow it for `view()`-layer rendering; do not
    /// insert/remove on it directly.
    pub snapshot: HashMap<K, V>,
}

impl<K: Eq + Hash + Clone, V: Clone> SnapshottedLru<K, V> {
    /// Construct an empty cache with the given LRU capacity.
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            lru: LruCache::new(capacity),
            snapshot: HashMap::new(),
        }
    }

    /// Insert `(key, value)`, evicting the oldest entry if the cache is at
    /// capacity, and refresh the snapshot. Returns the evicted value (if
    /// any), mirroring `LruCache::put`.
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        let evicted = self.lru.put(key, value);
        self.refresh_snapshot();
        evicted
    }

    /// Remove the entry for `key` (if present) and refresh the snapshot.
    pub fn pop(&mut self, key: &K) -> Option<V> {
        let removed = self.lru.pop(key);
        if removed.is_some() {
            self.refresh_snapshot();
        }
        removed
    }

    /// Read an entry without bumping LRU recency. Reads through the
    /// snapshot (which has the same contents as the LRU after every
    /// mutation), so no `&mut` is needed and no snapshot refresh is
    /// triggered. Matches `LruCache::peek`'s no-touch semantics.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.snapshot.get(key)
    }

    /// Read an entry AND bump LRU recency, then refresh the snapshot.
    /// Returns an owned clone because the underlying `LruCache::get` borrow
    /// would conflict with the subsequent snapshot rebuild. Only call when
    /// LRU recency actually matters for the workload; otherwise use
    /// `peek()`.
    pub fn get_touch(&mut self, key: &K) -> Option<V> {
        let value = self.lru.get(key).cloned();
        if value.is_some() {
            self.refresh_snapshot();
        }
        value
    }

    /// True if the cache contains `key`. Delegates to `LruCache::contains`.
    pub fn contains(&self, key: &K) -> bool {
        self.lru.contains(key)
    }

    /// Iterate over the LRU's `(&K, &V)` pairs in most-recent-first order.
    /// Delegates to `LruCache::iter`.
    pub fn iter(&self) -> lru::Iter<'_, K, V> {
        self.lru.iter()
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.lru.len()
    }

    /// True if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.lru.is_empty()
    }

    /// Rebuild the `snapshot` from the current LRU contents. Called
    /// automatically by every mutating method; not part of the public API
    /// surface (the whole point of the newtype is that callers never have
    /// to invoke this).
    fn refresh_snapshot(&mut self) {
        self.snapshot = self
            .lru
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }
}

impl<K, V> std::fmt::Debug for SnapshottedLru<K, V>
where
    K: Eq + Hash,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `LruCache::len()` is bounded on `K: Eq + Hash` (the underlying
        // hash map uses those operations), so we propagate the bound here.
        f.debug_struct("SnapshottedLru")
            .field("len", &self.lru.len())
            .finish()
    }
}

impl<K, V> Clone for SnapshottedLru<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        // `LruCache: Clone` requires `K: Hash + Eq + Clone, V: Clone`.
        Self {
            lru: self.lru.clone(),
            snapshot: self.snapshot.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    #[test]
    fn put_refreshes_snapshot() {
        let mut cache: SnapshottedLru<String, u32> = SnapshottedLru::new(cap(4));
        assert!(cache.snapshot.is_empty());

        cache.put("a".to_string(), 1);

        assert_eq!(cache.snapshot.get("a"), Some(&1));
        assert_eq!(cache.len(), 1);
        // Calling put a second time with a new key extends the snapshot
        // without any manual refresh — that's the whole point.
        cache.put("b".to_string(), 2);
        assert_eq!(cache.snapshot.get("b"), Some(&2));
        assert_eq!(cache.snapshot.len(), 2);
    }

    #[test]
    fn eviction_removes_from_snapshot() {
        // Fill the cache to capacity, then overflow by one and assert the
        // oldest entry is gone from BOTH the LRU and the snapshot. Catches
        // a regression where eviction would silently leave a stale entry
        // in the snapshot mirror.
        let mut cache: SnapshottedLru<String, u32> = SnapshottedLru::new(cap(2));
        cache.put("oldest".to_string(), 1);
        cache.put("middle".to_string(), 2);
        cache.put("newest".to_string(), 3);

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.snapshot.len(), 2);
        assert!(
            !cache.snapshot.contains_key("oldest"),
            "evicted key must disappear from the snapshot"
        );
        assert!(cache.snapshot.contains_key("middle"));
        assert!(cache.snapshot.contains_key("newest"));
    }

    #[test]
    fn get_touch_promotes_and_refreshes() {
        // get_touch must bump recency so the just-read entry survives the
        // next eviction. Without the touch the LRU would evict it next.
        let mut cache: SnapshottedLru<String, u32> = SnapshottedLru::new(cap(2));
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);

        // Touch "a" so it becomes the most-recently-used, demoting "b".
        let touched = cache.get_touch(&"a".to_string());
        assert_eq!(touched, Some(1));

        // Insert a third entry — "b" should be evicted, not "a".
        cache.put("c".to_string(), 3);
        assert!(cache.snapshot.contains_key("a"), "touched entry survived");
        assert!(!cache.snapshot.contains_key("b"), "untouched entry evicted");
        assert!(cache.snapshot.contains_key("c"));
    }

    #[test]
    fn clone_independence() {
        // After cloning, mutating the clone must not bleed into the
        // original — guards against accidentally sharing the underlying
        // LRU/HashMap via `Arc`.
        let mut original: SnapshottedLru<String, u32> = SnapshottedLru::new(cap(4));
        original.put("a".to_string(), 1);
        original.put("b".to_string(), 2);

        let mut clone = original.clone();
        clone.put("c".to_string(), 3);
        clone.pop(&"a".to_string());

        // Original is unchanged.
        assert_eq!(original.snapshot.len(), 2);
        assert!(original.snapshot.contains_key("a"));
        assert!(original.snapshot.contains_key("b"));
        assert!(!original.snapshot.contains_key("c"));

        // Clone reflects its own mutations.
        assert_eq!(clone.snapshot.len(), 2);
        assert!(!clone.snapshot.contains_key("a"));
        assert!(clone.snapshot.contains_key("b"));
        assert!(clone.snapshot.contains_key("c"));
    }

    #[test]
    fn pop_refreshes_snapshot() {
        // Covers the `pop()` arm — separate from eviction-driven removal.
        let mut cache: SnapshottedLru<String, u32> = SnapshottedLru::new(cap(4));
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);

        let popped = cache.pop(&"a".to_string());
        assert_eq!(popped, Some(1));
        assert!(!cache.snapshot.contains_key("a"));
        assert!(cache.snapshot.contains_key("b"));

        // Popping a missing key is a no-op for the snapshot.
        let missing = cache.pop(&"missing".to_string());
        assert_eq!(missing, None);
        assert_eq!(cache.snapshot.len(), 1);
    }
}
