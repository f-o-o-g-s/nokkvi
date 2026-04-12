//! Radios Page Component
//!
//! Flat slot list for internet radio stations. No expansion, no artwork, no star/rating.
//! Activating a station transitions to ActivePlayback::Radio and plays the stream URL.
//!
//! NOTE from Claude: Scaffolded ahead of Gemini for Phase 6.
//! Gemini — the update() logic and routing are complete. The view() has a
//! functional placeholder renderer; polish the row rendering when ready.

use iced::{Element, Length, Task};
use nokkvi_data::types::radio_station::RadioStation;

use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, slot_list_page::SlotListPageAction, view_header::SortMode},
};

// ============================================================================
// State
// ============================================================================

/// Radios page local state — minimal, no expansion
#[derive(Debug)]
pub struct RadiosPage {
    pub common: SlotListPageState,
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
}

// ============================================================================
// Messages & Actions
// ============================================================================

/// Messages for local radio page interactions
#[derive(Debug, Clone)]
pub enum RadiosMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter, // Enter → play radio station
    SlotListClickPlay(usize),
    FocusCurrentPlaying(String), // Auto-scroll slot list to center currently playing station (station_id)

    // View header
    SortModeSelected(SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,

    // Data loading
    RadioStationsLoaded(Result<Vec<RadioStation>, String>),

    // Context Menu Actions
    EditStationDialog(RadioStation),
    DeleteStationConfirmation(String, String),
    CopyStreamUrl(String),

    AddRadioStation,
    NoOp,
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
    None,
}

impl super::HasCommonAction for RadiosAction {
    fn as_common(&self) -> super::CommonViewAction {
        match self {
            Self::SearchChanged(_) => super::CommonViewAction::SearchChanged,
            Self::SortModeChanged(m) => super::CommonViewAction::SortModeChanged(*m),
            Self::SortOrderChanged(a) => super::CommonViewAction::SortOrderChanged(*a),
            Self::RefreshViewData => super::CommonViewAction::RefreshViewData,
            Self::None => super::CommonViewAction::None,
            _ => super::CommonViewAction::ViewSpecific,
        }
    }
}

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
            // Slot list navigation — flat list, no expansion
            // -----------------------------------------------------------------
            RadiosMessage::SlotListNavigateUp => {
                self.common.handle_navigate_up(total);
                (Task::none(), RadiosAction::None)
            }
            RadiosMessage::SlotListNavigateDown => {
                self.common.handle_navigate_down(total);
                (Task::none(), RadiosAction::None)
            }
            RadiosMessage::SlotListSetOffset(offset, modifiers) => {
                self.common.handle_slot_click(offset, total, modifiers);
                (Task::none(), RadiosAction::None)
            }
            RadiosMessage::SlotListScrollSeek(offset) => {
                self.common.handle_set_offset(offset, total);
                (Task::none(), RadiosAction::None)
            }
            RadiosMessage::SlotListClickPlay(offset) => {
                self.common.handle_set_offset(offset, total);
                self.update(RadiosMessage::SlotListActivateCenter, stations)
            }
            RadiosMessage::FocusCurrentPlaying(station_id) => {
                (Task::none(), RadiosAction::FocusOnStation(station_id))
            }
            RadiosMessage::SlotListActivateCenter => {
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

            // -----------------------------------------------------------------
            // View header — sort/search/refresh
            // Uses SlotListPageAction enum, not Option
            // -----------------------------------------------------------------
            RadiosMessage::SortModeSelected(mode) => {
                match self.common.handle_sort_mode_selected(mode) {
                    SlotListPageAction::SortModeChanged(m) => {
                        (Task::none(), RadiosAction::SortModeChanged(m))
                    }
                    _ => (Task::none(), RadiosAction::None),
                }
            }
            RadiosMessage::ToggleSortOrder => match self.common.handle_toggle_sort_order() {
                SlotListPageAction::SortOrderChanged(a) => {
                    (Task::none(), RadiosAction::SortOrderChanged(a))
                }
                _ => (Task::none(), RadiosAction::None),
            },
            RadiosMessage::SearchQueryChanged(query) => {
                match self.common.handle_search_query_changed(query, total) {
                    SlotListPageAction::SearchChanged(q) => {
                        (Task::none(), RadiosAction::SearchChanged(q))
                    }
                    _ => (Task::none(), RadiosAction::None),
                }
            }
            RadiosMessage::SearchFocused(focused) => {
                self.common.handle_search_focused(focused);
                (Task::none(), RadiosAction::None)
            }
            RadiosMessage::RefreshViewData => (Task::none(), RadiosAction::RefreshViewData),

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
        }
    }

    // ========================================================================
    // View
    // ========================================================================

    pub fn view<'a>(&'a self, data: RadiosViewData<'a>) -> Element<'a, RadiosMessage> {
        // Gemini: Only Name sort mode makes sense for radio stations.
        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            &[SortMode::Name],
            self.common.sort_ascending,
            &self.common.search_query,
            data.stations.len(),
            data.total_station_count,
            "stations",
            crate::views::RADIOS_SEARCH_ID,
            RadiosMessage::SortModeSelected,
            Some(RadiosMessage::ToggleSortOrder),
            None, // No shuffle button
            Some(RadiosMessage::RefreshViewData),
            None, // No "center on playing" — radio has no queue position
            Some(("Add Station", RadiosMessage::AddRadioStation)), // on_add
            true, // show_search
            RadiosMessage::SearchQueryChanged,
        );

        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: false, // No artwork for radio stations
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

        // Configure slot list
        use crate::widgets::slot_list::{
            SlotListConfig, SlotListSlotStyle, chrome_height_with_header,
            slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(Default::default());

        let stations = data.stations.as_ref();

        // Render slot list — flat list, each row is a radio station
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            stations,
            &config,
            RadiosMessage::SlotListNavigateUp,
            RadiosMessage::SlotListNavigateDown,
            {
                let total = stations.len();
                move |f| RadiosMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |station, ctx| {
                // Gemini: This is a minimal functional renderer. Polish as desired.
                // Pattern follows render_genre_row from genres.rs.
                use iced::widget::{container, row};

                let style = SlotListSlotStyle::for_slot(
                    ctx.is_center,
                    false, // no highlight state for radio
                    ctx.is_selected,
                    ctx.has_multi_selection,
                    ctx.opacity,
                    0, // depth 0: flat list
                );

                // 📻 icon for visual flair
                let radio_icon = iced::widget::container(
                    crate::embedded_svg::svg_widget("assets/icons/radio-tower.svg")
                        .width(iced::Length::Fixed(ctx.metrics.title_size))
                        .height(iced::Length::Fixed(ctx.metrics.title_size))
                        .style(move |_, _| iced::widget::svg::Style {
                            color: Some(style.text_color),
                        }),
                )
                .align_y(iced::Alignment::Center)
                .align_x(iced::Alignment::Center);

                // Using slot_list_text_column for consistent typography and truncation
                use crate::widgets::slot_list::slot_list_text_column;
                let text_col = slot_list_text_column::<RadiosMessage>(
                    station.name.clone(),
                    None,
                    station.stream_url.clone(),
                    None,
                    ctx.metrics.title_size,
                    ctx.metrics.subtitle_size,
                    style,
                    ctx.is_center, // Bold if center
                    100,           // 100% since there are no other columns
                );

                let row_content = row![radio_icon, text_col]
                    .spacing(12)
                    .align_y(iced::Alignment::Center)
                    .height(Length::Fill);

                let slot = container(row_content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding([
                        crate::widgets::slot_list::SLOT_LIST_SLOT_PADDING, // top/bottom 8.0
                        12.0,                                              // left/right 12.0
                    ])
                    .style(move |_| style.to_container_style());

                // Click handler: center slot → activate (play), other → focus
                let click_msg = if ctx.is_center {
                    RadiosMessage::SlotListActivateCenter
                } else if data.stable_viewport {
                    RadiosMessage::SlotListSetOffset(ctx.item_index, data.modifiers)
                } else {
                    RadiosMessage::SlotListClickPlay(ctx.item_index)
                };

                let slot_button: Element<'a, RadiosMessage> =
                    iced::widget::mouse_area(slot).on_press(click_msg).into();

                crate::widgets::context_menu::context_menu(
                    slot_button,
                    crate::widgets::context_menu::radio_entries(),
                    {
                        let station_cloned = station.clone();
                        move |entry, length| {
                             let s = station_cloned.clone();
                             crate::widgets::context_menu::radio_entry_view(entry, length, move |a| match a {
                                 crate::widgets::context_menu::RadioContextEntry::Edit => {
                                     RadiosMessage::EditStationDialog(s.clone())
                                 }
                                 crate::widgets::context_menu::RadioContextEntry::CopyStreamUrl => {
                                     RadiosMessage::CopyStreamUrl(s.stream_url.clone())
                                 }
                                 crate::widgets::context_menu::RadioContextEntry::Delete => {
                                     RadiosMessage::DeleteStationConfirmation(s.id.clone(), s.name.clone())
                                 }
                             })
                        }
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
        Some(Message::Radios(RadiosMessage::SortModeSelected(mode)))
    }

    fn toggle_sort_order_message(&self) -> Message {
        Message::Radios(RadiosMessage::ToggleSortOrder)
    }

    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadRadioStations)
    }
}
