//! Queue view — `impl QueuePage { fn view }`.
//!
//! Rendering for the queue page, plus the per-mode column-visibility helpers
//! and the `BREAKPOINT_HIDE_QUEUE_STARS` constant. Update/state logic lives
//! in `update.rs`; types live in `mod.rs`.

use iced::{
    Alignment, Element, Length,
    widget::{Row, Space, column, container, mouse_area, row},
};
use nokkvi_data::backend::queue::QueueSongUIViewData;

use super::{
    QueueColumn, QueueContextEntry, QueueMessage, QueuePage, QueueSortMode, QueueViewData,
};
use crate::widgets::{
    self,
    hover_overlay::HoverOverlay,
    view_header::{HeaderButton, ViewHeaderConfig},
};

/// Hide the queue stars column when the queue panel is narrower than this.
/// Queue panel is measured (via `iced::widget::responsive`), so this fires
/// correctly in split-view where the queue is roughly half the window.
pub(crate) const BREAKPOINT_HIDE_QUEUE_STARS: f32 = 400.0;

/// Pure decision: should the queue's stars rating column be rendered?
///
/// Two independent gates: the user toggle (always wins when off) and the
/// responsive width gate (always wins when below the breakpoint).
pub(crate) fn rating_column_visible(
    _sort: QueueSortMode,
    panel_width: f32,
    user_visible: bool,
) -> bool {
    user_visible && panel_width >= BREAKPOINT_HIDE_QUEUE_STARS
}

/// Pure decision: should the album column be rendered? User toggle only —
/// no responsive gate yet (the album column carries inline genre when
/// sort = Genre, so hiding it on narrow widths is a separate question).
pub(crate) fn album_column_visible(user_visible: bool) -> bool {
    user_visible
}

/// Pure decision: should the duration column be rendered? User toggle only.
pub(crate) fn duration_column_visible(user_visible: bool) -> bool {
    user_visible
}

/// Pure decision: should the love (heart) column be rendered? User toggle only.
pub(crate) fn love_column_visible(user_visible: bool) -> bool {
    user_visible
}

/// Pure decision: should the plays column be rendered? Either the user toggle
/// is on, OR the queue is sorted by Most Played (auto-show so the user always
/// sees the data they're sorting by).
pub(crate) fn plays_column_visible(sort: QueueSortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, QueueSortMode::MostPlayed)
}

/// Pure decision: should the genre be rendered (stacked under album, or in
/// place of the album when album is hidden)? Toggle on, OR queue is sorted by
/// Genre — mirrors the plays-on-MostPlayed auto-show.
pub(crate) fn genre_column_visible(sort: QueueSortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, QueueSortMode::Genre)
}

impl QueuePage {
    /// Build the view
    pub fn view<'a>(&'a self, data: QueueViewData<'a>) -> Element<'a, QueueMessage> {
        use crate::widgets::slot_list::{SlotListConfig, SlotListRowContext};

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
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into();

        // When the user has enabled the default-playlist chip, render it
        // alongside the column-visibility dropdown in the trailing slot.
        // Order: chip first, then column dropdown — chip claims the more
        // prominent left-of-trailing position.
        let trailing: Element<'a, QueueMessage> = if data.show_default_playlist_chip {
            let chip = crate::widgets::default_playlist_chip::default_playlist_chip(
                data.default_playlist_name,
                QueueMessage::OpenDefaultPlaylistPicker,
            );
            iced::widget::row![chip, column_dropdown]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            column_dropdown
        };

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
                if let Some(idx) = data.current_playing_queue_index {
                    btns.push(HeaderButton::CenterOnPlaying(
                        QueueMessage::FocusCurrentPlaying(idx, true),
                    ));
                }
                btns.push(HeaderButton::Trailing(trailing));
                btns
            },
            on_roulette: Some(QueueMessage::Roulette),
        });

        // Build final header: regular header + optional edit mode bar.
        //
        // Every branch produces the same `column![extra, sep, header]` shape so
        // iced's positional reconciler keeps the search `text_input::Id` stable
        // across edit / playlist-context / read-only mode toggles. In read-only
        // mode `extra` and `sep` are zero-sized `Space` placeholders.
        let extra: Element<'a, QueueMessage> = if let Some((ref name, _)) = data.edit_mode_info {
            use iced::widget::svg;

            // Pencil-line icon to indicate editing
            let edit_icon = crate::embedded_svg::svg_widget("assets/icons/pencil-line.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::accent()),
                });

            let name_input = iced::widget::text_input("Playlist name", name)
                .on_input(QueueMessage::PlaylistNameChanged)
                .font(iced::font::Font {
                    weight: iced::font::Weight::Medium,
                    ..crate::theme::ui_font()
                })
                .size(12)
                .width(Length::FillPortion(3))
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
                .width(Length::FillPortion(2))
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

            let name_comment_col: Element<'a, QueueMessage> =
                iced::widget::column![name_input, comment_input]
                    .spacing(1)
                    .width(Length::Fill)
                    .into();

            let edit_bar = container(
                row![
                    edit_icon,
                    name_comment_col,
                    public_toggle,
                    save_btn,
                    discard_btn,
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .padding([0, 8])
                .width(Length::Fill),
            )
            .height(Length::Fixed(44.0))
            .style(|_theme| container::Style {
                background: Some(crate::theme::bg0_soft().into()),
                ..Default::default()
            })
            .width(Length::Fill);

            edit_bar.into()
        } else if let Some(ref ctx) = data.playlist_context_info {
            // Read-only playlist context bar (playing a playlist, not editing)
            use iced::widget::svg;

            let playlist_icon = crate::embedded_svg::svg_widget("assets/icons/list-music.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::accent()),
                });

            let name_label = iced::widget::text(ctx.name.clone())
                .font(iced::font::Font {
                    weight: iced::font::Weight::Medium,
                    ..crate::theme::ui_font()
                })
                .size(12)
                .color(crate::theme::fg0());

            // Build name + optional comment as a column, constrained to prevent overflow.
            // Without a width constraint, long comments expand to intrinsic text width
            // and push save/edit icons off-screen, cascading layout breakage.
            let name_area: Element<'a, QueueMessage> = if ctx.comment.is_empty() {
                container(name_label).width(Length::Fill).clip(true).into()
            } else {
                let comment_label = iced::widget::text(ctx.comment.clone())
                    .font(crate::theme::ui_font())
                    .size(10)
                    .color(crate::theme::fg2())
                    .wrapping(iced::widget::text::Wrapping::None);
                container(column![name_label, comment_label].spacing(1))
                    .width(Length::Fill)
                    .clip(true)
                    .into()
            };

            // Save button — quick-saves the current queue back to this playlist
            let save_icon = crate::embedded_svg::svg_widget("assets/icons/save.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::fg2()),
                });

            let save_btn: Element<'a, QueueMessage> = mouse_area(
                HoverOverlay::new(
                    container(save_icon)
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
            .on_press(QueueMessage::QuickSavePlaylist)
            .interaction(iced::mouse::Interaction::Pointer)
            .into();

            // Edit button — enters split-view playlist edit mode
            let edit_icon = crate::embedded_svg::svg_widget("assets/icons/pencil-line.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::fg2()),
                });

            let edit_btn: Element<'a, QueueMessage> = mouse_area(
                HoverOverlay::new(
                    container(edit_icon)
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
            .on_press(QueueMessage::EditPlaylist)
            .interaction(iced::mouse::Interaction::Pointer)
            .into();

            let playlist_bar = container(
                row![playlist_icon, name_area, save_btn, edit_btn]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .padding([0, 8])
                    .width(Length::Fill),
            )
            .height(Length::Fixed(32.0))
            .style(|_theme| container::Style {
                background: Some(crate::theme::bg0_soft().into()),
                ..Default::default()
            })
            .width(Length::Fill);

            playlist_bar.into()
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
            chrome_height_with_header() + 45.0 // 44px edit bar + 1px separator
        } else if data.playlist_context_info.is_some() {
            chrome_height_with_header() + 33.0 // 32px context bar + 1px separator
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
        let current_playing_queue_index = data.current_playing_queue_index;
        let current_sort_mode = self.queue_sort_mode; // For conditional column/genre display
        let album_art = data.album_art; // Move artwork maps
        let large_artwork = data.large_artwork;
        let queue_songs = data.queue_songs; // Move ownership to extend lifetime
        // User-toggle gates from the columns dropdown; combined with responsive
        // gates inside the per-row `responsive(...)` closure below.
        let column_visibility = self.column_visibility;
        let show_album_column = album_column_visible(column_visibility.album);
        let show_genre_column = genre_column_visible(current_sort_mode, column_visibility.genre);
        let show_duration_column = duration_column_visible(column_visibility.duration);
        let show_love_column = love_column_visible(column_visibility.love);
        let show_plays_column = plays_column_visible(current_sort_mode, column_visibility.plays);

        // Build the render_item closure (shared between drag and non-drag paths)
        let render_item = |song: &QueueSongUIViewData,
                           ctx: SlotListRowContext|
         -> Element<'a, QueueMessage> {
            // Clone all data from song at the start to avoid lifetime issues
            let title = song.title.clone();
            let artist = song.artist.clone();
            let album = song.album.clone();
            let album_id = song.album_id.clone();
            let duration = song.duration.clone();
            let genre = song.genre.clone();
            let starred = song.starred;
            let rating = song.rating.unwrap_or(0).min(5) as usize;
            let play_count = song.play_count.unwrap_or(0);
            let song_id = song.id.clone();
            let artist_id = song.artist_id.clone();
            let track_number = song.track_number;
            let stable_viewport = data.stable_viewport;

            // Match on both song ID AND queue position (track_number) to
            // disambiguate duplicate tracks sharing the same song ID.
            // Suppressed while ctrl/shift is held (active multi-selection) so
            // users can clearly see which items are selected.
            let is_current = !(ctx.modifiers.shift() || ctx.modifiers.control())
                && current_playing_queue_index.is_some_and(|idx| idx == track_number as usize - 1)
                && current_playing_song_id.as_ref() == Some(&song_id);

            // Wrap the row in `responsive(...)` so the queue-stars column hide
            // is gated by the queue panel's measured width rather than the full
            // window width. This is correct in split-view (Ctrl+E), where the
            // queue panel is roughly half the window.
            let responsive_row = iced::widget::responsive(move |size| {
                let panel_width = size.width;

                // Re-clone owned values each layout pass: the responsive
                // closure is `Fn`, so it borrows captured strings; the row
                // builders below take owned values.
                let title = title.clone();
                let artist = artist.clone();
                let album = album.clone();
                let album_id = album_id.clone();
                let duration = duration.clone();
                let genre = genre.clone();
                let artist_id = artist_id.clone();

                // Get centralized slot list slot styling
                use crate::widgets::slot_list::{
                    SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
                    slot_list_text,
                };
                let style = SlotListSlotStyle::for_slot(
                    ctx.is_center,
                    is_current,
                    ctx.is_selected,
                    ctx.has_multi_selection,
                    ctx.opacity,
                    0,
                );

                let m = ctx.metrics;
                let artwork_size = m.artwork_size;
                let title_size = m.title_size_lg;
                let subtitle_size = m.subtitle_size;
                let index_size = m.metadata_size;
                let duration_size = m.metadata_size;
                let icon_size = m.star_size;

                // Dynamic column proportions: title gets more space when album/rating columns are hidden
                let show_rating_column =
                    rating_column_visible(current_sort_mode, panel_width, column_visibility.stars);
                let title_portion: u16 = if show_rating_column { 35 } else { 40 };

                // Layout: [Index?] [Thumbnail?] [Title/Artist] [Album?] [Rating?] [Duration] [Heart]
                let mut content_row = Row::new().spacing(6.0).align_y(Alignment::Center);
                if column_visibility.index {
                    content_row = content_row.push(slot_list_index_column(
                        ctx.item_index,
                        index_size,
                        style,
                        ctx.opacity,
                    ));
                }
                if column_visibility.thumbnail {
                    use crate::widgets::slot_list::slot_list_artwork_column;
                    content_row = content_row.push(slot_list_artwork_column(
                        album_art.get(&album_id),
                        artwork_size,
                        ctx.is_center,
                        is_current,
                        ctx.opacity,
                    ));
                }
                content_row = content_row.push({
                    use crate::widgets::slot_list::slot_list_text_column;
                    let title_click = Some(QueueMessage::ContextMenuAction(
                        ctx.item_index,
                        QueueContextEntry::GetInfo,
                    ));
                    slot_list_text_column(
                        title,
                        title_click,
                        artist.clone(),
                        Some(QueueMessage::NavigateAndExpandArtist(artist_id.clone())),
                        title_size,
                        subtitle_size,
                        style,
                        ctx.is_center || is_current,
                        title_portion,
                    )
                });

                // 3. Album / genre column — slot renders when either is visible.
                //    Both → column![album, small_genre]. Album only → album.
                //    Genre only → genre at album-size font, vertically centered.
                if show_album_column || show_genre_column {
                    content_row = content_row.push(
                        container({
                            let links_enabled = crate::theme::is_slot_text_links();
                            let click_album =
                                QueueMessage::NavigateAndExpandAlbum(album_id.clone());
                            let click_genre = QueueMessage::NavigateAndExpandGenre(genre.clone());
                            let genre_label = if genre.is_empty() {
                                "Unknown".to_string()
                            } else {
                                genre.clone()
                            };
                            let stacked_genre_size = nokkvi_data::utils::scale::calculate_font_size(
                                10.0,
                                ctx.row_height,
                                ctx.scale_factor,
                            ) * ctx.scale_factor;
                            let make_link =
                                |label: String,
                                 font_size: f32,
                                 click: QueueMessage|
                                 -> Element<'_, QueueMessage> {
                                    crate::widgets::link_text::LinkText::new(label)
                                        .size(font_size)
                                        .color(style.subtext_color)
                                        .hover_color(style.hover_text_color)
                                        .font(crate::theme::ui_font())
                                        .on_press(if links_enabled { Some(click) } else { None })
                                        .into()
                                };
                            let content: Element<'_, QueueMessage> =
                                match (show_album_column, show_genre_column) {
                                    (true, true) => {
                                        let album_widget =
                                            make_link(album, subtitle_size, click_album);
                                        let genre_widget =
                                            make_link(genre_label, stacked_genre_size, click_genre);
                                        column![album_widget, genre_widget].spacing(2.0).into()
                                    }
                                    (true, false) => make_link(album, subtitle_size, click_album),
                                    (false, true) => {
                                        make_link(genre_label, subtitle_size, click_genre)
                                    }
                                    (false, false) => unreachable!(),
                                };
                            content
                        })
                        .width(Length::FillPortion(30))
                        .height(Length::Fill)
                        .clip(true)
                        .align_y(Alignment::Center),
                    );
                }

                // 4. Rating column — only shown for Rating sort mode (dedicated column, not inline with title)
                if show_rating_column {
                    let star_icon_size = m.title_size;
                    let idx = ctx.item_index;
                    use crate::widgets::slot_list::slot_list_star_rating;
                    content_row = content_row.push(slot_list_star_rating(
                        rating,
                        star_icon_size,
                        ctx.is_center,
                        ctx.opacity,
                        Some(15),
                        Some(move |star: usize| QueueMessage::ClickSetRating(idx, star)),
                    ));
                }

                // 5. Duration - right aligned (user-toggleable)
                if show_duration_column {
                    content_row = content_row.push(
                        container(slot_list_text(duration, duration_size, style.subtext_color))
                            .width(Length::FillPortion(10))
                            .align_x(Alignment::End)
                            .align_y(Alignment::Center),
                    );
                }

                // 6. Plays - right aligned. User-toggleable, also auto-shown
                // when sort = MostPlayed.
                if show_plays_column {
                    content_row = content_row.push(
                        container(slot_list_text(
                            format!("{play_count} plays"),
                            duration_size,
                            style.subtext_color,
                        ))
                        .width(Length::FillPortion(10))
                        .align_x(Alignment::End)
                        .align_y(Alignment::Center),
                    );
                }

                // 7. Heart Icon - use reusable component, with symmetric padding
                // for centering (user-toggleable via columns dropdown).
                if show_love_column {
                    content_row = content_row.push(
                        container({
                            use crate::widgets::slot_list::slot_list_favorite_icon;
                            slot_list_favorite_icon(
                                starred,
                                ctx.is_center,
                                is_current,
                                ctx.opacity,
                                icon_size,
                                "heart",
                                Some(QueueMessage::ClickToggleStar(ctx.item_index)),
                            )
                        })
                        .width(Length::FillPortion(5))
                        .padding(iced::Padding {
                            left: 4.0,
                            right: 4.0,
                            ..Default::default()
                        })
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center),
                    );
                }

                // When the love column is hidden, the rightmost trailing
                // column (duration or plays) sits flush against the slot
                // edge — bump the row's right padding to restore the
                // breathing room the love column would have provided.
                let row_right_padding = if show_love_column { 4.0 } else { 12.0 };
                let content = content_row
                    .padding(iced::Padding {
                        left: SLOT_LIST_SLOT_PADDING,
                        right: row_right_padding,
                        top: 4.0,
                        bottom: 4.0,
                    })
                    .height(Length::Fill);

                // Wrap in clickable container
                let clickable = container(content)
                    .style(move |_theme| style.to_container_style())
                    .width(Length::Fill);

                // Make it interactive
                let slot_button = crate::widgets::slot_list::primary_slot_button(
                    clickable,
                    &ctx,
                    stable_viewport,
                    QueueMessage::SlotList,
                );

                // Wrap in context menu
                use crate::widgets::context_menu::{context_menu, menu_button, menu_separator};
                let item_idx = ctx.item_index;
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
                    crate::widgets::context_menu::open_state_for(data.open_menu, &cm_id);
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
            });
            crate::widgets::slot_list::wrap_with_select_column(
                column_visibility.select,
                ctx.is_selected,
                ctx.item_index,
                |idx| {
                    QueueMessage::SlotList(crate::widgets::SlotListPageMessage::SelectionToggle(
                        idx,
                    ))
                },
                responsive_row.into(),
            )
        };

        // Build slot list content: always use DragColumn so we detect drag attempts
        // (toast shown if drag is disabled for current sort/search state)
        let slot_list_content = {
            use crate::widgets::slot_list::slot_list_view_with_drag;
            slot_list_view_with_drag(
                &self.common.slot_list,
                &queue_songs,
                &config,
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateUp),
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateDown),
                {
                    let total = queue_songs.len();
                    move |f| {
                        QueueMessage::SlotList(crate::widgets::SlotListPageMessage::ScrollSeek(
                            (f * total as f32) as usize,
                        ))
                    }
                },
                QueueMessage::DragReorder,
                Some(crate::widgets::slot_list::SlotHoverCallback::new(
                    |h| {
                        QueueMessage::SlotList(crate::widgets::SlotListPageMessage::HoverEnterSlot(
                            h,
                        ))
                    },
                    |h| {
                        QueueMessage::SlotList(crate::widgets::SlotListPageMessage::HoverExitSlot(
                            h,
                        ))
                    },
                )),
                data.drop_indicator_slot,
                render_item,
            )
        };

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        let slot_list_content: Element<'a, QueueMessage> = slot_list_content;

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
        let artwork_menu_id = crate::app_message::ContextMenuId::ArtworkPanel(crate::View::Queue);
        let (artwork_menu_open, artwork_menu_position) =
            crate::widgets::context_menu::open_state_for(data.open_menu, &artwork_menu_id);
        let artwork_content = Some(single_artwork_panel_with_menu(
            center_artwork_handle,
            on_refresh,
            artwork_menu_open,
            artwork_menu_position,
            move |position| match position {
                Some(p) => QueueMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                    id: artwork_menu_id.clone(),
                    position: p,
                })),
                None => QueueMessage::SetOpenMenu(None),
            },
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
    use super::*;

    const WIDE_PANEL: f32 = 1200.0;

    #[test]
    fn rating_column_visible_for_all_sort_modes() {
        for sort in QueueSortMode::all() {
            assert!(
                rating_column_visible(sort, WIDE_PANEL, true),
                "stars column should render for sort mode {sort:?}"
            );
        }
    }

    #[test]
    fn rating_column_hidden_below_breakpoint() {
        for sort in QueueSortMode::all() {
            assert!(
                !rating_column_visible(sort, BREAKPOINT_HIDE_QUEUE_STARS - 1.0, true),
                "stars column should hide below breakpoint for {sort:?}"
            );
        }
    }

    #[test]
    fn rating_column_visible_at_breakpoint() {
        // Boundary is `>=`: the exact breakpoint width keeps stars visible.
        for sort in QueueSortMode::all() {
            assert!(
                rating_column_visible(sort, BREAKPOINT_HIDE_QUEUE_STARS, true),
                "stars column should remain visible at exact breakpoint for {sort:?}"
            );
        }
    }

    #[test]
    fn rating_column_responsive_overrides_sort_mode() {
        // Width wins over sort mode: even Rating sort hides when too narrow.
        assert!(!rating_column_visible(
            QueueSortMode::Rating,
            BREAKPOINT_HIDE_QUEUE_STARS - 1.0,
            true,
        ));
    }

    #[test]
    fn rating_column_user_toggle_off_overrides_wide_panel() {
        // User toggle wins over width: a wide panel still hides stars when
        // the user has toggled them off.
        for sort in QueueSortMode::all() {
            assert!(
                !rating_column_visible(sort, WIDE_PANEL, false),
                "user toggle off should hide stars even at wide panel ({sort:?})"
            );
        }
    }

    #[test]
    fn rating_column_responsive_still_hides_when_user_visible_true() {
        // The two gates AND together: user wants stars visible, but the
        // panel is too narrow → still hidden.
        assert!(!rating_column_visible(
            QueueSortMode::Album,
            BREAKPOINT_HIDE_QUEUE_STARS - 1.0,
            true,
        ));
    }

    #[test]
    fn album_column_visible_follows_user_toggle() {
        assert!(album_column_visible(true));
        assert!(!album_column_visible(false));
    }

    #[test]
    fn duration_column_visible_follows_user_toggle() {
        assert!(duration_column_visible(true));
        assert!(!duration_column_visible(false));
    }

    #[test]
    fn plays_column_visible_auto_shows_on_most_played() {
        // Sort overrides the user toggle: MostPlayed always shows, regardless of toggle.
        assert!(plays_column_visible(QueueSortMode::MostPlayed, false));
        assert!(plays_column_visible(QueueSortMode::MostPlayed, true));
    }

    #[test]
    fn plays_column_visible_follows_user_toggle_for_other_sorts() {
        assert!(!plays_column_visible(QueueSortMode::Title, false));
        assert!(plays_column_visible(QueueSortMode::Title, true));
        assert!(!plays_column_visible(QueueSortMode::Rating, false));
        assert!(plays_column_visible(QueueSortMode::Rating, true));
    }

    #[test]
    fn genre_column_visible_auto_shows_on_genre_sort() {
        assert!(genre_column_visible(QueueSortMode::Genre, false));
        assert!(genre_column_visible(QueueSortMode::Genre, true));
    }

    #[test]
    fn genre_column_visible_follows_user_toggle_for_other_sorts() {
        assert!(!genre_column_visible(QueueSortMode::Title, false));
        assert!(genre_column_visible(QueueSortMode::Title, true));
        assert!(!genre_column_visible(QueueSortMode::MostPlayed, false));
        assert!(genre_column_visible(QueueSortMode::MostPlayed, true));
    }
}
