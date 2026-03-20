//! Toast notification types
//!
//! Simple in-app notification data model (Iced-free).
//! Inspired by rmpc's `StatusMessage` pattern.

use std::time::{Duration, Instant};

/// Severity level of a toast notification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// A single toast notification
#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub created: Instant,
    pub duration: Duration,
    /// Optional key for sticky / upsert toasts.
    /// When set, pushing a toast with the same key replaces the existing one.
    /// Keyed toasts don't auto-expire — they stay until explicitly dismissed.
    pub key: Option<String>,
    /// When true, text is right-aligned within the toast bar.
    pub right_aligned: bool,
}

impl Toast {
    /// Create a new toast with a default duration based on level
    pub fn new(message: impl Into<String>, level: ToastLevel) -> Self {
        let duration = match level {
            ToastLevel::Info | ToastLevel::Success => Duration::from_secs(3),
            ToastLevel::Warning => Duration::from_secs(5),
            ToastLevel::Error => Duration::from_secs(8),
        };
        Self {
            message: message.into(),
            level,
            created: Instant::now(),
            duration,
            key: None,
            right_aligned: false,
        }
    }

    /// Create a keyed (sticky) toast that uses upsert semantics.
    pub fn keyed(key: impl Into<String>, message: impl Into<String>, level: ToastLevel) -> Self {
        let mut t = Self::new(message, level);
        t.key = Some(key.into());
        t
    }

    /// Create a short-lived info toast (1.5s) — ideal for transient feedback
    /// like volume changes, mode toggles, etc.
    pub fn info_short(message: impl Into<String>) -> Self {
        let mut t = Self::new(message, ToastLevel::Info);
        t.duration = Duration::from_millis(1500);
        t
    }

    /// Builder: set right-aligned text rendering.
    pub fn right_aligned(mut self) -> Self {
        self.right_aligned = true;
        self
    }

    /// Render-time expiry check (no GC pass needed — checked at display time).
    /// Keyed toasts never auto-expire.
    pub fn is_expired(&self) -> bool {
        if self.key.is_some() {
            return false;
        }
        self.created.elapsed() >= self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_not_expired_immediately() {
        let toast = Toast::new("hello", ToastLevel::Info);
        assert!(!toast.is_expired());
    }

    #[test]
    fn toast_default_durations() {
        let info = Toast::new("", ToastLevel::Info);
        assert_eq!(info.duration, Duration::from_secs(3));

        let success = Toast::new("", ToastLevel::Success);
        assert_eq!(success.duration, Duration::from_secs(3));

        let warning = Toast::new("", ToastLevel::Warning);
        assert_eq!(warning.duration, Duration::from_secs(5));

        let error = Toast::new("", ToastLevel::Error);
        assert_eq!(error.duration, Duration::from_secs(8));
    }

    #[test]
    fn toast_expired_with_zero_duration() {
        let toast = Toast {
            message: "expired".to_string(),
            level: ToastLevel::Info,
            created: Instant::now() - Duration::from_secs(10),
            duration: Duration::from_secs(1),
            key: None,
            right_aligned: false,
        };
        assert!(toast.is_expired());
    }

    #[test]
    fn keyed_toast_never_expires() {
        let toast = Toast {
            message: "sticky".to_string(),
            level: ToastLevel::Info,
            created: Instant::now() - Duration::from_secs(999),
            duration: Duration::from_secs(0),
            key: Some("test_key".to_string()),
            right_aligned: false,
        };
        assert!(!toast.is_expired());
    }
}
