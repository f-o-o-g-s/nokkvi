//! Radios Page Component
//!
//! Flat slot list for internet radio stations. No expansion, no artwork, no star/rating.
//! Activating a station transitions to ActivePlayback::Radio and plays the stream URL.

use iced::{Alignment, Element, Length, Task, widget::container};
use nokkvi_data::types::radio_station::RadioStation;

use crate::{
    app_message::Message,
    widgets::{
        self, SlotListPageMessage, SlotListPageState,
        slot_list_page::SlotListPageAction,
        view_header::{HeaderButton, SortMode, ViewHeaderConfig},
    },
};

// ============================================================================
// State
// ============================================================================

/// Radios page local state — minimal, no expansion
#[derive(Debug)]
pub struct RadiosPage {
    pub common: SlotListPageState,
    /// Cache of the last `(ascending, station_count)` that was sorted. Same
    /// short-circuit policy as `QueuePage::last_sort_signature`.
    pub last_sort_signature: Option<(bool, usize)>,
}

/// View data passed from root (borrows from app state)
pub struct RadiosViewData<'a> {
    pub stations: std::borrow::Cow<'a, [RadioStation]>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub loading: bool,
    pub total_station_count: usize,
    pub stable_viewport: bool,
    /// Whether artwork-elevation is in effect for this frame. Forwarded into
    /// BaseSlotListLayoutConfig.elevated. Always false in split-view /
    /// side-nav / none-nav.
    pub elevated: bool,
    pub modifiers: iced::keyboard::Modifiers,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus can resolve their own open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
    /// The station currently driving radio playback, if any — the SINGLE
    /// playing-station source for this view (the row glow derives its id
    /// from it, so the pair can't drift). Resolved at `app_view` from the
    /// LIBRARY list by id when possible (fresh `coverArt` token — the
    /// `active_playback` copy is snapshotted at play time and goes stale
    /// after an upload/reset), falling back to the `active_playback` copy
    /// when the library hasn't loaded the station.
    pub playing_station: Option<&'a RadioStation>,
    /// Mini station artwork (`station_id -> Handle`) for the per-row thumbnail:
    /// an uploaded logo or the remembered last-played stream art. Ids absent
    /// here fall back to the radio-tower glyph.
    pub radio_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    /// Large station artwork (`station_id -> Handle`) for the artwork panel:
    /// a resolution-sized logo or the live now-playing stream image.
    pub radio_large_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    /// Over-cover visualizer drawn over the station artwork (Scope always;
    /// Bars/Lines when their placement is `OverCover`). `None` for bottom-band
    /// placement or `Off`. Mirrors the Queue view's field.
    pub over_art_visualizer: Option<(
        crate::widgets::visualizer::Visualizer,
        crate::widgets::visualizer::VisualizationMode,
        f32,
    )>,
    /// Surfing boat over the over-cover Lines visualizer (Lines + `OverCover` +
    /// boat visible), else `None`. `pub(crate)` — `OverCoverBoat` wraps the
    /// crate-private `BoatState`.
    pub(crate) over_art_boat: Option<crate::widgets::base_slot_list_layout::OverCoverBoat<'a>>,
}

// ============================================================================
// Messages & Actions
// ============================================================================

/// Messages for local radio page interactions
#[derive(Debug, Clone)]
pub enum RadiosMessage {
    // Slot-list navigation, activation, sort, search — all carried through the
    // shared SlotListPageMessage enum and dispatched via common.handle().
    SlotList(crate::widgets::SlotListPageMessage),

    // Per-view: station identity matters for auto-scroll
    FocusCurrentPlaying(String), // Auto-scroll slot list to center currently playing station (station_id)

    /// Sort dropdown's "Roulette" entry was selected — intercepted at the
    /// root handler before the page's `update` runs.
    Roulette,

    // Data loading
    RadioStationsLoaded(Result<Vec<RadioStation>, String>),

    // Context Menu Actions
    EditStationDialog(RadioStation),
    DeleteStationConfirmation(String, String),
    CopyStreamUrl(String),
    /// Forget a station's remembered/stale artwork (right-click → Refresh
    /// Artwork): clears the cached thumbnail (memory + disk) so it reverts to
    /// the tower glyph, then re-fetches the uploaded logo if the station has one.
    RefreshStationArtwork(RadioStation),
    /// Right-click → "Set Custom Artwork…": open the native file picker and
    /// upload the chosen image as the station's server-side logo.
    SetStationArtwork(RadioStation),
    /// Right-click → "Reset Artwork": delete the uploaded logo server-side so
    /// the automatic artwork (ICY capture / tower glyph) returns.
    ResetStationArtwork(RadioStation),

    AddRadioStation,
    NoOp,

    /// Context-menu open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_radios` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum RadiosAction {
    /// User activated a radio station — root should transition to radio playback
    PlayRadioStation(RadioStation),
    FocusOnStation(String),
    AddRadioStation,
    EditRadioStation(RadioStation),
    DeleteStation(String, String),
    RefreshStationArtwork(RadioStation),
    /// Root should run the pick-file → upload flow for this station.
    SetStationArtwork(RadioStation),
    /// Root should DELETE the station's uploaded logo and refresh.
    ResetStationArtwork(RadioStation),
    SearchChanged(String),
    SortModeChanged(SortMode),
    SortOrderChanged(bool),
    RefreshViewData,
    CenterOnPlaying,
    None,
}

crate::views::impl_has_common_action!(RadiosAction, no_navigate_filter);

// ============================================================================
// Default / Constructor
// ============================================================================

impl Default for RadiosPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                SortMode::Name,
                true, // sort_ascending
            ),
            last_sort_signature: None,
        }
    }
}

impl RadiosPage {
    pub fn new() -> Self {
        Self::default()
    }

    // ========================================================================
    // Update
    // ========================================================================

    pub fn update(
        &mut self,
        message: RadiosMessage,
        stations: &[RadioStation],
    ) -> (Task<RadiosMessage>, RadiosAction) {
        let total = stations.len();

        match message {
            // -----------------------------------------------------------------
            // Slot-list navigation, activation, sort, search
            // -----------------------------------------------------------------
            RadiosMessage::SlotList(msg) => {
                match self.common.handle(msg, total) {
                    SlotListPageAction::ActivateCenter(_) => {
                        if let Some(center_idx) = self.common.get_center_item_index(total)
                            && let Some(station) = stations.get(center_idx)
                        {
                            self.common.slot_list.flash_center();
                            return (
                                Task::none(),
                                RadiosAction::PlayRadioStation(station.clone()),
                            );
                        }
                        (Task::none(), RadiosAction::None)
                    }
                    SlotListPageAction::SearchChanged(q) => {
                        (Task::none(), RadiosAction::SearchChanged(q))
                    }
                    SlotListPageAction::SortModeChanged(m) => {
                        (Task::none(), RadiosAction::SortModeChanged(m))
                    }
                    SlotListPageAction::SortOrderChanged(b) => {
                        (Task::none(), RadiosAction::SortOrderChanged(b))
                    }
                    SlotListPageAction::RefreshViewData => {
                        (Task::none(), RadiosAction::RefreshViewData)
                    }
                    SlotListPageAction::CenterOnPlaying => {
                        (Task::none(), RadiosAction::CenterOnPlaying)
                    }
                    SlotListPageAction::None => (Task::none(), RadiosAction::None),
                    SlotListPageAction::AddCenterToQueue => (Task::none(), RadiosAction::None), // Radios doesn't queue
                }
            }

            RadiosMessage::FocusCurrentPlaying(station_id) => {
                (Task::none(), RadiosAction::FocusOnStation(station_id))
            }

            RadiosMessage::AddRadioStation => (Task::none(), RadiosAction::AddRadioStation),

            RadiosMessage::EditStationDialog(station) => {
                (Task::none(), RadiosAction::EditRadioStation(station))
            }
            RadiosMessage::DeleteStationConfirmation(id, name) => {
                (Task::none(), RadiosAction::DeleteStation(id, name))
            }
            RadiosMessage::CopyStreamUrl(url) => {
                let task = iced::clipboard::write(url).map(|_| RadiosMessage::NoOp);
                (task, RadiosAction::None)
            }
            RadiosMessage::RefreshStationArtwork(station) => {
                (Task::none(), RadiosAction::RefreshStationArtwork(station))
            }
            RadiosMessage::SetStationArtwork(station) => {
                (Task::none(), RadiosAction::SetStationArtwork(station))
            }
            RadiosMessage::ResetStationArtwork(station) => {
                (Task::none(), RadiosAction::ResetStationArtwork(station))
            }

            RadiosMessage::NoOp => (Task::none(), RadiosAction::None),

            // Data loading — handled at root level
            RadiosMessage::RadioStationsLoaded(_) => (Task::none(), RadiosAction::None),

            // Routed up to root in `handle_radios` before this match runs;
            // arm exists only for exhaustiveness.
            RadiosMessage::SetOpenMenu(_) => (Task::none(), RadiosAction::None),
            RadiosMessage::Roulette => (Task::none(), RadiosAction::None),
        }
    }

    // ========================================================================
    // View
    // ========================================================================

    pub fn view<'a>(&'a self, data: RadiosViewData<'a>) -> Element<'a, RadiosMessage> {
        // Only Name sort mode is meaningful for radio stations — no date, artist,
        // or album metadata to sort on.
        // Auto-hide toolbar: collapse to a hairline when enabled and not
        // currently revealed (hover / active search / hotkey window).
        let autohide = crate::theme::is_autohide_toolbar();
        // Radios has no columns picker, so the sort dropdown (folded into
        // `toolbar_revealed`) is the only reveal-lock — pass `false`.
        let toolbar_collapsed = self.common.toolbar_collapsed(autohide, false);

        let header = widgets::view_header::view_header(ViewHeaderConfig {
            current_view: self.common.current_sort_mode,
            view_options: &[SortMode::Name],
            sort_ascending: self.common.sort_ascending,
            search_query: &self.common.search_query,
            filtered_count: data.stations.len(),
            total_count: data.total_station_count,
            item_type: "stations",
            search_input_id: crate::views::RADIOS_SEARCH_ID,
            on_view_selected: Box::new(|m| {
                RadiosMessage::SlotList(SlotListPageMessage::SortModeSelected(m))
            }),
            show_search: true,
            on_search_change: Box::new(|q| {
                RadiosMessage::SlotList(SlotListPageMessage::SearchQueryChanged(q))
            }),
            buttons: vec![
                HeaderButton::SortToggle(RadiosMessage::SlotList(
                    SlotListPageMessage::ToggleSortOrder,
                )),
                HeaderButton::Refresh(RadiosMessage::SlotList(
                    SlotListPageMessage::RefreshViewData,
                )),
                HeaderButton::CenterOnPlaying(RadiosMessage::SlotList(
                    SlotListPageMessage::CenterOnPlaying,
                )),
                HeaderButton::Add("Add Station", RadiosMessage::AddRadioStation),
            ],
            on_roulette: Some(RadiosMessage::Roulette),
            collapsed: toolbar_collapsed,
            on_hover_enter: autohide.then_some(RadiosMessage::SlotList(
                SlotListPageMessage::ToolbarHoverEnter,
            )),
            on_hover_exit: autohide.then_some(RadiosMessage::SlotList(
                SlotListPageMessage::ToolbarHoverExit,
            )),
            on_dropdown_open: autohide.then_some(RadiosMessage::SlotList(
                SlotListPageMessage::ToolbarDropdownToggled(true),
            )),
            on_dropdown_close: autohide.then_some(RadiosMessage::SlotList(
                SlotListPageMessage::ToolbarDropdownToggled(false),
            )),
            // Radio stations have no duration — count only.
            total_duration_secs: None,
            sort_placeholder: None,
        });

        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListConfig, chrome_height_with_header,
            slot_list_text_column, slot_list_view_with_scroll,
        };

        let slot_list_chrome = chrome_height_with_header(toolbar_collapsed);

        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true, // station logos / remembered stream art
            slot_list_chrome,
            elevated: data.elevated,
        };

        if data.loading {
            return widgets::base_slot_list_empty_state(header, "Loading...", &layout_config);
        }

        if data.stations.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No radio stations found. Add stations in Navidrome.",
                &layout_config,
            );
        }

        // Account for a vertically-stacked artwork panel (Always-Vertical /
        // Auto portrait fallback) in the slot-row math, like the other artwork
        // views — otherwise rows render too tall and overflow behind it. Returns
        // 0 in horizontal modes, so landscape is unaffected.
        let vertical_artwork_chrome =
            crate::widgets::base_slot_list_layout::vertical_artwork_chrome(&layout_config);
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            slot_list_chrome + vertical_artwork_chrome,
        )
        .with_modifiers(data.modifiers);

        let stations = data.stations.as_ref();
        let open_menu_for_rows = data.open_menu;
        let current_station_id = data.playing_station.map(|s| s.id.as_str());

        // Render slot list — flat list, each row is a radio station
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            stations,
            &config,
            RadiosMessage::SlotList(SlotListPageMessage::NavigateUp),
            RadiosMessage::SlotList(SlotListPageMessage::NavigateDown),
            crate::views::scroll_seek_msg(stations.len(), RadiosMessage::SlotList),
            Some(crate::widgets::slot_list::SlotHoverCallback::for_slot_list(
                RadiosMessage::SlotList,
            )),
            |station, ctx| {
                // The station currently driving radio playback gets the
                // now-playing highlight + breathing glow, matching the
                // queue/song-list slot. Both `is_highlighted` and `is_playing`
                // take this flag so the row breathes rather than wearing the
                // static highlight ring.
                let is_playing = current_station_id == Some(station.id.as_str());
                let style = ctx.slot_style(is_playing, is_playing, 0);

                let m = ctx.metrics;

                // Station thumbnail: an uploaded logo / remembered stream art
                // when available, else the radio-tower glyph tinted to the slot
                // text color. Sized to the slot artwork cell so the column lines
                // up with the album/song/queue views.
                let art_size = m.artwork_size;
                let thumb: Element<RadiosMessage> = if let Some(handle) =
                    data.radio_art.get(&station.id)
                {
                    container(
                        iced::widget::image(handle.clone())
                            .content_fit(iced::ContentFit::Cover)
                            .width(Length::Fill)
                            .height(Length::Fill),
                    )
                    .width(Length::Fixed(art_size))
                    .height(Length::Fixed(art_size))
                    .clip(true)
                    .style(|_theme| iced::widget::container::Style {
                        background: Some(crate::theme::bg2().into()),
                        border: iced::Border {
                            radius: crate::theme::ui_radius_sm(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .into()
                } else {
                    container(
                        crate::embedded_svg::svg_widget(
                            crate::widgets::track_info_strip::RADIO_TOWER_ICON_PATH,
                        )
                        .width(Length::Fixed(art_size * 0.55))
                        .height(Length::Fixed(art_size * 0.55))
                        .style(move |_, _| iced::widget::svg::Style {
                            color: Some(style.text_color),
                        }),
                    )
                    .width(Length::Fixed(art_size))
                    .height(Length::Fixed(art_size))
                    .align_y(Alignment::Center)
                    .align_x(Alignment::Center)
                    .into()
                };

                // Name as title; stream URL as subtitle (aids identification when
                // station names are ambiguous or duplicated across sources).
                let subtitle = station
                    .home_page_url
                    .as_deref()
                    .unwrap_or(&station.stream_url)
                    .to_owned();

                let text_col = slot_list_text_column::<RadiosMessage>(
                    station.name.clone(),
                    None,
                    subtitle,
                    None,
                    m.title_size,
                    m.subtitle_size,
                    style,
                    ctx.is_center,
                    100,
                );

                let content = iced::widget::Row::new()
                    .push(thumb)
                    .push(text_col)
                    .spacing(10.0)
                    .align_y(Alignment::Center)
                    .padding(iced::Padding {
                        left: SLOT_LIST_SLOT_PADDING,
                        right: 4.0,
                        top: 4.0,
                        bottom: 4.0,
                    })
                    .height(Length::Fill);

                let clickable = container(content)
                    .style(move |_theme| style.to_container_style())
                    .width(Length::Fill);

                let slot_button = crate::widgets::slot_list::primary_slot_button(
                    clickable,
                    &ctx,
                    data.stable_viewport,
                    RadiosMessage::SlotList,
                );

                // Overlay the breathing glow (pulsing inner glow + travelling
                // shimmer) on the now-playing station row; a no-op pass-through
                // otherwise.
                let glowing = crate::widgets::slot_list::glow_overlay(slot_button, style);

                let cm_id = crate::app_message::ContextMenuId::RadioRow(ctx.item_index);
                let (cm_open, cm_position) =
                    crate::widgets::context_menu::open_state_for(open_menu_for_rows, &cm_id);
                let cm_id_for_msg = cm_id.clone();
                crate::widgets::context_menu::context_menu(
                    glowing,
                    crate::widgets::context_menu::radio_entries(
                        station.logo_cover_art().is_some(),
                    ),
                    {
                        let station_cloned = station.clone();
                        move |entry, length| {
                            let s = station_cloned.clone();
                            crate::widgets::context_menu::radio_entry_view(
                                entry,
                                length,
                                move |a| match a {
                                    crate::widgets::context_menu::RadioContextEntry::Edit => {
                                        RadiosMessage::EditStationDialog(s.clone())
                                    }
                                    crate::widgets::context_menu::RadioContextEntry::CopyStreamUrl => {
                                        RadiosMessage::CopyStreamUrl(s.stream_url.clone())
                                    }
                                    crate::widgets::context_menu::RadioContextEntry::SetArtwork => {
                                        RadiosMessage::SetStationArtwork(s.clone())
                                    }
                                    crate::widgets::context_menu::RadioContextEntry::ResetArtwork => {
                                        RadiosMessage::ResetStationArtwork(s.clone())
                                    }
                                    crate::widgets::context_menu::RadioContextEntry::RefreshArtwork => {
                                        RadiosMessage::RefreshStationArtwork(s.clone())
                                    }
                                    crate::widgets::context_menu::RadioContextEntry::Delete => {
                                        RadiosMessage::DeleteStationConfirmation(
                                            s.id.clone(),
                                            s.name.clone(),
                                        )
                                    }
                                },
                            )
                        }
                    },
                    cm_open,
                    cm_position,
                    move |position| match position {
                        Some(p) => RadiosMessage::SetOpenMenu(Some(
                            crate::app_message::OpenMenu::Context {
                                id: cm_id_for_msg.clone(),
                                position: p,
                            },
                        )),
                        None => RadiosMessage::SetOpenMenu(None),
                    },
                )
                .into()
            },
        );

        // Wrap in background container
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        // Large artwork panel. Mirrors the Queue view's cover selection: while a
        // radio station is playing the panel LOCKS to that station's art (so it
        // doesn't change as you scroll the list); otherwise it follows the
        // centered station so you can browse the different stations' artwork.
        //
        // ONE "panel station" drives BOTH the displayed handle and the
        // right-click menu target, so they can never diverge: the playing
        // station while its art is cached, else the centered station (whose
        // art — or the tower placeholder — is what actually shows while the
        // playing station's art hasn't loaded yet). Deriving the menu from
        // anything else lets Set/Reset mutate a station whose cover isn't
        // the one on screen.
        let panel_station: Option<&RadioStation> = data
            .playing_station
            .filter(|s| {
                data.radio_large_art.contains_key(&s.id) || data.radio_art.contains_key(&s.id)
            })
            .or_else(|| {
                self.common
                    .get_center_item_index(stations.len())
                    .and_then(|idx| stations.get(idx))
            });
        let panel_handle = panel_station.and_then(|s| {
            data.radio_large_art
                .get(&s.id)
                .or_else(|| data.radio_art.get(&s.id))
        });
        // Over-cover visualizer + surfing boat are UNGATED, matching the Queue
        // view: while audio plays they animate over the cover, and when it pauses
        // no fresh chunk reaches the FFT so they freeze in place rather than
        // vanishing. `None` only for bottom-band placement or `Off`.
        let over_art_visualizer = data.over_art_visualizer;
        let over_art_boat = data.over_art_boat;
        // Render the station art through the SHARED panel — the same helper the
        // Queue now-playing cover uses — so the over-cover visualizer/boat stack
        // on top even when there's no art: `ArtworkPlaceholder::RadioTower` draws
        // the tower glyph as the cover, and the visualizer rides over it (instead
        // of a bespoke art-less panel that dropped the overlay).
        //
        // Panel menu entries act on the SAME resolved panel station whose art
        // is displayed (above), carried in each entry's message so the
        // handler never re-resolves (the viewport may have moved by then).
        let panel_menu_entries: Vec<_> = panel_station
            .map(|s| {
                use crate::widgets::context_menu::PanelMenuEntry;
                let mut entries = vec![PanelMenuEntry::set_custom_artwork(
                    RadiosMessage::SetStationArtwork(s.clone()),
                )];
                if s.logo_cover_art().is_some() {
                    entries.push(PanelMenuEntry::reset_artwork(
                        RadiosMessage::ResetStationArtwork(s.clone()),
                    ));
                }
                entries.push(PanelMenuEntry::refresh_artwork(
                    RadiosMessage::RefreshStationArtwork(s.clone()),
                ));
                entries
            })
            .unwrap_or_default();
        let (menu_open, menu_position, on_menu_change) =
            crate::widgets::context_menu::artwork_panel_open_state(
                crate::View::Radios,
                data.open_menu,
                RadiosMessage::SetOpenMenu,
            );
        let artwork_content = Some(
            crate::widgets::base_slot_list_layout::single_artwork_panel_with_visualizer_and_menu(
                panel_handle,
                over_art_visualizer,
                over_art_boat,
                // Lyrics are Queue-only (ICY stream titles starve the matcher).
                None,
                crate::widgets::base_slot_list_layout::ArtworkPlaceholder::RadioTower,
                panel_menu_entries,
                menu_open,
                menu_position,
                on_menu_change,
            ),
        );

        use crate::widgets::base_slot_list_layout::base_slot_list_layout;
        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }
}

// ============================================================================
// ViewPage trait implementation — for hotkey dispatch
// ============================================================================

impl super::ViewPage for RadiosPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }

    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn search_input_id(&self) -> &'static str {
        crate::views::RADIOS_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(&[SortMode::Name])
    }

    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Radios(RadiosMessage::SlotList(
            crate::widgets::SlotListPageMessage::SortModeSelected(mode),
        )))
    }

    fn toggle_sort_order_message(&self) -> Message {
        Message::Radios(RadiosMessage::SlotList(
            crate::widgets::SlotListPageMessage::ToggleSortOrder,
        ))
    }

    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadRadioStations)
    }

    fn synth_set_offset_message(&self, offset: usize) -> Option<Message> {
        Some(Message::Radios(RadiosMessage::SlotList(
            crate::widgets::SlotListPageMessage::SetOffset(
                offset,
                iced::keyboard::Modifiers::default(),
            ),
        )))
    }

    fn slot_list_message(&self, msg: crate::widgets::SlotListPageMessage) -> Message {
        Message::Radios(RadiosMessage::SlotList(msg))
    }

    /// Radios renders a horizontal artwork column (`show_artwork_column: true`),
    /// so it participates in the artwork-elevation feature like every other
    /// artwork view — without this the elevated top-nav bar spans the full
    /// window and the artwork column stops below the nav instead of extending
    /// to the top of the window. See [`Nokkvi::elevated_artwork_extent`].
    fn uses_horizontal_artwork_column(&self) -> bool {
        true
    }
}
