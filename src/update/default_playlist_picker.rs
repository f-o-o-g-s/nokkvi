//! Default-playlist picker handler — open from chip click, search-filter,
//! select playlist (or clear default), close on Escape/backdrop/X.
//!
//! State lives on `Nokkvi.default_playlist_picker`. The actual default-playlist
//! write reuses the path established by the right-click context menu
//! (`shell.settings().set_default_playlist`).

use iced::Task;
use tracing::info;

use crate::{
    Nokkvi,
    app_message::Message,
    widgets::default_playlist_picker::{
        DefaultPlaylistPickerMessage, DefaultPlaylistPickerState, PICKER_SEARCH_INPUT_ID,
        PickerEntry,
    },
};

impl Nokkvi {
    /// Dispatch a default-playlist picker message.
    pub(crate) fn handle_default_playlist_picker(
        &mut self,
        msg: DefaultPlaylistPickerMessage,
    ) -> Task<Message> {
        match msg {
            DefaultPlaylistPickerMessage::Open => self.open_default_playlist_picker(),
            DefaultPlaylistPickerMessage::Close => {
                self.default_playlist_picker = None;
                Task::none()
            }
            DefaultPlaylistPickerMessage::SearchChanged(query) => {
                if let Some(state) = self.default_playlist_picker.as_mut() {
                    state.search_query = query;
                    state.refilter();
                }
                Task::none()
            }
            DefaultPlaylistPickerMessage::SlotListUp => {
                if let Some(state) = self.default_playlist_picker.as_mut() {
                    let total = state.filtered.len();
                    state.slot_list.move_up(total);
                }
                Task::none()
            }
            DefaultPlaylistPickerMessage::SlotListDown => {
                if let Some(state) = self.default_playlist_picker.as_mut() {
                    let total = state.filtered.len();
                    state.slot_list.move_down(total);
                }
                Task::none()
            }
            DefaultPlaylistPickerMessage::SlotListSetOffset(offset, _modifiers) => {
                if let Some(state) = self.default_playlist_picker.as_mut() {
                    let total = state.filtered.len();
                    state.slot_list.set_offset(offset, total);
                }
                Task::none()
            }
            DefaultPlaylistPickerMessage::ClickItem(index) => self.select_picker_index(index),
            DefaultPlaylistPickerMessage::ActivateCenter => {
                let center_index = self
                    .default_playlist_picker
                    .as_ref()
                    .and_then(|s| s.slot_list.get_center_item_index(s.filtered.len()));
                if let Some(idx) = center_index {
                    self.select_picker_index(idx)
                } else {
                    Task::none()
                }
            }
        }
    }

    fn open_default_playlist_picker(&mut self) -> Task<Message> {
        self.default_playlist_picker =
            Some(DefaultPlaylistPickerState::new(&self.library.playlists));

        let mut tasks = vec![iced::widget::operation::focus(PICKER_SEARCH_INPUT_ID)];

        // Playlists are loaded lazily on first navigation to the Playlists
        // view. If the user opens the picker from elsewhere (e.g. the queue
        // header chip) before that's happened, kick off a load so the picker
        // populates. `handle_playlists_loaded` rebuilds picker state when it
        // arrives. Skip the fetch when a load is already in flight.
        if self.library.playlists.is_empty() && !self.library.playlists.is_loading() {
            tasks.push(self.handle_load_playlists());
        }

        Task::batch(tasks)
    }

    /// Rebuild the picker's entry list from the current library playlists,
    /// preserving the user's search query and scroll position.
    ///
    /// Called from `handle_playlists_loaded` so a picker opened before the
    /// library was loaded refreshes seamlessly when the data arrives.
    pub(crate) fn refresh_default_playlist_picker_after_load(&mut self) {
        let Some(picker) = self.default_playlist_picker.as_mut() else {
            return;
        };
        let saved_query = std::mem::take(&mut picker.search_query);
        let saved_offset = picker.slot_list.viewport_offset;

        let mut rebuilt = DefaultPlaylistPickerState::new(&self.library.playlists);
        rebuilt.search_query = saved_query;
        rebuilt.refilter();
        let total = rebuilt.filtered.len();
        rebuilt.slot_list.set_offset(saved_offset.min(total), total);

        self.default_playlist_picker = Some(rebuilt);
    }

    fn select_picker_index(&mut self, index: usize) -> Task<Message> {
        let entry = self
            .default_playlist_picker
            .as_ref()
            .and_then(|s| s.filtered.get(index).cloned());
        let Some(entry) = entry else {
            return Task::none();
        };
        self.default_playlist_picker = None;

        match entry {
            PickerEntry::Clear => {
                info!(" Clearing default playlist");
                self.default_playlist_id = None;
                self.default_playlist_name.clear();
                self.settings_page.config_dirty = true;
                self.toast_info("Default playlist cleared");
                self.shell_spawn("persist_default_playlist", |shell| async move {
                    shell
                        .settings()
                        .set_default_playlist(None, String::new())
                        .await
                });
            }
            PickerEntry::Playlist { id, name, .. } => {
                info!(" Setting default playlist: '{}' ({})", name, id);
                self.default_playlist_id = Some(id.clone());
                self.default_playlist_name = name.clone();
                self.settings_page.config_dirty = true;
                self.toast_success(format!("Default playlist set to '{name}'"));
                self.shell_spawn("persist_default_playlist", move |shell| async move {
                    shell.settings().set_default_playlist(Some(id), name).await
                });
            }
        }
        Task::none()
    }
}
