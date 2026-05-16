//! Generic paged data buffer for server-side pagination.
//!
//! `PagedBuffer<T>` replaces `Vec<T>` for views that load data from the API.
//! It tracks loaded items, total server count, and triggers page fetches
//! when the viewport nears the edge of loaded data.
//!
//! Implements `Deref<Target = [T]>` so existing code using `.get()`,
//! `.iter()`, `.len()`, `.is_empty()`, and indexing works unchanged.

use std::{
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

/// Default page size for API requests (items per page).
pub const PAGE_SIZE: usize = 500;

/// A windowed buffer that loads data from the server in pages.
///
/// Items are stored contiguously from index 0, with new pages appended
/// as the user scrolls. `total_count` tracks the server's full count
/// (from X-Total-Count header) so the UI can display accurate totals
/// and the slot list can report the correct list length.
///
/// The buffer also exposes a monotonically increasing `generation()`
/// counter that bumps on every method that mutates `items` or
/// `total_count`. Downstream caches (e.g. genres-view id→index map,
/// future filter-result memoization) snapshot the generation alongside
/// their derived data and rebuild only on mismatch. Every mutation site
/// MUST go through these methods; touching `items` directly via the
/// `DerefMut` slice impl will not bump the counter (only the items'
/// fields can change that way, not the slice's identity).
#[derive(Debug, Clone)]
pub struct PagedBuffer<T> {
    /// Loaded items (contiguous from offset 0, expanded as pages arrive)
    items: Vec<T>,
    /// Total count from X-Total-Count header (full server-side count)
    total_count: usize,
    /// Whether a page fetch is currently in progress
    loading: bool,
    /// When loading was set to true (for stale-load watchdog detection)
    loading_since: Option<Instant>,
    /// Mutation counter — bumped by every method that mutates `items`.
    generation: u64,
}

impl<T> PagedBuffer<T> {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            total_count: 0,
            loading: false,
            loading_since: None,
            generation: 0,
        }
    }

    /// Monotonically increasing mutation counter. Downstream caches keyed
    /// on `(query, generation)` rebuild whenever the generation moves.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[inline]
    fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Total items on the server (not just loaded items).
    /// This is the value from the `X-Total-Count` response header.
    pub fn total_count(&self) -> usize {
        self.total_count
    }

    /// Number of items currently loaded in the buffer.
    pub fn loaded_count(&self) -> usize {
        self.items.len()
    }

    /// Whether all items have been loaded from the server.
    pub fn fully_loaded(&self) -> bool {
        self.items.len() >= self.total_count
    }

    /// Whether a page fetch is in progress.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Mark a fetch as in progress.
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
        self.loading_since = if loading { Some(Instant::now()) } else { None };
    }

    /// Check whether loading has been stuck for longer than `timeout`.
    /// Used by the tick-handler watchdog to auto-clear stale loading states.
    pub fn is_stale_loading(&self, timeout: Duration) -> bool {
        self.loading
            && self
                .loading_since
                .is_some_and(|since| since.elapsed() > timeout)
    }

    /// Check if the viewport position requires fetching more data.
    ///
    /// Returns `Some((start, end))` pagination params if the viewport
    /// is within the dynamic threshold of the edge of loaded data
    /// and there are more items on the server.
    ///
    /// The threshold is calculated dynamically based on `page_size`.
    ///
    /// Returns `None` if no fetch is needed (data already loaded,
    /// already fetching, or all data loaded).
    pub fn needs_fetch(&self, viewport_offset: usize, page_size: usize) -> Option<(usize, usize)> {
        // Don't fetch if already loading or fully loaded
        if self.loading || self.fully_loaded() {
            return None;
        }

        let loaded = self.items.len();
        let fetch_threshold = (page_size / 5).clamp(20, 500);

        // Trigger fetch when viewport is within fetch_threshold of the edge
        if loaded == 0 || viewport_offset + fetch_threshold >= loaded {
            let start = loaded;
            let end = (loaded + page_size).min(self.total_count);
            if start < end {
                return Some((start, end));
            }
        }

        None
    }

    /// Replace all items with a fresh first page from the server.
    /// Used on initial load, sort changes, and search query changes.
    pub fn set_first_page(&mut self, items: Vec<T>, total_count: usize) {
        self.items = items;
        self.total_count = total_count;
        self.loading = false;
        self.loading_since = None;
        self.bump_generation();
    }

    /// Append a page of items to the buffer.
    /// Used when loading subsequent pages as the user scrolls.
    pub fn append_page(&mut self, items: Vec<T>, total_count: usize) {
        self.total_count = total_count;
        self.items.extend(items);
        self.loading = false;
        self.loading_since = None;
        self.bump_generation();
    }

    /// Clear all loaded data. Used when sort/search params change.
    pub fn clear(&mut self) {
        self.items.clear();
        self.total_count = 0;
        self.loading = false;
        self.loading_since = None;
        self.bump_generation();
    }

    /// Get a mutable reference to an item by index. Bumps the generation
    /// counter unconditionally — even if the index is out of bounds and
    /// `None` is returned. For find-then-mutate patterns where the
    /// mutation is conditional, prefer [`update_by`] which bumps only
    /// when the predicate matches.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.bump_generation();
        self.items.get_mut(index)
    }

    /// Get a mutable iterator over all loaded items. Bumps the generation
    /// counter unconditionally — even if the caller never actually
    /// mutates anything. For find-then-mutate-one patterns, prefer
    /// [`update_by`]; for unconditional batch mutation (e.g.
    /// `iter_mut().enumerate()` that touches every item), `iter_mut`
    /// is the right tool.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.bump_generation();
        self.items.iter_mut()
    }

    /// Find the first item matching `pred` and apply `f` to it. Returns
    /// `true` iff an item was found and the closure ran. Bumps the
    /// generation counter only when the predicate matches — unlike
    /// `iter_mut` / `get_mut`, a no-op `find()` does not invalidate
    /// downstream caches keyed on the generation.
    ///
    /// Prefer this over `iter_mut().find(|x| ...).map(...)` for the
    /// common find-then-mutate pattern. The closure is the caller's
    /// declared intent to mutate; whether the mutation is observably
    /// a no-op (e.g. assigning a field to its current value) is invisible
    /// to this method, so the generation bumps whenever the closure runs.
    pub fn update_by<P, F>(&mut self, pred: P, f: F) -> bool
    where
        P: Fn(&T) -> bool,
        F: FnOnce(&mut T),
    {
        if let Some(item) = self.items.iter_mut().find(|item| pred(item)) {
            f(item);
            self.bump_generation();
            true
        } else {
            false
        }
    }

    /// Direct access to the underlying Vec (for compatibility with
    /// code that needs `Vec<T>` specifically, like queue building).
    pub fn as_vec(&self) -> &Vec<T> {
        &self.items
    }

    /// Assign directly from a Vec (backwards compatibility for tests).
    pub fn set_from_vec(&mut self, items: Vec<T>) {
        let count = items.len();
        self.items = items;
        self.total_count = count;
        self.loading = false;
        self.loading_since = None;
        self.bump_generation();
    }
}

impl<T> Default for PagedBuffer<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `PagedBuffer<T>` transparently dereferences to `[T]`, so existing code
/// using `.get()`, `.iter()`, `.len()`, `.is_empty()`, and `[index]` works unchanged.
impl<T> Deref for PagedBuffer<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items
    }
}

/// Mutable deref allows `&mut PagedBuffer<T>` to coerce to `&mut [T]`,
/// enabling functions like `update_starred_in_list(&mut [T], ...)` to work unchanged.
impl<T> DerefMut for PagedBuffer<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_empty() {
        let buf: PagedBuffer<u32> = PagedBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.total_count(), 0);
        assert_eq!(buf.loaded_count(), 0);
        assert!(buf.fully_loaded()); // 0 loaded of 0 total
    }

    #[test]
    fn set_first_page() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3, 4, 5], 100);

        assert_eq!(buf.len(), 5);
        assert_eq!(buf.loaded_count(), 5);
        assert_eq!(buf.total_count(), 100);
        assert!(!buf.fully_loaded());
        assert!(!buf.is_loading());
    }

    #[test]
    fn append_page_extends_items() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 10);
        buf.append_page(vec![4, 5, 6], 10);

        assert_eq!(buf.len(), 6);
        assert_eq!(buf.total_count(), 10);
        assert_eq!(buf[0], 1);
        assert_eq!(buf[5], 6);
    }

    #[test]
    fn fully_loaded_when_all_fetched() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 3);

        assert!(buf.fully_loaded());
    }

    #[test]
    fn needs_fetch_near_edge() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        let items: Vec<u32> = (0..500).collect();
        buf.set_first_page(items, 2000);

        // Viewport at 450 — within threshold (implicit 100) of edge (500)
        let result = buf.needs_fetch(450, PAGE_SIZE);
        assert!(result.is_some());
        let (start, end) = result.expect("should need fetch");
        assert_eq!(start, 500);
        assert_eq!(end, 1000);
    }

    #[test]
    fn needs_fetch_returns_none_when_far_from_edge() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        let items: Vec<u32> = (0..500).collect();
        buf.set_first_page(items, 2000);

        // Viewport at 100 — far from edge
        assert!(buf.needs_fetch(100, PAGE_SIZE).is_none());
    }

    #[test]
    fn needs_fetch_returns_none_when_fully_loaded() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 3);

        assert!(buf.needs_fetch(2, PAGE_SIZE).is_none());
    }

    #[test]
    fn needs_fetch_returns_none_when_loading() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        let items: Vec<u32> = (0..500).collect();
        buf.set_first_page(items, 2000);
        buf.set_loading(true);

        assert!(buf.needs_fetch(499, PAGE_SIZE).is_none());
    }

    #[test]
    fn clear_resets_everything() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 100);
        buf.clear();

        assert!(buf.is_empty());
        assert_eq!(buf.total_count(), 0);
        assert!(buf.fully_loaded());
    }

    #[test]
    fn deref_provides_slice_operations() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![10, 20, 30], 3);

        // All these work via Deref<Target = [T]>
        assert_eq!(buf.len(), 3);
        assert!(!buf.is_empty());
        assert_eq!(buf.get(1), Some(&20));
        assert_eq!(buf.iter().sum::<u32>(), 60);
        assert_eq!(buf[2], 30);
    }

    #[test]
    fn get_mut_modifies_items() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 3);

        if let Some(item) = buf.get_mut(1) {
            *item = 42;
        }
        assert_eq!(buf[1], 42);
    }

    #[test]
    fn set_from_vec_sets_total_to_vec_len() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_from_vec(vec![1, 2, 3]);

        assert_eq!(buf.len(), 3);
        assert_eq!(buf.total_count(), 3);
        assert!(buf.fully_loaded());
    }

    #[test]
    fn needs_fetch_on_empty_buffer_with_nonzero_total() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.total_count = 1000; // Server knows there are items but none loaded

        let result = buf.needs_fetch(0, PAGE_SIZE);
        assert!(result.is_some());
        let (start, end) = result.expect("should need fetch");
        assert_eq!(start, 0);
        assert_eq!(end, 500);
    }

    #[test]
    fn is_stale_loading_false_when_not_loading() {
        let buf: PagedBuffer<u32> = PagedBuffer::new();
        assert!(!buf.is_stale_loading(Duration::from_secs(0)));
    }

    #[test]
    fn is_stale_loading_false_when_freshly_loading() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_loading(true);
        // Just started loading — not stale yet with a 30s timeout
        assert!(!buf.is_stale_loading(Duration::from_secs(30)));
    }

    #[test]
    fn is_stale_loading_true_after_zero_timeout() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_loading(true);
        // With zero timeout, immediately stale
        std::thread::sleep(Duration::from_millis(1));
        assert!(buf.is_stale_loading(Duration::from_secs(0)));
    }

    #[test]
    fn set_loading_false_clears_staleness() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_loading(true);
        std::thread::sleep(Duration::from_millis(1));
        buf.set_loading(false);
        assert!(!buf.is_stale_loading(Duration::from_secs(0)));
    }

    #[test]
    fn set_first_page_clears_loading_since() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_loading(true);
        buf.set_first_page(vec![1, 2, 3], 3);
        assert!(!buf.is_loading());
        assert!(!buf.is_stale_loading(Duration::from_secs(0)));
    }

    /// Generation must bump on every mutation method. The genres view's
    /// id→index map and any future filter-result cache trust this contract.
    #[test]
    fn generation_bumps_on_every_mutation() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        let g0 = buf.generation();

        buf.set_first_page(vec![1, 2, 3], 100);
        let g1 = buf.generation();
        assert!(g1 > g0, "set_first_page must bump generation");

        buf.append_page(vec![4, 5], 100);
        let g2 = buf.generation();
        assert!(g2 > g1, "append_page must bump generation");

        let _ = buf.get_mut(0);
        let g3 = buf.generation();
        assert!(g3 > g2, "get_mut must bump generation");

        let _ = buf.iter_mut();
        let g4 = buf.generation();
        assert!(g4 > g3, "iter_mut must bump generation");

        buf.set_from_vec(vec![10, 20]);
        let g5 = buf.generation();
        assert!(g5 > g4, "set_from_vec must bump generation");

        buf.clear();
        let g6 = buf.generation();
        assert!(g6 > g5, "clear must bump generation");
    }

    /// `set_loading` is not a content mutation — it must NOT bump the
    /// generation. A cache keyed on the generation should survive transient
    /// loading state flips.
    #[test]
    fn generation_does_not_bump_on_set_loading() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 100);
        let before = buf.generation();
        buf.set_loading(true);
        buf.set_loading(false);
        assert_eq!(buf.generation(), before);
    }

    /// `update_by` bumps the generation counter when the predicate
    /// matches and the closure runs.
    #[test]
    fn update_by_bumps_generation_on_match() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 100);
        let before = buf.generation();
        let matched = buf.update_by(|x| *x == 2, |x| *x = 42);
        assert!(matched, "predicate matched 2, closure must have run");
        assert!(buf.generation() > before, "match must bump generation");
        assert_eq!(buf[1], 42);
    }

    /// `update_by` does NOT bump the generation when no item matches
    /// the predicate — closing the spurious-cache-invalidation bug
    /// that motivated this API.
    #[test]
    fn update_by_does_not_bump_generation_on_miss() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 100);
        let before = buf.generation();
        let matched = buf.update_by(|x| *x == 999, |x| *x = 42);
        assert!(!matched, "no element matches 999");
        assert_eq!(buf.generation(), before, "miss must NOT bump generation");
    }

    /// `update_by` bumps once per call, regardless of how many items
    /// could match — only the first match is mutated.
    #[test]
    fn update_by_mutates_only_first_match() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![5, 5, 5], 100);
        let before = buf.generation();
        let matched = buf.update_by(|x| *x == 5, |x| *x = 99);
        assert!(matched);
        assert_eq!(buf.generation(), before + 1, "exactly one bump per call");
        assert_eq!(buf[0], 99);
        assert_eq!(buf[1], 5);
        assert_eq!(buf[2], 5);
    }

    /// Predicate matching is the bump trigger — even if the mutation
    /// closure is observably a no-op (writing the same value back),
    /// the generation still bumps. The closure is the caller's
    /// declared intent; introspecting it is impossible.
    #[test]
    fn update_by_bumps_when_closure_is_observable_noop() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 100);
        let before = buf.generation();
        let matched = buf.update_by(|x| *x == 2, |_| {}); // empty closure
        assert!(matched);
        assert_eq!(buf.generation(), before + 1, "predicate-match-always-bumps");
    }

    /// Existing iter_mut callers continue to bump generation
    /// unconditionally — `update_by` is additive and does not change
    /// the legacy semantics.
    #[test]
    fn iter_mut_still_bumps_generation_unconditionally() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 100);
        let before = buf.generation();
        // Consume but do NOT mutate
        for _ in buf.iter_mut() {
            // empty body
        }
        assert!(
            buf.generation() > before,
            "iter_mut must keep its eager-invalidation contract for legacy callers"
        );
    }

    /// Existing get_mut callers continue to bump generation even
    /// when the index is out of bounds (returns None).
    #[test]
    fn get_mut_still_bumps_generation_on_out_of_bounds() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 100);
        let before = buf.generation();
        let _ = buf.get_mut(999); // out of bounds → None
        assert!(
            buf.generation() > before,
            "get_mut must keep its eager-invalidation contract for legacy callers"
        );
    }
}
