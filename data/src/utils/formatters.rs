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
