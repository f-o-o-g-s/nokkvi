//! Track info strip — shared widget rendering now-playing metadata.
//!
//! Used by both the player bar (bottom info strip) and the top bar
//! (when `TrackInfoDisplay::TopBar` is active in side nav mode).
//!
//! Layout: `FLAC 44.1kHz │ │ title: xxx │ artist: xxx │ album: xxx │ │ 1411kbps`

use iced::{
    Alignment, Element, Length,
    font::{Font, Weight},
    widget::{container, mouse_area, row, space, text},
};

use crate::theme;

/// Height of the track info strip (including 1px separator).
pub(crate) const STRIP_HEIGHT: f32 = 21.0;

/// Strip height plus its 1px separator — used by chrome height calculations.
pub(crate) const STRIP_HEIGHT_WITH_SEPARATOR: f32 = STRIP_HEIGHT + 1.0;

/// Data needed to render the track info strip.
pub(crate) struct TrackInfoStripData<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub format_suffix: &'a str,
    pub sample_rate: u32,
    pub bitrate: u32,
    pub radio_name: Option<&'a str>,
    pub radio_url: Option<&'a str>,
    pub icy_artist: Option<&'a str>,
    pub icy_title: Option<&'a str>,
}

/// Labeled metadata field: dimmed label + scrolling marquee value.
///
/// Shared by `track_info_strip` (player bar / top bar strip) and `nav_bar`
/// (top nav mode) — single source of truth for the scrolling info field pattern.
pub(crate) fn info_field_widget<'a, M: 'static>(
    label: &'static str,
    value: String,
    value_color: iced::Color,
) -> Element<'a, M> {
    row![
        text(label)
            .size(9.0)
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            })
            .color(theme::fg4())
            .wrapping(text::Wrapping::None),
        super::marquee_text::marquee_text(value)
            .size(9.0)
            .font(theme::ui_font())
            .color(value_color),
    ]
    .spacing(3)
    .align_y(Alignment::Center)
    .width(Length::FillPortion(3))
    .into()
}

/// Build the track info strip element, generic over any message type.
///
/// Returns a padded row with codec/kHz left, track metadata center, kbps right,
/// separated by 1px vertical lines.
///
/// When `on_press` is `Some`, clicking the center metadata (title/artist/album)
/// emits the given message. The codec/sample-rate and bitrate sections remain
/// non-clickable.
pub(crate) fn track_info_strip<'a, M: Clone + 'static>(
    data: &TrackInfoStripData<'_>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let show_format = theme::strip_show_format_info();
    let format_split = if show_format {
        super::format_info::format_audio_info_split(
            data.format_suffix,
            data.sample_rate as f32 / 1000.0,
            data.bitrate,
        )
    } else {
        None
    };

    let title = data.title.to_string();
    let artist = data.artist.to_string();
    let album = data.album.to_string();

    let show_title = theme::strip_show_title();
    let show_artist = theme::strip_show_artist();
    let show_album = theme::strip_show_album();
    let merged_mode = theme::strip_merged_mode();
    let is_radio = data.radio_name.is_some();

    // Helper: 1px vertical separator
    let info_sep = || -> Element<'a, M> { theme::vertical_separator(STRIP_HEIGHT - 2.0) };

    // Helper: labeled field (dimmed label: + scrolling value)
    let info_field = |label: &'static str,
                      value: String,
                      value_color: iced::Color|
     -> Element<'a, M> { info_field_widget(label, value, value_color) };

    // Merged mode (non-radio): use a 3-column layout with equal-portion edge
    // columns so the metadata sits at the container's true horizontal center,
    // independent of asymmetric codec/kbps text widths.
    if merged_mode && !is_radio {
        let merged =
            merged_strip_string(show_title, show_artist, show_album, &title, &artist, &album);
        if !merged.is_empty() {
            return build_merged_centered_strip(merged, format_split, on_press);
        }
    }

    // Build the 3-column layout:
    // [codec+kHz │] [fill] [│ title │ artist │ album │] [fill] [│ kbps]
    let mut info_row = iced::widget::Row::new()
        .spacing(6)
        .align_y(Alignment::Center);

    // LEFT: codec + sample rate (gated by strip_show_format_info)
    if let Some((ref left, _)) = format_split {
        info_row = info_row.push(
            text(left.clone())
                .size(9.0)
                .font(Font {
                    weight: Weight::Medium,
                    ..theme::ui_font()
                })
                .color(theme::fg3())
                .wrapping(text::Wrapping::None),
        );
        info_row = info_row.push(info_sep());
    }

    // Fill spacer → center
    info_row = info_row.push(space().width(Length::Fill));

    // CENTER: │ title: │ artist: │ album: │
    // Each field is independently toggleable. Separators only between visible fields.
    let mut center_row = iced::widget::Row::new()
        .spacing(6)
        .align_y(Alignment::Center);

    if merged_mode {
        // Merged mode: one bookend pair around a single marquee that scrolls
        // all visible fields together as one unit.
        let merged =
            merged_strip_string(show_title, show_artist, show_album, &title, &artist, &album);
        if !merged.is_empty() {
            center_row = center_row.push(info_sep());
            center_row = center_row.push(
                iced::widget::row![
                    super::marquee_text::marquee_text(merged)
                        .size(9.0)
                        .font(theme::ui_font())
                        .color(theme::selected_color()),
                ]
                .align_y(Alignment::Center)
                .width(Length::FillPortion(9)),
            );
            center_row = center_row.push(info_sep());
        }
    } else {
        let mut has_prev_field = false;

        // Leading separator
        if show_title || show_artist || show_album {
            center_row = center_row.push(info_sep());
        }

        if show_title {
            center_row = center_row.push(info_field("title:", title, theme::now_playing_color()));
            has_prev_field = true;
        }
        if show_artist {
            if has_prev_field {
                center_row = center_row.push(info_sep());
            }
            center_row = center_row.push(info_field("artist:", artist, theme::selected_color()));
            has_prev_field = true;
        }
        if show_album {
            if has_prev_field {
                center_row = center_row.push(info_sep());
            }
            center_row = center_row.push(info_field("album:", album, theme::fg2()));
            has_prev_field = true;
        }

        // Trailing separator
        if has_prev_field {
            center_row = center_row.push(info_sep());
        }
    }

    if let Some(radio_name) = data.radio_name {
        // OVERRIDE: If radio mode is active, display radio station info
        center_row = iced::widget::Row::new()
            .spacing(6)
            .align_y(Alignment::Center);
        center_row = center_row.push(info_sep());

        let icon_widget = crate::embedded_svg::svg_widget("assets/icons/radio-tower.svg")
            .width(Length::Fixed(12.0))
            .height(Length::Fixed(12.0))
            .style(|_theme, _status| iced::widget::svg::Style {
                color: Some(theme::fg4()),
            });

        center_row = center_row.push(icon_widget);

        center_row = center_row.push(
            text(radio_name.to_string())
                .size(11.0)
                .font(Font {
                    weight: Weight::Bold,
                    ..theme::ui_font()
                })
                .color(theme::now_playing_color()),
        );

        let icy_title = data.icy_title.unwrap_or("");
        let icy_artist = data.icy_artist.unwrap_or("");

        if !icy_title.is_empty() {
            center_row = center_row.push(info_sep());
            center_row = center_row.push(info_field(
                "playing:",
                icy_title.to_string(),
                theme::accent_bright(),
            ));
        }

        if !icy_artist.is_empty() {
            center_row = center_row.push(info_sep());
            center_row = center_row.push(info_field(
                "artist:",
                icy_artist.to_string(),
                theme::selected_color(),
            ));
        }

        if icy_title.is_empty()
            && icy_artist.is_empty()
            && let Some(url) = data.radio_url
        {
            center_row = center_row.push(info_sep());
            center_row = center_row.push(info_field("url:", url.to_string(), theme::fg2()));
        }
        center_row = center_row.push(info_sep());
    }

    let center_element: Element<'a, M> = if let Some(msg) = on_press {
        mouse_area(center_row).on_press(msg).into()
    } else {
        center_row.into()
    };
    info_row = info_row.push(center_element);

    // Fill spacer → right
    info_row = info_row.push(space().width(Length::Fill));

    // RIGHT: bitrate (gated by strip_show_format_info)
    if let Some((_, Some(ref right))) = format_split {
        info_row = info_row.push(info_sep());
        info_row = info_row.push(
            text(right.clone())
                .size(9.0)
                .font(Font {
                    weight: Weight::Medium,
                    ..theme::ui_font()
                })
                .color(theme::fg3())
                .wrapping(text::Wrapping::None),
        );
    }

    container(info_row.padding([0, 8]))
        .width(Length::Fill)
        .height(Length::Fixed(STRIP_HEIGHT))
        .center_y(STRIP_HEIGHT)
        .style(move |_| container::Style {
            background: Some(theme::bg0_hard().into()),
            ..Default::default()
        })
        .into()
}

/// Merged-mode layout: a single row with shrink-sized codec/bitrate bookends
/// flanking a `Length::Fill` marquee. The marquee's scroll lane spans the
/// entire gap between the bookends, and `align_x: Center` keeps non-overflowing
/// text centered within that lane. Bookends are `Shrink`-sized so they never
/// clip on narrow windows.
fn build_merged_centered_strip<'a, M: Clone + 'static>(
    merged: String,
    format_split: Option<(String, Option<String>)>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let info_sep = || -> Element<'a, M> { theme::vertical_separator(STRIP_HEIGHT - 2.0) };

    let format_text = |s: String| {
        text(s)
            .size(9.0)
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            })
            .color(theme::fg3())
            .wrapping(text::Wrapping::None)
    };

    let marquee = iced::widget::row![
        super::marquee_text::marquee_text(merged)
            .size(9.0)
            .font(theme::ui_font())
            .color(theme::selected_color())
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let marquee_clickable: Element<'a, M> = if let Some(msg) = on_press {
        mouse_area(marquee).on_press(msg).into()
    } else {
        marquee.into()
    };

    let mut info_row = iced::widget::Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .padding([0, 8]);
    if let Some((ref left, _)) = format_split {
        info_row = info_row.push(format_text(left.clone()));
        info_row = info_row.push(info_sep());
    }
    info_row = info_row.push(marquee_clickable);
    if let Some((_, Some(ref right))) = format_split {
        info_row = info_row.push(info_sep());
        info_row = info_row.push(format_text(right.clone()));
    }

    container(info_row)
        .width(Length::Fill)
        .height(Length::Fixed(STRIP_HEIGHT))
        .center_y(STRIP_HEIGHT)
        .style(move |_| container::Style {
            background: Some(theme::bg0_hard().into()),
            ..Default::default()
        })
        .into()
}

/// Build a full strip with separator line above it.
pub(crate) fn track_info_strip_with_separator<'a, M: Clone + 'static>(
    data: &TrackInfoStripData<'_>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let strip = track_info_strip(data, on_press);
    let separator = theme::horizontal_separator(1.0);
    iced::widget::column![separator, strip].into()
}

/// Build the merged-mode metadata string for the center row.
///
/// Joins the visible fields with `  ·  ` and prefixes each with its label,
/// matching the labels used in the default per-field rendering. Hidden
/// fields are dropped; the resulting string contains no orphan separators.
pub(crate) fn merged_strip_string(
    show_title: bool,
    show_artist: bool,
    show_album: bool,
    title: &str,
    artist: &str,
    album: &str,
) -> String {
    const JOIN: &str = "  ·  ";
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if show_title {
        parts.push(format!("title: {title}"));
    }
    if show_artist {
        parts.push(format!("artist: {artist}"));
    }
    if show_album {
        parts.push(format!("album: {album}"));
    }
    parts.join(JOIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_string_all_three_visible() {
        let s = merged_strip_string(true, true, true, "T", "A", "L");
        assert_eq!(s, "title: T  ·  artist: A  ·  album: L");
    }

    #[test]
    fn merged_string_drops_hidden_fields_without_orphan_separators() {
        let s = merged_strip_string(true, false, true, "T", "_", "L");
        assert_eq!(s, "title: T  ·  album: L");

        let s = merged_strip_string(false, true, false, "_", "A", "_");
        assert_eq!(s, "artist: A");
    }

    #[test]
    fn merged_string_all_hidden_is_empty() {
        let s = merged_strip_string(false, false, false, "T", "A", "L");
        assert_eq!(s, "");
    }

    #[test]
    fn merged_string_only_title() {
        let s = merged_strip_string(true, false, false, "Only Title", "_", "_");
        assert_eq!(s, "title: Only Title");
    }
}
