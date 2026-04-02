//! View functions for Nokkvi
//!
//! Contains all rendering logic: view(), login_view(), home_view(), navigation_bar(), main_content()

use iced::{
    Element, Length,
    widget::{Stack, column, container},
};

use crate::{
    Nokkvi, Screen, View,
    app_message::{Message, PlaybackMessage},
    views, widgets,
};

/// Convert a `NavBarMessage` into the root `Message` type.
///
/// Shared between the horizontal nav bar (top mode) and the vertical
/// side nav bar — avoids duplicating the `NavView → View` mapping.
fn map_nav_bar_message(msg: widgets::NavBarMessage) -> Message {
    match msg {
        widgets::NavBarMessage::SwitchView(nav_view) => {
            let view = match nav_view {
                widgets::NavView::Queue => View::Queue,
                widgets::NavView::Albums => View::Albums,
                widgets::NavView::Artists => View::Artists,
                widgets::NavView::Genres => View::Genres,
                widgets::NavView::Songs => View::Songs,
                widgets::NavView::Playlists => View::Playlists,
            };
            Message::SwitchView(view)
        }
        widgets::NavBarMessage::ToggleLightMode => Message::ToggleLightMode,
        widgets::NavBarMessage::ToggleSoundEffects => {
            Message::Playback(PlaybackMessage::ToggleSoundEffects)
        }
        widgets::NavBarMessage::OpenSettings => Message::SwitchView(View::Settings),
        widgets::NavBarMessage::StripClicked => Message::StripClicked,
        widgets::NavBarMessage::StripContextAction(entry) => Message::StripContextAction(entry),
        widgets::NavBarMessage::About => {
            Message::AboutModal(crate::widgets::about_modal::AboutModalMessage::Open)
        }
        widgets::NavBarMessage::Quit => Message::QuitApp,
    }
}

impl Nokkvi {
    // =========================================================================
    // SECTION: View Functions
    // =========================================================================

    /// Root view dispatcher
    pub fn view(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Login => self.login_view(),
            Screen::Home => self.home_view(),
        }
    }

    // -------------------------------------------------------------------------
    // Login View: Delegate to LoginPage
    // -------------------------------------------------------------------------

    /// Login screen view - delegates to LoginPage component
    fn login_view(&self) -> Element<'_, Message> {
        self.login_page.view().map(Message::Login)
    }

    /// Home screen layout (nav bar + content + player bar)
    fn home_view(&self) -> Element<'_, Message> {
        let has_queue = !self.library.queue_songs.is_empty();
        let player_bar_data = widgets::PlayerBarViewData {
            playback_position: self.playback.position,
            playback_duration: self.playback.duration,
            playback_playing: self.playback.playing,
            playback_paused: self.playback.paused,
            volume: self.playback.volume,
            show_volume_percentage: self.playback.show_volume_percentage,
            has_queue,
            is_random_mode: self.modes.random,
            is_repeat_mode: self.modes.repeat,
            is_repeat_queue_mode: self.modes.repeat_queue,
            is_consume_mode: self.modes.consume,
            eq_enabled: self.playback.eq_state.is_enabled(),
            sound_effects_enabled: self.sfx.enabled,
            sfx_volume: self.sfx.volume,
            show_sfx_volume_percentage: self.sfx.show_percentage,
            visualization_mode: self.engine.visualization_mode,
            window_width: self.window.width,
            is_light_mode: crate::theme::is_light_mode(),
            track_title: self.playback.title.clone(),
            track_artist: self.playback.artist.clone(),
            track_album: self.playback.album.clone(),
            format_suffix: self.playback.format_suffix.clone(),
            sample_rate: self.playback.sample_rate,
            bitrate: self.playback.bitrate,
        };

        // Shared strip data — borrows playback state, no clones needed.
        let strip_data = widgets::track_info_strip::TrackInfoStripData {
            title: &self.playback.title,
            artist: &self.playback.artist,
            album: &self.playback.album,
            format_suffix: &self.playback.format_suffix,
            sample_rate: self.playback.sample_rate,
            bitrate: self.playback.bitrate,
        };

        // Build the player bar info strip if PlayerBar mode is active
        let player_strip: Option<Element<'_, widgets::PlayerBarMessage>> =
            if crate::theme::show_player_bar_strip() {
                Some(widgets::track_info_strip::track_info_strip_with_separator(
                    &strip_data,
                    Some(widgets::PlayerBarMessage::StripClicked),
                ))
            } else {
                None
            };

        // Wrap player bar strip in context menu for right-click actions
        let player_strip: Option<Element<'_, widgets::PlayerBarMessage>> =
            player_strip.map(|strip| {
                let has_local_path = !self.local_music_path.is_empty();
                let is_starred = self.is_current_track_starred();
                widgets::context_menu::context_menu(
                    strip,
                    widgets::context_menu::strip_entries(has_local_path),
                    move |entry, length| {
                        widgets::context_menu::strip_entry_view(
                            entry,
                            length,
                            is_starred,
                            widgets::PlayerBarMessage::StripContextAction,
                        )
                    },
                )
                .into()
            });

        // Base layout: nav bar + content + player bar
        // In Side mode, add vertical sidebar next to content

        let base_layer: Element<'_, Message> = if crate::theme::is_side_nav() {
            let side_nav_view = match self.current_view {
                View::Queue | View::Settings => widgets::NavView::Queue,
                View::Albums => widgets::NavView::Albums,
                View::Artists => widgets::NavView::Artists,
                View::Genres => widgets::NavView::Genres,
                View::Songs => widgets::NavView::Songs,
                View::Playlists => widgets::NavView::Playlists,
            };
            let side_data = widgets::SideNavBarData {
                current_view: side_nav_view,
                settings_open: self.current_view == View::Settings,
            };

            // Build the outer column: optionally top bar strip → sidebar+content row → player bar
            let mut outer = iced::widget::Column::new();

            // Top bar info strip (full window width, above sidebar)
            if crate::theme::show_top_bar_strip() {
                let strip = widgets::track_info_strip::track_info_strip(
                    &strip_data,
                    Some(Message::StripClicked),
                );
                let has_local_path = !self.local_music_path.is_empty();
                let is_starred = self.is_current_track_starred();
                let wrapped: Element<'_, Message> = widgets::context_menu::context_menu(
                    strip,
                    widgets::context_menu::strip_entries(has_local_path),
                    move |entry, length| {
                        widgets::context_menu::strip_entry_view(
                            entry,
                            length,
                            is_starred,
                            Message::StripContextAction,
                        )
                    },
                )
                .into();
                outer = outer.push(wrapped);
                // Bottom separator to delineate strip from content below
                outer = outer.push(crate::theme::horizontal_separator::<Message>(1.0));
            }

            // Sidebar + content row
            outer = outer.push(iced::widget::row![
                widgets::side_nav_bar(side_data).map(map_nav_bar_message),
                self.main_content(),
            ]);

            // Player bar
            outer = outer
                .push(widgets::player_bar(&player_bar_data, player_strip).map(Message::PlayerBar));

            outer.into()
        } else {
            iced::widget::column![
                self.navigation_bar(),
                self.main_content(),
                widgets::player_bar(&player_bar_data, player_strip).map(Message::PlayerBar),
            ]
            .into()
        };

        // Create stack with base layer
        let mut stack = Stack::new().push(base_layer);

        // Add visualizer as overlay if enabled
        use nokkvi_data::types::player_settings::VisualizationMode;
        if self.engine.visualization_mode != VisualizationMode::Off
            && let Some(ref viz) = self.visualizer
        {
            // Set mode based on current visualization_mode state
            let widget_mode = match self.engine.visualization_mode {
                VisualizationMode::Lines => widgets::visualizer::VisualizationMode::Lines,
                _ => widgets::visualizer::VisualizationMode::Bars,
            };
            let viz_with_mode = viz
                .clone()
                .mode(widget_mode)
                .window_height(self.window.height)
                .width(self.window.width);

            // Visualizer height scales with window (configurable via config.toml, min 80px)
            // Read height_percent and opacity from shared config (hot-reloadable)
            // Height also scales proportionally with window width for better aesthetics
            let cfg = self.visualizer_config.read();
            let height_percent = cfg.height_percent;
            drop(cfg);

            // Scale height proportionally with window width
            // window_width=800 is baseline (1.0x), larger windows get taller visualizer
            // Using sqrt for gentler scaling curve
            let width_scale = (self.window.width / 800.0).sqrt().clamp(0.7, 1.5);
            let scaled_height_percent = height_percent * width_scale;

            // Create a column with a spacer to push visualizer to the bottom
            let visualizer_height = (self.window.height * scaled_height_percent).max(80.0);
            let spacer_height =
                (self.window.height - widgets::player_bar::player_bar_height() - visualizer_height)
                    .max(0.0);
            let visualizer_overlay = column![
                container(iced::widget::Space::new()).height(Length::Fixed(spacer_height)),
                container(viz_with_mode.view())
                    .width(Length::Fill)
                    .height(Length::Fixed(visualizer_height))
            ]
            .width(Length::Fill)
            .height(Length::Fill);

            stack = stack.push(visualizer_overlay);
        }

        // Add text input dialog overlay (if visible)
        if let Some(dialog_overlay) =
            crate::widgets::text_input_dialog::text_input_dialog_overlay(&self.text_input_dialog)
        {
            stack = stack.push(dialog_overlay.map(Message::TextInputDialog));
        }

        // Add info modal overlay (if visible)
        if let Some(info_overlay) = crate::widgets::info_modal::info_modal_overlay(&self.info_modal)
        {
            stack = stack.push(info_overlay.map(Message::InfoModal));
        }

        // Add about modal overlay (if visible)
        if let Some(about_overlay) = crate::widgets::about_modal::about_modal_overlay(
            &self.about_modal,
            crate::widgets::about_modal::AboutViewData {
                server_url: &self.login_page.server_url,
                username: &self.login_page.username,
            },
        ) {
            stack = stack.push(about_overlay.map(Message::AboutModal));
        }

        // When EQ is disabled, show flat gains in the UI so sliders read 0 —
        // avoids the misleading appearance of active boosts. Real gains are
        // preserved in EqState and restore visually when re-enabled.
        let eq_enabled = self.playback.eq_state.is_enabled();
        let eq_gains = if eq_enabled {
            let mut gains = [0.0; 10];
            for (i, g) in gains.iter_mut().enumerate() {
                *g = self.playback.eq_state.get_band_gain(i);
            }
            gains
        } else {
            [0.0; 10]
        };
        if let Some(eq_overlay) = crate::widgets::eq_modal_overlay(
            self.window.eq_modal_open,
            eq_enabled,
            eq_gains,
            &self.window.custom_eq_presets,
            self.window.eq_save_mode,
            &self.window.eq_save_name,
        ) {
            stack = stack.push(eq_overlay.map(Message::EqModal));
        }

        // Add toast status bar overlay (if any active toast)
        if let Some(toast) = self.toast.current() {
            // Toast icon prefix based on level
            let h_align = if toast.right_aligned {
                iced::alignment::Horizontal::Right
            } else {
                iced::alignment::Horizontal::Left
            };
            let toast_text = iced::widget::text(&toast.message)
                .color(crate::theme::toast_level_color(toast.level))
                .font(crate::theme::ui_font())
                .size(14)
                .width(Length::Fill)
                .align_x(h_align);

            // Status bar at bottom of content area, above the player bar:
            // Push to bottom with spacer, leave room for player bar (~56px)
            let toast_bar = container(
                container(toast_text)
                    .padding([4, 12])
                    .style(|_theme: &iced::Theme| container::Style {
                        background: Some(crate::theme::bg0_hard().into()),
                        border: iced::Border {
                            color: crate::theme::bg3(),
                            width: 1.0,
                            radius: crate::theme::ui_border_radius(),
                        },
                        ..Default::default()
                    })
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(iced::Padding {
                top: 0.0,
                right: 0.0,
                bottom: widgets::player_bar::player_bar_height(),
                left: 0.0,
            }); // Offset above player bar

            stack = stack.push(toast_bar);
        }

        // Add floating drag indicator during cross-pane drag — renders a copy
        // of the centered browsing slot at the cursor position.
        if let Some(ref drag) = self.cross_pane_drag {
            let slot_element = self.render_drag_slot();

            // Position the slot near the cursor. Use a width that matches
            // the browser pane slot width, and a fixed row height.
            let slot_width = (self.window.width * 0.42).min(600.0);
            let slot_height = 64.0_f32;

            let offset_x = 12.0_f32;
            let offset_y = -(slot_height / 2.0); // Center vertically on cursor
            let pad_left =
                (drag.cursor.x + offset_x).clamp(0.0, self.window.width - slot_width - 20.0);
            let pad_top = (drag.cursor.y + offset_y).clamp(0.0, self.window.height - slot_height);

            let drag_overlay = container(
                container(slot_element)
                    .width(Length::Fixed(slot_width))
                    .height(Length::Fixed(slot_height)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(iced::Padding {
                top: pad_top,
                left: pad_left,
                right: 0.0,
                bottom: 0.0,
            });

            stack = stack.push(drag_overlay);

            // Drop indicator: thin accent line between queue slots showing insertion point
            if let Some(slot) = drag.drop_target_slot {
                use crate::widgets::slot_list::{
                    EDIT_BAR_HEIGHT, SLOT_SPACING, SlotListConfig, chrome_height_with_header,
                };

                // Match the queue view's chrome height calculation (edit bar adds 32px)
                let edit_bar_height: f32 =
                    if self.playlist_edit.is_some() || self.active_playlist_info.is_some() {
                        EDIT_BAR_HEIGHT
                    } else {
                        0.0
                    };
                let chrome_height = chrome_height_with_header() + edit_bar_height;
                let config = SlotListConfig::with_dynamic_slots(self.window.height, chrome_height);
                let row_height = config.row_height();
                let slot_spacing = SLOT_SPACING;
                let slot_step = row_height + slot_spacing;

                // Account for side nav bar when computing X offsets for the drop indicator.
                let is_side_nav = crate::theme::is_side_nav();
                let sidebar_width = if is_side_nav {
                    crate::widgets::side_nav_bar::SIDE_NAV_WIDTH + 2.0 // +2 for border
                } else {
                    0.0
                };

                let slot_list_start_y =
                    crate::widgets::slot_list::queue_slot_list_start_y(edit_bar_height);

                // Convert the item index back to a slot position for rendering.
                // We need the slot that corresponds to this item in the current viewport.
                let total_queue = self.library.queue_songs.len();
                let viewport_offset = self.queue_page.common.slot_list.viewport_offset;

                // Find which visual slot this item index maps to
                let visual_slot = if total_queue == 0 {
                    0
                } else {
                    // slot = item_index - viewport_offset + effective_center
                    let effective_center = if total_queue < config.slot_count {
                        0
                    } else {
                        let items_at_and_after = total_queue.saturating_sub(viewport_offset);
                        let end_push = config.slot_count.saturating_sub(items_at_and_after);
                        config.center_slot.min(viewport_offset).max(end_push)
                    };
                    (slot as i32 - viewport_offset as i32 + effective_center as i32).max(0) as usize
                };

                // Position the line at the TOP edge of the hovered slot (between it and slot above)
                let line_y =
                    slot_list_start_y + (visual_slot as f32 * slot_step) - (slot_spacing / 2.0);

                // Queue pane width: FillPortion(55) of the content area.
                // Content area = window_width - sidebar_width.
                let content_width = self.window.width - sidebar_width;
                let queue_width = content_width * 55.0 / 100.0;
                let indicator_left = sidebar_width + 8.0;

                let indicator_line = container(
                    container(iced::widget::Space::new())
                        .width(Length::Fixed(queue_width - 16.0))
                        .height(Length::Fixed(2.0))
                        .style(|_theme: &iced::Theme| container::Style {
                            background: Some(crate::theme::accent_bright().into()),
                            border: iced::Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(iced::Padding {
                    top: line_y.max(0.0),
                    left: indicator_left,
                    right: 0.0,
                    bottom: 0.0,
                });

                stack = stack.push(indicator_line);
            }
        }

        stack.into()
    }

    // -------------------------------------------------------------------------
    // Navigation Bar: Delegate to nav_bar component
    // -------------------------------------------------------------------------

    /// Navigation bar - delegates to nav_bar component with playback data
    fn navigation_bar(&self) -> Element<'_, Message> {
        // Convert app::View to widgets::NavView for the component
        let settings_open = matches!(self.current_view, View::Settings);
        let current_nav_view = match self.current_view {
            View::Queue | View::Settings => widgets::NavView::Queue, // fallback; ignored when settings_open
            View::Albums => widgets::NavView::Albums,
            View::Artists => widgets::NavView::Artists,
            View::Genres => widgets::NavView::Genres,
            View::Songs => widgets::NavView::Songs,
            View::Playlists => widgets::NavView::Playlists,
        };

        // Create NavBarViewData with current playback state
        let nav_bar_data = widgets::NavBarViewData {
            current_view: current_nav_view,
            track_title: self.playback.title.clone(),
            track_artist: self.playback.artist.clone(),
            track_album: self.playback.album.clone(),
            is_playing: self.playback.has_track(),
            format_suffix: self.playback.format_suffix.clone(),
            sample_rate_khz: self.playback.sample_rate as f32 / 1000.0,
            bitrate_kbps: self.playback.bitrate,
            window_width: self.window.width,
            is_light_mode: crate::theme::is_light_mode(),
            sound_effects_enabled: self.sfx.enabled,
            settings_open,
            local_music_path: self.local_music_path.clone(),
            is_current_starred: self.is_current_track_starred(),
        };

        // Use the nav_bar component, mapping NavBarMessage to app Message
        widgets::nav_bar(nav_bar_data).map(map_nav_bar_message)
    }

    /// Main content area - dispatches to current view's page
    fn main_content(&self) -> Element<'_, Message> {
        // Borrow the pre-computed large_artwork snapshot (refreshed after each LRU mutation).
        // This avoids re-creating the HashMap on every render frame.
        let large_artwork = &self.artwork.large_artwork_snapshot;

        // =====================================================================
        // Split-view layout for playlist edit mode or browsing panel toggle
        // =====================================================================
        if self.browsing_panel.is_some() && self.current_view == View::Queue {
            use iced::widget::{column as col, row as r};

            // --- LEFT PANE: Queue (editing surface) ---
            let filtered_queue_songs = self.filter_queue_songs();
            let current_playing_song_id = self.scrobble.current_song_id.clone();

            // Build edit_mode_info only when actually editing a playlist
            let edit_mode_info = self.playlist_edit.as_ref().map(|edit_state| {
                let current_ids = self.queue_song_ids();
                let is_dirty = edit_state.is_dirty(&current_ids)
                    || edit_state.is_name_dirty()
                    || edit_state.is_comment_dirty();
                (edit_state.playlist_name.clone(), is_dirty)
            });

            let edit_mode_comment = self
                .playlist_edit
                .as_ref()
                .map(|edit_state| edit_state.playlist_comment.clone());

            let queue_view_data = views::QueueViewData {
                queue_songs: filtered_queue_songs,
                album_art: &self.artwork.album_art,
                large_artwork,
                window_width: self.window.width * 0.55,
                window_height: self.window.height,
                scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                current_playing_song_id,
                current_playing_queue_index: self.last_queue_current_index,
                is_playing: self.playback.playing && !self.playback.paused,
                total_queue_count: self
                    .library
                    .queue_loading_target
                    .unwrap_or(self.library.queue_songs.len()),
                stable_viewport: self.stable_viewport,
                edit_mode_info,
                edit_mode_comment,
                playlist_context_info: self.active_playlist_info.clone(),
            };

            let queue_pane = self.queue_page.view(queue_view_data).map(Message::Queue);
            let queue_focused = self.pane_focus == crate::state::PaneFocus::Queue;

            // Shared pane border style: accent + thick when focused, bg3 + thin otherwise
            let pane_border_style =
                |focused: bool| -> Box<dyn Fn(&iced::Theme) -> container::Style> {
                    let border_color = if focused {
                        crate::theme::accent()
                    } else {
                        crate::theme::bg3()
                    };
                    let border_width = if focused { 2.0 } else { 1.0 };
                    Box::new(move |_theme| container::Style {
                        border: iced::Border {
                            color: border_color,
                            width: border_width,
                            radius: crate::theme::ui_border_radius(),
                        },
                        ..Default::default()
                    })
                };

            let queue_container = container(queue_pane)
                .width(Length::FillPortion(55))
                .height(Length::Fill)
                .style(if self.cross_pane_drag.is_some() {
                    // Drop target highlight during active drag
                    let accent = crate::theme::accent_bright();
                    Box::new(move |_theme: &iced::Theme| container::Style {
                        border: iced::Border {
                            color: accent,
                            width: 3.0,
                            radius: crate::theme::ui_border_radius(),
                        },
                        background: Some(iced::Color { a: 0.05, ..accent }.into()),
                        ..Default::default()
                    }) as Box<dyn Fn(&iced::Theme) -> container::Style>
                } else {
                    pane_border_style(queue_focused)
                });

            // --- RIGHT PANE: Browsing panel ---
            let browser_focused = self.pane_focus == crate::state::PaneFocus::Browser;

            let browser_content: Element<'_, Message> = if let Some(ref panel) = self.browsing_panel
            {
                let tab_bar = panel.tab_bar().map(Message::BrowsingPanel);

                // The tab bar eats into available height — subtract it so the
                // slot list slot calculation doesn't overflow the last slot.
                use crate::widgets::slot_list::TAB_BAR_HEIGHT;
                let browser_height = self.window.height - TAB_BAR_HEIGHT;

                // Delegate to the active view's existing page
                let view_content: Element<'_, Message> = match panel.active_view {
                    views::BrowsingView::Albums => {
                        let view_data = views::AlbumsViewData {
                            albums: &self.library.albums,
                            album_art: &self.artwork.album_art,
                            large_artwork,
                            window_width: self.window.width * 0.45,
                            window_height: browser_height,
                            scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                            total_album_count: self.library.counts.albums,
                            loading: self.library.albums.is_loading(),
                            stable_viewport: true, // Browser pane: click to highlight, not play
                        };
                        self.albums_page.view(view_data).map(Message::Albums)
                    }
                    views::BrowsingView::Songs => {
                        let view_data = views::SongsViewData {
                            songs: &self.library.songs,
                            album_art: &self.artwork.album_art,
                            large_artwork,
                            window_width: self.window.width * 0.45,
                            window_height: browser_height,
                            scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                            total_song_count: self.library.counts.songs,
                            loading: self.library.songs.is_loading(),
                            stable_viewport: true, // Browser pane: click to highlight, not play
                        };
                        self.songs_page.view(view_data).map(Message::Songs)
                    }
                    views::BrowsingView::Artists => {
                        let view_data = views::ArtistsViewData {
                            artists: &self.library.artists,
                            artist_art: &self.artwork.album_art,
                            large_artwork,
                            window_width: self.window.width * 0.45,
                            window_height: browser_height,
                            scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                            total_artist_count: self.library.counts.artists,
                            loading: self.library.artists.is_loading(),
                            stable_viewport: true, // Browser pane: click to highlight, not play
                        };
                        self.artists_page.view(view_data).map(Message::Artists)
                    }
                    views::BrowsingView::Genres => {
                        let view_data = views::GenresViewData {
                            genres: &self.library.genres,
                            genre_artwork: &self.artwork.genre.mini,
                            genre_collage_artwork: &self.artwork.genre.collage,
                            window_width: self.window.width * 0.45,
                            window_height: browser_height,
                            scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                            total_genre_count: self.library.counts.genres,
                            loading: self.library.genres.is_loading(),
                            stable_viewport: true, // Browser pane: click to highlight, not play
                        };
                        self.genres_page.view(view_data).map(Message::Genres)
                    }
                };

                col![tab_bar, view_content].into()
            } else {
                container(iced::widget::text("No library browser"))
                    .center(Length::Fill)
                    .into()
            };

            let browser_container = container(browser_content)
                .width(Length::FillPortion(45))
                .height(Length::Fill)
                .style(pane_border_style(browser_focused));

            return r![queue_container, browser_container]
                .height(Length::Fill)
                .into();
        }

        // =====================================================================
        // Normal single-view layout
        // =====================================================================
        match self.current_view {
            View::Albums => {
                let view_data = views::AlbumsViewData {
                    albums: &self.library.albums,
                    album_art: &self.artwork.album_art,
                    large_artwork,
                    window_width: self.window.width,
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                    total_album_count: self.library.counts.albums,
                    loading: self.library.albums.is_loading(),
                    stable_viewport: self.stable_viewport,
                };
                self.albums_page.view(view_data).map(Message::Albums)
            }
            View::Queue => {
                let filtered_queue_songs = self.filter_queue_songs();
                let current_playing_song_id = self.scrobble.current_song_id.clone();
                let view_data = views::QueueViewData {
                    queue_songs: filtered_queue_songs,
                    album_art: &self.artwork.album_art,
                    large_artwork,
                    window_width: self.window.width,
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                    current_playing_song_id,
                    current_playing_queue_index: self.last_queue_current_index,
                    is_playing: self.playback.playing && !self.playback.paused,
                    total_queue_count: self
                        .library
                        .queue_loading_target
                        .unwrap_or(self.library.queue_songs.len()),
                    stable_viewport: self.stable_viewport,
                    edit_mode_info: None,
                    edit_mode_comment: None,
                    playlist_context_info: self.active_playlist_info.clone(),
                };
                self.queue_page.view(view_data).map(Message::Queue)
            }
            View::Artists => {
                let view_data = views::ArtistsViewData {
                    artists: &self.library.artists,
                    artist_art: &self.artwork.album_art, // Reuse album art cache for artist images
                    large_artwork,
                    window_width: self.window.width,
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                    total_artist_count: self.library.counts.artists,
                    loading: self.library.artists.is_loading(),
                    stable_viewport: self.stable_viewport,
                };
                self.artists_page.view(view_data).map(Message::Artists)
            }
            View::Songs => {
                let view_data = views::SongsViewData {
                    songs: &self.library.songs,
                    album_art: &self.artwork.album_art,
                    large_artwork,
                    window_width: self.window.width,
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                    total_song_count: self.library.counts.songs,
                    loading: self.library.songs.is_loading(),
                    stable_viewport: self.stable_viewport,
                };
                self.songs_page.view(view_data).map(Message::Songs)
            }
            View::Genres => {
                let view_data = views::GenresViewData {
                    genres: &self.library.genres,
                    genre_artwork: &self.artwork.genre.mini,
                    genre_collage_artwork: &self.artwork.genre.collage,
                    window_width: self.window.width,
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                    total_genre_count: self.library.counts.genres,
                    loading: self.library.genres.is_loading(),
                    stable_viewport: self.stable_viewport,
                };
                self.genres_page.view(view_data).map(Message::Genres)
            }
            View::Playlists => {
                let view_data = views::PlaylistsViewData {
                    playlists: &self.library.playlists,
                    playlist_artwork: &self.artwork.playlist.mini,
                    playlist_collage_artwork: &self.artwork.playlist.collage,
                    window_width: self.window.width,
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                modifiers: self.window.keyboard_modifiers,
                    total_playlist_count: self.library.counts.playlists,
                    loading: self.library.playlists.is_loading(),
                    stable_viewport: self.stable_viewport,
                };
                self.playlists_page.view(view_data).map(Message::Playlists)
            }
            View::Settings => {
                let viz_config = self.visualizer_config.read().clone();
                let theme_file = crate::theme_config::load_active_theme_file();
                let active_theme_stem =
                    nokkvi_data::services::theme_loader::read_theme_name_from_config();
                let settings_data = views::SettingsViewData {
                    visualizer_config: viz_config,
                    theme_file,
                    active_theme_stem,
                    window_height: self.window.height,
                    hotkey_config: self.hotkey_config.clone(),
                    server_url: self.login_page.server_url.clone(),
                    username: self.login_page.username.clone(),
                    is_light_mode: crate::theme::is_light_mode(),
                    scrobbling_enabled: self.scrobbling_enabled,
                    scrobble_threshold: self.scrobble_threshold,
                    start_view: self.start_view.clone(),
                    stable_viewport: self.stable_viewport,
                    auto_follow_playing: self.auto_follow_playing,
                    enter_behavior: self.enter_behavior.as_label(),
                    local_music_path: self.local_music_path.clone(),
                    rounded_mode: crate::theme::is_rounded_mode(),
                    nav_layout: if crate::theme::is_side_nav() {
                        "Side"
                    } else {
                        "Top"
                    },
                    nav_display_mode: crate::theme::nav_display_mode().as_label(),
                    track_info_display: crate::theme::track_info_display().as_label(),
                    slot_row_height: crate::theme::slot_row_height_variant().as_label(),
                    opacity_gradient: crate::theme::is_opacity_gradient(),
                    crossfade_enabled: self.engine.crossfade_enabled,
                    crossfade_duration_secs: self.engine.crossfade_duration_secs,
                    volume_normalization: self.engine.volume_normalization,
                    normalization_level: self.engine.normalization_level.as_label(),
                    default_playlist_name: self.default_playlist_name.clone(),
                    quick_add_to_playlist: self.quick_add_to_playlist,
                    horizontal_volume: crate::theme::is_horizontal_volume(),
                    font_family: crate::theme::font_family(),
                    strip_show_title: crate::theme::strip_show_title(),
                    strip_show_artist: crate::theme::strip_show_artist(),
                    strip_show_album: crate::theme::strip_show_album(),
                    strip_show_format_info: crate::theme::strip_show_format_info(),
                    strip_click_action: crate::theme::strip_click_action().as_label(),
                    verbose_config: self.verbose_config,
                };
                self.settings_page
                    .view(settings_data)
                    .map(Message::Settings)
            }
        }
    }
}
