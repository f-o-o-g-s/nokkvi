//! Radios Page Component
//!
//! Flat slot list for internet radio stations. No expansion, no artwork, no star/rating.
//! Activating a station transitions to ActivePlayback::Radio and plays the stream URL.

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container},
};
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
    pub modifiers: iced::keyboard::Modifiers,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus can resolve their own open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
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
                    SlotListPageAction::ActivateCenter => {
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
        });

        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListConfig, SlotListSlotStyle, chrome_height_with_header,
            slot_list_text_column, slot_list_view_with_scroll,
        };

        let slot_list_chrome = chrome_height_with_header();

        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: false, // No artwork for radio stations
            slot_list_chrome,
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

        let config = SlotListConfig::with_dynamic_slots(data.window_height, slot_list_chrome)
            .with_modifiers(data.modifiers);

        let stations = data.stations.as_ref();
        let open_menu_for_rows = data.open_menu;

        // Render slot list — flat list, each row is a radio station
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            stations,
            &config,
            RadiosMessage::SlotList(SlotListPageMessage::NavigateUp),
            RadiosMessage::SlotList(SlotListPageMessage::NavigateDown),
            {
                let total = stations.len();
                move |f| {
                    RadiosMessage::SlotList(SlotListPageMessage::ScrollSeek(
                        (f * total as f32) as usize,
                    ))
                }
            },
            |station, ctx| {
                let style = SlotListSlotStyle::for_slot(
                    ctx.is_center,
                    false,
                    ctx.is_selected,
                    ctx.has_multi_selection,
                    ctx.opacity,
                    0,
                );

                let m = ctx.metrics;

                // Radio tower icon — tinted to match slot text color
                let radio_icon = container(
                    crate::embedded_svg::svg_widget(
                        crate::widgets::track_info_strip::RADIO_TOWER_ICON_PATH,
                    )
                    .width(Length::Fixed(m.title_size))
                    .height(Length::Fixed(m.title_size))
                    .style(move |_, _| iced::widget::svg::Style {
                        color: Some(style.text_color),
                    }),
                )
                .align_y(Alignment::Center)
                .align_x(Alignment::Center);

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
                    .push(radio_icon)
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

                let slot_button = button(clickable)
                    .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                        RadiosMessage::SlotList(SlotListPageMessage::SetOffset(
                            ctx.item_index,
                            ctx.modifiers,
                        ))
                    } else if ctx.is_center {
                        RadiosMessage::SlotList(SlotListPageMessage::ActivateCenter)
                    } else if data.stable_viewport {
                        RadiosMessage::SlotList(SlotListPageMessage::SetOffset(
                            ctx.item_index,
                            ctx.modifiers,
                        ))
                    } else {
                        RadiosMessage::SlotList(SlotListPageMessage::ClickPlay(ctx.item_index))
                    })
                    .style(|_theme, _status| button::Style {
                        background: None,
                        border: iced::Border::default(),
                        ..Default::default()
                    })
                    .padding(0)
                    .width(Length::Fill);

                let cm_id = crate::app_message::ContextMenuId::RadioRow(ctx.item_index);
                let (cm_open, cm_position) =
                    crate::widgets::context_menu::open_state_for(open_menu_for_rows, &cm_id);
                let cm_id_for_msg = cm_id.clone();
                crate::widgets::context_menu::context_menu(
                    slot_button,
                    crate::widgets::context_menu::radio_entries(),
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

        use crate::widgets::base_slot_list_layout::base_slot_list_layout;
        base_slot_list_layout(&layout_config, header, slot_list_content, None)
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
}
