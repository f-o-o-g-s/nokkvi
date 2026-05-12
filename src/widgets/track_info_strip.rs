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

/// Embedded-SVG path for the radio-tower glyph. Shared between the nav-tab row,
/// both metadata-strip widgets, and the radios view so a future rename touches
/// one site instead of five (the embedded-svg lookup falls back to play.svg on
/// a typo — see CLAUDE.md "Gotchas").
pub(crate) const RADIO_TOWER_ICON_PATH: &str = "assets/icons/radio-tower.svg";

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
///
/// When `label` is empty, the prefix collapses to a zero-width text node so
/// the row structure stays stable across the `show_labels` toggle (Iced keys
/// widget-tree state by position; structural changes can leave stale layout
/// cached until the surrounding tree is rebuilt).
pub(crate) fn info_field_widget<'a, M: 'static>(
    label: &'static str,
    value: String,
    value_color: iced::Color,
) -> Element<'a, M> {
    let label_widget = text(label)
        .size(10.0)
        .font(Font {
            weight: Weight::Medium,
            ..theme::ui_font()
        })
        .color(theme::fg4())
        .wrapping(text::Wrapping::None);

    // Drop the row's spacing when the label is hidden so the marquee doesn't
    // get a phantom 3px indent from the (now empty) label slot.
    let spacing = if label.is_empty() { 0 } else { 3 };

    row![
        label_widget,
        super::marquee_text::marquee_text(value)
            .size(10.0)
            .font(theme::ui_font())
            .color(value_color),
    ]
    .spacing(spacing)
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
    let show_labels = theme::strip_show_labels();
    let separator = theme::strip_separator();
    let merged_mode = theme::strip_merged_mode();
    let is_radio = data.radio_name.is_some();

    // Per-field label: empty string drops the dimmed `title:` / `artist:` /
    // `album:` prefix when the user has turned labels off.
    let title_label = if show_labels { "title:" } else { "" };
    let artist_label = if show_labels { "artist:" } else { "" };
    let album_label = if show_labels { "album:" } else { "" };

    // Helper: 1px vertical separator
    let info_sep = || -> Element<'a, M> { theme::vertical_separator(STRIP_HEIGHT - 2.0) };

    // Helper: labeled field (dimmed label: + scrolling value)
    let info_field = |label: &'static str,
                      value: String,
                      value_color: iced::Color|
     -> Element<'a, M> { info_field_widget(label, value, value_color) };

    // Merged mode: use a 3-column layout with equal-portion edge columns so the
    // metadata sits at the container's true horizontal center, independent of
    // asymmetric codec/kbps text widths. Radio takes a parallel path that
    // builds its string from station name + ICY fields and prepends the
    // radio-tower icon as a leading bookend.
    if merged_mode && is_radio {
        let radio_merged = merged_radio_strip_string(
            data.radio_name.unwrap_or(""),
            data.icy_title.unwrap_or(""),
            data.icy_artist.unwrap_or(""),
            data.radio_url.unwrap_or(""),
            show_labels,
            separator.as_join_str(),
        );
        if !radio_merged.is_empty() {
            return build_merged_centered_strip(
                radio_merged,
                format_split,
                on_press,
                Some(RADIO_TOWER_ICON_PATH),
            );
        }
    } else if merged_mode {
        let merged = merged_strip_string(
            show_title,
            show_artist,
            show_album,
            show_labels,
            separator.as_join_str(),
            &title,
            &artist,
            &album,
        );
        if !merged.is_empty() {
            return build_merged_centered_strip(merged, format_split, on_press, None);
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
                .size(10.0)
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
        let merged = merged_strip_string(
            show_title,
            show_artist,
            show_album,
            show_labels,
            separator.as_join_str(),
            &title,
            &artist,
            &album,
        );
        if !merged.is_empty() {
            center_row = center_row.push(info_sep());
            center_row = center_row.push(
                iced::widget::row![
                    super::marquee_text::marquee_text(merged)
                        .size(10.0)
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
            center_row =
                center_row.push(info_field(title_label, title, theme::now_playing_color()));
            has_prev_field = true;
        }
        if show_artist {
            if has_prev_field {
                center_row = center_row.push(info_sep());
            }
            center_row = center_row.push(info_field(artist_label, artist, theme::selected_color()));
            has_prev_field = true;
        }
        if show_album {
            if has_prev_field {
                center_row = center_row.push(info_sep());
            }
            center_row = center_row.push(info_field(album_label, album, theme::fg2()));
            has_prev_field = true;
        }

        // Trailing separator
        if has_prev_field {
            center_row = center_row.push(info_sep());
        }
    }

    if !merged_mode && let Some(radio_name) = data.radio_name {
        // OVERRIDE: If radio mode is active (and we're NOT in merged mode —
        // the merged path is handled by an early-return above), display radio
        // station info in the columnar layout.
        center_row = iced::widget::Row::new()
            .spacing(6)
            .align_y(Alignment::Center);
        center_row = center_row.push(info_sep());
        center_row = center_row.push(super::nav_bar::colored_icon(
            RADIO_TOWER_ICON_PATH,
            12.0,
            theme::fg4(),
        ));

        center_row = center_row.push(
            text(radio_name.to_string())
                .size(12.0)
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
                .size(10.0)
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
///
/// `leading_icon`, when `Some(path)`, prepends a 12px colored SVG glyph
/// immediately after the codec bookend (or at the row start when codec info
/// is hidden). Used by the radio render path to preserve the radio-tower
/// visual cue inside the merged marquee.
fn build_merged_centered_strip<'a, M: Clone + 'static>(
    merged: String,
    format_split: Option<(String, Option<String>)>,
    on_press: Option<M>,
    leading_icon: Option<&'static str>,
) -> Element<'a, M> {
    let info_sep = || -> Element<'a, M> { theme::vertical_separator(STRIP_HEIGHT - 2.0) };

    let format_text = |s: String| {
        text(s)
            .size(10.0)
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            })
            .color(theme::fg3())
            .wrapping(text::Wrapping::None)
    };

    let marquee = iced::widget::row![
        super::marquee_text::marquee_text(merged)
            .size(10.0)
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
    if let Some(icon_path) = leading_icon {
        info_row = info_row.push(super::nav_bar::colored_icon(icon_path, 12.0, theme::fg4()));
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

/// Kind of fragment in a [`MetadataSegment`] list — used by renderers that
/// want different visual treatment for labels vs values vs separators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MetadataSegmentKind {
    Label,
    Value,
    Separator,
}

/// One ordered fragment of the now-playing metadata display: a dimmed label
/// (e.g. `"title: "`), a colored value (the title text itself), or a separator
/// joining two visible fields. Single source of truth for the merged-mode
/// strip marquee and the progress-track overlay — both consume the same list.
///
/// `kind` is currently only inspected by tests; production renderers route on
/// `text` + `color`. It is kept on the struct so a future per-field renderer
/// (which needs to distinguish labels from values to build labeled rows) can
/// adopt the same builder without changing the data shape.
#[derive(Clone, Debug)]
pub(crate) struct MetadataSegment {
    #[allow(dead_code)]
    pub kind: MetadataSegmentKind,
    pub text: String,
    pub color: iced::Color,
}

/// Build the ordered fragment list for now-playing metadata.
///
/// Renderers can either flatten `.text` into a single string (merged-mode
/// marquee) or map each segment 1:1 to their native visual primitive
/// (progress-track `OverlaySegment`).
///
/// Field order is fixed: title → artist → album. Empty values are skipped
/// even if their `show_*` toggle is true — this prevents orphan
/// `"title:    ·  album:"` when a tag is missing. The list never starts or
/// ends with a [`MetadataSegmentKind::Separator`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_now_playing_segments(
    title: &str,
    artist: &str,
    album: &str,
    show_title: bool,
    show_artist: bool,
    show_album: bool,
    show_labels: bool,
    separator: &str,
) -> Vec<MetadataSegment> {
    let label_color = theme::fg4();
    let mut segments: Vec<MetadataSegment> = Vec::new();

    let mut push_field = |label: &'static str, value: &str, color: iced::Color| {
        if value.is_empty() {
            return;
        }
        if !segments.is_empty() {
            segments.push(MetadataSegment {
                kind: MetadataSegmentKind::Separator,
                text: separator.to_string(),
                color: label_color,
            });
        }
        if show_labels {
            segments.push(MetadataSegment {
                kind: MetadataSegmentKind::Label,
                text: format!("{label}: "),
                color: label_color,
            });
        }
        segments.push(MetadataSegment {
            kind: MetadataSegmentKind::Value,
            text: value.to_string(),
            color,
        });
    };

    if show_title {
        push_field("title", title, theme::now_playing_color());
    }
    if show_artist {
        push_field("artist", artist, theme::selected_color());
    }
    if show_album {
        push_field("album", album, theme::fg2());
    }

    segments
}

/// Build the merged-mode metadata string for the center row.
///
/// Thin wrapper over [`build_now_playing_segments`] — concatenates segment
/// texts in order. Hidden or empty fields are dropped; the resulting string
/// contains no orphan separators.
#[allow(clippy::too_many_arguments)]
pub(crate) fn merged_strip_string(
    show_title: bool,
    show_artist: bool,
    show_album: bool,
    show_labels: bool,
    join: &str,
    title: &str,
    artist: &str,
    album: &str,
) -> String {
    build_now_playing_segments(
        title,
        artist,
        album,
        show_title,
        show_artist,
        show_album,
        show_labels,
        join,
    )
    .into_iter()
    .map(|s| s.text)
    .collect()
}

/// Build the merged-mode metadata string for radio playback.
///
/// Order: station name → ICY title → ICY artist. With `show_labels` the
/// ICY fields render as `playing: <value>` / `artist: <value>`; the station
/// name has no label (mirrors the columnar radio layout which renders it
/// bold without a prefix). When both ICY fields are empty, falls back to
/// `url: <radio_url>` if a URL is provided. Empty parts are skipped so the
/// result never contains orphan separators.
pub(crate) fn merged_radio_strip_string(
    station_name: &str,
    icy_title: &str,
    icy_artist: &str,
    radio_url: &str,
    show_labels: bool,
    join: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if !station_name.is_empty() {
        parts.push(station_name.to_string());
    }
    if !icy_title.is_empty() {
        parts.push(if show_labels {
            format!("playing: {icy_title}")
        } else {
            icy_title.to_string()
        });
    }
    if !icy_artist.is_empty() {
        parts.push(if show_labels {
            format!("artist: {icy_artist}")
        } else {
            icy_artist.to_string()
        });
    }
    if icy_title.is_empty() && icy_artist.is_empty() && !radio_url.is_empty() {
        parts.push(if show_labels {
            format!("url: {radio_url}")
        } else {
            radio_url.to_string()
        });
    }

    parts.join(join)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOT: &str = "  ·  ";
    const PIPE: &str = "  |  ";

    #[test]
    fn merged_string_all_three_visible() {
        let s = merged_strip_string(true, true, true, true, DOT, "T", "A", "L");
        assert_eq!(s, "title: T  ·  artist: A  ·  album: L");
    }

    #[test]
    fn merged_string_drops_hidden_fields_without_orphan_separators() {
        let s = merged_strip_string(true, false, true, true, DOT, "T", "_", "L");
        assert_eq!(s, "title: T  ·  album: L");

        let s = merged_strip_string(false, true, false, true, DOT, "_", "A", "_");
        assert_eq!(s, "artist: A");
    }

    #[test]
    fn merged_string_all_hidden_is_empty() {
        let s = merged_strip_string(false, false, false, true, DOT, "T", "A", "L");
        assert_eq!(s, "");
    }

    #[test]
    fn merged_string_only_title() {
        let s = merged_strip_string(true, false, false, true, DOT, "Only Title", "_", "_");
        assert_eq!(s, "title: Only Title");
    }

    #[test]
    fn merged_string_drops_labels_when_disabled() {
        let s = merged_strip_string(true, true, true, false, DOT, "T", "A", "L");
        assert_eq!(s, "T  ·  A  ·  L");
    }

    #[test]
    fn merged_string_uses_supplied_separator() {
        let s = merged_strip_string(true, true, true, true, PIPE, "T", "A", "L");
        assert_eq!(s, "title: T  |  artist: A  |  album: L");

        let s = merged_strip_string(true, true, true, false, PIPE, "T", "A", "L");
        assert_eq!(s, "T  |  A  |  L");
    }

    #[test]
    fn build_segments_with_labels_joins_to_merged_strip_string() {
        // Joining the segment texts in order is byte-for-byte equivalent to
        // merged_strip_string — keeps overlay and merged-marquee in lockstep.
        let segments = build_now_playing_segments("T", "A", "L", true, true, true, true, DOT);
        let joined: String = segments.iter().map(|s| s.text.as_str()).collect();
        let merged = merged_strip_string(true, true, true, true, DOT, "T", "A", "L");
        assert_eq!(joined, merged);
        assert_eq!(joined, "title: T  ·  artist: A  ·  album: L");
    }

    #[test]
    fn build_segments_drops_empty_values_to_avoid_orphan_separators() {
        // Even with show_artist=true, an empty artist shouldn't render as
        // "title: T  ·    ·  album: L" with a phantom dot.
        let segments = build_now_playing_segments("T", "", "L", true, true, true, true, DOT);
        let joined: String = segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "title: T  ·  album: L");
    }

    #[test]
    fn build_segments_skips_separator_at_head_and_tail() {
        let segments = build_now_playing_segments("T", "A", "L", true, true, true, true, DOT);
        assert_ne!(
            segments.first().unwrap().kind,
            MetadataSegmentKind::Separator
        );
        assert_ne!(
            segments.last().unwrap().kind,
            MetadataSegmentKind::Separator
        );
    }

    #[test]
    fn build_segments_returns_empty_when_all_hidden_or_empty() {
        let segments = build_now_playing_segments("T", "A", "L", false, false, false, true, DOT);
        assert!(segments.is_empty());
        let segments = build_now_playing_segments("", "", "", true, true, true, true, DOT);
        assert!(segments.is_empty());
    }

    #[test]
    fn merged_radio_string_full_metadata_with_labels() {
        let s = merged_radio_strip_string("KEXP 90.3 FM", "Song Title", "Band Name", "", true, DOT);
        assert_eq!(
            s,
            "KEXP 90.3 FM  ·  playing: Song Title  ·  artist: Band Name"
        );
    }

    #[test]
    fn merged_radio_string_full_metadata_without_labels() {
        let s =
            merged_radio_strip_string("KEXP 90.3 FM", "Song Title", "Band Name", "", false, DOT);
        assert_eq!(s, "KEXP 90.3 FM  ·  Song Title  ·  Band Name");
    }

    #[test]
    fn merged_radio_string_only_station_when_no_icy_no_url() {
        let s = merged_radio_strip_string("KEXP 90.3 FM", "", "", "", true, DOT);
        assert_eq!(s, "KEXP 90.3 FM");
    }

    #[test]
    fn merged_radio_string_url_fallback_when_no_icy() {
        let s = merged_radio_strip_string(
            "KEXP 90.3 FM",
            "",
            "",
            "http://example.com/stream.mp3",
            true,
            DOT,
        );
        assert_eq!(s, "KEXP 90.3 FM  ·  url: http://example.com/stream.mp3");
    }

    #[test]
    fn merged_radio_string_url_suppressed_when_icy_present() {
        let s = merged_radio_strip_string(
            "KEXP 90.3 FM",
            "Song Title",
            "",
            "http://example.com/stream.mp3",
            true,
            DOT,
        );
        assert_eq!(s, "KEXP 90.3 FM  ·  playing: Song Title");
    }

    #[test]
    fn merged_radio_string_skips_empty_icy_artist_without_orphan_separator() {
        let s = merged_radio_strip_string("Station", "Title", "", "", true, DOT);
        assert_eq!(s, "Station  ·  playing: Title");

        let s = merged_radio_strip_string("Station", "", "Artist", "", true, DOT);
        assert_eq!(s, "Station  ·  artist: Artist");
    }

    #[test]
    fn merged_radio_string_uses_supplied_separator() {
        let s = merged_radio_strip_string("S", "T", "A", "", true, PIPE);
        assert_eq!(s, "S  |  playing: T  |  artist: A");
    }

    #[test]
    fn merged_radio_string_empty_station_drops_leading_separator() {
        let s = merged_radio_strip_string("", "Title", "Artist", "", true, DOT);
        assert_eq!(s, "playing: Title  ·  artist: Artist");
    }

    #[test]
    fn merged_radio_string_all_empty_returns_empty() {
        let s = merged_radio_strip_string("", "", "", "", true, DOT);
        assert_eq!(s, "");
    }

    #[test]
    fn merged_radio_string_pipe_separator_labels_off() {
        let s = merged_radio_strip_string("S", "T", "A", "", false, PIPE);
        assert_eq!(s, "S  |  T  |  A");
    }
}
