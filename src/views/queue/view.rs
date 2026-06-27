//! Queue view — `impl QueuePage { fn view }`.
//!
//! Rendering for the queue page. The per-row song-list composition (columns,
//! drag, context menu) is delegated to the shared `views::song_list_pane`
//! renderer; the per-mode column-visibility helpers and the
//! `BREAKPOINT_HIDE_QUEUE_STARS` constant live there too. Update/state logic
//! lives in `update.rs`; types live in `mod.rs`.

use iced::{
    Alignment, Element, Length,
    widget::{Row, Space, column, container, mouse_area, row},
};

use super::{QueueContextEntry, QueueMessage, QueuePage, QueueSortMode, QueueViewData};
use crate::{
    views::song_list_pane::{SongListPaneParams, SongListRowEvent, song_list_pane},
    widgets::{
        self,
        hover_overlay::HoverOverlay,
        view_header::{HeaderButton, ViewHeaderConfig},
    },
};

/// Compact height of the read-only playlist "Playing From" banner.
pub(crate) const PLAYLIST_STRIP_COMPACT_H: f32 = 46.0;
/// Resolve the artwork layout the queue's slot list will render for the given
/// window — shared by the playlist strip's width math and its top-separator
/// gate. `resolve_artwork_layout` reads only the window dimensions, the
/// show-artwork flag, and the display-mode atomics, so `slot_list_chrome` /
/// `elevated` are irrelevant here and neutral values are fine.
fn playlist_strip_artwork_layout(
    window_width: f32,
    window_height: f32,
) -> Option<crate::widgets::base_slot_list_layout::ArtworkLayout> {
    use crate::widgets::base_slot_list_layout::{BaseSlotListLayoutConfig, resolve_artwork_layout};
    resolve_artwork_layout(&BaseSlotListLayoutConfig {
        window_width,
        window_height,
        show_artwork_column: true,
        slot_list_chrome: 0.0,
        elevated: false,
    })
}

/// Width available to the expanded playlist strip's content — the song-list
/// column, i.e. the content pane minus the horizontal artwork column when one
/// is shown. Vertical / hidden artwork leave the strip full-width. The comment
/// wraps within this width, so sizing the detail block to it (rather than to
/// the full pane) is what keeps the meta row from being clipped.
fn playlist_strip_band_width(window_width: f32, window_height: f32) -> f32 {
    use crate::widgets::base_slot_list_layout::ArtworkOrientation;
    match playlist_strip_artwork_layout(window_width, window_height) {
        Some(layout) if matches!(layout.orientation, ArtworkOrientation::Horizontal) => {
            (window_width - layout.extent).max(120.0)
        }
        _ => window_width,
    }
}

/// Whether the read-only "Playing From" banner needs a top hairline separating
/// it from a large album-artwork column stacked directly above it.
///
/// The banner is a full-bleed accent wash with no top margin, so in the
/// Auto-mode portrait fallback it runs flush against the artwork panel above it
/// with no visual break — that's the case this hairline fixes. Every other
/// layout already reads as separated: Horizontal / hidden artwork places the
/// banner below the nav chrome, the regular view header carries its own top
/// margin, and the opt-in Always-Vertical* modes render a 6px drag-handle bar
/// between the artwork and the chrome. That handle is exactly what
/// [`ArtworkColumnMode::is_vertical`] gates in `vertical_layout`, so excluding
/// those modes scopes the hairline to the genuinely-flush handle-less case.
fn playlist_strip_needs_top_separator(
    playlist_loaded: bool,
    window_width: f32,
    window_height: f32,
) -> bool {
    use crate::widgets::base_slot_list_layout::ArtworkOrientation;
    // Always-Vertical* modes resolve to Vertical too but separate the banner
    // with their drag handle — skip them so only the flush Auto fallback fires.
    if !playlist_loaded || crate::theme::artwork_column_mode().is_vertical() {
        return false;
    }
    matches!(
        playlist_strip_artwork_layout(window_width, window_height),
        Some(layout) if matches!(layout.orientation, ArtworkOrientation::Vertical)
    )
}

/// Lay out the hover-expanded detail block: clamp the comment to at most
/// `MAX_LINES` rendered lines (appending an ellipsis when it would overflow)
/// and return the display string together with the block height, sized to the
/// (clamped) comment plus the meta row.
///
/// `content_width` is the comment's real rendered width (band width minus the
/// strip padding). Sizing the block to it — and truncating the comment to fit
/// — keeps a long description from pushing the meta row past the container's
/// `clip(true)` and vanishing. Short comments still reserve no dead space.
/// `ui_font()` is monospace, so the char-count/width estimate is reliable.
fn playlist_strip_detail(comment: &str, content_width: f32) -> (String, f32) {
    const LINE_H: f32 = 16.0;
    const CHAR_W: f32 = 7.3;
    const META_ROW_H: f32 = 20.0;
    const ROW_GAP: f32 = 8.0;
    const BOTTOM_PAD: f32 = 9.0;
    const MAX_LINES: f32 = 5.0;
    let cols = (content_width / CHAR_W).floor().max(1.0);
    let char_count = comment.chars().count();
    let raw_lines = (char_count as f32 / cols).ceil().max(1.0);
    let (display, lines) = if raw_lines <= MAX_LINES {
        (comment.to_string(), raw_lines)
    } else {
        // Keep MAX_LINES worth of characters (less one for the ellipsis glyph)
        // so the rendered comment occupies at most MAX_LINES lines.
        let keep = ((MAX_LINES * cols) as usize).saturating_sub(1);
        let truncated: String = comment.chars().take(keep).collect();
        (format!("{}…", truncated.trim_end()), MAX_LINES)
    };
    (display, lines * LINE_H + ROW_GAP + META_ROW_H + BOTTOM_PAD)
}

/// Format a playlist's total duration for the strip, e.g. `4h 53m` / `47m`.
fn format_strip_duration(secs: f32) -> String {
    let total_mins = (secs / 60.0).round() as u32;
    let (h, m) = (total_mins / 60, total_mins % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

impl QueuePage {
    /// Build the view
    pub fn view<'a>(&'a self, data: QueueViewData<'a>) -> Element<'a, QueueMessage> {
        use crate::widgets::slot_list::SlotListConfig;

        // Build ViewHeader using generic component
        const QUEUE_VIEW_OPTIONS: &[QueueSortMode] = &[
            QueueSortMode::Album,
            QueueSortMode::Artist,
            QueueSortMode::Title,
            QueueSortMode::Duration,
            QueueSortMode::Genre,
            QueueSortMode::Rating,
            QueueSortMode::MostPlayed,
            QueueSortMode::Random,
        ];

        // Build the columns-visibility dropdown for the queue's view header.
        let column_dropdown: Element<'a, QueueMessage> =
            crate::widgets::checkbox_dropdown::view_columns_dropdown(
                crate::View::Queue,
                self.column_visibility.dropdown_entries(),
                QueueMessage::ToggleColumnVisible,
                QueueMessage::SetOpenMenu,
                data.overlay.column_dropdown_open,
                data.overlay.column_dropdown_trigger_bounds,
            )
            .into();

        // Auto-hide toolbar: collapse to a hairline when enabled and not
        // currently revealed (hover / active search / hotkey window).
        let autohide = crate::theme::is_autohide_toolbar();
        let toolbar_collapsed = self
            .common
            .toolbar_collapsed(autohide, data.overlay.column_dropdown_open);

        let header = widgets::view_header::view_header(ViewHeaderConfig {
            current_view: self.queue_sort_mode,
            view_options: QUEUE_VIEW_OPTIONS,
            sort_ascending: self.common.sort_ascending,
            search_query: &self.common.search_query,
            filtered_count: data.queue_songs.len(),
            total_count: data.total_queue_count,
            item_type: "songs",
            search_input_id: crate::views::QUEUE_SEARCH_ID,
            on_view_selected: Box::new(QueueMessage::SortModeSelected),
            show_search: true,
            on_search_change: Box::new(|q| {
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::SearchQueryChanged(q))
            }),
            // Queue has no refresh button; CenterOnPlaying only when there's a
            // currently-playing track in the queue.
            buttons: {
                let mut btns = vec![HeaderButton::SortToggle(QueueMessage::SlotList(
                    crate::widgets::SlotListPageMessage::ToggleSortOrder,
                ))];
                if let Some(entry_id) = data.current_playing_entry_id {
                    btns.push(HeaderButton::CenterOnPlaying(
                        QueueMessage::FocusCurrentPlaying(entry_id, true),
                    ));
                }
                // Default-playlist chip is gated by a user setting; when on,
                // it sits left of the columns dropdown in the trailing region.
                if data.show_default_playlist_chip {
                    let chip = crate::widgets::default_playlist_chip::default_playlist_chip(
                        data.default_playlist_name,
                        QueueMessage::OpenDefaultPlaylistPicker,
                    );
                    btns.push(HeaderButton::Trailing(chip));
                }
                btns.push(HeaderButton::Trailing(column_dropdown));
                btns
            },
            on_roulette: Some(QueueMessage::Roulette),
            collapsed: toolbar_collapsed,
            on_hover_enter: autohide.then_some({
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverEnter)
            }),
            on_hover_exit: autohide.then_some({
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverExit)
            }),
            on_dropdown_open: autohide.then_some(QueueMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(true),
            )),
            on_dropdown_close: autohide.then_some(QueueMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(false),
            )),
            total_duration_secs: Some(
                data.queue_songs
                    .iter()
                    .map(|s| u64::from(s.duration_seconds))
                    .sum(),
            ),
            // Show "Unsorted" until the user applies a queue sort — the queue
            // takes its order from whatever populated it, so the remembered
            // `queue_sort_mode` would otherwise misrepresent the actual order.
            sort_placeholder: (!self.queue_sorted).then_some("Unsorted"),
        });

        // Build final header: regular header + optional "Playing From" banner.
        //
        // Expanded read-only-strip detail: clamp the comment to the real band
        // width (the song-list column, excluding the horizontal artwork column)
        // so it can't overflow the clipped block and push the meta row out of
        // view. Returns the (possibly ellipsized) display string plus the block
        // height; both feed the band render and the chrome math below so they
        // stay in lockstep.
        let (playlist_comment_display, playlist_detail_h) =
            data.playlist_context_info.as_ref().map_or_else(
                || (String::new(), 0.0),
                |ctx| {
                    let content_width =
                        (playlist_strip_band_width(data.window_width, data.window_height) - 73.0)
                            .max(120.0);
                    playlist_strip_detail(&ctx.comment, content_width)
                },
            );

        // `extra`, `sep`, and the `top_sep` built below are the three leading
        // children of the final `column![top_sep, extra, sep, header]` (assembled
        // after the banner). Each collapses to a zero-sized `Space` placeholder
        // when inactive, so the column is ALWAYS a 4-child shape — iced's
        // positional reconciler then keeps the search `text_input::Id` (inside
        // `header`, the trailing child) stable across the playlist-context /
        // read-only / orientation toggles.
        let extra: Element<'a, QueueMessage> = if let Some(ref ctx) = data.playlist_context_info {
            // Read-only "Playing From" banner (Direction 2). Renders only while a
            // playlist is loaded for playback. Hovering the band reveals a detail
            // block; the banner grows in flow and the slot-list chrome height
            // tracks it. Editing happens in the decoupled `PlaylistEditor` view.
            use iced::widget::svg;

            let accent = crate::theme::accent();
            let expanded = data.playlist_strip_expanded;

            // Icon action button — mouse_area + HoverOverlay(container) so the
            // press scale fires; the inner press is independent of the band's
            // hover-enter/exit so save/edit clicks don't toggle the panel.
            let icon_btn =
                |icon_path: &'static str, msg: QueueMessage| -> Element<'a, QueueMessage> {
                    let icon = crate::embedded_svg::svg_widget(icon_path)
                        .width(Length::Fixed(14.0))
                        .height(Length::Fixed(14.0))
                        .style(|_theme, _status| svg::Style {
                            color: Some(crate::theme::fg2()),
                        });
                    mouse_area(
                        HoverOverlay::new(
                            container(icon)
                                .padding([4, 6])
                                .style(|_theme| container::Style {
                                    background: None,
                                    border: iced::Border {
                                        color: iced::Color::TRANSPARENT,
                                        width: 2.0,
                                        radius: crate::theme::ui_border_radius(),
                                    },
                                    ..Default::default()
                                })
                                .center_y(Length::Shrink),
                        )
                        .border_radius(crate::theme::ui_border_radius()),
                    )
                    .on_press(msg)
                    .interaction(iced::mouse::Interaction::Pointer)
                    .into()
                };

            // Accent stripe — 3px, full banner height.
            let stripe = container(Space::new())
                .width(Length::Fixed(3.0))
                .height(Length::Fill)
                .style(move |_theme| container::Style {
                    background: Some(accent.into()),
                    ..Default::default()
                });

            // Eyebrow + name stack; clip so a long name can't shove the right
            // cluster off-screen (same overflow guard the old bar relied on).
            let eyebrow = iced::widget::text("PLAYING FROM PLAYLIST")
                .font(crate::theme::weighted_ui_font(iced::font::Weight::Semibold))
                .size(9.5)
                .color(accent)
                .wrapping(iced::widget::text::Wrapping::None);
            let name = iced::widget::text(ctx.name.clone())
                .font(crate::theme::weighted_ui_font(iced::font::Weight::Bold))
                .size(14)
                .color(crate::theme::fg0())
                .wrapping(iced::widget::text::Wrapping::None);
            let name_stack = container(column![eyebrow, name].spacing(1))
                .width(Length::Fill)
                .clip(true);

            let save_btn = icon_btn("assets/icons/save.svg", QueueMessage::QuickSavePlaylist);
            let edit_btn = icon_btn("assets/icons/pencil-line.svg", QueueMessage::EditPlaylist);

            // Compact row. The cover is pushed only when its handle is cached so
            // an absent cover leaves no phantom leading gap. A 2×2 quad of the
            // queue's first distinct album covers is preferred — it reads as
            // "a playlist", not "the song playing now" — with the single
            // first-album cover as the warm-up fallback.
            let mut compact = Row::new()
                .spacing(10)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 13.0,
                    bottom: 0.0,
                    left: 11.0,
                });
            if let Some(tiles) = &data.playlist_quad {
                compact = compact.push(
                    container(crate::widgets::base_slot_list_layout::quad_artwork_grid(
                        tiles, 34.0, 1.0,
                    ))
                    .width(Length::Fixed(34.0))
                    .height(Length::Fixed(34.0))
                    .clip(true),
                );
            } else if let Some(handle) = data.playlist_cover {
                compact = compact.push(
                    container(
                        iced::widget::image(handle.clone())
                            .width(Length::Fixed(34.0))
                            .height(Length::Fixed(34.0))
                            .content_fit(iced::ContentFit::Cover),
                    )
                    .width(Length::Fixed(34.0))
                    .height(Length::Fixed(34.0))
                    .clip(true),
                );
            }
            // Identity on the left (cover + eyebrow/name); actions grouped on the
            // right. All metadata (count / duration / updated / visibility) lives
            // in the hover-expanded detail block — keeping the compact band from
            // duplicating the song count the view-header already shows beneath it.
            let actions = row![save_btn, edit_btn]
                .spacing(2)
                .align_y(Alignment::Center);
            let compact = compact.push(name_stack).push(actions);
            let compact = container(compact)
                .center_y(Length::Fixed(PLAYLIST_STRIP_COMPACT_H))
                .width(Length::Fill);

            let total_h = if expanded {
                PLAYLIST_STRIP_COMPACT_H + playlist_detail_h
            } else {
                PLAYLIST_STRIP_COMPACT_H
            };

            // Body: compact row alone, or compact + fixed-height detail block.
            let body: Element<'a, QueueMessage> = if expanded {
                let meta_item =
                    |icon_path: &'static str, label: String| -> Element<'a, QueueMessage> {
                        row![
                            crate::embedded_svg::svg_widget(icon_path)
                                .width(Length::Fixed(12.0))
                                .height(Length::Fixed(12.0))
                                .style(|_theme, _status| svg::Style {
                                    color: Some(crate::theme::fg3()),
                                }),
                            iced::widget::text(label)
                                .font(crate::theme::ui_font())
                                .size(11)
                                .color(crate::theme::fg3()),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center)
                        .into()
                    };

                let count = if ctx.song_count > 0 {
                    ctx.song_count as usize
                } else {
                    data.total_queue_count
                };
                let count_label = if count == 1 {
                    "1 song".to_string()
                } else {
                    format!("{count} songs")
                };

                let mut meta_row = Row::new().spacing(14).align_y(Alignment::Center);
                meta_row = meta_row.push(meta_item("assets/icons/music.svg", count_label));
                if ctx.duration_secs > 0.0 {
                    meta_row = meta_row.push(meta_item(
                        "assets/icons/clock.svg",
                        format_strip_duration(ctx.duration_secs),
                    ));
                }
                if !ctx.updated.is_empty() {
                    let date = nokkvi_data::utils::formatters::format_date_concise(&ctx.updated);
                    meta_row = meta_row.push(meta_item(
                        "assets/icons/calendar.svg",
                        format!("Updated {date}"),
                    ));
                }

                // Public/private chip — pill outline at 30% accent alpha.
                let (chip_icon, chip_text) = if ctx.public {
                    ("assets/icons/lock-open.svg", "Public")
                } else {
                    ("assets/icons/lock.svg", "Private")
                };
                let chip_border = iced::Color { a: 0.30, ..accent };
                let chip = container(
                    row![
                        crate::embedded_svg::svg_widget(chip_icon)
                            .width(Length::Fixed(11.0))
                            .height(Length::Fixed(11.0))
                            .style(|_theme, _status| svg::Style {
                                color: Some(crate::theme::fg2()),
                            }),
                        iced::widget::text(chip_text)
                            .font(crate::theme::ui_font())
                            .size(10.5)
                            .color(crate::theme::fg2()),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .style(move |_theme| container::Style {
                    border: iced::Border {
                        color: chip_border,
                        width: 1.0,
                        radius: crate::theme::ui_radius_pill(),
                    },
                    ..Default::default()
                });
                meta_row = meta_row.push(chip);

                let comment_text = iced::widget::text(playlist_comment_display)
                    .font(crate::theme::ui_font())
                    .size(12)
                    .color(crate::theme::fg2());

                let detail = container(column![comment_text, meta_row].spacing(8))
                    .width(Length::Fill)
                    .height(Length::Fixed(playlist_detail_h))
                    .padding(iced::Padding {
                        top: 0.0,
                        right: 13.0,
                        bottom: 9.0,
                        left: 57.0,
                    })
                    .clip(true);

                column![compact, detail].width(Length::Fill).into()
            } else {
                compact.into()
            };

            // Faint accent wash over bg0_soft (flat blend — reads correctly on
            // every theme without gradient-API risk). Shares the single
            // accent-wash recipe with the hover overlay via `theme::accent_wash`.
            let wash =
                crate::theme::accent_wash(crate::theme::bg0_soft(), crate::theme::HEADER_WASH);

            let banner = container(
                row![stripe, body]
                    .width(Length::Fill)
                    .height(Length::Fixed(total_h)),
            )
            .width(Length::Fill)
            .height(Length::Fixed(total_h))
            .style(move |_theme| container::Style {
                background: Some(wash.into()),
                ..Default::default()
            });

            mouse_area(banner)
                .on_enter(QueueMessage::PlaylistStripHoverEnter)
                .on_exit(QueueMessage::PlaylistStripHoverExit)
                .into()
        } else {
            Space::new()
                .width(Length::Shrink)
                .height(Length::Fixed(0.0))
                .into()
        };
        let sep: Element<'a, QueueMessage> = if data.playlist_context_info.is_some() {
            crate::theme::horizontal_separator(1.0)
        } else {
            Space::new()
                .width(Length::Shrink)
                .height(Length::Fixed(0.0))
                .into()
        };
        // Top hairline above the banner — only when a large album-artwork column
        // is stacked flush directly above it (the Auto-mode portrait fallback;
        // see `playlist_strip_needs_top_separator`). Every other layout already
        // reads as separated. Kept as a fourth column child (zero-`Space` when
        // absent) so the `column![top_sep, extra, sep, header]` shape — and the
        // search `text_input::Id` inside `header` — stay positionally stable
        // across the playlist / orientation toggles. Its 1 px is folded into
        // `chrome_height` below so the vertical slot-list pinning math stays exact.
        let needs_top_sep = playlist_strip_needs_top_separator(
            data.playlist_context_info.is_some(),
            data.window_width,
            data.window_height,
        );
        let top_sep: Element<'a, QueueMessage> = if needs_top_sep {
            crate::theme::horizontal_separator(1.0)
        } else {
            Space::new()
                .width(Length::Shrink)
                .height(Length::Fixed(0.0))
                .into()
        };
        let header: Element<'a, QueueMessage> = column![top_sep, extra, sep, header].into();

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. The bar's tri-state derives from the
        // current selection set against the *filtered* (visible) row count.
        let header = crate::widgets::slot_list::compose_header_with_select(
            self.column_visibility.select,
            self.common.select_all_state(data.queue_songs.len()),
            QueueMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
            header,
        );

        // Configure slot list with queue-specific chrome height (with view header now).
        // The "Playing From" banner adds its own height; account for it so the last
        // slot isn't shorter than the rest.
        use crate::widgets::slot_list::{
            chrome_height_with_header, chrome_height_with_select_header,
        };
        let select_header_visible = self.column_visibility.select;
        let chrome_height = if data.playlist_context_info.is_some() {
            // Compact "Playing From" banner + 1px bottom separator, plus the
            // detail block height when the strip is hover-expanded
            // (grow-in-flow), plus the 1px top hairline when a vertical artwork
            // column sits directly above the banner. The top hairline must be
            // counted here so the vertical layout pins the slot-list rect with
            // the exact chrome height it renders — otherwise the column overflows
            // by 1px and clips the last slot.
            let strip = if data.playlist_strip_expanded {
                PLAYLIST_STRIP_COMPACT_H + playlist_detail_h
            } else {
                PLAYLIST_STRIP_COMPACT_H
            };
            let top_sep_h = if needs_top_sep { 1.0 } else { 0.0 };
            chrome_height_with_header(toolbar_collapsed) + strip + 1.0 + top_sep_h
        } else {
            chrome_height_with_select_header(toolbar_collapsed, select_header_visible)
        };
        let chrome_height = if select_header_visible && data.playlist_context_info.is_some() {
            chrome_height + crate::widgets::slot_list::SELECT_HEADER_HEIGHT
        } else {
            chrome_height
        };

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
            slot_list_chrome: chrome_height,
            elevated: data.elevated,
        };

        // If no songs in filtered results, show appropriate message (like albums view)
        if data.queue_songs.is_empty() {
            let message = if data.total_queue_count == 0 {
                "Queue is empty."
            } else {
                "No songs match your search."
            };
            return widgets::base_slot_list_empty_state(header, message, &layout_config);
        }

        let vertical_artwork_chrome =
            crate::widgets::base_slot_list_layout::vertical_artwork_chrome(&layout_config);
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            chrome_height + vertical_artwork_chrome,
        )
        .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let _scale_factor = data.scale_factor;
        let current_playing_song_id = data.current_playing_song_id;
        let current_playing_entry_id = data.current_playing_entry_id;
        // Effective applied sort: `None` when the queue is unsorted, so the
        // plays/genre columns auto-show only when a genuine Most Played / Genre
        // sort is in effect (not merely the remembered mode).
        let applied_sort_mode = self.queue_sorted.then_some(self.queue_sort_mode);
        let album_art = data.album_art; // Move artwork maps
        let large_artwork = data.large_artwork;
        let queue_songs = data.queue_songs; // Move ownership to extend lifetime
        let column_visibility = self.column_visibility;

        // Render the queue's song rows through the shared `song_list_pane`.
        // The queue maps the neutral row-event vocabulary back to the exact
        // `QueueMessage` each interaction emitted before extraction, and
        // supplies the queue-specific 11-entry context menu via the
        // `build_context_menu` closure — so behavior is byte-identical.
        let overlay_open_menu = data.overlay.open_menu;
        let slot_list_content = song_list_pane(
            SongListPaneParams {
                slot_list: &self.common.slot_list,
                songs: queue_songs.as_ref(),
                list_config: &config,
                drop_indicator_slot: data.drop_indicator_slot,
                columns: column_visibility,
                sort_mode: applied_sort_mode,
                album_art,
                current_playing_song_id: current_playing_song_id.clone(),
                current_playing_entry_id,
                stable_viewport: data.stable_viewport,
            },
            |e| match e {
                SongListRowEvent::Slot(m) => QueueMessage::SlotList(m),
                SongListRowEvent::Drag(d) => QueueMessage::DragReorder(d),
                SongListRowEvent::TitleClick(i) => {
                    QueueMessage::ContextMenuAction(i, QueueContextEntry::GetInfo)
                }
                SongListRowEvent::NavArtist(a) => QueueMessage::NavigateAndExpandArtist(a),
                SongListRowEvent::NavAlbum(a) => QueueMessage::NavigateAndExpandAlbum(a),
                SongListRowEvent::NavGenre(g) => QueueMessage::NavigateAndExpandGenre(g),
                SongListRowEvent::SetRating(i, s) => QueueMessage::ClickSetRating(i, s),
                SongListRowEvent::ToggleLove(i) => QueueMessage::ClickToggleStar(i),
            },
            move |slot_button, item_idx| {
                // Wrap in context menu — queue-specific 11-entry menu.
                use crate::widgets::context_menu::{context_menu, menu_button, menu_separator};
                let entries = vec![
                    QueueContextEntry::Play,
                    QueueContextEntry::PlayNext,
                    QueueContextEntry::Separator,
                    QueueContextEntry::RemoveFromQueue,
                    QueueContextEntry::Separator,
                    QueueContextEntry::AddToPlaylist,
                    QueueContextEntry::SaveAsPlaylist,
                    QueueContextEntry::Separator,
                    QueueContextEntry::OpenBrowsingPanel,
                    QueueContextEntry::Separator,
                    QueueContextEntry::GetInfo,
                    QueueContextEntry::ShowInFolder,
                    QueueContextEntry::FindSimilar,
                    QueueContextEntry::TopSongs,
                ];

                let cm_id = crate::app_message::ContextMenuId::QueueRow(item_idx);
                let (cm_open, cm_position) =
                    crate::widgets::context_menu::open_state_for(overlay_open_menu, &cm_id);
                let cm_id_for_msg = cm_id.clone();
                context_menu(
                    slot_button,
                    entries,
                    move |entry, _length| match entry {
                        QueueContextEntry::Play => menu_button(
                            Some("assets/icons/circle-play.svg"),
                            "Play",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::Play),
                        ),
                        QueueContextEntry::PlayNext => menu_button(
                            Some("assets/icons/list-end.svg"),
                            "Play Next",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::PlayNext),
                        ),
                        QueueContextEntry::RemoveFromQueue => menu_button(
                            Some("assets/icons/trash-2.svg"),
                            "Remove from Queue",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::RemoveFromQueue,
                            ),
                        ),
                        QueueContextEntry::Separator => menu_separator(),
                        QueueContextEntry::AddToPlaylist => menu_button(
                            Some("assets/icons/list-music.svg"),
                            "Add to Playlist",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::AddToPlaylist,
                            ),
                        ),
                        QueueContextEntry::SaveAsPlaylist => menu_button(
                            Some("assets/icons/list-music.svg"),
                            "Save Queue as Playlist",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::SaveAsPlaylist,
                            ),
                        ),
                        QueueContextEntry::OpenBrowsingPanel => menu_button(
                            Some("assets/icons/panel-right-open.svg"),
                            "Library Browser",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::OpenBrowsingPanel,
                            ),
                        ),
                        QueueContextEntry::GetInfo => menu_button(
                            Some("assets/icons/info.svg"),
                            "Get Info",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::GetInfo),
                        ),
                        QueueContextEntry::ShowInFolder => menu_button(
                            Some("assets/icons/folder-open.svg"),
                            "Show in File Manager",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::ShowInFolder,
                            ),
                        ),
                        QueueContextEntry::FindSimilar => menu_button(
                            Some("assets/icons/radar.svg"),
                            "Find Similar",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::FindSimilar,
                            ),
                        ),
                        QueueContextEntry::TopSongs => menu_button(
                            Some("assets/icons/star.svg"),
                            "Top Songs",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::TopSongs),
                        ),
                    },
                    cm_open,
                    cm_position,
                    move |position| match position {
                        Some(p) => {
                            QueueMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                                id: cm_id_for_msg.clone(),
                                position: p,
                            }))
                        }
                        None => QueueMessage::SetOpenMenu(None),
                    },
                )
                .into()
            },
        );

        // Get large artwork: prioritize currently playing song, fall back
        // to centered song's large, then to either song's mini. Mini is
        // upscaled by Iced — blurry, but lets the panel track the centered
        // slot during a roulette spin's fast cruise where LoadLarge can't
        // keep up with offset changes (see Albums view for the same
        // pattern).
        let center_artwork_handle: Option<&iced::widget::image::Handle> = if data.is_playing {
            current_playing_song_id
                .as_ref()
                .and_then(|song_id| queue_songs.iter().find(|s| &s.id == song_id))
                .and_then(|song| {
                    large_artwork
                        .get(&song.album_id)
                        .or_else(|| album_art.get(&song.album_id))
                })
        } else {
            None
        }
        .or_else(|| {
            self.common
                .slot_list
                .get_center_item_index(queue_songs.len())
                .and_then(|center_idx| queue_songs.get(center_idx))
                .and_then(|song| {
                    large_artwork
                        .get(&song.album_id)
                        .or_else(|| album_art.get(&song.album_id))
                })
        });

        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_visualizer_and_menu;

        // Build artwork column component — determine album_id for refresh action
        let center_album_id: Option<String> = if data.is_playing {
            current_playing_song_id
                .as_ref()
                .and_then(|song_id| queue_songs.iter().find(|s| &s.id == song_id))
                .map(|song| song.album_id.clone())
        } else {
            None
        }
        .or_else(|| {
            self.common
                .slot_list
                .get_center_item_index(queue_songs.len())
                .and_then(|center_idx| queue_songs.get(center_idx))
                .map(|song| song.album_id.clone())
        });
        let on_refresh = center_album_id.map(QueueMessage::RefreshArtwork);
        let (artwork_menu_open, artwork_menu_position, on_artwork_menu_change) =
            crate::widgets::context_menu::artwork_panel_open_state(
                crate::View::Queue,
                data.overlay.open_menu,
                QueueMessage::SetOpenMenu,
            );
        // Over-cover visualizer overlay: render whenever the active mode is set
        // to draw over the cover (carried by `over_art_visualizer` — Scope
        // always, Bars/Lines when their placement is OverCover), independent of
        // play state. This mirrors the bottom-band path (`app_view`), which is
        // also ungated: when audio pauses, no fresh chunk reaches the FFT worker,
        // so `display.bars` / the ring waveform hold their last values and the
        // overlay freezes in place rather than vanishing. Otherwise (bottom-band
        // placement or `Off`) `over_art_visualizer` is `None` → plain cover.
        let over_art_overlay = data.over_art_visualizer;
        // Surfing boat over the cover — ungated to match the ring above; the boat
        // tick holds `visible` and the frozen position/handle while paused.
        let over_art_boat = data.over_art_boat;
        let artwork_content = Some(single_artwork_panel_with_visualizer_and_menu(
            center_artwork_handle,
            over_art_overlay,
            over_art_boat,
            crate::widgets::base_slot_list_layout::ArtworkPlaceholder::Blank,
            on_refresh,
            artwork_menu_open,
            artwork_menu_position,
            on_artwork_menu_change,
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(QueueMessage::ArtworkColumnDrag),
            Some(QueueMessage::ArtworkColumnVerticalDrag),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        playlist_strip_band_width, playlist_strip_detail, playlist_strip_needs_top_separator,
    };

    /// Both top-separator / band-width tests mutate the artwork-column-mode
    /// atomics that `resolve_artwork_layout` reads. Take the crate-wide theme
    /// lock so they serialize against every other atomic-mutating test family.
    fn with_auto_artwork_mode() -> parking_lot::MutexGuard<'static, ()> {
        use nokkvi_data::types::player_settings::ArtworkColumnMode;
        let guard = crate::theme::THEME_MODE_LOCK.lock();
        crate::theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
        // Match the resolver tests' default so the portrait/landscape dims below
        // resolve deterministically regardless of any earlier test's writes.
        crate::theme::set_artwork_auto_max_pct(0.40);
        guard
    }

    // 1 line: 1*16 + 8 (gap) + 20 (meta) + 9 (bottom) = 53.
    const ONE_LINE_H: f32 = 53.0;
    // 5 lines (MAX_LINES): 5*16 + 8 + 20 + 9 = 117.
    const MAX_LINES_H: f32 = 117.0;

    #[test]
    fn short_comment_renders_verbatim_at_one_line() {
        let (display, height) = playlist_strip_detail("Short note", 555.0);
        assert_eq!(display, "Short note", "a one-line comment is not altered");
        assert!(
            (height - ONE_LINE_H).abs() < f32::EPSILON,
            "short comment reserves exactly one line + meta row, got {height}"
        );
    }

    #[test]
    fn empty_comment_still_reserves_one_line() {
        let (display, height) = playlist_strip_detail("", 555.0);
        assert_eq!(display, "");
        assert!((height - ONE_LINE_H).abs() < f32::EPSILON);
    }

    #[test]
    fn overflowing_comment_is_ellipsized_and_height_caps() {
        // ~600 chars at a narrow width vastly exceeds the 5-line cap.
        let comment = "word ".repeat(120);
        let (display, height) = playlist_strip_detail(&comment, 200.0);
        assert!(
            display.ends_with('…'),
            "an overflowing comment must end with an ellipsis"
        );
        assert!(
            display.chars().count() < comment.chars().count(),
            "the display string must be shorter than the source"
        );
        assert!(
            (height - MAX_LINES_H).abs() < f32::EPSILON,
            "height must cap at MAX_LINES so the meta row stays in view, got {height}"
        );
    }

    #[test]
    fn comment_at_the_cap_is_not_truncated() {
        // cols = floor(555/7.3) = 76; 5 lines worth ≈ 380 chars. 300 fits.
        let comment = "x".repeat(300);
        let (display, _height) = playlist_strip_detail(&comment, 555.0);
        assert!(
            !display.ends_with('…'),
            "a comment within the 5-line budget keeps its full text"
        );
        assert_eq!(display.chars().count(), 300);
    }

    #[test]
    fn top_separator_only_for_auto_portrait_fallback_with_playlist() {
        use nokkvi_data::types::player_settings::ArtworkColumnMode;
        let _g = with_auto_artwork_mode();
        // 530 × 1430 → Auto resolves to a Vertical (portrait) artwork column
        // stacked flush above the banner, so it needs the top hairline.
        assert!(
            playlist_strip_needs_top_separator(true, 530.0, 1430.0),
            "Auto portrait fallback + playlist loaded → banner needs a top hairline"
        );
        // No playlist loaded → no banner at all → never a separator.
        assert!(
            !playlist_strip_needs_top_separator(false, 530.0, 1430.0),
            "no playlist → no banner → no hairline regardless of layout"
        );
        // 1920 × 1080 → Horizontal artwork (side column); the banner sits below
        // the nav chrome and already reads as separated.
        assert!(
            !playlist_strip_needs_top_separator(true, 1920.0, 1080.0),
            "horizontal artwork → no top hairline"
        );
        // 766 × 1370 → portrait but the artwork hides (would letterbox), so
        // nothing large sits above the banner.
        assert!(
            !playlist_strip_needs_top_separator(true, 766.0, 1370.0),
            "portrait with hidden artwork → no top hairline"
        );
        // Always-Vertical* modes also resolve to Vertical, but render a drag
        // handle between the artwork and the banner — that handle separates
        // them, so the hairline stays scoped to the handle-less Auto fallback.
        crate::theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        assert!(
            !playlist_strip_needs_top_separator(true, 530.0, 1430.0),
            "Always-Vertical mode has a drag handle separating the banner → no hairline"
        );
        // Leave the shared atomic back at the default for sibling test families.
        crate::theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
    }

    #[test]
    fn band_width_excludes_only_the_horizontal_artwork_column() {
        let _g = with_auto_artwork_mode();
        // Horizontal: the strip's content band is the pane minus the artwork
        // column, so it's narrower than the full window.
        let horizontal = playlist_strip_band_width(1920.0, 1080.0);
        assert!(
            horizontal > 0.0 && horizontal < 1920.0,
            "horizontal artwork narrows the strip band, got {horizontal}"
        );
        // Vertical: artwork stacks above, leaving the strip full-width.
        let vertical = playlist_strip_band_width(530.0, 1430.0);
        assert!(
            (vertical - 530.0).abs() < 1e-3,
            "vertical artwork leaves the strip full-width, got {vertical}"
        );
    }
}
