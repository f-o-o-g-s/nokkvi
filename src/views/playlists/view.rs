//! Playlists view — `impl PlaylistsPage { fn view, fn render_playlist_row, fn render_track_row }`
//! plus the column-visibility helpers and playlist context-menu rendering.
//!
//! Update/state logic lives in `update.rs`; types live in `mod.rs`.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length,
    widget::{container, image},
};
use nokkvi_data::{
    backend::{playlists::PlaylistUIViewData, songs::SongUIViewData},
    utils::formatters::format_date_concise,
};

use super::{
    super::expansion::SlotListEntry, PlaylistContextEntry, PlaylistsMessage, PlaylistsPage,
    PlaylistsViewData,
};
use crate::widgets::{
    self,
    view_header::{HeaderButton, ViewHeaderConfig},
};

/// SongCount column auto-shows when sort = SongCount regardless of toggle.
pub(crate) fn playlists_song_count_visible(
    sort: crate::widgets::view_header::SortMode,
    user_visible: bool,
) -> bool {
    user_visible || matches!(sort, crate::widgets::view_header::SortMode::SongCount)
}

/// Duration column auto-shows when sort = Duration regardless of toggle.
pub(crate) fn playlists_duration_visible(
    sort: crate::widgets::view_header::SortMode,
    user_visible: bool,
) -> bool {
    user_visible || matches!(sort, crate::widgets::view_header::SortMode::Duration)
}

/// UpdatedAt column auto-shows when sort = UpdatedAt regardless of toggle.
pub(crate) fn playlists_updated_at_visible(
    sort: crate::widgets::view_header::SortMode,
    user_visible: bool,
) -> bool {
    user_visible || matches!(sort, crate::widgets::view_header::SortMode::UpdatedAt)
}

/// Build the full set of context menu entries for a playlist parent item.
/// `has_custom_art` (the playlist's `uploaded_image.is_some()`) gates the
/// "Reset Artwork" entry — there is nothing to reset without an uploaded
/// cover.
pub(crate) fn playlist_context_entries(has_custom_art: bool) -> Vec<PlaylistContextEntry> {
    use crate::widgets::context_menu::LibraryContextEntry;
    let mut entries = vec![
        PlaylistContextEntry::Library(LibraryContextEntry::AddToQueue),
        PlaylistContextEntry::Library(LibraryContextEntry::AddToMix),
        PlaylistContextEntry::Separator,
        PlaylistContextEntry::EditPlaylist,
        PlaylistContextEntry::Rename,
        PlaylistContextEntry::Delete,
        PlaylistContextEntry::Separator,
        PlaylistContextEntry::SetCustomArtwork,
    ];
    if has_custom_art {
        entries.push(PlaylistContextEntry::ResetArtwork);
    }
    entries.push(PlaylistContextEntry::Separator);
    entries.push(PlaylistContextEntry::SetAsDefault);
    entries.push(PlaylistContextEntry::Library(LibraryContextEntry::GetInfo));
    entries
}

/// Render a playlist context menu entry.
fn playlist_entry_view<'a, Message: Clone + 'a>(
    entry: PlaylistContextEntry,
    length: Length,
    on_action: impl Fn(PlaylistContextEntry) -> Message,
) -> Element<'a, Message> {
    use crate::widgets::context_menu::{library_entry_view, menu_button, menu_separator};
    match entry {
        PlaylistContextEntry::Library(lib_entry) => library_entry_view(lib_entry, length, |e| {
            on_action(PlaylistContextEntry::Library(e))
        }),
        PlaylistContextEntry::Separator => menu_separator(),
        PlaylistContextEntry::Delete => menu_button(
            Some("assets/icons/trash-2.svg"),
            "Delete Playlist",
            on_action(PlaylistContextEntry::Delete),
        ),
        PlaylistContextEntry::Rename => menu_button(
            Some("assets/icons/pencil.svg"),
            "Rename Playlist",
            on_action(PlaylistContextEntry::Rename),
        ),
        PlaylistContextEntry::EditPlaylist => menu_button(
            Some("assets/icons/list.svg"),
            "Edit Playlist",
            on_action(PlaylistContextEntry::EditPlaylist),
        ),
        PlaylistContextEntry::SetAsDefault => menu_button(
            Some("assets/icons/star.svg"),
            "Set as Default Playlist",
            on_action(PlaylistContextEntry::SetAsDefault),
        ),
        // Delegate to the shared PanelMenuEntry constructors — ONE icon +
        // label definition across row menus and the artwork-panel menus.
        PlaylistContextEntry::SetCustomArtwork => {
            crate::widgets::context_menu::PanelMenuEntry::set_custom_artwork(on_action(
                PlaylistContextEntry::SetCustomArtwork,
            ))
            .view()
        }
        PlaylistContextEntry::ResetArtwork => {
            crate::widgets::context_menu::PanelMenuEntry::reset_artwork(on_action(
                PlaylistContextEntry::ResetArtwork,
            ))
            .view()
        }
    }
}

impl PlaylistsPage {
    /// Build the view
    pub fn view<'a>(&'a self, data: PlaylistsViewData<'a>) -> Element<'a, PlaylistsMessage> {
        let chip: Element<'a, PlaylistsMessage> =
            crate::widgets::default_playlist_chip::default_playlist_chip(
                data.default_playlist_name,
                PlaylistsMessage::OpenDefaultPlaylistPicker,
            );

        let column_dropdown: Element<'a, PlaylistsMessage> =
            crate::widgets::checkbox_dropdown::view_columns_dropdown(
                crate::View::Playlists,
                self.column_visibility.dropdown_entries(),
                PlaylistsMessage::ToggleColumnVisible,
                PlaylistsMessage::SetOpenMenu,
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
            view_options: crate::views::sort_api::sort_modes_for_view(crate::View::Playlists),
            sort_ascending: self.common.sort_ascending,
            search_query: &self.common.search_query,
            filtered_count: data.playlists.len(),
            total_count: data.total_playlist_count,
            item_type: "playlists",
            search_input_id: crate::views::PLAYLISTS_SEARCH_ID,
            on_view_selected: Box::new(PlaylistsMessage::SortModeSelected),
            show_search: true,
            on_search_change: Box::new(PlaylistsMessage::SearchQueryChanged),
            // Playlists view doesn't need a center-on-playing button.
            buttons: vec![
                HeaderButton::SortToggle(PlaylistsMessage::ToggleSortOrder),
                HeaderButton::Refresh(PlaylistsMessage::SlotList(
                    crate::widgets::SlotListPageMessage::RefreshViewData,
                )),
                HeaderButton::Add("New Playlist", PlaylistsMessage::OpenCreatePlaylistDialog),
                HeaderButton::Trailing(chip),
                HeaderButton::Trailing(column_dropdown),
            ],
            on_roulette: Some(PlaylistsMessage::Roulette),
            collapsed: toolbar_collapsed,
            on_hover_enter: autohide.then_some({
                PlaylistsMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverEnter)
            }),
            on_hover_exit: autohide.then_some({
                PlaylistsMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverExit)
            }),
            on_dropdown_open: autohide.then_some(PlaylistsMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(true),
            )),
            on_dropdown_close: autohide.then_some(PlaylistsMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(false),
            )),
            sort_placeholder: None,
            total_duration_secs: {
                let secs: f64 = data.playlists.iter().map(|p| f64::from(p.duration)).sum();
                (secs > 0.0).then_some(secs as u64)
            },
        });

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *flattened* (visible) row count.
        let header = {
            let flattened_len = self
                .expansion
                .build_flattened_list(data.playlists, |p| &p.id)
                .len();
            crate::widgets::slot_list::compose_header_with_select(
                self.column_visibility.select,
                self.common.select_all_state(flattened_len),
                PlaylistsMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
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

        // If no playlists match search, show message but keep the header
        if data.playlists.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No playlists match your search.",
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
        let playlists = data.playlists; // Borrow slice to extend lifetime
        let playlist_artwork = data.playlist_artwork;
        let playlist_collage_artwork = data.playlist_collage_artwork;
        let album_art = data.album_art;
        let playlist_custom_art = data.playlist_custom_art;
        let open_menu_for_rows = data.overlay.open_menu;

        // Build flattened list (playlists + injected tracks when expanded)
        let flattened = self.expansion.build_flattened_list(playlists, |p| &p.id);
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            PlaylistsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateUp),
            PlaylistsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateDown),
            crate::views::scroll_seek_msg(flattened.len(), PlaylistsMessage::SlotList),
            Some(crate::widgets::slot_list::SlotHoverCallback::for_slot_list(
                PlaylistsMessage::SlotList,
            )),
            |entry, ctx| match entry {
                SlotListEntry::Parent(playlist) => {
                    let row = self.render_playlist_row(
                        playlist,
                        &ctx,
                        playlist_artwork,
                        album_art,
                        playlist_custom_art,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column_for(
                        select_header_visible,
                        &ctx,
                        PlaylistsMessage::SlotList,
                        row,
                    )
                }
                SlotListEntry::Child(song, _parent_playlist_id) => {
                    let sub_index_label =
                        self.expansion
                            .child_sub_index_label(ctx.item_index, playlists, |p| &p.id);
                    let row =
                        self.render_track_row(song, &ctx, &sub_index_label, data.stable_viewport);
                    crate::widgets::slot_list::wrap_with_select_column_for(
                        select_header_visible,
                        &ctx,
                        PlaylistsMessage::SlotList,
                        row,
                    )
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        // Build artwork column — show parent playlist art even when on a child track
        let centered_playlist = center_index.and_then(|idx| match flattened.get(idx) {
            Some(SlotListEntry::Parent(playlist)) => Some(playlist),
            Some(SlotListEntry::Child(_, parent_id)) => {
                playlists.iter().find(|p| &p.id == parent_id)
            }
            None => None,
        });
        let playlist_id = centered_playlist.map(|p| p.id.clone()).unwrap_or_default();

        // Get collage handles for centered playlist (borrow, don't clone)
        let collage_handles = playlist_collage_artwork.get(&playlist_id);

        // Show single full-res when 0-1 albums, collage when 2+ albums
        let album_count = centered_playlist.map_or(0, |p| p.artwork_album_ids.len());

        let pill_content = centered_playlist
            .filter(|_| crate::theme::playlists_artwork_overlay())
            .map(|playlist| {
                use iced::widget::{column, text};

                use crate::theme;

                let mut col = column![
                    text(playlist.name.clone())
                        .size(24)
                        .font(theme::weighted_ui_font(iced::font::Weight::Bold))
                        .color(theme::fg0()),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center);

                if !playlist.comment.is_empty() {
                    let comment = &playlist.comment;
                    let preview: String = comment.chars().take(100).collect();
                    let preview = if comment.chars().count() > 100 {
                        format!("{}...", preview.trim_end())
                    } else {
                        preview
                    };
                    col = col.push(
                        text(preview)
                            .size(14)
                            .color(theme::fg2())
                            .font(theme::ui_font())
                            .center(),
                    );
                }

                let duration_min = playlist.duration / 60.0;
                let mut stats = vec![
                    format!("{} songs", playlist.song_count),
                    format!("{} mins", duration_min.round()),
                ];
                let ymd = playlist
                    .updated_at
                    .split('T')
                    .next()
                    .unwrap_or(&playlist.updated_at);
                stats.push(format!("Updated: {ymd}"));

                use crate::widgets::metadata_pill::dot_row;
                if let Some(row) = dot_row::<PlaylistsMessage>(stats, 13.0, theme::fg3()) {
                    col = col.push(row);
                }

                col.into()
            });

        use crate::widgets::base_slot_list_layout::{
            collage_artwork_panel_with_pill, single_artwork_panel_with_pill,
        };

        // Panel right-click menu: Set/Reset custom artwork for the CENTERED
        // playlist. Resolved here and carried IN each entry's message so the
        // handler never re-resolves (the viewport may move before the click
        // lands). "Reset Artwork" is gated on the live `uploaded_image`.
        let panel_menu_entries: Vec<_> = centered_playlist
            .map(|p| {
                use crate::widgets::context_menu::PanelMenuEntry;
                let mut entries = vec![PanelMenuEntry::set_custom_artwork(
                    PlaylistsMessage::SetCustomArtwork(p.id.clone(), p.name.clone()),
                )];
                if p.uploaded_image.is_some() {
                    entries.push(PanelMenuEntry::reset_artwork(
                        PlaylistsMessage::ResetCustomArtwork(p.id.clone(), p.name.clone()),
                    ));
                }
                entries
            })
            .unwrap_or_default();
        let (panel_menu_open, panel_menu_position, on_panel_menu_change) =
            crate::widgets::context_menu::artwork_panel_open_state(
                crate::View::Playlists,
                data.overlay.open_menu,
                PlaylistsMessage::SetOpenMenu,
            );

        // A user-uploaded custom cover wins over the collage/quad branches:
        // one single-image panel at large size, with the 80px mini as an
        // instantly-available (if blurry) fallback while the large loads.
        // Gated on the live `uploaded_image` field AND cache membership,
        // mirroring the row thumbnail.
        let custom_panel_handle = centered_playlist
            .filter(|p| p.uploaded_image.is_some())
            .and_then(|p| {
                data.playlist_custom_large_art
                    .get(&p.id)
                    .or_else(|| data.playlist_custom_art.get(&p.id))
            });

        let artwork_content = if custom_panel_handle.is_some() {
            Some(single_artwork_panel_with_pill::<PlaylistsMessage>(
                custom_panel_handle,
                pill_content,
                panel_menu_entries,
                panel_menu_open,
                panel_menu_position,
                on_panel_menu_change,
            ))
        } else if album_count <= 1 {
            // Show single artwork full-size (use collage[0] if available, else mini)
            let handle = collage_handles
                .and_then(|v| v.first())
                .or_else(|| playlist_artwork.get(&playlist_id));
            Some(single_artwork_panel_with_pill::<PlaylistsMessage>(
                handle,
                pill_content,
                panel_menu_entries,
                panel_menu_open,
                panel_menu_position,
                on_panel_menu_change,
            ))
        } else if let Some(handles) = collage_handles.filter(|v| !v.is_empty()) {
            // Render 3x3 collage grid (2+ albums)
            Some(collage_artwork_panel_with_pill::<PlaylistsMessage>(
                handles,
                pill_content,
                panel_menu_entries,
                panel_menu_open,
                panel_menu_position,
                on_panel_menu_change,
            ))
        } else {
            // Multi-album playlist with no collage cached — fall back to
            // the slot-list mini at single-image size. Lets the panel
            // track the centered slot during a roulette spin's fast
            // cruise where the 9-tile fetch can't keep up with offset
            // changes.
            Some(single_artwork_panel_with_pill::<PlaylistsMessage>(
                playlist_artwork.get(&playlist_id),
                pill_content,
                panel_menu_entries,
                panel_menu_open,
                panel_menu_position,
                on_panel_menu_change,
            ))
        };

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(PlaylistsMessage::ArtworkColumnDrag),
            Some(PlaylistsMessage::ArtworkColumnVerticalDrag),
        )
    }

    /// Render a parent playlist row in the slot list
    // Three artwork maps (collage mini, album quad tiles, custom cover) plus
    // per-row context tip this over clippy's 7-arg line; the args are all
    // borrowed snapshots with distinct roles, so a bundling struct would only
    // add indirection. Same tradeoff as the other 15 allows in the tree.
    #[allow(clippy::too_many_arguments)]
    fn render_playlist_row<'a>(
        &self,
        playlist: &PlaylistUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        playlist_artwork: &'a HashMap<String, image::Handle>,
        album_art: &'a HashMap<String, image::Handle>,
        playlist_custom_art: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, PlaylistsMessage> {
        use crate::widgets::slot_list::{SLOT_LIST_SLOT_PADDING, slot_list_index_column};

        let is_expanded = self.expansion.is_expanded_parent(&playlist.id);
        let style = ctx.slot_style(is_expanded, false, 0);

        let m = ctx.metrics;
        let artwork_size = m.artwork_size;
        let title_size = m.title_size;
        let metadata_size = m.metadata_size;
        let index_size = m.metadata_size;

        // Format duration
        let duration_mins = (playlist.duration / 60.0) as u32;
        let duration_str = if duration_mins < 60 {
            format!("{duration_mins} min")
        } else {
            let hours = duration_mins / 60;
            let mins = duration_mins % 60;
            format!("{hours}h {mins}m")
        };

        let sort_mode = self.common.current_sort_mode;
        let show_song_count_col =
            playlists_song_count_visible(sort_mode, self.column_visibility.songcount);
        let show_duration_col =
            playlists_duration_visible(sort_mode, self.column_visibility.duration);
        let show_updated_at =
            playlists_updated_at_visible(sort_mode, self.column_visibility.updatedat);

        // Song-count text is only consumed by the dedicated column below
        // (when toggled on or auto-shown by sort). The subtitle no longer
        // falls back to it — toggling a column off means hide, full stop.
        let count_text = if playlist.song_count == 1 {
            "1 song".to_string()
        } else {
            format!("{} songs", playlist.song_count)
        };
        let subtitle = String::new();

        // Extra columns reduce the name portion to make room
        let extra_cols =
            show_song_count_col as u16 + show_duration_col as u16 + show_updated_at as u16;
        let name_portion = 55 - extra_cols * 10;

        // Layout: [Index?] [Artwork?] [Name+subtitle] [SongCount?] [Duration?] [UpdatedAt?]
        use crate::widgets::slot_list::{slot_list_artwork_column, slot_list_metadata_column};

        let mut columns: Vec<Element<'a, PlaylistsMessage>> = Vec::new();
        if self.column_visibility.index {
            columns.push(slot_list_index_column(
                ctx.item_index,
                index_size,
                style,
                ctx.opacity,
            ));
        }
        if self.column_visibility.thumbnail {
            // A user-uploaded custom cover (uploaded_image set AND its handle
            // warmed) takes precedence as a single image. Gating on BOTH the
            // live field and cache membership means an external reset (SSE
            // reload clearing the field) drops the stale custom art even
            // while its handle lingers in the LRU.
            let custom_handle = playlist
                .uploaded_image
                .as_ref()
                .and_then(|_| playlist_custom_art.get(&playlist.id));
            if let Some(handle) = custom_handle {
                columns.push(slot_list_artwork_column(
                    Some(handle),
                    artwork_size,
                    ctx.is_center,
                    false,
                    ctx.opacity,
                ));
            } else {
                // 2×2 quad of the playlist's first distinct album covers;
                // while any tile is still cold (or the playlist spans ≤1
                // album) the column keeps the single first-album mini,
                // mirroring the large panel's mini→collage upgrade.
                let quad_tiles = crate::services::collage_artwork::resolve_quad_handles(
                    &playlist.artwork_album_ids,
                    album_art,
                );
                columns.push(match quad_tiles {
                    Some(tiles) => crate::widgets::slot_list::slot_list_artwork_quad_column(
                        &tiles,
                        artwork_size,
                        ctx.is_center,
                        false,
                        ctx.opacity,
                    ),
                    None => slot_list_artwork_column(
                        playlist_artwork.get(&playlist.id),
                        artwork_size,
                        ctx.is_center,
                        false,
                        ctx.opacity,
                    ),
                });
            }
        }
        columns.push({
            let click_title = Some(PlaylistsMessage::PlaylistContextAction(
                ctx.item_index,
                PlaylistContextEntry::Library(
                    crate::widgets::context_menu::LibraryContextEntry::GetInfo,
                ),
            ));
            use crate::widgets::slot_list::slot_list_text_column;
            slot_list_text_column(
                playlist.name.clone(),
                click_title,
                subtitle,
                Some(PlaylistsMessage::FocusAndExpand(ctx.item_index)),
                title_size,
                metadata_size,
                style,
                ctx.is_center,
                name_portion,
            )
        });

        // Visibility glyph slot — always pushed so the row's widget tree
        // shape stays identical between public/private states. Public renders
        // a zero-width Space; private renders a lock SVG whose tint tracks
        // the row's text color (dark on selected/expanded rows, fg3 otherwise).
        columns.push(if playlist.public {
            iced::widget::Space::new()
                .width(Length::Fixed(0.0))
                .height(Length::Fixed(14.0))
                .into()
        } else {
            let lock_color = crate::widgets::slot_list::slot_list_static_icon_color(
                style,
                crate::theme::fg3(),
                1.0,
            );
            let lock_icon = crate::embedded_svg::svg_widget("assets/icons/lock.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(move |_theme, _status| iced::widget::svg::Style {
                    color: Some(lock_color),
                });
            iced::widget::tooltip(
                lock_icon,
                iced::widget::container(
                    iced::widget::text("Private playlist")
                        .size(11.0)
                        .font(crate::theme::ui_font()),
                )
                .padding(4),
                iced::widget::tooltip::Position::Top,
            )
            .gap(4)
            .style(crate::theme::container_tooltip)
            .into()
        });

        if show_song_count_col {
            columns.push(slot_list_metadata_column(
                count_text.clone(),
                Some(PlaylistsMessage::FocusAndExpand(ctx.item_index)),
                metadata_size,
                style,
                20,
            ));
        }
        if show_duration_col {
            columns.push(slot_list_metadata_column(
                duration_str.clone(),
                None,
                metadata_size,
                style,
                20,
            ));
        }
        if show_updated_at {
            let date_str = format_date_concise(&playlist.updated_at);
            columns.push(slot_list_metadata_column(
                date_str,
                None,
                metadata_size,
                style,
                20,
            ));
        }

        let content = iced::widget::Row::with_children(columns)
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

        let slot_button = crate::widgets::slot_list::primary_slot_button(
            clickable,
            ctx,
            stable_viewport,
            PlaylistsMessage::SlotList,
        );

        use crate::widgets::context_menu::{context_menu, open_state_for};
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Playlists,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            slot_button,
            playlist_context_entries(playlist.uploaded_image.is_some()),
            move |entry, length| {
                playlist_entry_view(entry, length, |e| {
                    PlaylistsMessage::PlaylistContextAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    PlaylistsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => PlaylistsMessage::SetOpenMenu(None),
            },
        )
        .into()
    }

    /// Render a child track row in the slot list (indented, simpler layout)
    fn render_track_row<'a>(
        &self,
        song: &SongUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        sub_index_label: &str,
        stable_viewport: bool,
    ) -> Element<'a, PlaylistsMessage> {
        super::super::expansion::render_child_track_row(
            song,
            ctx,
            sub_index_label,
            stable_viewport,
            PlaylistsMessage::SlotList,
            Some(PlaylistsMessage::ClickToggleStar(ctx.item_index)),
            song.artist_id
                .as_ref()
                .map(|id| PlaylistsMessage::NavigateAndExpandArtist(id.clone())),
            1, // depth 1: child tracks under playlist
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Set Custom Artwork…" is always offered on playlist rows; "Reset
    /// Artwork" only when the playlist has an uploaded cover.
    #[test]
    fn playlist_entries_gate_reset_on_custom_art() {
        let with = playlist_context_entries(true);
        assert!(
            with.iter()
                .any(|e| matches!(e, PlaylistContextEntry::ResetArtwork)),
            "custom-art playlist must offer Reset Artwork"
        );
        let without = playlist_context_entries(false);
        assert!(
            !without
                .iter()
                .any(|e| matches!(e, PlaylistContextEntry::ResetArtwork)),
            "a playlist without custom art has nothing to reset"
        );
        for entries in [&with, &without] {
            assert!(
                entries
                    .iter()
                    .any(|e| matches!(e, PlaylistContextEntry::SetCustomArtwork)),
                "Set Custom Artwork… must always be offered"
            );
            // The pre-existing playlist actions survive in both forms.
            for check in [
                entries
                    .iter()
                    .any(|e| matches!(e, PlaylistContextEntry::EditPlaylist)),
                entries
                    .iter()
                    .any(|e| matches!(e, PlaylistContextEntry::Rename)),
                entries
                    .iter()
                    .any(|e| matches!(e, PlaylistContextEntry::Delete)),
                entries
                    .iter()
                    .any(|e| matches!(e, PlaylistContextEntry::SetAsDefault)),
            ] {
                assert!(check, "pre-existing entries must survive the gating");
            }
        }
    }
}
