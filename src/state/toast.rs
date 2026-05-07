//! In-app toast notification ring buffer with render-time expiry.

use std::collections::VecDeque;

/// In-app notification state (bounded ring buffer, render-time expiry)
///
/// Follows rmpc's `StatusMessage` pattern: no GC pass needed — `current()`
/// checks expiry at render time.
#[derive(Debug, Clone, Default)]
pub struct ToastState {
    pub toasts: VecDeque<nokkvi_data::types::toast::Toast>,
}

impl ToastState {
    /// Maximum active toasts before oldest is evicted
    const MAX_TOASTS: usize = 10;

    /// Push a new toast. If the toast has a `key`, remove any existing toast
    /// with the same key and re-insert at the back (most-recent position).
    pub fn push(&mut self, toast: nokkvi_data::types::toast::Toast) {
        if let Some(ref key) = toast.key {
            // Remove existing keyed toast so the updated one lands at the back
            self.toasts.retain(|t| t.key.as_deref() != Some(key));
        }
        if self.toasts.len() >= Self::MAX_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    /// Remove a keyed toast by its key.
    pub fn dismiss_key(&mut self, key: &str) {
        self.toasts.retain(|t| t.key.as_deref() != Some(key));
    }

    /// Most recent non-expired toast (scans from back to find the first visible one)
    pub fn current(&self) -> Option<&nokkvi_data::types::toast::Toast> {
        self.toasts.iter().rev().find(|t| !t.is_expired())
    }
}
