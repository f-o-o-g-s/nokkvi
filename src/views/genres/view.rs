//! Genres view — `impl GenresPage { fn view, fn render_genre_row, fn render_album_row }`.
//!
//! Rendering for the genres page. Update/state logic lives in `update.rs`;
//! types live in `mod.rs`.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length,
    widget::{button, container, image},
};
use nokkvi_data::backend::{albums::AlbumUIViewData, genres::GenreUIViewData};

use super::{super::expansion::SlotListEntry, GenresMessage, GenresPage, GenresViewData};
use crate::widgets;

impl GenresPage {
    /// Build the view
    pub fn view<'a>(&'a self, data: GenresViewData<'a>) -> Element<'a, GenresMessage> {
        let column_dropdown: Element<'a, GenresMessage> = {
            use crate::widgets::checkbox_dropdown::checkbox_dropdown;
            let items: Vec<(super::GenresColumn, &'static str, bool)> = vec![
                (
                    super::GenresColumn::Select,
                    "Select",
                    self.column_visibility.select,
                ),
                (
                    super::GenresColumn::Index,
                    "Index",
                    self.column_visibility.index,
                ),
                (
                    super::GenresColumn::Thumbnail,
                    "Thumbnail",
                    self.column_visibility.thumbnail,
                ),
                (
                    super::GenresColumn::AlbumCount,
                    "Album count",
                    self.column_visibility.albumcount,
                ),
                (
                    super::GenresColumn::SongCount,
                    "Song count",
                    self.column_visibility.songcount,
                ),
            ];
            checkbox_dropdown(
                "assets/icons/columns-3-cog.svg",
                "Show/hide columns",
                items,
                GenresMessage::ToggleColumnVisible,
                |trigger_bounds| match trigger_bounds {
                    Some(b) => GenresMessage::SetOpenMenu(Some(
                        crate::app_message::OpenMenu::CheckboxDropdown {
                            view: crate::View::Genres,
                            trigger_bounds: b,
                        },
                    )),
                    None => GenresMessage::SetOpenMenu(None),
                },
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into()
        };

        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            crate::views::sort_api::sort_modes_for_view(crate::View::Genres),
            self.common.sort_ascending,
            &self.common.search_query,
            data.genres.len(),
            data.total_genre_count,
            "genres",
            crate::views::GENRES_SEARCH_ID,
            GenresMessage::SortModeSelected,
            Some(GenresMessage::ToggleSortOrder),
            Some(GenresMessage::RefreshViewData),
            // Hidden in the browsing panel — the narrower pane needs the
            // header space for sort/refresh/columns/search, and the user
            // already has the main-pane button when they want to center.
            if data.in_browsing_panel {
                None
            } else {
                Some(GenresMessage::CenterOnPlaying)
            },
            None,                  // on_add
            Some(column_dropdown), // trailing_button
            true,                  // show_search
            GenresMessage::SearchQueryChanged,
            // Roulette is main-pane only — see Albums view for rationale.
            if data.in_browsing_panel {
                None
            } else {
                Some(GenresMessage::Roulette)
            },
        );

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *flattened* (visible) row count.
        let header = {
            let flattened_len = self.expansion.flattened_len(data.genres);
            crate::widgets::slot_list::compose_header_with_select(
                self.column_visibility.select,
                self.common.select_all_state(flattened_len),
                GenresMessage::SlotListSelectAllToggle,
                header,
            )
        };

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
        };

        // If loading, show header with loading message
        if data.loading {
            return widgets::base_slot_list_empty_state(header, "Loading...", &layout_config);
        }

        // If no genres match search, show message but keep the header
        if data.genres.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No genres match your search.",
                &layout_config,
            );
        }

        // Configure slot list with genres-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            chrome_height_with_select_header(select_header_visible),
        )
        .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let genres = data.genres; // Borrow slice to extend lifetime
        let genre_artwork = data.genre_artwork;
        let genre_collage_artwork = data.genre_collage_artwork;
        let open_menu_for_rows = data.open_menu;

        // Build flattened list (genres + injected albums when expanded)
        let flattened = self.expansion.build_flattened_list(genres, |g| &g.id);
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            GenresMessage::SlotListNavigateUp,
            GenresMessage::SlotListNavigateDown,
            {
                let total = flattened.len();
                move |f| GenresMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |entry, ctx| match entry {
                SlotListEntry::Parent(genre) => {
                    let row = self.render_genre_row(
                        genre,
                        &ctx,
                        genre_artwork,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        GenresMessage::SlotListSelectionToggle,
                        row,
                    )
                }
                SlotListEntry::Child(album, _parent_genre_id) => {
                    let sub_index_label =
                        self.expansion
                            .child_sub_index_label(ctx.item_index, genres, |g| &g.id);
                    let row = self.render_album_row(
                        album,
                        &ctx,
                        &sub_index_label,
                        data.album_art,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        GenresMessage::SlotListSelectionToggle,
                        row,
                    )
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{collage_artwork_panel, single_artwork_panel};

        // Build artwork column — show parent genre art even when on a child album
        let centered_genre = center_index.and_then(|idx| match flattened.get(idx) {
            Some(SlotListEntry::Parent(genre)) => Some(genre),
            Some(SlotListEntry::Child(_, parent_id)) => genres.iter().find(|g| &g.id == parent_id),
            None => None,
        });
        let genre_id = centered_genre.map(|g| g.id.clone()).unwrap_or_default();

        // Get collage handles for centered genre (borrow, don't clone)
        let collage_handles = genre_collage_artwork.get(&genre_id);

        // Show single full-res when 0-1 albums, collage when 2+ albums
        let album_count = centered_genre.map_or(0, |g| g.album_count);

        let artwork_content = if album_count <= 1 {
            // Show single artwork full-size (use collage[0] if available, else mini)
            let handle = collage_handles
                .and_then(|v| v.first())
                .or_else(|| genre_artwork.get(&genre_id));
            Some(single_artwork_panel::<GenresMessage>(handle))
        } else if let Some(handles) = collage_handles.filter(|v| !v.is_empty()) {
            // Render 3x3 collage grid (2+ albums)
            Some(collage_artwork_panel::<GenresMessage>(handles))
        } else {
            // album_count > 1 but collage NOT loaded yet - show placeholder
            Some(single_artwork_panel::<GenresMessage>(None))
        };

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(GenresMessage::ArtworkColumnDrag),
        )
    }

    /// Render a parent genre row in the slot list
    fn render_genre_row<'a>(
        &self,
        genre: &GenreUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        genre_artwork: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, GenresMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column, slot_list_text,
        };

        let is_expanded = self.expansion.is_expanded_parent(&genre.id);
        let style = SlotListSlotStyle::for_slot(
            ctx.is_center,
            is_expanded,
            ctx.is_selected,
            ctx.has_multi_selection,
            ctx.opacity,
            0,
        );

        let m = ctx.metrics;
        let artwork_size = m.artwork_size;
        let title_size = m.title_size;
        let metadata_size = m.metadata_size;
        let index_size = m.metadata_size;

        // Layout: [Index? (5%)] [Artwork?] [Genre Name (45%)] [Album Count? (20%)] [Song Count? (20%)]
        let mut content = iced::widget::Row::new();
        if self.column_visibility.index {
            content = content.push(slot_list_index_column(
                ctx.item_index,
                index_size,
                style,
                ctx.opacity,
            ));
        }
        if self.column_visibility.thumbnail {
            use crate::widgets::slot_list::slot_list_artwork_column;
            content = content.push(slot_list_artwork_column(
                genre_artwork.get(&genre.id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ));
        }
        content = content.push(
            container(slot_list_text(
                genre.name.clone(),
                title_size,
                style.text_color,
            ))
            .width(Length::FillPortion(45))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center),
        );
        if self.column_visibility.albumcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            let album_text = if genre.album_count == 1 {
                "1 album".to_string()
            } else {
                format!("{} albums", genre.album_count)
            };
            let idx = ctx.item_index;
            content = content.push(slot_list_metadata_column(
                album_text,
                Some(GenresMessage::FocusAndExpand(idx)),
                metadata_size,
                style,
                20,
            ));
        }
        if self.column_visibility.songcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            content = content.push(slot_list_metadata_column(
                format!("{} songs", genre.song_count),
                None,
                metadata_size,
                style,
                20,
            ));
        }
        let content = content
            .spacing(6.0)
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

        let slot_button = button(clickable)
            .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else if ctx.is_center {
                GenresMessage::SlotListActivateCenter
            } else if stable_viewport {
                GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                GenresMessage::SlotListClickPlay(ctx.item_index)
            })
            .style(|_theme, _status| button::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .padding(0)
            .width(Length::Fill);

        use crate::widgets::context_menu::{
            context_menu, library_entries, library_entry_view, open_state_for,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Genres,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            slot_button,
            library_entries(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    GenresMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    GenresMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => GenresMessage::SetOpenMenu(None),
            },
        )
        .into()
    }

    /// Render a child album row in the slot list (indented, simpler layout)
    fn render_album_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        sub_index_label: &str,
        album_art: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, GenresMessage> {
        let navigate_msg = GenresMessage::NavigateAndExpandAlbum(album.id.clone());
        let album_el = super::super::expansion::render_child_album_row(
            album,
            ctx,
            sub_index_label,
            album_art.get(&album.id),
            self.column_visibility.thumbnail,
            GenresMessage::SlotListActivateCenter,
            if stable_viewport {
                GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                GenresMessage::SlotListClickPlay(ctx.item_index)
            },
            true, // show artist since genre groups albums from different artists
            Some(GenresMessage::ClickToggleStar(ctx.item_index)),
            Some(navigate_msg.clone()),
            Some(navigate_msg),
            Some(GenresMessage::NavigateAndExpandArtist(
                album.artist_id.clone(),
            )),
            1, // depth 1: child albums under genre
        );

        use crate::widgets::context_menu::{
            context_menu, library_entries_with_folder, library_entry_view, open_state_for,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Genres,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            album_el,
            library_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    GenresMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    GenresMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => GenresMessage::SetOpenMenu(None),
            },
        )
        .into()
    }
}
