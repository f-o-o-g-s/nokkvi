// Formatting utilities for display strings

pub fn format_duration(seconds: u32) -> String {
    let minutes = seconds / 60;
    let secs = seconds % 60;
    format!("{minutes}:{secs:02}")
}

/// Format seconds into a short human-readable string using the two most significant units.
///
/// Ported from Feishin's `formatDurationStringShort`.
///
/// # Examples
/// - `45.0` → `"45s"`
/// - `2576.0` → `"42m 56s"`
/// - `4320.0` → `"1h 12m"`
/// - `104400.0` → `"1d 5h"`
pub fn format_duration_short(seconds: f64) -> String {
    let total_secs = seconds as u64;

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

/// Format an ISO 8601 date string into a human-readable absolute date.
///
/// Output format: `"Mar 24, 1973"` (abbreviated month, day without leading zero, 4-digit year).
///
/// Handles both date-only (`"1973-03-24"`) and full timestamp (`"1973-03-24T00:00:00Z"`) inputs.
/// Returns the raw string unchanged if parsing fails, or empty string for empty input.
pub fn format_release_date(date_str: &str) -> String {
    if date_str.is_empty() {
        return String::new();
    }

    use chrono::NaiveDate;

    // Try date-only first (YYYY-MM-DD)
    let date_portion = date_str.split('T').next().unwrap_or(date_str);
    if let Ok(date) = NaiveDate::parse_from_str(date_portion, "%Y-%m-%d") {
        return date.format("%b %-d, %Y").to_string();
    }

    // Fallback: return raw string
    date_str.to_string()
}

pub fn format_time(seconds: u32) -> String {
    format_duration(seconds)
}

/// Format an ISO 8601 timestamp as a concise US date string.
///
/// Output format: `M/D/YY` (e.g., `1/15/24`)
/// Falls back to the date portion of the raw string if parsing fails.
pub fn format_date_concise(date_str: &str) -> String {
    use chrono::{DateTime, Local, Utc};

    // Try parsing as RFC 3339 / ISO 8601 (Navidrome uses this format)
    if let Ok(utc) = date_str.parse::<DateTime<Utc>>() {
        let local: DateTime<Local> = utc.into();
        return local.format("%-m/%-d/%y").to_string();
    }

    // Fallback: return the date portion before 'T'
    date_str.split('T').next().unwrap_or(date_str).to_string()
}

pub fn format_date(date_str: &str) -> Result<String, anyhow::Error> {
    Ok(format_date_concise(date_str))
}

/// Compute the new rating when a user clicks a star.
///
/// If the clicked rating matches the current rating, decrements by 1 (toggle off).
/// Otherwise, sets to the clicked rating.
///
/// # Examples
/// - Current 3, click 3 → 2 (toggle off)
/// - Current 3, click 5 → 5 (set new)
/// - Current 1, click 1 → 0 (toggle to zero)
/// - Current 0, click 0 → 0 (saturating, can't go below zero)
pub const fn compute_rating_toggle(current: usize, clicked: usize) -> usize {
    if clicked == current {
        clicked.saturating_sub(1)
    } else {
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_same_rating_decrements() {
        assert_eq!(compute_rating_toggle(3, 3), 2);
    }

    #[test]
    fn toggle_different_rating_sets_clicked() {
        assert_eq!(compute_rating_toggle(3, 5), 5);
    }

    #[test]
    fn toggle_rating_one_same_goes_to_zero() {
        assert_eq!(compute_rating_toggle(1, 1), 0);
    }

    #[test]
    fn toggle_zero_same_stays_zero() {
        // saturating_sub(1) on 0 should stay at 0
        assert_eq!(compute_rating_toggle(0, 0), 0);
    }

    #[test]
    fn toggle_zero_to_nonzero() {
        assert_eq!(compute_rating_toggle(0, 4), 4);
    }

    #[test]
    fn toggle_five_same_goes_to_four() {
        assert_eq!(compute_rating_toggle(5, 5), 4);
    }

    // =========================================================================
    // format_duration_short — Red-Green TDD
    // =========================================================================

    #[test]
    fn duration_short_seconds_only() {
        assert_eq!(format_duration_short(45.0), "45s");
    }

    #[test]
    fn duration_short_minutes_and_seconds() {
        assert_eq!(format_duration_short(2576.0), "42m 56s");
    }

    #[test]
    fn duration_short_exact_minutes() {
        assert_eq!(format_duration_short(120.0), "2m 0s");
    }

    #[test]
    fn duration_short_hours_and_minutes() {
        assert_eq!(format_duration_short(4320.0), "1h 12m");
    }

    #[test]
    fn duration_short_days_and_hours() {
        assert_eq!(format_duration_short(104400.0), "1d 5h");
    }

    #[test]
    fn duration_short_zero() {
        assert_eq!(format_duration_short(0.0), "0s");
    }

    #[test]
    fn duration_short_fractional_seconds() {
        // Fractional seconds should be truncated
        assert_eq!(format_duration_short(65.7), "1m 5s");
    }

    // =========================================================================
    // format_release_date — Red-Green TDD
    // =========================================================================

    #[test]
    fn release_date_full_iso() {
        assert_eq!(format_release_date("1973-03-24"), "Mar 24, 1973");
    }

    #[test]
    fn release_date_another_date() {
        assert_eq!(format_release_date("2023-11-05"), "Nov 5, 2023");
    }

    #[test]
    fn release_date_iso_with_time() {
        // Navidrome sometimes sends full timestamps
        assert_eq!(format_release_date("1973-03-24T00:00:00Z"), "Mar 24, 1973");
    }

    #[test]
    fn release_date_invalid_returns_raw() {
        // Graceful fallback for unparseable strings
        assert_eq!(format_release_date("unknown"), "unknown");
    }

    #[test]
    fn release_date_empty() {
        assert_eq!(format_release_date(""), "");
    }
}
