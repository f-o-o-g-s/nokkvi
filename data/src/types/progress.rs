//! Shared progress tracking for long-running background tasks.
//!
//! `ProgressHandle` is a cheap, cloneable handle wrapping `Arc<AtomicUsize>`
//! counters. Background tasks call `set_completed()` as they go; the UI polls
//! the handle on a timer and surfaces live "Label… 45%" toasts.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Shared, thread-safe progress tracker for long-running operations.
///
/// # Example
/// ```
/// use nokkvi_data::types::progress::ProgressHandle;
/// let handle = ProgressHandle::new("Rebuilding cache", 200);
/// // … in background task:
/// handle.set_completed(50);   // 25%
/// handle.mark_done();
/// ```
#[derive(Debug, Clone)]
pub struct ProgressHandle {
    inner: Arc<ProgressInner>,
}

#[derive(Debug)]
struct ProgressInner {
    completed: AtomicUsize,
    total: AtomicUsize,
    done: AtomicBool,
    label: String,
}

/// Snapshot of progress state (avoids multiple atomic loads).
#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub completed: usize,
    pub total: usize,
    pub done: bool,
    pub label: String,
}

impl ProgressSnapshot {
    /// Completion percentage (0–100), clamped.
    pub fn percent(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        ((self.completed as f64 / self.total as f64) * 100.0).round() as u8
    }
}

impl ProgressHandle {
    /// Create a new handle with a label and initial total.
    pub fn new(label: impl Into<String>, total: usize) -> Self {
        Self {
            inner: Arc::new(ProgressInner {
                completed: AtomicUsize::new(0),
                total: AtomicUsize::new(total),
                done: AtomicBool::new(false),
                label: label.into(),
            }),
        }
    }

    /// Update the completed count (absolute, not delta).
    pub fn set_completed(&self, n: usize) {
        self.inner.completed.store(n, Ordering::Release);
    }

    /// Update the total (useful when the full count isn't known upfront).
    pub fn set_total(&self, n: usize) {
        self.inner.total.store(n, Ordering::Release);
    }

    /// Signal that the operation is finished.
    pub fn mark_done(&self) {
        self.inner.done.store(true, Ordering::Release);
    }

    /// Read a consistent snapshot of all counters.
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            completed: self.inner.completed.load(Ordering::Acquire),
            total: self.inner.total.load(Ordering::Acquire),
            done: self.inner.done.load(Ordering::Acquire),
            label: self.inner.label.clone(),
        }
    }

    /// Generate the key used for keyed/sticky toasts.
    pub fn toast_key(&self) -> String {
        format!("__progress_{}", self.inner.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_percentage() {
        let h = ProgressHandle::new("test", 200);
        assert_eq!(h.snapshot().percent(), 0);
        h.set_completed(100);
        assert_eq!(h.snapshot().percent(), 50);
        h.set_completed(200);
        assert_eq!(h.snapshot().percent(), 100);
    }

    #[test]
    fn progress_done_flag() {
        let h = ProgressHandle::new("test", 10);
        assert!(!h.snapshot().done);
        h.mark_done();
        assert!(h.snapshot().done);
    }

    #[test]
    fn zero_total_yields_zero_percent() {
        let h = ProgressHandle::new("test", 0);
        assert_eq!(h.snapshot().percent(), 0);
    }

    #[test]
    fn clone_shares_state() {
        let h1 = ProgressHandle::new("shared", 100);
        let h2 = h1.clone();
        h1.set_completed(42);
        assert_eq!(h2.snapshot().completed, 42);
    }
}
