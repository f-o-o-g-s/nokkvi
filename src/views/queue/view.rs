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

use super::{
    QueueColumn, QueueContextEntry, QueueMessage, QueuePage, QueueSortMode, QueueViewData,
};
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
/// Height of the playlist edit-mode header. Taller than the read-only band
/// because it stacks an eyebrow over the name + comment inputs.
pub(crate) const PLAYLIST_EDIT_BAR_H: f32 = 60.0;
/// Height of the hover-expanded detail block, sized to fit the comment plus
/// the meta row. `ui_font()` renders monospace, so the wrapped line count is
/// predictable from the character count and available width — letting the band
/// fit its content instead of reserving a fixed slab of dead space under short
/// comments. Capped at `MAX_LINES` (longer comments clip via the container's
/// `clip(true)`) so a wall-of-text comment can't swallow the queue. The same
/// value drives the band height and the slot-list chrome math, keeping them in
/// lockstep.
fn playlist_strip_detail_height(comment: &str, content_width: f32) -> f32 {
    const LINE_H: f32 = 16.0;
    const CHAR_W: f32 = 7.3;
    const META_ROW_H: f32 = 20.0;
    const ROW_GAP: f32 = 8.0;
    const BOTTOM_PAD: f32 = 9.0;
    const MAX_LINES: f32 = 5.0;
    let cols = (content_width / CHAR_W).floor().max(1.0);
    let lines = (comment.chars().count() as f32 / cols)
        .ceil()
        .clamp(1.0, MAX_LINES);
    lines * LINE_H + ROW_GAP + META_ROW_H + BOTTOM_PAD
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

/// Linear blend of `base` toward `toward` by `t` (0..1), preserving `base`'s
/// alpha. Used for the strip's faint accent wash over `bg0_soft()`.
fn blend_toward(base: iced::Color, toward: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: base.r + (toward.r - base.r) * t,
        g: base.g + (toward.g - base.g) * t,
        b: base.b + (toward.b - base.b) * t,
        a: base.a,
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
        // Indices match the order in `items` below; the closure converts
        // them back to `QueueColumn` variants for the toggle message.
        let column_dropdown: Element<'a, QueueMessage> =
            crate::widgets::checkbox_dropdown::view_columns_dropdown(
                crate::View::Queue,
                vec![
                    (QueueColumn::Select, "Select", self.column_visibility.select),
                    (QueueColumn::Index, "Index", self.column_visibility.index),
                    (
                        QueueColumn::Thumbnail,
                        "Thumbnail",
                        self.column_visibility.thumbnail,
                    ),
                    (QueueColumn::Stars, "Stars", self.column_visibility.stars),
                    (QueueColumn::Album, "Album", self.column_visibility.album),
                    (QueueColumn::Genre, "Genre", self.column_visibility.genre),
                    (
                        QueueColumn::Duration,
                        "Duration",
                        self.column_visibility.duration,
                    ),
                    (QueueColumn::Love, "Love", self.column_visibility.love),
                    (QueueColumn::Plays, "Plays", self.column_visibility.plays),
                ],
                QueueMessage::ToggleColumnVisible,
                QueueMessage::SetOpenMenu,
                data.overlay.column_dropdown_open,
                data.overlay.column_dropdown_trigger_bounds,
            )
            .into();

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
        });

        // Build final header: regular header + optional edit mode bar.
        //
        // Expanded read-only-strip detail height, sized to the comment so the
        // band reserves no dead space below short comments (monospace estimate).
        // Shared by the band and the chrome math below so they stay in sync.
        let playlist_detail_h = data.playlist_context_info.as_ref().map_or(0.0, |ctx| {
            playlist_strip_detail_height(&ctx.comment, (data.window_width - 73.0).max(120.0))
        });

        // Every branch produces the same `column![extra, sep, header]` shape so
        // iced's positional reconciler keeps the search `text_input::Id` stable
        // across edit / playlist-context / read-only mode toggles. In read-only
        // mode `extra` and `sep` are zero-sized `Space` placeholders.
        let extra: Element<'a, QueueMessage> = if let Some((ref name, _)) = data.edit_mode_info {
            use iced::widget::svg;

            let accent = crate::theme::accent();

            // Eyebrow mirrors the read-only banner's "PLAYING FROM PLAYLIST".
            let eyebrow = iced::widget::text("EDITING PLAYLIST")
                .font(iced::font::Font {
                    weight: iced::font::Weight::Semibold,
                    ..crate::theme::ui_font()
                })
                .size(9.5)
                .color(accent)
                .wrapping(iced::widget::text::Wrapping::None);

            let name_input = iced::widget::text_input("Playlist name", name)
                .on_input(QueueMessage::PlaylistNameChanged)
                .font(iced::font::Font {
                    weight: iced::font::Weight::Bold,
                    ..crate::theme::ui_font()
                })
                .size(14)
                .width(Length::Fill)
                .padding([2, 4])
                .style(|_theme, _status| iced::widget::text_input::Style {
                    background: iced::Background::Color(iced::Color::TRANSPARENT),
                    border: iced::Border {
                        color: crate::theme::bg3(),
                        width: 0.0,
                        radius: crate::theme::ui_border_radius(),
                    },
                    icon: crate::theme::fg0(),
                    placeholder: crate::theme::fg2(),
                    value: crate::theme::fg0(),
                    selection: crate::theme::selection_color(),
                });

            // Comment text input — lighter, smaller, visually secondary
            let comment_value = data.edit_mode_comment.as_deref().unwrap_or_default();
            let comment_input = iced::widget::text_input("Comment", comment_value)
                .on_input(QueueMessage::PlaylistCommentChanged)
                .font(crate::theme::ui_font())
                .size(11)
                .width(Length::Fill)
                .padding([2, 4])
                .style(|_theme, _status| iced::widget::text_input::Style {
                    background: iced::Background::Color(iced::Color::TRANSPARENT),
                    border: iced::Border {
                        color: crate::theme::bg3(),
                        width: 0.0,
                        radius: crate::theme::ui_border_radius(),
                    },
                    icon: crate::theme::fg2(),
                    placeholder: crate::theme::fg2(),
                    value: crate::theme::fg2(),
                    selection: crate::theme::selection_color(),
                });

            // Icon-only action button — mouse_area + HoverOverlay(container) so the
            // press scale effect fires (native button captures ButtonPressed first).
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

            // Public/Private toggle — accent when public (default), muted when
            // private. Built inline (not via `icon_btn`) so the icon path and
            // tint can vary with the current state.
            let is_public = data.edit_mode_public.unwrap_or(true);
            let public_toggle: Element<'a, QueueMessage> = {
                let icon_path = if is_public {
                    "assets/icons/lock-open.svg"
                } else {
                    "assets/icons/lock.svg"
                };
                let tint = if is_public {
                    crate::theme::accent()
                } else {
                    crate::theme::fg2()
                };
                let tooltip_label = if is_public {
                    "Public — click to make private"
                } else {
                    "Private — click to make public"
                };
                let icon = crate::embedded_svg::svg_widget(icon_path)
                    .width(Length::Fixed(14.0))
                    .height(Length::Fixed(14.0))
                    .style(move |_theme, _status| svg::Style { color: Some(tint) });
                let trigger = mouse_area(
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
                .on_press(QueueMessage::PlaylistEditPublicToggled(!is_public))
                .interaction(iced::mouse::Interaction::Pointer);
                iced::widget::tooltip(
                    trigger,
                    container(
                        iced::widget::text(tooltip_label)
                            .size(11.0)
                            .font(crate::theme::ui_font()),
                    )
                    .padding(4),
                    iced::widget::tooltip::Position::Bottom,
                )
                .gap(4)
                .style(crate::theme::container_tooltip)
                .into()
            };

            let save_btn = icon_btn("assets/icons/save.svg", QueueMessage::SavePlaylist);
            let discard_btn = icon_btn("assets/icons/x.svg", QueueMessage::DiscardEdits);

            // Accent stripe + faint wash mirror the read-only banner chrome.
            let stripe = container(Space::new())
                .width(Length::Fixed(3.0))
                .height(Length::Fill)
                .style(move |_theme| container::Style {
                    background: Some(accent.into()),
                    ..Default::default()
                });

            // Left: eyebrow over the name + comment inputs (mirrors the banner's
            // eyebrow/name stack). Right: the action icons grouped as a tidy set.
            let left = column![eyebrow, name_input, comment_input]
                .spacing(2)
                .width(Length::Fill);
            let actions = row![public_toggle, save_btn, discard_btn]
                .spacing(2)
                .align_y(Alignment::Center);

            let content = container(
                row![left, actions]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .width(Length::Fill)
                    .padding(iced::Padding {
                        top: 0.0,
                        right: 13.0,
                        bottom: 0.0,
                        left: 11.0,
                    }),
            )
            .center_y(Length::Fixed(PLAYLIST_EDIT_BAR_H))
            .width(Length::Fill);

            let wash = blend_toward(crate::theme::bg0_soft(), accent, 0.07);

            container(
                row![stripe, content]
                    .width(Length::Fill)
                    .height(Length::Fixed(PLAYLIST_EDIT_BAR_H)),
            )
            .width(Length::Fill)
            .height(Length::Fixed(PLAYLIST_EDIT_BAR_H))
            .style(move |_theme| container::Style {
                background: Some(wash.into()),
                ..Default::default()
            })
            .into()
        } else if let Some(ref ctx) = data.playlist_context_info {
            // Read-only "Playing From" banner (Direction 2). Renders only while a
            // playlist is loaded for playback and not editing (the edit arm above
            // takes precedence). Hovering the band reveals a detail block; the
            // banner grows in flow and the slot-list chrome height tracks it.
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
                .font(iced::font::Font {
                    weight: iced::font::Weight::Semibold,
                    ..crate::theme::ui_font()
                })
                .size(9.5)
                .color(accent)
                .wrapping(iced::widget::text::Wrapping::None);
            let name = iced::widget::text(ctx.name.clone())
                .font(iced::font::Font {
                    weight: iced::font::Weight::Bold,
                    ..crate::theme::ui_font()
                })
                .size(14)
                .color(crate::theme::fg0())
                .wrapping(iced::widget::text::Wrapping::None);
            let name_stack = container(column![eyebrow, name].spacing(1))
                .width(Length::Fill)
                .clip(true);

            let save_btn = icon_btn("assets/icons/save.svg", QueueMessage::QuickSavePlaylist);
            let edit_btn = icon_btn("assets/icons/pencil-line.svg", QueueMessage::EditPlaylist);

            // Compact row. The cover is pushed only when its handle is cached so
            // an absent cover leaves no phantom leading gap.
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
            if let Some(handle) = data.playlist_cover {
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

                let comment_text = iced::widget::text(ctx.comment.clone())
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
            // every theme without gradient-API risk).
            let wash = blend_toward(crate::theme::bg0_soft(), accent, 0.07);

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
        let sep: Element<'a, QueueMessage> =
            if data.edit_mode_info.is_some() || data.playlist_context_info.is_some() {
                crate::theme::horizontal_separator(1.0)
            } else {
                Space::new()
                    .width(Length::Shrink)
                    .height(Length::Fixed(0.0))
                    .into()
            };
        let header: Element<'a, QueueMessage> = column![extra, sep, header].into();

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. The bar's tri-state derives from the
        // current selection set against the *filtered* (visible) row count.
        let header = crate::widgets::slot_list::compose_header_with_select(
            self.column_visibility.select,
            self.common.select_all_state(data.queue_songs.len()),
            QueueMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
            header,
        );

        // Configure slot list with queue-specific chrome height (with view header now)
        // Edit mode adds a 44px bar + context bar adds 32px bar; account for the tallest so
        // the last slot isn't shorter than the rest.
        use crate::widgets::slot_list::{
            chrome_height_with_header, chrome_height_with_select_header,
        };
        let select_header_visible = self.column_visibility.select;
        let chrome_height = if data.edit_mode_info.is_some() {
            // Edit-mode header + 1px separator.
            chrome_height_with_header() + PLAYLIST_EDIT_BAR_H + 1.0
        } else if data.playlist_context_info.is_some() {
            // Compact "Playing From" banner + 1px separator, plus the detail
            // block height when the strip is hover-expanded (grow-in-flow).
            let strip = if data.playlist_strip_expanded {
                PLAYLIST_STRIP_COMPACT_H + playlist_detail_h
            } else {
                PLAYLIST_STRIP_COMPACT_H
            };
            chrome_height_with_header() + strip + 1.0
        } else {
            chrome_height_with_select_header(select_header_visible)
        };
        let chrome_height = if select_header_visible
            && (data.edit_mode_info.is_some() || data.playlist_context_info.is_some())
        {
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
        let current_sort_mode = self.queue_sort_mode; // For conditional column/genre display
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
                sort_mode: current_sort_mode,
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

        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_menu;

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
        let artwork_content = Some(single_artwork_panel_with_menu(
            center_artwork_handle,
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
