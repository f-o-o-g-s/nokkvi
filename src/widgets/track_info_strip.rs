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
    let format_split = super::format_info::format_audio_info_split(
        data.format_suffix,
        data.sample_rate as f32 / 1000.0,
        data.bitrate,
    );

    let title = data.title.to_string();
    let artist = data.artist.to_string();
    let album = data.album.to_string();

    // Helper: 1px vertical separator
    let info_sep = || -> Element<'a, M> { theme::vertical_separator(STRIP_HEIGHT - 2.0) };

    // Helper: labeled field (dimmed label: + scrolling value)
    let info_field = |label: &'static str,
                      value: String,
                      value_color: iced::Color|
     -> Element<'a, M> { info_field_widget(label, value, value_color) };

    // Build the 3-column layout:
    // [codec+kHz │] [fill] [│ title │ artist │ album │] [fill] [│ kbps]
    let mut info_row = iced::widget::Row::new()
        .spacing(6)
        .align_y(Alignment::Center);

    // LEFT: codec + sample rate
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
    // Wrapped in mouse_area so clicking navigates to queue.
    let mut center_row = iced::widget::Row::new()
        .spacing(6)
        .align_y(Alignment::Center);
    center_row = center_row.push(info_sep());
    center_row = center_row.push(info_field("title:", title, theme::now_playing_color()));
    center_row = center_row.push(info_sep());
    center_row = center_row.push(info_field("artist:", artist, theme::selected_color()));
    center_row = center_row.push(info_sep());
    center_row = center_row.push(info_field("album:", album, theme::fg2()));
    center_row = center_row.push(info_sep());

    let center_element: Element<'a, M> = if let Some(msg) = on_press {
        mouse_area(center_row).on_press(msg).into()
    } else {
        center_row.into()
    };
    info_row = info_row.push(center_element);

    // Fill spacer → right
    info_row = info_row.push(space().width(Length::Fill));

    // RIGHT: bitrate
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

/// Build a full strip with separator line above it.
pub(crate) fn track_info_strip_with_separator<'a, M: Clone + 'static>(
    data: &TrackInfoStripData<'_>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let strip = track_info_strip(data, on_press);
    let separator = theme::horizontal_separator(1.0);
    iced::widget::column![separator, strip].into()
}
