//! Radio data loading and component message handlers

use iced::Task;
use tracing::{debug, error, info};

use crate::{
    Nokkvi,
    app_message::Message,
    views::{self, RadiosAction, RadiosMessage},
};

impl Nokkvi {
    pub(crate) fn handle_load_radio_stations(&mut self) -> Task<Message> {
        debug!(" LoadRadioStations message received, loading stations...");

        self.shell_task(
            move |shell| async move {
                let service = match shell.radios_api().await {
                    Ok(s) => s,
                    Err(e) => return (Err(e.to_string()), 0),
                };

                match service.load_radio_stations().await {
                    Ok(stations) => {
                        let total_count = stations.len();
                        (Ok(stations), total_count)
                    }
                    Err(e) => (Err(format!("{e:#}")), 0),
                }
            },
            |(result, _total_count)| {
                // Wrap in RadiosMessage
                Message::Radios(views::RadiosMessage::RadioStationsLoaded(result))
            },
        )
    }

    pub(crate) fn handle_radio_stations_loaded(
        &mut self,
        result: Result<Vec<nokkvi_data::types::radio_station::RadioStation>, String>,
    ) -> Task<Message> {
        match result {
            Ok(new_stations) => {
                info!(" Loaded {} internet radio stations", new_stations.len());
                self.library.radio_stations = new_stations;
                self.sort_radio_stations();
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    return self.handle_session_expired();
                }
                error!("Error loading radio stations: {}", e);
                self.toast_error(format!("Failed to load radio stations: {e}"));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_radios(&mut self, msg: views::RadiosMessage) -> Task<Message> {
        if let RadiosMessage::SetOpenMenu(next) = msg {
            return Task::done(Message::SetOpenMenu(next));
        }
        self.play_view_sfx(
            matches!(
                msg,
                RadiosMessage::SlotListNavigateUp | RadiosMessage::SlotListNavigateDown
            ),
            false,
        );

        // Inline the filter call to decouple borrows.
        // `filtered_stations` borrows `self.library.radio_stations` immutably,
        // which leaves `self.radios_page` free to be borrowed mutably below.
        let filtered_stations = nokkvi_data::utils::search::filter_items(
            &self.library.radio_stations,
            &self.radios_page.common.search_query,
        );

        // Capture data before passing slice
        let (cmd, action) = self.radios_page.update(msg.clone(), &filtered_stations);

        match action {
            RadiosAction::SortModeChanged(_) | RadiosAction::SortOrderChanged(_) => {
                self.sort_radio_stations();
                return Task::none();
            }
            RadiosAction::SearchChanged(_) => {
                self.sort_radio_stations(); // Re-sort and reset offset on search
                return Task::none();
            }
            RadiosAction::FocusOnStation(station_id) => {
                if let Some(idx) = filtered_stations.iter().position(|s| s.id == station_id) {
                    self.radios_page
                        .common
                        .slot_list
                        .set_offset(idx, filtered_stations.len());
                    self.radios_page.common.slot_list.flash_center();
                }
                return Task::none();
            }
            RadiosAction::RefreshViewData => {
                return self.handle_load_radio_stations();
            }
            RadiosAction::AddRadioStation => {
                self.text_input_dialog.open_two_fields(
                    "Add Radio Station",
                    "",
                    "Station Name...",
                    "",
                    "Stream URL...",
                    crate::widgets::text_input_dialog::TextInputDialogAction::CreateRadioStation,
                );
                return Task::none();
            }
            RadiosAction::DeleteStation(id, name) => {
                self.text_input_dialog
                    .open_delete_radio_confirmation(id, name);
                return Task::none();
            }
            RadiosAction::EditRadioStation(station) => {
                self.text_input_dialog.open_two_fields(
                    "Edit Radio Station",
                    station.name,
                    "Station Name...",
                    station.stream_url,
                    "Stream URL...",
                    crate::widgets::text_input_dialog::TextInputDialogAction::EditRadioStation(
                        station.id,
                    ),
                );
                return Task::none();
            }
            RadiosAction::PlayRadioStation(station) => {
                // Wait! This is the core logic.
                // We transition ActivePlayback into RadioPlaybackState.

                self.active_playback =
                    crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
                        station: station.clone(),
                        icy_artist: None,
                        icy_title: None,
                        icy_url: None,
                    });

                let stream_url = station.stream_url.clone();

                return self.shell_action_task(
                    move |shell| async move {
                        shell.playback().stop().await?;
                        let engine_arc = shell.playback().audio_engine();
                        let mut engine = engine_arc.lock().await;
                        engine.set_source(stream_url).await;
                        engine.play().await?;
                        Ok(())
                    },
                    Message::NoOp,
                    "play radio station",
                );
            }
            // For data loading directly from the view
            RadiosAction::None => {}
        }

        // Intercept RadiosLoaded message so we can process it in our state (to keep update() pure)
        if let RadiosMessage::RadioStationsLoaded(result) = msg {
            return self.handle_radio_stations_loaded(result);
        }

        cmd.map(Message::Radios)
    }
}
