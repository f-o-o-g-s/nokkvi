// Formatting utilities for display strings

/// Current wall-clock time as an RFC 3339 string — the same shape Navidrome
/// emits for `updatedAt`/`createdAt`, so optimistic client-side bumps of
/// those fields keep parsing through [`format_date_concise`] and friends.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

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

/// Format a duration in seconds as `h:mm:ss` when hours > 0, otherwise `m:ss`.
///
/// Used by the Get Info modal for tracks and albums. Distinct from
/// [`format_duration`], which never wraps to hours (caps at `mmm:ss`).
pub fn format_duration_hms(total_secs: u32) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Format a byte count as a binary-prefixed file size (`KiB` / `MiB` / `GiB`).
///
/// - `≥ 1 GiB`: `"X.XX GiB"` (2 decimals)
/// - `≥ 1 MiB`: `"X.XX MiB"` (2 decimals)
/// - `≥ 1 KiB`: `"N KiB"` (truncated to whole KiB)
/// - otherwise: `"N B"` (raw bytes)
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GiB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KiB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Format an ISO 8601 timestamp as a relative time string ("3 months ago", "9 days ago").
///
/// Falls back to the raw string if parsing fails. Negative durations (future timestamps)
/// collapse to `"just now"`.
pub fn format_relative_time(timestamp: &str) -> String {
    use chrono::{DateTime, Datelike, FixedOffset, Utc};

    let parsed = timestamp
        .parse::<DateTime<FixedOffset>>()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| timestamp.parse::<DateTime<Utc>>());

    let Ok(dt) = parsed else {
        return timestamp.to_string();
    };

    let now = Utc::now();
    let duration = now.signed_duration_since(dt);

    if duration.num_seconds() < 0 {
        return "just now".to_string();
    }

    let seconds = duration.num_seconds();
    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_days();

    // Calendar-aware month/year calculation
    let months = {
        let y = (now.year() - dt.year()) * 12;
        let m = now.month() as i32 - dt.month() as i32;
        let total = y + m;
        // Adjust: if we haven't reached the same day-of-month yet, subtract 1
        if now.day() < dt.day() {
            (total - 1).max(0)
        } else {
            total.max(0)
        }
    };
    let years = months / 12;

    if seconds < 60 {
        "just now".to_string()
    } else if minutes < 60 {
        if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{minutes} minutes ago")
        }
    } else if hours < 24 {
        if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        }
    } else if days < 30 {
        if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{days} days ago")
        }
    } else if months < 12 {
        if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{months} months ago")
        }
    } else if years == 1 {
        "1 year ago".to_string()
    } else {
        format!("{years} years ago")
    }
}

/// Format a 0-5 star rating as filled and empty Unicode star glyphs.
///
/// Ratings > 5 saturate at 5.
pub fn format_rating(rating: u32) -> String {
    let filled = rating.min(5) as usize;
    let empty = 5 - filled;
    format!("{}{}", "★".repeat(filled), "☆".repeat(empty))
}

/// Return the Unicode check/cross glyph for a boolean — used by the info modal.
pub fn bool_icon(value: bool) -> String {
    if value { "✓" } else { "✗" }.to_string()
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

/// Age-aware "evaluated …" stamp for smart playlists: within the last 24 h
/// the LOCAL clock time ("evaluated 14:03"); older, the concise date form
/// ("evaluated 7/12/26"). A bare clock time on a value that is routinely
/// days old (Navidrome's lazy re-evaluation) would actively mislead — the
/// stale case is the NORM on browse surfaces. `None` when the timestamp
/// doesn't parse (render nothing rather than a lie).
pub fn format_evaluated_stamp(
    evaluated_at: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    use chrono::{DateTime, Local, Utc};
    let utc: DateTime<Utc> = evaluated_at.parse().ok()?;
    let age = now.signed_duration_since(utc);
    if age.num_hours() < 24 && age.num_seconds() > -60 {
        let local: DateTime<Local> = utc.into();
        Some(format!("evaluated {}", local.format("%H:%M")))
    } else {
        Some(format!("evaluated {}", format_date_concise(evaluated_at)))
    }
}

/// [`format_evaluated_stamp`] against the current wall clock — the UI-crate
/// entry point (chrono is a data-crate dependency only).
pub fn format_evaluated_stamp_now(evaluated_at: &str) -> Option<String> {
    format_evaluated_stamp(evaluated_at, chrono::Utc::now())
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

    /// The optimistic-bump helper must emit the RFC 3339 shape the rest of
    /// the date plumbing parses (concise-formatter round-trip + the naive
    /// `split('T')` consumers).
    #[test]
    fn now_rfc3339_round_trips_through_date_parsing() {
        let now = now_rfc3339();
        assert!(now.contains('T'), "must carry the ISO date/time separator");
        assert!(
            now.parse::<chrono::DateTime<chrono::Utc>>().is_ok(),
            "must parse back as RFC 3339: {now}"
        );
        let concise = format_date_concise(&now);
        assert!(
            concise.contains('/'),
            "concise formatter must take the parsed path, got: {concise}"
        );
    }

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

    // =========================================================================
    // format_duration_hms — moved here from info_modal.rs, pins exact output
    // =========================================================================

    #[test]
    fn duration_hms_under_hour_uses_m_ss() {
        assert_eq!(format_duration_hms(0), "0:00");
        assert_eq!(format_duration_hms(45), "0:45");
        assert_eq!(format_duration_hms(3599), "59:59");
    }

    #[test]
    fn duration_hms_at_hour_or_above_uses_h_mm_ss() {
        assert_eq!(format_duration_hms(3600), "1:00:00");
        assert_eq!(format_duration_hms(3661), "1:01:01");
        assert_eq!(format_duration_hms(7322), "2:02:02");
    }

    // =========================================================================
    // format_file_size — moved here from info_modal.rs, pins exact output
    // =========================================================================

    #[test]
    fn file_size_bytes_below_kib() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(1023), "1023 B");
    }

    #[test]
    fn file_size_kib_truncated() {
        // KiB tier uses integer division — no decimal places
        assert_eq!(format_file_size(1024), "1 KiB");
        assert_eq!(format_file_size(2048), "2 KiB");
        assert_eq!(format_file_size(2047), "1 KiB"); // truncates
    }

    #[test]
    fn file_size_mib_two_decimals() {
        assert_eq!(format_file_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_file_size(1024 * 1024 * 5 / 2), "2.50 MiB");
    }

    #[test]
    fn file_size_gib_two_decimals() {
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.00 GiB");
    }

    // =========================================================================
    // format_rating + bool_icon — moved here from info_modal.rs
    // =========================================================================

    #[test]
    fn rating_renders_filled_and_empty_stars() {
        assert_eq!(format_rating(0), "☆☆☆☆☆");
        assert_eq!(format_rating(3), "★★★☆☆");
        assert_eq!(format_rating(5), "★★★★★");
    }

    #[test]
    fn rating_saturates_above_five() {
        // Out-of-range ratings clamp at five (no overflow on the empty side)
        assert_eq!(format_rating(7), "★★★★★");
        assert_eq!(format_rating(100), "★★★★★");
    }

    #[test]
    fn bool_icon_renders_check_or_cross() {
        assert_eq!(bool_icon(true), "✓");
        assert_eq!(bool_icon(false), "✗");
    }

    // =========================================================================
    // format_relative_time — sanity checks on graceful fallback
    // =========================================================================

    #[test]
    fn relative_time_invalid_returns_raw() {
        assert_eq!(format_relative_time("not a date"), "not a date");
    }

    // =========================================================================
    // format_evaluated_stamp — the age-aware smart-playlist freshness stamp
    // =========================================================================

    /// 10 minutes old ⇒ clock form; 3 days old ⇒ dated form; garbage ⇒
    /// None (never a fake stamp).
    #[test]
    fn evaluated_stamp_is_age_aware() {
        use chrono::{Duration, Utc};
        let now = "2026-07-15T12:00:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();

        let fresh = (now - Duration::minutes(10)).to_rfc3339();
        let stamp = format_evaluated_stamp(&fresh, now).expect("fresh stamp");
        assert!(
            stamp.starts_with("evaluated ") && stamp.len() <= "evaluated 23:59".len(),
            "a minutes-old evaluation shows the clock form, got: {stamp}"
        );
        assert!(
            !stamp.contains('/'),
            "clock form carries no date, got: {stamp}"
        );

        let stale = (now - Duration::days(3)).to_rfc3339();
        let stamp = format_evaluated_stamp(&stale, now).expect("stale stamp");
        assert!(
            stamp.contains('/'),
            "a days-old evaluation shows the dated form, got: {stamp}"
        );

        assert_eq!(format_evaluated_stamp("not a date", now), None);
    }
}
