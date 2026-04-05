// Formatting utilities for display strings

pub fn format_duration(seconds: u32) -> String {
    let minutes = seconds / 60;
    let secs = seconds % 60;
    format!("{minutes}:{secs:02}")
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
}
