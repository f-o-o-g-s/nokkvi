//! Track info strip — shared widget rendering now-playing metadata.
//!
//! Used by both the player bar (bottom info strip) and the top bar
//! (when `TrackInfoDisplay::TopBar` is active in side nav mode).
//!
//! Layout: `FLAC 44.1kHz │ │ title: xxx │ artist: xxx │ album: xxx │ │ 1411kbps`

use iced::{
    Alignment, Element, Length,
    font::{Font, Weight},
    widget::{container, mouse_area, row, space, text, tooltip},
};

use crate::theme;

/// Height of the track info strip — single source of truth lives in
/// `theme::STATUS_STRIP_HEIGHT` so the flat-redesign 24 px status strip
/// stays in sync between the theme module and the strip widget.
pub(crate) const STRIP_HEIGHT: f32 = theme::STATUS_STRIP_HEIGHT;

/// Strip height plus its 1px separator — used by chrome height calculations.
pub(crate) const STRIP_HEIGHT_WITH_SEPARATOR: f32 = STRIP_HEIGHT + 1.0;

/// Make `color` legible over the status-strip band — the surface every field
/// in this widget is painted on. Thin wrapper so the call sites that thread the
/// strip's own background stay readable (single source of "this strip's
/// surface" for the whole module).
#[inline]
fn strip_text(color: iced::Color) -> iced::Color {
    theme::legible_strip_text(color, theme::status_strip_bg())
}

/// The small honest bit-perfect indicator: a label + color, or `None` when
/// there's nothing to show (mode off, or the real device rate is unknown).
/// "BIT-PERFECT" = the DAC is clocked at the track rate; "RESAMPLED" = PipeWire
/// is converting (e.g. a live down-switch the device can't follow).
///
/// `pub(crate)` so the nav-bar's MiniPlayer strip reuses the SAME label/color
/// mapping instead of duplicating the match (one source of truth for the badge).
pub(crate) fn bit_perfect_badge(
    status: crate::state::BitPerfectStatus,
    holder: Option<&str>,
) -> Option<(String, iced::Color)> {
    use crate::state::BitPerfectStatus;
    match status {
        BitPerfectStatus::Verified => Some(("BIT-PERFECT".to_owned(), theme::accent_bright())),
        // Show the rate the device is actually clocked at, INLINE (the capsule
        // has no tooltip), so the badge itself says what it's resampled to —
        // e.g. "RESAMPLED→96k". When the PipeWire graph names the app holding
        // the device, tack it on: "RESAMPLED→96k · Zen".
        BitPerfectStatus::Resampled { device_rate } => {
            let mut label = format!("RESAMPLED→{}", khz_label(device_rate));
            if let Some(h) = holder {
                label.push_str(" · ");
                label.push_str(h);
            }
            Some((label, theme::fg3()))
        }
        BitPerfectStatus::Unverifiable => Some(("UNVERIFIED".to_owned(), theme::fg3())),
        BitPerfectStatus::Off | BitPerfectStatus::Unknown => None,
    }
}

/// The kHz NUMBER for a sample rate as a string — "96", "44.1", "176.4" (one
/// decimal only when it isn't a whole number of kHz). Shared by the compact
/// badge label (which appends "k") and the hover tooltip (which appends " kHz")
/// so the same device rate never renders two different ways across the two
/// adjacent surfaces.
fn khz_number(rate: u32) -> String {
    let khz = rate as f32 / 1000.0;
    if khz.fract().abs() < f32::EPSILON {
        format!("{}", khz as u32)
    } else {
        format!("{khz:.1}")
    }
}

/// Compact kHz label for a sample rate: "96k", "44.1k", "176.4k".
fn khz_label(rate: u32) -> String {
    format!("{}k", khz_number(rate))
}

/// One-line plain-language explanation for the bit-perfect badge — surfaced as a
/// hover tooltip on the now-playing badge (the "blocker diagnostic"). `Verified`
/// confirms; `Resampled` and `Unverifiable` explain *why* playback isn't
/// bit-perfect. `None` for `Off` and the transient `Unknown` (no badge to hover).
/// Single-sourced so every render site shares the same copy.
pub(crate) fn bit_perfect_status_tooltip(
    status: crate::state::BitPerfectStatus,
    holder: Option<&str>,
) -> Option<String> {
    use crate::state::BitPerfectStatus;
    match status {
        BitPerfectStatus::Verified => Some(
            "Bit-perfect — the DAC is clocked at the track's rate, with no resampling or DSP."
                .to_owned(),
        ),
        BitPerfectStatus::Resampled { device_rate } => {
            let khz = khz_number(device_rate);
            Some(match holder {
                Some(h) => format!(
                    "Output device is locked at {khz} kHz by {h}, so this track is being \
                     resampled. Close {h} for bit-perfect."
                ),
                None => format!(
                    "Output device is locked at {khz} kHz, so this track is being resampled. \
                     Another app may be holding the device — close other audio apps for \
                     bit-perfect."
                ),
            })
        }
        BitPerfectStatus::Unverifiable => Some(
            "Can't read the output device's clock — it's Bluetooth (which re-encodes the audio, so \
             bit-perfect isn't possible) or the device is idle."
                .to_owned(),
        ),
        BitPerfectStatus::Off | BitPerfectStatus::Unknown => None,
    }
}

/// The styled bit-perfect badge as a ready-to-push widget (size-10 bold,
/// no-wrap), or `None` when there's nothing to show. Single-sources the badge
/// WIDGET — not just the (label, color) tuple — so the three render sites (this
/// strip's two layouts + the MiniPlayer nav strip) can't drift in font, size,
/// or wrapping. `color_for` adapts the badge color to the host strip's
/// legibility transform (`strip_text` here, the nav bar's `nav_strip_text`
/// closure there). Callers still add the surrounding separator themselves
/// (its placement differs per layout).
pub(crate) fn bit_perfect_badge_widget<'a, M: 'a>(
    status: crate::state::BitPerfectStatus,
    holder: Option<&str>,
    color_for: impl Fn(iced::Color) -> iced::Color,
) -> Option<Element<'a, M>> {
    let (label, color) = bit_perfect_badge(status, holder)?;
    let badge = text(label)
        .size(10.0)
        .font(Font {
            weight: Weight::Bold,
            ..theme::ui_font()
        })
        .color(color_for(color))
        .wrapping(text::Wrapping::None);
    // The badge's hover tooltip is the "blocker diagnostic": it explains why
    // playback is / isn't bit-perfect (device locked at X by whom, Bluetooth,
    // etc.). The default look is unchanged — the explanation is hover-only.
    match bit_perfect_status_tooltip(status, holder) {
        Some(tip) => Some(
            tooltip(
                badge,
                container(text(tip).size(11.0).font(theme::ui_font())).padding(4),
                tooltip::Position::Top,
            )
            .gap(4)
            .style(theme::container_tooltip)
            .into(),
        ),
        None => Some(badge.into()),
    }
}

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
    /// Honest bit-perfect status for the small now-playing indicator.
    pub bit_perfect_status: crate::state::BitPerfectStatus,
    /// When resampled, the app holding the device (inline `· Zen`).
    pub bit_perfect_holder: Option<&'a str>,
    pub bitrate: u32,
    pub radio_name: Option<&'a str>,
    pub radio_url: Option<&'a str>,
    pub icy_artist: Option<&'a str>,
    pub icy_title: Option<&'a str>,
}

/// Build the columnar radio-strip widgets — radio-tower icon, bold station
/// name, ICY title (`playing:`) / ICY artist (`artist:`), URL fallback
/// (`url:`) — pushed onto a caller-provided row alongside a caller-provided
/// separator builder.
///
/// Both `track_info_strip` (player bar / top bar strip) and `nav_bar`
/// (top-nav radio path) previously open-coded this fan-out. The two
/// originals diverged on field labels — pre-`a5e6822` `nav_bar` labeled the
/// ICY title `title:` while track_info_strip used `playing:` — a drift class
/// the audit flagged for consolidation. The shared builder eliminates the
/// drift by routing both sites through one canonical label set.
///
/// `info_sep` returns a fresh separator element each time it's called.
/// Returns the input `row` with the radio fields appended, ready for the
/// caller to add any further trailing content.
pub(crate) fn columnar_radio_strip<'a, M: 'static, F>(
    mut row_in: iced::widget::Row<'a, M>,
    radio_name: &str,
    icy_title: &str,
    icy_artist: &str,
    radio_url: Option<&str>,
    info_sep: F,
) -> iced::widget::Row<'a, M>
where
    F: Fn() -> Element<'a, M>,
{
    row_in = row_in.push(info_sep());
    row_in = row_in.push(super::nav_bar::colored_icon(
        RADIO_TOWER_ICON_PATH,
        12.0,
        theme::fg4(),
    ));

    row_in = row_in.push(
        text(radio_name.to_string())
            .size(12.0)
            .font(Font {
                weight: Weight::Bold,
                ..theme::ui_font()
            })
            .color(strip_text(theme::fg2())),
    );

    if !icy_title.is_empty() {
        row_in = row_in.push(info_sep());
        row_in = row_in.push(info_field_widget(
            "playing:",
            icy_title.to_string(),
            strip_text(theme::fg2()),
        ));
    }

    if !icy_artist.is_empty() {
        row_in = row_in.push(info_sep());
        row_in = row_in.push(info_field_widget(
            "artist:",
            icy_artist.to_string(),
            strip_text(theme::fg3()),
        ));
    }

    if icy_title.is_empty()
        && icy_artist.is_empty()
        && let Some(url) = radio_url
    {
        row_in = row_in.push(info_sep());
        row_in = row_in.push(info_field_widget(
            "url:",
            url.to_string(),
            strip_text(theme::fg2()),
        ));
    }
    row_in = row_in.push(info_sep());

    row_in
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
                data.bit_perfect_status,
                data.bit_perfect_holder,
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
            return build_merged_centered_strip(
                merged,
                format_split,
                on_press,
                None,
                data.bit_perfect_status,
                data.bit_perfect_holder,
            );
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
                .color(strip_text(theme::fg3()))
                .wrapping(text::Wrapping::None),
        );
        if let Some(badge) =
            bit_perfect_badge_widget(data.bit_perfect_status, data.bit_perfect_holder, strip_text)
        {
            info_row = info_row.push(info_sep());
            info_row = info_row.push(badge);
        }
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
                        .color(strip_text(theme::fg2())),
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
            center_row = center_row.push(info_field(title_label, title, strip_text(theme::fg2())));
            has_prev_field = true;
        }
        if show_artist {
            if has_prev_field {
                center_row = center_row.push(info_sep());
            }
            center_row =
                center_row.push(info_field(artist_label, artist, strip_text(theme::fg3())));
            has_prev_field = true;
        }
        if show_album {
            if has_prev_field {
                center_row = center_row.push(info_sep());
            }
            center_row = center_row.push(info_field(album_label, album, strip_text(theme::fg2())));
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
        // station info in the columnar layout via the shared builder.
        center_row = columnar_radio_strip(
            iced::widget::Row::new()
                .spacing(6)
                .align_y(Alignment::Center),
            radio_name,
            data.icy_title.unwrap_or(""),
            data.icy_artist.unwrap_or(""),
            data.radio_url,
            info_sep,
        );
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
                .color(strip_text(theme::fg3()))
                .wrapping(text::Wrapping::None),
        );
    }

    container(info_row.padding([0, 8]))
        .width(Length::Fill)
        .height(Length::Fixed(STRIP_HEIGHT))
        .center_y(STRIP_HEIGHT)
        .style(move |_| container::Style {
            background: Some(theme::status_strip_bg().into()),
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
    bit_perfect_status: crate::state::BitPerfectStatus,
    // Elided (not `'a`): the badge builds an owned label, so the returned
    // element doesn't borrow the holder — keep it off the return lifetime.
    bit_perfect_holder: Option<&str>,
) -> Element<'a, M> {
    let info_sep = || -> Element<'a, M> { theme::vertical_separator(STRIP_HEIGHT - 2.0) };

    let format_text = |s: String| {
        text(s)
            .size(10.0)
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            })
            .color(strip_text(theme::fg3()))
            .wrapping(text::Wrapping::None)
    };

    let marquee = iced::widget::row![
        super::marquee_text::marquee_text(merged)
            .size(10.0)
            .font(theme::ui_font())
            .color(strip_text(theme::fg2()))
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
        if let Some(badge) =
            bit_perfect_badge_widget(bit_perfect_status, bit_perfect_holder, strip_text)
        {
            info_row = info_row.push(badge);
        }
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
            background: Some(theme::status_strip_bg().into()),
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

/// Build the ordered fragment list for now-playing metadata.
///
/// Returns a flat list of text fragments — labels (`"title: "`), values
/// (`"<title>"`), and separators — in the order the merged-mode marquee
/// renders them. Renderers concatenate the fragments into a single
/// scrolling string.
///
/// The struct-of-fragments shape (`MetadataSegment { kind, text, color }`)
/// was kept around for the deleted progress-bar overlay; both its readers
/// (the old per-segment color renderer and the `kind` test inspection)
/// were dead since the redesign, so the function now returns `Vec<String>`
/// directly. If a future renderer needs to distinguish labels from values
/// it can either revive the typed shape from git history or pattern-match
/// on the colon suffix the current builder already injects.
///
/// Field order is fixed: title → artist → album. Empty values are skipped
/// even if their `show_*` toggle is true — this prevents orphan
/// `"title:    ·  album:"` when a tag is missing. The list never starts or
/// ends with a separator.
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
) -> Vec<String> {
    let mut segments: Vec<String> = Vec::new();

    let mut push_field = |label: &'static str, value: &str| {
        if value.is_empty() {
            return;
        }
        if !segments.is_empty() {
            segments.push(separator.to_string());
        }
        if show_labels {
            segments.push(format!("{label}: "));
        }
        segments.push(value.to_string());
    };

    if show_title {
        push_field("title", title);
    }
    if show_artist {
        push_field("artist", artist);
    }
    if show_album {
        push_field("album", album);
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
    .concat()
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
    use crate::state::BitPerfectStatus;

    const DOT: &str = "  ·  ";
    const PIPE: &str = "  |  ";

    #[test]
    fn badge_label_per_status() {
        let label = |s, h| bit_perfect_badge(s, h).map(|(l, _)| l);
        assert_eq!(
            label(BitPerfectStatus::Verified, None).as_deref(),
            Some("BIT-PERFECT")
        );
        // Resampled shows the device rate INLINE so the (tooltip-less) capsule is
        // informative: a 96k device latch reads "RESAMPLED→96k".
        assert_eq!(
            label(
                BitPerfectStatus::Resampled {
                    device_rate: 96_000
                },
                None
            )
            .as_deref(),
            Some("RESAMPLED→96k")
        );
        // With the holder named (from the PipeWire graph), it's tacked on inline.
        assert_eq!(
            label(
                BitPerfectStatus::Resampled {
                    device_rate: 96_000
                },
                Some("Zen")
            )
            .as_deref(),
            Some("RESAMPLED→96k · Zen")
        );
        // Non-whole-kHz rates keep one decimal.
        assert_eq!(
            label(
                BitPerfectStatus::Resampled {
                    device_rate: 44_100
                },
                None
            )
            .as_deref(),
            Some("RESAMPLED→44.1k")
        );
        // The settled can't-read state (Bluetooth / idle) shows the UNVERIFIED hint.
        assert_eq!(
            label(BitPerfectStatus::Unverifiable, None).as_deref(),
            Some("UNVERIFIED")
        );
        // Off and the TRANSIENT Unknown (probe in flight) stay hidden — Unknown
        // must not flash a chip mid-transition (preserves the gate's reset-to-hidden).
        assert_eq!(bit_perfect_badge(BitPerfectStatus::Off, None), None);
        assert_eq!(bit_perfect_badge(BitPerfectStatus::Unknown, None), None);
    }

    #[test]
    fn status_tooltip_explains_the_blocker() {
        // Verified confirms; Resampled names the device rate + the likely cause;
        // Unverifiable explains the BT/idle case. Off/Unknown have no tooltip.
        assert!(bit_perfect_status_tooltip(BitPerfectStatus::Verified, None).is_some());
        let resampled = bit_perfect_status_tooltip(
            BitPerfectStatus::Resampled {
                device_rate: 48_000,
            },
            None,
        )
        .expect("resampled has a tooltip");
        // Whole-kHz rate renders without a trailing ".0" — consistent with the
        // badge's compact "48k" (both go through `khz_number`), not "48.0 kHz".
        assert!(
            resampled.contains("48 kHz") && !resampled.contains("48.0"),
            "names the device rate consistently with the badge: {resampled}"
        );
        assert!(
            resampled.contains("Another app"),
            "points at the likely cause: {resampled}"
        );
        // When the holder is known, the tooltip names it and says to close IT.
        let named = bit_perfect_status_tooltip(
            BitPerfectStatus::Resampled {
                device_rate: 48_000,
            },
            Some("Zen"),
        )
        .expect("resampled has a tooltip");
        assert!(
            named.contains("by Zen") && named.contains("Close Zen"),
            "names the holder: {named}"
        );
        assert!(
            bit_perfect_status_tooltip(BitPerfectStatus::Unverifiable, None)
                .expect("unverifiable has a tooltip")
                .contains("Bluetooth"),
            "unverifiable explains the Bluetooth/idle case"
        );
        assert_eq!(
            bit_perfect_status_tooltip(BitPerfectStatus::Off, None),
            None
        );
        assert_eq!(
            bit_perfect_status_tooltip(BitPerfectStatus::Unknown, None),
            None
        );
    }

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
        // Joining the segments in order is byte-for-byte equivalent to
        // merged_strip_string — pins the shape contract.
        let segments = build_now_playing_segments("T", "A", "L", true, true, true, true, DOT);
        let joined: String = segments.concat();
        let merged = merged_strip_string(true, true, true, true, DOT, "T", "A", "L");
        assert_eq!(joined, merged);
        assert_eq!(joined, "title: T  ·  artist: A  ·  album: L");
    }

    #[test]
    fn build_segments_drops_empty_values_to_avoid_orphan_separators() {
        // Even with show_artist=true, an empty artist shouldn't render as
        // "title: T  ·    ·  album: L" with a phantom dot.
        let segments = build_now_playing_segments("T", "", "L", true, true, true, true, DOT);
        assert_eq!(segments.concat(), "title: T  ·  album: L");
    }

    #[test]
    fn build_segments_skips_separator_at_head_and_tail() {
        // The first and last segment must never be the separator string —
        // otherwise the merged marquee would render as `"  ·  title: T ..."`.
        let segments = build_now_playing_segments("T", "A", "L", true, true, true, true, DOT);
        assert_ne!(segments.first().map(String::as_str), Some(DOT));
        assert_ne!(segments.last().map(String::as_str), Some(DOT));
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
