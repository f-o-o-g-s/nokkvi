//! Composable metadata pill builders
//!
//! Reusable row-level building blocks for artwork panel metadata overlays.
//! Each function returns `Option<Element>` — `None` when there's nothing to show,
//! letting callers chain with `if let Some(row) = ... { col = col.push(row); }`.

use iced::{Element, widget::text};

use crate::theme;

/// "Favorited ♥ • Rated ★★★★★" row.
///
/// Returns `None` when `!is_starred && rating <= 0`.
pub(crate) fn auth_status_row<'a, M: 'a>(
    is_starred: bool,
    rating: Option<u32>,
) -> Option<Element<'a, M>> {
    let mut items: Vec<Element<'a, M>> = Vec::new();

    if is_starred {
        let heart = crate::embedded_svg::svg_widget("assets/icons/heart-filled.svg")
            .width(13)
            .height(13)
            .style(|_, _| iced::widget::svg::Style {
                color: Some(theme::danger_bright()),
            });
        items.push(
            iced::widget::row![
                text("Favorited ")
                    .size(13)
                    .color(theme::fg2())
                    .font(theme::ui_font()),
                heart
            ]
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }

    if let Some(r) = rating
        && r > 0
    {
        let mut stars_row = iced::widget::row![
            text("Rated ")
                .size(13)
                .color(theme::fg2())
                .font(theme::ui_font())
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center);

        for _ in 0..r {
            stars_row = stars_row.push(
                crate::embedded_svg::svg_widget("assets/icons/star-filled.svg")
                    .width(13)
                    .height(13)
                    .style(|_, _| iced::widget::svg::Style {
                        color: Some(theme::star_bright()),
                    }),
            );
        }
        items.push(stars_row.into());
    }

    if items.is_empty() {
        return None;
    }

    let mut row = iced::widget::row![]
        .spacing(12)
        .align_y(iced::Alignment::Center);
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            row = row.push(text("•").size(13).color(theme::fg3()));
        }
        row = row.push(item);
    }
    Some(row.into())
}

/// "N plays • Last played: YYYY-MM-DD" row.
///
/// Returns `None` when both fields are absent.
pub(crate) fn play_stats_row<'a, M: 'a>(
    play_count: Option<u32>,
    play_date: Option<&str>,
) -> Option<Element<'a, M>> {
    let mut items = Vec::new();
    if let Some(plays) = play_count {
        items.push(format!("{plays} plays"));
    }
    if let Some(date) = play_date {
        let ymd = date.split('T').next().unwrap_or(date);
        items.push(format!("Last played: {ymd}"));
    }
    if items.is_empty() {
        return None;
    }
    Some(
        text(items.join(" • "))
            .size(13)
            .color(theme::fg2())
            .font(theme::ui_font())
            .into(),
    )
}

/// "FLAC • 16-bit • 44.1 kHz • 900 kbps • 120 BPM" tech specs row.
///
/// Returns `None` when all fields are absent or zero.
pub(crate) fn tech_specs_row<'a, M: 'a>(
    suffix: Option<&str>,
    bit_depth: Option<u32>,
    sample_rate: Option<u32>,
    bitrate: Option<u32>,
    bpm: Option<u32>,
) -> Option<Element<'a, M>> {
    let mut specs = Vec::new();
    if let Some(s) = suffix {
        specs.push(s.to_uppercase());
    }
    if let Some(depth) = bit_depth
        && depth > 0
    {
        specs.push(format!("{depth}-bit"));
    }
    if let Some(rate) = sample_rate
        && rate > 0
    {
        specs.push(format!("{} kHz", rate as f32 / 1000.0));
    }
    if let Some(br) = bitrate
        && br > 0
    {
        specs.push(format!("{br} kbps"));
    }
    if let Some(b) = bpm {
        specs.push(format!("{b} BPM"));
    }
    if specs.is_empty() {
        return None;
    }
    Some(
        text(specs.join(" • "))
            .size(12)
            .color(theme::fg3())
            .font(theme::ui_font())
            .into(),
    )
}

/// Generic dot-separated text row at a given size and color.
///
/// Returns `None` when `items` is empty.
pub(crate) fn dot_row<'a, M: 'a>(
    items: Vec<String>,
    size: f32,
    color: iced::Color,
) -> Option<Element<'a, M>> {
    if items.is_empty() {
        return None;
    }
    Some(
        text(items.join(" • "))
            .size(size)
            .color(color)
            .font(theme::ui_font())
            .into(),
    )
}
