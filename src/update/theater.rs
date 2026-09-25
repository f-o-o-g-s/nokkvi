//! Theater Mode — the now-playing layout: the slot list, the toolbar and the
//! nav leave the window, which shows only the playing track's cover with its
//! over-cover visualizer and lyrics.
//!
//! `Nokkvi::enter_theater` is the single entry point (F11, the panel menu,
//! the corner icon, the `theater` verb). Entering and leaving are both
//! unmount edges; see the two methods for what each one clears.

use std::collections::HashMap;

use iced::{Task, widget::image::Handle};
use nokkvi_data::types::hotkey_config::HotkeyAction;

use crate::{
    Nokkvi, Screen,
    app_message::{ArtworkMessage, Message, TheaterMessage},
};

/// What a hotkey does while Theater Mode is active. See
/// [`theater_key_policy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TheaterKeyPolicy {
    /// Acts as normal and stays in theater.
    Passthrough,
    /// Leaves theater, then runs the key against the restored layout.
    ExitThenPerform,
    /// Leaves theater and swallows the key.
    ExitOnly,
}

/// Classify a hotkey action for Theater Mode. Exhaustive on purpose: a new
/// action fails to compile until someone decides what it does here.
///
/// The rule: a key that drives playback or the now-playing surfaces stays;
/// a key that navigates, opens something the user will see, or acts on the
/// PLAYING track leaves and then runs; a key that would act on a row of the
/// hidden list, or edit a hidden Settings row, only leaves. Never mutate
/// blind.
pub(crate) fn theater_key_policy(action: HotkeyAction) -> TheaterKeyPolicy {
    use HotkeyAction as A;
    use TheaterKeyPolicy as P;
    match action {
        A::TogglePlay
        | A::ToggleRandom
        | A::ToggleRepeat
        | A::ToggleConsume
        | A::ToggleSoundEffects
        | A::CycleVisualization
        | A::ToggleEqModal
        | A::ToggleCrossfade
        | A::ToggleLyrics
        | A::ToggleBitPerfect
        | A::SeekBackward
        | A::SeekForward
        | A::ToggleTheater
        | A::Escape => P::Passthrough,
        A::SwitchToQueue
        | A::SwitchToAlbums
        | A::SwitchToArtists
        | A::SwitchToSongs
        | A::SwitchToGenres
        | A::SwitchToPlaylists
        | A::SwitchToRadios
        | A::SwitchToHarbour
        | A::SwitchToSettings
        | A::FocusSearch
        | A::ToggleBrowsingPanel
        | A::CenterOnPlaying
        | A::OpenTrawl
        | A::Roulette
        | A::NewSmartPlaylist
        | A::RefreshView
        | A::SaveQueueAsPlaylist
        | A::FindSimilar
        | A::FindTopSongs => P::ExitThenPerform,
        A::SlotListUp
        | A::SlotListDown
        | A::Activate
        | A::ExpandCenter
        | A::ShufflePlay
        | A::AddToQueue
        | A::RemoveFromQueue
        | A::ClearQueue
        | A::ToggleStar
        | A::IncreaseRating
        | A::DecreaseRating
        | A::GetInfo
        | A::MoveTrackUp
        | A::MoveTrackDown
        | A::TrawlSaveAsPlaylist
        | A::EditCenteredPlaylist
        | A::PrevSortMode
        | A::NextSortMode
        | A::ToggleSortOrder
        | A::EditUp
        | A::EditDown
        | A::ResetToDefault
        | A::SettingsCategoryNext
        | A::SettingsCategoryPrev => P::ExitOnly,
    }
}

/// The theater panel's cover: the frosted lyrics backdrop when the caller
/// passes one, else the large art, else the mini, all keyed by `key` (the
/// playing album id, or the playing station id for radio). Pure, so the
/// lookup order is testable without rendering. Callers pass `blurred` only
/// while the lyrics layer shows: the frost is not transport-gated, and a
/// stopped panel would otherwise show a scrim with nothing on it.
pub(crate) fn theater_cover<'a>(
    large: &'a HashMap<String, Handle>,
    mini: &'a HashMap<String, Handle>,
    blurred: Option<&'a Handle>,
    key: Option<&str>,
) -> Option<&'a Handle> {
    blurred.or_else(|| key.and_then(|k| large.get(k).or_else(|| mini.get(k))))
}

impl Nokkvi {
    pub(crate) fn handle_theater(&mut self, msg: TheaterMessage) -> Task<Message> {
        match msg {
            TheaterMessage::Toggle => self.toggle_theater(),
            TheaterMessage::Exit => self.exit_theater(),
        }
    }

    pub(crate) fn toggle_theater(&mut self) -> Task<Message> {
        if self.theater.active {
            self.exit_theater()
        } else {
            self.enter_theater()
        }
    }

    /// The single entry point. No-op off the Home screen or when already
    /// active. Entering unmounts the slot list, its header and the split view,
    /// so every flag their `on_exit` / `on_close` would have cleared is
    /// cleared here; the search query, the split view, the editor and the
    /// current view are left alone for the way back.
    pub(crate) fn enter_theater(&mut self) -> Task<Message> {
        if self.screen != Screen::Home || self.theater.active {
            return Task::none();
        }
        self.theater.active = true;

        self.open_menu = None;
        self.clear_all_toolbar_reveal_locks();
        self.clear_stranded_within_list_drag();
        self.cancel_roulette_restoring_offset();
        let _ = self.handle_cross_pane_drag_cancel();
        for common in self.all_slot_list_commons_mut() {
            // The box unmounts whether or not auto-hide is on, so this cannot
            // go through `clear_all_search_input_focus` (which returns early
            // when auto-hide is off).
            common.search_input_focused = false;
            // A row's hover clears only on `on_exit`, which cannot fire once
            // the row unmounts; a stale one would let a click-drag on the
            // cover arm a cross-pane drag of a hidden row.
            common.slot_list.hovered_slot = None;
        }

        // Theater may be entered from a view that is not a lyrics surface, so
        // the playing track's cover and lyrics may never have been fetched.
        Task::batch([
            self.theater_large_art_task().unwrap_or_else(Task::none),
            self.lyrics_kick_if_unresolved(),
        ])
    }

    /// Leave Theater Mode. Also an unmount edge: the theater panel's menu
    /// closes with it. Never touches the current view, the browsing panel,
    /// the editor or any search query.
    pub(crate) fn exit_theater(&mut self) -> Task<Message> {
        if !self.theater.active {
            return Task::none();
        }
        self.theater.active = false;
        self.open_menu = None;
        Task::none()
    }

    /// What the hidden layout would show: the view, the split view's tab and
    /// whether an editor session is open. The root `update` leaves theater
    /// when a message changes it.
    pub(crate) fn theater_route_snapshot(
        &self,
    ) -> (crate::View, Option<crate::views::BrowsingView>, bool) {
        (
            self.current_view,
            self.browsing_panel.as_ref().map(|p| p.active_view),
            self.playlist_editor.is_some(),
        )
    }

    /// Load the playing album's large cover when the large LRU is cold. The
    /// Queue view warms it through its centered-row prefetch, but theater can
    /// be entered from any view and a song can change while it is active;
    /// without this the panel would upscale the 80 px thumbnail.
    pub(crate) fn theater_large_art_task(&self) -> Option<Task<Message>> {
        let album_id = self.current_queue_song_album_id()?;
        if self.artwork.large_artwork.snapshot.contains_key(album_id) {
            return None;
        }
        Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
            album_id.to_string(),
        ))))
    }

    /// The now-playing cover theater shows: the playing station's art for
    /// radio, else the playing album's (frosted only while the lyrics layer
    /// shows). `None` means the placeholder.
    pub(crate) fn theater_now_playing_cover(&self) -> Option<&Handle> {
        if let Some(station) = self.active_playback.radio_station() {
            return theater_cover(
                &self.artwork.radio_large_art.snapshot,
                &self.artwork.radio_art.snapshot,
                None,
                Some(&station.id),
            );
        }
        let blurred = if self.queue_lyrics_panel_data().is_some() {
            self.lyrics_blurred_cover_for_view()
        } else {
            None
        };
        theater_cover(
            &self.artwork.large_artwork.snapshot,
            &self.artwork.album_art.snapshot,
            blurred,
            self.current_queue_song_album_id(),
        )
    }
}
