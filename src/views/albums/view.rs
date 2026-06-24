//! Albums view — `impl AlbumsPage { fn view, fn render_album_row, fn render_track_row }`.
//!
//! Rendering for the albums page, plus the dynamic-slot value resolver
//! (`get_extra_column_value`). The Stars / Plays auto-show-on-sort
//! decision is centralized in `crate::views::auto_show_on_sort`.
//!
//! Update/state logic lives in `update.rs`; types live in `mod.rs`.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length,
    widget::{Row, container, image},
};
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, songs::SongUIViewData},
    utils::formatters,
};

use super::{super::expansion::SlotListEntry, AlbumsMessage, AlbumsPage, AlbumsViewData};
use crate::widgets::{
    self,
    view_header::{HeaderButton, SortMode, ViewHeaderConfig},
};

impl AlbumsPage {
    /// Build the view
    pub fn view<'a>(&'a self, data: AlbumsViewData<'a>) -> Element<'a, AlbumsMessage> {
        let column_dropdown: Element<'a, AlbumsMessage> =
            crate::widgets::checkbox_dropdown::view_columns_dropdown(
                crate::View::Albums,
                self.column_visibility.dropdown_entries(),
                AlbumsMessage::ToggleColumnVisible,
                AlbumsMessage::SetOpenMenu,
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
            current_view: self.common.current_sort_mode,
            view_options: crate::views::sort_api::sort_modes_for_view(crate::View::Albums),
            sort_ascending: self.common.sort_ascending,
            search_query: &self.common.search_query,
            filtered_count: data.albums.len(),
            total_count: data.total_album_count,
            item_type: "albums",
            search_input_id: crate::views::ALBUMS_SEARCH_ID,
            on_view_selected: Box::new(AlbumsMessage::SortModeSelected),
            show_search: true,
            on_search_change: Box::new(AlbumsMessage::SearchQueryChanged),
            buttons: {
                let mut btns = vec![
                    HeaderButton::SortToggle(AlbumsMessage::ToggleSortOrder),
                    HeaderButton::Refresh(AlbumsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::RefreshViewData,
                    )),
                ];
                // Hidden in the browsing panel — the narrower pane needs the
                // header space for sort/refresh/columns/search, and the user
                // already has the main-pane button when they want to center.
                if !data.in_browsing_panel {
                    btns.push(HeaderButton::CenterOnPlaying(AlbumsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::CenterOnPlaying,
                    )));
                }
                btns.push(HeaderButton::Trailing(column_dropdown));
                btns
            },
            // Roulette lives on the main pane only — browsing-panel
            // dispatch routes plays through add-to-queue, which would
            // turn the slot-machine "play this" into a silent append.
            on_roulette: if data.in_browsing_panel {
                None
            } else {
                Some(AlbumsMessage::Roulette)
            },
            collapsed: toolbar_collapsed,
            on_hover_enter: autohide.then_some({
                AlbumsMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverEnter)
            }),
            on_hover_exit: autohide.then_some({
                AlbumsMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverExit)
            }),
            on_dropdown_open: autohide.then_some(AlbumsMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(true),
            )),
            on_dropdown_close: autohide.then_some(AlbumsMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(false),
            )),
            sort_placeholder: None,
            total_duration_secs: {
                let secs: f64 = data.albums.iter().filter_map(|a| a.duration).sum();
                (secs > 0.0).then_some(secs as u64)
            },
        });

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *flattened* (visible) row count.
        let header = {
            let flattened_len = self
                .expansion
                .build_flattened_list(data.albums, |a| &a.id)
                .len();
            crate::widgets::slot_list::compose_header_with_select(
                self.column_visibility.select,
                self.common.select_all_state(flattened_len),
                AlbumsMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
                header,
            )
        };

        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let slot_list_chrome =
            chrome_height_with_select_header(toolbar_collapsed, select_header_visible);

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
            slot_list_chrome,
            elevated: data.elevated,
        };

        // If loading, show header with loading message
        if data.loading {
            return widgets::base_slot_list_empty_state(header, "Loading...", &layout_config);
        }

        // If no albums match search, show message but keep the header
        if data.albums.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No albums match your search.",
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
        let _scale_factor = data.scale_factor;
        let albums = data.albums; // Borrow slice to extend lifetime
        let album_art = data.album_art;
        let current_sort_mode = self.common.current_sort_mode;

        // Build flattened list (albums + injected tracks when expanded)
        let flattened = self.expansion.build_flattened_list(albums, |a| &a.id);
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            AlbumsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateUp),
            AlbumsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateDown),
            crate::views::scroll_seek_msg(flattened.len(), AlbumsMessage::SlotList),
            Some(crate::widgets::slot_list::SlotHoverCallback::for_slot_list(
                AlbumsMessage::SlotList,
            )),
            |entry, ctx| match entry {
                SlotListEntry::Parent(album) => {
                    let row = self.render_album_row(
                        album,
                        &ctx,
                        album_art,
                        current_sort_mode,
                        data.stable_viewport,
                        data.overlay.open_menu,
                    );
                    crate::widgets::slot_list::wrap_with_select_column_for(
                        select_header_visible,
                        &ctx,
                        AlbumsMessage::SlotList,
                        row,
                    )
                }
                SlotListEntry::Child(song, _parent_album_id) => {
                    let sub_index_label =
                        self.expansion
                            .child_sub_index_label(ctx.item_index, albums, |a| &a.id);
                    let row = self.render_track_row(
                        song,
                        &ctx,
                        &sub_index_label,
                        data.stable_viewport,
                        data.overlay.open_menu,
                    );
                    crate::widgets::slot_list::wrap_with_select_column_for(
                        select_header_visible,
                        &ctx,
                        AlbumsMessage::SlotList,
                        row,
                    )
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        // Use base slot list layout with artwork column
        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_pill;

        // Build artwork column component — show parent album art even when on a child track
        let centered_album = center_index.and_then(|idx| match flattened.get(idx) {
            Some(SlotListEntry::Parent(album)) => Some(album),
            Some(SlotListEntry::Child(_, parent_id)) => albums.iter().find(|a| &a.id == parent_id),
            None => None,
        });

        // Fall back to the slot-list mini when the large isn't loaded yet
        // (cruise phase of a roulette spin scrolls through items faster
        // than LoadLarge can roundtrip). The mini is upscaled by Iced's
        // image widget — blurry but instantly available, so the panel
        // tracks the centered slot instead of going blank.
        let artwork_handle = centered_album.and_then(|album| {
            data.large_artwork
                .get(&album.id)
                .or_else(|| data.album_art.get(&album.id))
        });

        let on_refresh =
            centered_album.map(|album| AlbumsMessage::RefreshArtwork(album.id.clone()));

        // Overlay building (gated by Settings → Interface → Views → Text Overlay On Artwork)
        let overlay_content = centered_album
            .filter(|_| crate::theme::albums_artwork_overlay())
            .map(|album| {
                use iced::widget::{column, text};

                use crate::theme;

                let mut col = column![
                    text(album.name.clone())
                        .size(24)
                        .font(theme::weighted_ui_font(iced::font::Weight::Bold))
                        .color(theme::fg0()),
                    text(album.artist.clone())
                        .size(16)
                        .color(theme::fg1())
                        .font(theme::ui_font()),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center);

                // Date Resolution (Feishin logic cascade)
                let date_text = if let Some(orig_date) = &album.original_date {
                    nokkvi_data::utils::formatters::format_release_date(orig_date)
                } else if let Some(rel_date) = &album.release_date {
                    nokkvi_data::utils::formatters::format_release_date(rel_date)
                } else if let Some(year) = album.original_year.or(album.year) {
                    year.to_string()
                } else {
                    String::new()
                };

                let mut info_stats = Vec::new();
                if !date_text.is_empty() {
                    let mut full_date = date_text;
                    if let (Some(orig_yr), Some(yr)) = (album.original_year, album.year)
                        && orig_yr != yr
                    {
                        full_date = format!("{full_date} ({yr})");
                    }
                    info_stats.push(full_date);
                }

                let count = album.song_count;
                if count > 0 {
                    info_stats.push(format!("{count} tracks"));
                }

                if let Some(secs) = album.duration {
                    info_stats.push(nokkvi_data::utils::formatters::format_duration_short(secs));
                }

                use crate::widgets::metadata_pill::{auth_status_row, dot_row, play_stats_row};

                if let Some(row) = dot_row::<AlbumsMessage>(info_stats, 14.0, theme::fg2()) {
                    col = col.push(row);
                }

                // Genre row
                if let Some(genres_display) = &album.genres {
                    col = col.push(
                        text(genres_display.clone())
                            .size(13)
                            .color(theme::fg3())
                            .font(theme::ui_font()),
                    );
                }

                if let Some(row) =
                    play_stats_row::<AlbumsMessage>(album.play_count, album.play_date.as_deref())
                {
                    col = col.push(row);
                }

                if let Some(row) = auth_status_row::<AlbumsMessage>(album.is_starred, album.rating)
                {
                    col = col.push(row);
                }

                col.into()
            });

        let (artwork_menu_open, artwork_menu_position, on_artwork_menu_change) =
            crate::widgets::context_menu::artwork_panel_open_state(
                crate::View::Albums,
                data.overlay.open_menu,
                AlbumsMessage::SetOpenMenu,
            );
        let artwork_content = Some(single_artwork_panel_with_pill(
            artwork_handle,
            overlay_content,
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
            Some(AlbumsMessage::ArtworkColumnDrag),
            Some(AlbumsMessage::ArtworkColumnVerticalDrag),
        )
    }

    /// Render an album row in the slot list (existing album layout)
    fn render_album_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        album_art: &'a HashMap<String, image::Handle>,
        current_sort_mode: SortMode,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, AlbumsMessage> {
        use crate::widgets::slot_list::{SLOT_LIST_SLOT_PADDING, slot_list_index_column};

        let album_id = album.id.clone();
        let album_name = album.name.clone();
        let album_artist = album.artist.clone();
        let song_count = album.song_count;
        let is_starred = album.is_starred;
        let rating = album.rating.unwrap_or(0).min(5) as usize;
        let extra_value = get_extra_column_value(album, current_sort_mode);

        // Check if this album is the expanded one
        let is_expanded = self.expansion.is_expanded_parent(&album.id);
        let style = ctx.slot_style(is_expanded, false, 0);

        let m = ctx.metrics;
        let artwork_size = m.artwork_size;
        let title_size = m.title_size_lg;
        let subtitle_size = m.subtitle_size;
        let song_count_size = m.metadata_size;
        let star_size = m.star_size;
        let index_size = m.metadata_size;
        let play_count = album.play_count.unwrap_or(0);

        // Per-column visibility (Stars/Plays auto-shown by their sort modes).
        let vis = self.column_visibility;
        let show_stars =
            crate::views::auto_show_on_sort(current_sort_mode, vis.stars, &[SortMode::Rating]);
        let show_songcount = vis.songcount;
        let show_plays =
            crate::views::auto_show_on_sort(current_sort_mode, vis.plays, &[SortMode::MostPlayed]);
        let show_love = vis.love;
        // Dynamic slot now only carries Date/Year/Duration/Genre — Rating
        // and MostPlayed have been promoted to dedicated columns.
        let show_dynamic_slot = !extra_value.is_empty();

        const SONGCOUNT_PORTION: u16 = 22;
        const STARS_PORTION: u16 = 12;
        const PLAYS_PORTION: u16 = 16;
        const DYNAMIC_PORTION: u16 = 21;
        const LOVE_PORTION: u16 = 5;
        let mut consumed: u16 = 0;
        if show_songcount {
            consumed += SONGCOUNT_PORTION;
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
            content_row = content_row.push(slot_list_artwork_column(
                album_art.get(&album_id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ));
        }
        content_row = content_row.push({
            use crate::widgets::slot_list::slot_list_text_column;
            let artist_click = Some(AlbumsMessage::NavigateAndExpandArtist(
                album.artist_id.clone(),
            ));
            let title_click = Some(AlbumsMessage::ContextMenuAction(
                ctx.item_index,
                crate::widgets::context_menu::LibraryContextEntry::GetInfo,
            ));
            slot_list_text_column(
                album_name,
                title_click,
                album_artist,
                artist_click,
                title_size,
                subtitle_size,
                style,
                ctx.is_center,
                title_portion,
            )
        });

        if show_songcount {
            let idx = ctx.item_index;
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                format!("{song_count} songs"),
                Some(AlbumsMessage::FocusAndExpand(idx)),
                song_count_size,
                style,
                SONGCOUNT_PORTION,
            ));
        }

        if show_stars {
            let star_icon_size = m.title_size;
            let idx = ctx.item_index;
            use crate::widgets::slot_list::slot_list_star_rating;
            content_row = content_row.push(slot_list_star_rating(
                rating,
                star_icon_size,
                style,
                Some(STARS_PORTION),
                Some(move |star: usize| AlbumsMessage::ClickSetRating(idx, star)),
            ));
        }

        if show_plays {
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                format!("{play_count} plays"),
                None,
                song_count_size,
                style,
                PLAYS_PORTION,
            ));
        }

        if show_dynamic_slot {
            let mut click_msg = None;
            if current_sort_mode == SortMode::Genre {
                click_msg = Some(AlbumsMessage::NavigateAndExpandGenre(extra_value.clone()));
            }
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                extra_value,
                click_msg,
                m.metadata_size,
                style,
                DYNAMIC_PORTION,
            ));
        }

        if show_love {
            use crate::widgets::slot_list::{FavoriteIconKind, slot_list_favorite_icon};
            content_row = content_row.push(
                container(slot_list_favorite_icon(
                    is_starred,
                    style,
                    star_size,
                    FavoriteIconKind::Heart,
                    Some(AlbumsMessage::ClickToggleStar(ctx.item_index)),
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

        let clickable = container(content)
            .style(move |_theme| style.to_container_style())
            .width(Length::Fill);

        let slot_button = crate::widgets::slot_list::primary_slot_button(
            clickable,
            ctx,
            stable_viewport,
            AlbumsMessage::SlotList,
        );

        use crate::widgets::context_menu::{library_entries_with_folder, wrap_library_row};
        wrap_library_row(
            crate::View::Albums,
            ctx.item_index,
            slot_button,
            library_entries_with_folder(),
            open_menu,
            AlbumsMessage::ContextMenuAction,
            AlbumsMessage::SetOpenMenu,
        )
    }

    /// Render a child track row in the slot list (indented, simpler layout)
    fn render_track_row<'a>(
        &self,
        song: &SongUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        sub_index_label: &str,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, AlbumsMessage> {
        let track_el = super::super::expansion::render_child_track_row(
            song,
            ctx,
            sub_index_label,
            stable_viewport,
            AlbumsMessage::SlotList,
            Some(AlbumsMessage::ClickToggleStar(ctx.item_index)),
            song.artist_id
                .as_ref()
                .map(|id| AlbumsMessage::NavigateAndExpandArtist(id.clone())),
            1, // depth 1: child tracks under album
        );

        use crate::widgets::context_menu::{song_entries_with_folder, wrap_library_row};
        wrap_library_row(
            crate::View::Albums,
            ctx.item_index,
            track_el,
            song_entries_with_folder(),
            open_menu,
            AlbumsMessage::ContextMenuAction,
            AlbumsMessage::SetOpenMenu,
        )
    }
}

/// Dynamic-slot value based on current sort mode. Rating and MostPlayed are
/// no longer rendered here — they're dedicated, toggleable columns now.
fn get_extra_column_value(album: &AlbumUIViewData, sort_mode: SortMode) -> String {
    match sort_mode {
        SortMode::RecentlyAdded => album
            .created_at
            .as_ref()
            .and_then(|d| formatters::format_date(d).ok())
            .unwrap_or_default(),
        SortMode::RecentlyPlayed => album.play_date.as_ref().map_or_else(
            || "never".to_string(),
            |d| d.split('T').next().unwrap_or(d).to_string(),
        ),
        SortMode::ReleaseYear => album.year.map(|y| y.to_string()).unwrap_or_default(),
        SortMode::Duration => album
            .duration
            .map(|d| formatters::format_time(d as u32))
            .unwrap_or_default(),
        SortMode::Genre => album.genre.clone().unwrap_or_default(),
        // Stars and Plays are dedicated columns (auto-show on Rating /
        // MostPlayed sort respectively). All other sort modes have no
        // extra-column data.
        SortMode::Rating
        | SortMode::MostPlayed
        | SortMode::Favorited
        | SortMode::Random
        | SortMode::Name
        | SortMode::AlbumArtist
        | SortMode::Artist
        | SortMode::SongCount
        | SortMode::AlbumCount
        | SortMode::Title
        | SortMode::Album
        | SortMode::Bpm
        | SortMode::Channels
        | SortMode::Comment
        | SortMode::UpdatedAt => String::new(),
    }
}
