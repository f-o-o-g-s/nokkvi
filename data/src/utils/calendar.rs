//! Calendar grid geometry for the rules-editor date picker.
//!
//! Iced-free (data crate): the UI renders the returned plain values without
//! ever importing chrono. Dates are the zero-padded `YYYY-MM-DD` literals the
//! smart-rules criteria use — the same form
//! [`crate::types::smart_criteria::is_valid_date_literal`] enforces, so a value
//! this module formats always passes validation.

use chrono::{Datelike, Duration, Local, NaiveDate};

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Sunday-first month geometry for rendering a 7-column day grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthGrid {
    pub year: i32,
    pub month: u32,
    pub month_name: &'static str,
    /// Empty leading cells before day 1 (0 = the month starts on a Sunday).
    pub leading_blanks: u32,
    pub days_in_month: u32,
}

/// Days in `month` of `year`, leap-year aware. Mirrors the hand-rolled cap in
/// `is_valid_date_literal` so the two date paths agree; returns 0 for an
/// out-of-range month rather than panicking.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

/// Geometry for the given month, for the day-grid renderer.
pub fn month_grid(year: i32, month: u32) -> MonthGrid {
    let leading_blanks =
        NaiveDate::from_ymd_opt(year, month, 1).map_or(0, |d| d.weekday().num_days_from_sunday());
    MonthGrid {
        year,
        month,
        month_name: MONTH_NAMES
            .get(month.wrapping_sub(1) as usize)
            .copied()
            .unwrap_or(""),
        leading_blanks,
        days_in_month: days_in_month(year, month),
    }
}

/// Step the displayed month one forward/back, wrapping the year.
pub fn shift_month(year: i32, month: u32, forward: bool) -> (i32, u32) {
    if forward {
        if month >= 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        }
    } else if month <= 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

/// Move a date by `delta` days, rolling across month/year boundaries. Returns
/// the input unchanged if the arithmetic leaves chrono's representable range.
pub fn add_days(year: i32, month: u32, day: u32, delta: i64) -> (i32, u32, u32) {
    NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.checked_add_signed(Duration::days(delta)))
        .map_or((year, month, day), |d| (d.year(), d.month(), d.day()))
}

/// Clamp `day` to the last valid day of `month` — used after a month step keeps
/// a day index the shorter month lacks (e.g. Jan 31 → Feb).
pub fn clamp_day(year: i32, month: u32, day: u32) -> u32 {
    day.clamp(1, days_in_month(year, month).max(1))
}

/// Today in the local timezone as `(year, month, day)`.
pub fn today_ymd() -> (i32, u32, u32) {
    let t = Local::now().date_naive();
    (t.year(), t.month(), t.day())
}

/// Parse a `YYYY-MM-DD` literal into `(year, month, day)` — `None` if it is not
/// a real calendar date. Used to seed the picker from a leaf's current value;
/// an empty or malformed value falls back to today at the call site.
pub fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let d = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
    Some((d.year(), d.month(), d.day()))
}

/// Format `(year, month, day)` as the zero-padded `YYYY-MM-DD` literal the
/// criteria wire (and `is_valid_date_literal`) require.
pub fn format_ymd(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_in_month_covers_lengths_and_leap_years() {
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29, "divisible-by-4 leap year");
        assert_eq!(days_in_month(2000, 2), 29, "divisible-by-400 leap year");
        assert_eq!(days_in_month(1900, 2), 28, "century non-leap year");
        assert_eq!(days_in_month(2026, 13), 0, "out-of-range month");
    }

    #[test]
    fn month_grid_reports_the_leading_blank_count() {
        // 2026-07-01 is a Wednesday → 3 leading blanks (Sun,Mon,Tue).
        let g = month_grid(2026, 7);
        assert_eq!(g.leading_blanks, 3);
        assert_eq!(g.days_in_month, 31);
        assert_eq!(g.month_name, "July");
        // 2026-02-01 is a Sunday → 0 leading blanks.
        assert_eq!(month_grid(2026, 2).leading_blanks, 0);
    }

    #[test]
    fn shift_month_wraps_the_year() {
        assert_eq!(shift_month(2026, 12, true), (2027, 1));
        assert_eq!(shift_month(2026, 1, false), (2025, 12));
        assert_eq!(shift_month(2026, 7, true), (2026, 8));
        assert_eq!(shift_month(2026, 7, false), (2026, 6));
    }

    #[test]
    fn add_days_rolls_across_boundaries() {
        assert_eq!(add_days(2026, 7, 31, 1), (2026, 8, 1));
        assert_eq!(add_days(2026, 1, 1, -1), (2025, 12, 31));
        assert_eq!(add_days(2024, 2, 28, 1), (2024, 2, 29), "leap day");
        assert_eq!(add_days(2026, 7, 15, -7), (2026, 7, 8));
    }

    #[test]
    fn clamp_day_snaps_into_the_month() {
        assert_eq!(clamp_day(2026, 2, 31), 28, "Feb has no 31st");
        assert_eq!(clamp_day(2024, 2, 31), 29, "leap Feb caps at 29");
        assert_eq!(clamp_day(2026, 7, 15), 15, "an in-range day is untouched");
        assert_eq!(clamp_day(2026, 7, 0), 1, "never below day 1");
    }

    #[test]
    fn parse_and_format_round_trip() {
        assert_eq!(parse_ymd("2026-07-18"), Some((2026, 7, 18)));
        assert_eq!(parse_ymd(""), None);
        assert_eq!(parse_ymd("garbage"), None);
        assert_eq!(format_ymd(2026, 7, 18), "2026-07-18");
        assert_eq!(format_ymd(26, 3, 1), "0026-03-01", "zero-padded");
    }
}
