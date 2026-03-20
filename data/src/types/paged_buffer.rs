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

/// How many items from the edge of loaded data before triggering a prefetch.
/// With PAGE_SIZE=500, this means we start fetching the next page when the
/// viewport is within 100 items of the boundary.
const FETCH_THRESHOLD: usize = 100;

/// A windowed buffer that loads data from the server in pages.
///
/// Items are stored contiguously from index 0, with new pages appended
/// as the user scrolls. `total_count` tracks the server's full count
/// (from X-Total-Count header) so the UI can display accurate totals
/// and the slot list can report the correct list length.
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
}

impl<T> PagedBuffer<T> {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            total_count: 0,
            loading: false,
            loading_since: None,
        }
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
    /// is within `FETCH_THRESHOLD` items of the edge of loaded data
    /// and there are more items on the server.
    ///
    /// Returns `None` if no fetch is needed (data already loaded,
    /// already fetching, or all data loaded).
    pub fn needs_fetch(&self, viewport_offset: usize) -> Option<(usize, usize)> {
        // Don't fetch if already loading or fully loaded
        if self.loading || self.fully_loaded() {
            return None;
        }

        let loaded = self.items.len();

        // Trigger fetch when viewport is within FETCH_THRESHOLD of the edge
        if loaded == 0 || viewport_offset + FETCH_THRESHOLD >= loaded {
            let start = loaded;
            let end = (loaded + PAGE_SIZE).min(self.total_count);
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
    }

    /// Append a page of items to the buffer.
    /// Used when loading subsequent pages as the user scrolls.
    pub fn append_page(&mut self, items: Vec<T>, total_count: usize) {
        self.total_count = total_count;
        self.items.extend(items);
        self.loading = false;
        self.loading_since = None;
    }

    /// Clear all loaded data. Used when sort/search params change.
    pub fn clear(&mut self) {
        self.items.clear();
        self.total_count = 0;
        self.loading = false;
        self.loading_since = None;
    }

    /// Get a mutable reference to an item by index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.items.get_mut(index)
    }

    /// Get a mutable iterator over all loaded items.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.items.iter_mut()
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

        // Viewport at 450 — within FETCH_THRESHOLD (100) of edge (500)
        let result = buf.needs_fetch(450);
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
        assert!(buf.needs_fetch(100).is_none());
    }

    #[test]
    fn needs_fetch_returns_none_when_fully_loaded() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        buf.set_first_page(vec![1, 2, 3], 3);

        assert!(buf.needs_fetch(2).is_none());
    }

    #[test]
    fn needs_fetch_returns_none_when_loading() {
        let mut buf: PagedBuffer<u32> = PagedBuffer::new();
        let items: Vec<u32> = (0..500).collect();
        buf.set_first_page(items, 2000);
        buf.set_loading(true);

        assert!(buf.needs_fetch(499).is_none());
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

        let result = buf.needs_fetch(0);
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
}
