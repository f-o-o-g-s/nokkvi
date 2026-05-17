//! Songs view — `impl SongsPage { fn view }`.
//!
//! Rendering for the songs page, plus the per-mode column-visibility helpers
//! (`songs_stars_visible`, `songs_plays_visible`, `songs_genre_visible`).
//!
//! Update/state logic lives in `update.rs`; types live in `mod.rs`.

use iced::{
    Alignment, Element, Length,
    widget::{Row, container},
};
use nokkvi_data::utils::formatters;

use super::{SongsColumn, SongsMessage, SongsPage, SongsViewData};
use crate::widgets::{
    self,
    view_header::{HeaderButton, SortMode, ViewHeaderConfig},
};

pub(crate) fn songs_stars_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::Rating)
}

pub(crate) fn songs_plays_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::MostPlayed)
}

/// Pure decision: should the genre be rendered (stacked under album, or in
/// place of album when album is hidden)? Toggle on, OR sort = Genre.
pub(crate) fn songs_genre_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::Genre)
}

impl SongsPage {
    /// Build the view
    pub fn view<'a>(&'a self, data: SongsViewData<'a>) -> Element<'a, SongsMessage> {
        let column_dropdown: Element<'a, SongsMessage> =
            crate::widgets::checkbox_dropdown::view_columns_dropdown(
                crate::View::Songs,
                vec![
                    (SongsColumn::Select, "Select", self.column_visibility.select),
                    (SongsColumn::Index, "Index", self.column_visibility.index),
                    (
                        SongsColumn::Thumbnail,
                        "Thumbnail",
                        self.column_visibility.thumbnail,
                    ),
                    (SongsColumn::Stars, "Stars", self.column_visibility.stars),
                    (SongsColumn::Album, "Album", self.column_visibility.album),
                    (SongsColumn::Genre, "Genre", self.column_visibility.genre),
                    (
                        SongsColumn::Duration,
                        "Duration",
                        self.column_visibility.duration,
                    ),
                    (SongsColumn::Plays, "Plays", self.column_visibility.plays),
                    (SongsColumn::Love, "Love", self.column_visibility.love),
                ],
                SongsMessage::ToggleColumnVisible,
                SongsMessage::SetOpenMenu,
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into();

        let header = widgets::view_header::view_header(ViewHeaderConfig {
            current_view: self.common.current_sort_mode,
            view_options: crate::views::sort_api::sort_modes_for_view(crate::View::Songs),
            sort_ascending: self.common.sort_ascending,
            search_query: &self.common.search_query,
            filtered_count: data.songs.len(),
            total_count: data.total_song_count,
            item_type: "songs",
            search_input_id: crate::views::SONGS_SEARCH_ID,
            on_view_selected: Box::new(|m| {
                SongsMessage::SlotList(crate::widgets::SlotListPageMessage::SortModeSelected(m))
            }),
            show_search: true,
            on_search_change: Box::new(|q| {
                SongsMessage::SlotList(crate::widgets::SlotListPageMessage::SearchQueryChanged(q))
            }),
            buttons: {
                let mut btns = vec![
                    HeaderButton::SortToggle(SongsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::ToggleSortOrder,
                    )),
                    HeaderButton::Refresh(SongsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::RefreshViewData,
                    )),
                ];
                // Hidden in the browsing panel — the narrower pane needs the
                // header space for sort/refresh/columns/search, and the user
                // already has the main-pane button when they want to center.
                if !data.in_browsing_panel {
                    btns.push(HeaderButton::CenterOnPlaying(SongsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::CenterOnPlaying,
                    )));
                }
                btns.push(HeaderButton::Trailing(column_dropdown));
                btns
            },
            // Roulette is main-pane only — see Albums view for rationale.
            on_roulette: if data.in_browsing_panel {
                None
            } else {
                Some(SongsMessage::Roulette)
            },
        });

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *visible* (filtered) row count.
        let header = crate::widgets::slot_list::compose_header_with_select(
            self.column_visibility.select,
            self.common.select_all_state(data.songs.len()),
            SongsMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
            header,
        );

        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let slot_list_chrome = chrome_height_with_select_header(select_header_visible);

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
            slot_list_chrome,
        };

        // If loading, show header with loading message
        if data.loading {
            return widgets::base_slot_list_empty_state(header, "Loading...", &layout_config);
        }

        // If no songs match search, show message but keep the header
        if data.songs.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No songs match your search.",
                &layout_config,
            );
        }

        let vertical_artwork_chrome =
            crate::widgets::base_slot_list_layout::vertical_artwork_chrome(&layout_config);
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            slot_list_chrome + vertical_artwork_chrome,
        )
        .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let songs = data.songs;
        let song_artwork = data.album_art;
        let current_sort_mode = self.common.current_sort_mode;
        let center_index = self.common.slot_list.get_center_item_index(songs.len());
        let open_menu_for_rows = data.open_menu;

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            songs,
            &config,
            SongsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateUp),
            SongsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateDown),
            {
                let total = songs.len();
                move |f| {
                    SongsMessage::SlotList(crate::widgets::SlotListPageMessage::ScrollSeek(
                        (f * total as f32) as usize,
                    ))
                }
            },
            Some(crate::widgets::slot_list::SlotHoverCallback::for_slot_list(
                SongsMessage::SlotList,
            )),
            |song, ctx| {
                // Clone all data from song at the start to avoid lifetime issues
                let song_title = song.title.clone();
                let song_artist = song.artist.clone();
                let song_album = song.album.clone();
                let song_genre = song.genre.clone().unwrap_or_default();
                let album_id = song.album_id.clone();
                let duration = song.duration;
                let is_starred = song.is_starred;
                let rating = song.rating.unwrap_or(0).min(5) as usize;

                // Get extra column value based on current sort mode
                let extra_value = nokkvi_data::backend::songs::SongsService::get_extra_column_data(
                    song,
                    Self::sort_mode_to_api_string(current_sort_mode),
                );

                // Get centralized slot list slot styling
                use crate::widgets::slot_list::{
                    SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
                    slot_list_text,
                };
                let style = SlotListSlotStyle::for_slot(
                    ctx.is_center,
                    false,
                    ctx.is_selected,
                    ctx.has_multi_selection,
                    ctx.opacity,
                    0,
                );

                let m = ctx.metrics;
                let artwork_size = m.artwork_size;
                let title_size = m.title_size_lg;
                let subtitle_size = m.subtitle_size;
                let metadata_size = m.metadata_size;
                let star_size = m.star_size;
                let index_size = m.metadata_size;
                let play_count = song.play_count.unwrap_or(0);

                // Per-column visibility (Stars/Plays/Genre auto-shown by sort mode).
                let vis = self.column_visibility;
                let show_stars = songs_stars_visible(current_sort_mode, vis.stars);
                let show_album = vis.album;
                let show_genre = songs_genre_visible(current_sort_mode, vis.genre);
                let show_plays = songs_plays_visible(current_sort_mode, vis.plays);
                let show_duration = vis.duration;
                let show_love = vis.love;
                // Dynamic slot carries year / BPM / channels / comment /
                // albumArtist for those sort modes. Genre lives in the album
                // column slot via `show_genre`, not here.
                let show_dynamic_slot = !extra_value.is_empty();

                const ALBUM_PORTION: u16 = 22;
                const STARS_PORTION: u16 = 11;
                const PLAYS_PORTION: u16 = 13;
                const DYNAMIC_PORTION: u16 = 18;
                const DURATION_PORTION: u16 = 10;
                const LOVE_PORTION: u16 = 5;
                let mut consumed: u16 = 0;
                if show_album || show_genre {
                    consumed += ALBUM_PORTION;
                }
                if show_stars {
                    consumed += STARS_PORTION;
                }
                if show_plays {
                    consumed += PLAYS_PORTION;
                }
                if show_dynamic_slot {
                    consumed += DYNAMIC_PORTION;
                }
                if show_duration {
                    consumed += DURATION_PORTION;
                }
                if show_love {
                    consumed += LOVE_PORTION;
                }
                let title_portion = 100u16.saturating_sub(consumed).max(20);

                let mut content_row = Row::new().spacing(6.0).align_y(Alignment::Center);
                if vis.index {
                    content_row = content_row.push(slot_list_index_column(
                        ctx.item_index,
                        index_size,
                        style,
                        ctx.opacity,
                    ));
                }
                if vis.thumbnail {
                    use crate::widgets::slot_list::slot_list_artwork_column;
                    let artwork_handle = album_id.as_ref().and_then(|id| song_artwork.get(id));
                    content_row = content_row.push(slot_list_artwork_column(
                        artwork_handle,
                        artwork_size,
                        ctx.is_center,
                        false,
                        ctx.opacity,
                    ));
                }
                content_row = content_row.push({
                    use crate::widgets::slot_list::slot_list_text_column;
                    let artist_click = song
                        .artist_id
                        .as_ref()
                        .map(|id| SongsMessage::NavigateAndExpandArtist(id.clone()));
                    let title_click = Some(SongsMessage::ContextMenuAction(
                        ctx.item_index,
                        crate::widgets::context_menu::LibraryContextEntry::GetInfo,
                    ));
                    slot_list_text_column(
                        song_title,
                        title_click,
                        song_artist,
                        artist_click,
                        title_size,
                        subtitle_size,
                        style,
                        ctx.is_center,
                        title_portion,
                    )
                });

                // Album / genre column — slot renders when either is visible.
                //   Both    → column![album, small_genre]
                //   Album   → album alone (today's behavior)
                //   Genre   → genre alone at album-size font, vertically centered
                if show_album || show_genre {
                    use iced::widget::column;
                    let album_click = song
                        .album_id
                        .as_ref()
                        .map(|id| SongsMessage::NavigateAndExpandAlbum(id.clone()));
                    let genre_click =
                        Some(SongsMessage::NavigateAndExpandGenre(song_genre.clone()));
                    let genre_label = if song_genre.is_empty() {
                        "Unknown".to_string()
                    } else {
                        song_genre.clone()
                    };
                    let stacked_genre_size = nokkvi_data::utils::scale::calculate_font_size(
                        10.0,
                        ctx.row_height,
                        ctx.scale_factor,
                    ) * ctx.scale_factor;
                    let links_enabled = crate::theme::is_slot_text_links();
                    let make_link = |label: String,
                                     font_size: f32,
                                     click: Option<SongsMessage>|
                     -> Element<'_, SongsMessage> {
                        crate::widgets::link_text::LinkText::new(label)
                            .size(font_size)
                            .color(style.subtext_color)
                            .hover_color(style.hover_text_color)
                            .font(crate::theme::ui_font())
                            .on_press(if links_enabled { click } else { None })
                            .into()
                    };
                    let content: Element<'_, SongsMessage> = match (show_album, show_genre) {
                        (true, true) => {
                            let album_widget = make_link(song_album, metadata_size, album_click);
                            let genre_widget =
                                make_link(genre_label, stacked_genre_size, genre_click);
                            column![album_widget, genre_widget].spacing(2.0).into()
                        }
                        (true, false) => make_link(song_album, metadata_size, album_click),
                        (false, true) => make_link(genre_label, metadata_size, genre_click),
                        (false, false) => unreachable!(),
                    };
                    content_row = content_row.push(
                        container(content)
                            .width(Length::FillPortion(ALBUM_PORTION))
                            .height(Length::Fill)
                            .clip(true)
                            .align_y(Alignment::Center),
                    );
                }

                if show_stars {
                    use crate::widgets::slot_list::slot_list_star_rating;
                    let star_icon_size = m.title_size;
                    let idx = ctx.item_index;
                    content_row = content_row.push(slot_list_star_rating(
                        rating,
                        star_icon_size,
                        ctx.is_center,
                        ctx.opacity,
                        Some(STARS_PORTION),
                        Some(move |star: usize| SongsMessage::ClickSetRating(idx, star)),
                    ));
                }

                if show_plays {
                    use crate::widgets::slot_list::slot_list_metadata_column;
                    content_row = content_row.push(slot_list_metadata_column(
                        format!("{play_count} plays"),
                        None,
                        metadata_size,
                        style,
                        PLAYS_PORTION,
                    ));
                }

                if show_dynamic_slot {
                    use crate::widgets::slot_list::slot_list_metadata_column;
                    content_row = content_row.push(slot_list_metadata_column(
                        extra_value,
                        None,
                        m.title_size,
                        style,
                        DYNAMIC_PORTION,
                    ));
                }

                if show_duration {
                    let duration_str = formatters::format_time(duration);
                    content_row = content_row.push(
                        container(slot_list_text(
                            duration_str,
                            metadata_size,
                            style.subtext_color,
                        ))
                        .width(Length::FillPortion(DURATION_PORTION))
                        .height(Length::Fill)
                        .align_x(Alignment::End)
                        .align_y(Alignment::Center),
                    );
                }

                if show_love {
                    use crate::widgets::slot_list::slot_list_favorite_icon;
                    content_row = content_row.push(
                        container(slot_list_favorite_icon(
                            is_starred,
                            ctx.is_center,
                            false,
                            ctx.opacity,
                            star_size,
                            "heart",
                            Some(SongsMessage::ClickToggleStar(ctx.item_index)),
                        ))
                        .width(Length::FillPortion(LOVE_PORTION))
                        .padding(iced::Padding {
                            left: 4.0,
                            right: 4.0,
                            ..Default::default()
                        })
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center),
                    );
                }

                let content = content_row
                    .padding(iced::Padding {
                        left: SLOT_LIST_SLOT_PADDING,
                        right: 4.0,
                        top: 4.0,
                        bottom: 4.0,
                    })
                    .align_y(Alignment::Center)
                    .height(Length::Fill);

                // Wrap in clickable container
                let clickable = container(content)
                    .style(move |_theme| style.to_container_style())
                    .width(Length::Fill);

                let slot_button = crate::widgets::slot_list::primary_slot_button(
                    clickable,
                    &ctx,
                    data.stable_viewport,
                    SongsMessage::SlotList,
                );

                use crate::widgets::context_menu::{song_entries_with_folder, wrap_library_row};
                let cm_row = wrap_library_row(
                    crate::View::Songs,
                    ctx.item_index,
                    slot_button,
                    song_entries_with_folder(),
                    open_menu_for_rows,
                    SongsMessage::ContextMenuAction,
                    SongsMessage::SetOpenMenu,
                );
                crate::widgets::slot_list::wrap_with_select_column(
                    select_header_visible,
                    ctx.is_selected,
                    ctx.item_index,
                    |i| {
                        SongsMessage::SlotList(
                            crate::widgets::SlotListPageMessage::SelectionToggle(i),
                        )
                    },
                    cm_row,
                )
            },
        );

        // Wrap slot list content with standard background
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_pill;

        // Build artwork column component - use album artwork of centered song.
        // Fall back to the slot-list mini when the large isn't loaded yet —
        // see Albums view for rationale.
        let centered_song = center_index.and_then(|idx| songs.get(idx));
        let artwork_handle = centered_song
            .and_then(|song| song.album_id.as_ref())
            .and_then(|album_id| {
                data.large_artwork
                    .get(album_id)
                    .or_else(|| data.album_art.get(album_id))
            });
        let active_dominant_color = centered_song
            .and_then(|song| song.album_id.as_ref())
            .and_then(|album_id| data.dominant_colors.get(album_id).copied());

        let on_refresh = centered_song
            .and_then(|song| song.album_id.clone())
            .map(SongsMessage::RefreshArtwork);

        let pill_content = centered_song
            .filter(|_| crate::theme::songs_artwork_overlay())
            .map(|song| {
                use iced::widget::{column, text};

                use crate::theme;

                let mut col = column![
                    text(song.title.clone())
                        .size(20)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..theme::ui_font()
                        })
                        .color(theme::fg0()),
                    text(song.artist.clone())
                        .size(16)
                        .color(theme::fg1())
                        .font(theme::ui_font()),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center);

                use crate::widgets::metadata_pill::{
                    auth_status_row, dot_row, play_stats_row, tech_specs_row,
                };

                // Row 1: Track • Year • Genre
                let mut info_stats = Vec::new();
                if let Some(track) = song.track {
                    info_stats.push(format!("Track {track}"));
                }
                if let Some(year) = song.year {
                    info_stats.push(year.to_string());
                }
                if let Some(genre) = &song.genre {
                    info_stats.push(genre.clone());
                }
                if let Some(row) = dot_row::<SongsMessage>(info_stats, 13.0, theme::fg2()) {
                    col = col.push(row);
                }

                // Row 2: Plays • Last played
                if let Some(row) =
                    play_stats_row::<SongsMessage>(song.play_count, song.play_date.as_deref())
                {
                    col = col.push(row);
                }

                // Row 3: Favorited / Rating
                if let Some(row) = auth_status_row::<SongsMessage>(song.is_starred, song.rating) {
                    col = col.push(row);
                }

                // Row 4: Tech specs
                if let Some(row) = tech_specs_row::<SongsMessage>(
                    song.suffix.as_deref(),
                    song.bit_depth,
                    song.sample_rate,
                    song.bitrate,
                    song.bpm,
                ) {
                    col = col.push(row);
                }

                col.into()
            });

        let artwork_menu_id = crate::app_message::ContextMenuId::ArtworkPanel(crate::View::Songs);
        let (artwork_menu_open, artwork_menu_position) =
            crate::widgets::context_menu::open_state_for(data.open_menu, &artwork_menu_id);
        let artwork_content = Some(single_artwork_panel_with_pill(
            artwork_handle,
            pill_content,
            active_dominant_color,
            on_refresh,
            artwork_menu_open,
            artwork_menu_position,
            move |position| match position {
                Some(p) => SongsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                    id: artwork_menu_id.clone(),
                    position: p,
                })),
                None => SongsMessage::SetOpenMenu(None),
            },
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(SongsMessage::ArtworkColumnDrag),
            Some(SongsMessage::ArtworkColumnVerticalDrag),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn songs_stars_visible_auto_shows_on_rating_sort() {
        assert!(songs_stars_visible(SortMode::Rating, false));
        assert!(songs_stars_visible(SortMode::Rating, true));
        assert!(!songs_stars_visible(SortMode::Title, false));
        assert!(songs_stars_visible(SortMode::Title, true));
    }

    #[test]
    fn songs_plays_visible_auto_shows_on_most_played() {
        assert!(songs_plays_visible(SortMode::MostPlayed, false));
        assert!(songs_plays_visible(SortMode::MostPlayed, true));
        assert!(!songs_plays_visible(SortMode::Title, false));
        assert!(songs_plays_visible(SortMode::Title, true));
    }

    #[test]
    fn songs_genre_visible_auto_shows_on_genre_sort() {
        assert!(songs_genre_visible(SortMode::Genre, false));
        assert!(songs_genre_visible(SortMode::Genre, true));
        assert!(!songs_genre_visible(SortMode::Title, false));
        assert!(songs_genre_visible(SortMode::Title, true));
    }
}
