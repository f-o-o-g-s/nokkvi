//! Theater Mode — the now-playing layout: the slot list, the toolbar and the
//! nav leave the window, which shows only the playing track's cover with its
//! over-cover visualizer and lyrics.
//!
//! `Nokkvi::enter_theater` is the single entry point: every way in (today
//! the F11 hotkey) calls it. Entering and leaving are both unmount edges; see
//! the two methods for what each one clears.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use iced::{Task, widget::image::Handle};
use nokkvi_data::types::{hotkey_config::HotkeyAction, player_settings::TheaterControls};

use crate::{
    Nokkvi, Screen,
    app_message::{ArtworkMessage, Message, TheaterMessage},
    state::{ChromeMotion, TheaterState},
};

/// Idle time before the auto-hiding bar (and the cursor) leave; the same
/// hold as the auto-hide toolbar's hotkey reveal.
pub(crate) const HIDE_DELAY: Duration = Duration::from_millis(2500);
/// How long the bar takes to slide fully in or out.
pub(crate) const SLIDE_DURATION: Duration = Duration::from_millis(220);

/// Where the chrome should be right now. `AutoHide` shows it while the
/// window has focus and the cursor is on the bar, an overlay menu is open, or
/// there was activity within [`HIDE_DELAY`].
pub(crate) fn chrome_target(
    state: &TheaterState,
    now: Instant,
    menu_open: bool,
    setting: TheaterControls,
) -> bool {
    match setting {
        TheaterControls::AlwaysShown => true,
        TheaterControls::AlwaysHidden => false,
        TheaterControls::AutoHide => {
            state.window_focused && (state.bar_hovered || menu_open || activity_recent(state, now))
        }
    }
}

/// Activity within [`HIDE_DELAY`] of `now`.
fn activity_recent(state: &TheaterState, now: Instant) -> bool {
    state
        .last_activity
        .is_some_and(|at| now.saturating_duration_since(at) < HIDE_DELAY)
}

/// The chrome's slide offset at `now`: 0 = fully shown, 1 = fully hidden,
/// easing from the flip's recorded `from`.
pub(crate) fn slide_offset(chrome: ChromeMotion, now: Instant) -> f32 {
    let (since, from, target) = match chrome {
        ChromeMotion::Shown { since, from } => (since, from, 0.0),
        ChromeMotion::Hidden { since, from } => (since, from, 1.0),
    };
    let t = now.saturating_duration_since(since).as_secs_f32() / SLIDE_DURATION.as_secs_f32();
    let eased = crate::widgets::lyrics_viewport::ease_out_expo(t.clamp(0.0, 1.0));
    (from + (target - from) * eased).clamp(0.0, 1.0)
}

/// Whether the cursor hides over the theater panel. Its own rule,
/// independent of focus and of the controls setting: with the bar always
/// hidden the user must still be able to aim a right-click, and a pointer
/// moved over an unfocused window must show.
pub(crate) fn cursor_hidden(state: &TheaterState, now: Instant, menu_open: bool) -> bool {
    state.active && !menu_open && !activity_recent(state, now)
}

/// Per-frame step (from the boat tick): flip the chrome when its target
/// changed, recording the offset it leaves from. The only place the chrome
/// flips.
pub(crate) fn tick(app: &mut Nokkvi, now: Instant) {
    if !app.theater.active {
        return;
    }
    let target = chrome_target(
        &app.theater,
        now,
        app.open_menu.is_some(),
        app.theater_controls(),
    );
    let current = app.theater.chrome;
    if target != current.is_shown() {
        let from = slide_offset(current, now);
        app.theater.chrome = if target {
            ChromeMotion::Shown { since: now, from }
        } else {
            ChromeMotion::Hidden { since: now, from }
        };
    }
}

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

/// Clear what a slot-list page's `on_exit` / blur would have cleared, for a
/// page whose widgets are about to unmount under Theater Mode.
fn clear_unmounting_list_state(common: &mut crate::widgets::SlotListPageState) {
    common.reset_reveal_locks();
    // The box unmounts whether or not auto-hide is on, so this cannot go
    // through `clear_all_search_input_focus` (which returns early when
    // auto-hide is off).
    common.search_input_focused = false;
    // A row's hover clears only on `on_exit`, which cannot fire once the row
    // unmounts; a stale one would let a click-drag on the cover arm a
    // cross-pane drag of a hidden row, or aim a drop from the browser at a
    // hidden editor row.
    common.slot_list.hovered_slot = None;
}

impl Nokkvi {
    pub(crate) fn handle_theater(&mut self, msg: TheaterMessage) -> Task<Message> {
        match msg {
            TheaterMessage::Toggle => self.toggle_theater(),
            TheaterMessage::Exit => self.exit_theater(),
            TheaterMessage::Activity => {
                self.stamp_theater_activity();
                Task::none()
            }
            TheaterMessage::BarHover(hovered) => {
                // Only while active: a stray exit from an unmounting bar must
                // not re-arm a flag the exit edge just cleared.
                if self.theater.active {
                    self.theater.bar_hovered = hovered;
                }
                Task::none()
            }
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
        let now = Instant::now();
        self.theater.last_activity = Some(now);
        self.theater.bar_hovered = false;
        self.theater.chrome = ChromeMotion::Shown {
            since: now,
            from: 0.0,
        };

        self.open_menu = None;
        self.clear_stranded_within_list_drag();
        self.cancel_roulette_restoring_offset();
        let _ = self.handle_cross_pane_drag_cancel();
        // A find-and-expand chain would land its expansion on a hidden row.
        self.cancel_pending_expand();
        for common in self.all_slot_list_commons_mut() {
            clear_unmounting_list_state(common);
        }
        if let Some(editor) = self.playlist_editor.as_mut() {
            clear_unmounting_list_state(&mut editor.common);
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
        // The bar unmounts, so its `on_exit` may never fire.
        self.theater.bar_hovered = false;
        self.open_menu = None;
        Task::none()
    }

    /// Keep the bar and the cursor on screen for another [`HIDE_DELAY`].
    pub(crate) fn stamp_theater_activity(&mut self) {
        if self.theater.active {
            self.theater.last_activity = Some(Instant::now());
        }
    }

    /// The Theater Controls setting. Always `AutoHide` until the setting
    /// ships.
    pub(crate) fn theater_controls(&self) -> TheaterControls {
        TheaterControls::AutoHide
    }

    /// [`theater_key_policy`] refined by the hidden state: a toggle whose
    /// target is already on behind theater (Settings' key over a hidden
    /// Settings view, the split-view key with the split view open) only
    /// leaves, which reveals that target instead of toggling it away unseen.
    pub(crate) fn theater_key_policy_here(&self, action: HotkeyAction) -> TheaterKeyPolicy {
        let already_on = match action {
            HotkeyAction::SwitchToSettings => self.current_view == crate::View::Settings,
            HotkeyAction::ToggleBrowsingPanel => self.browsing_panel.is_some(),
            _ => false,
        };
        match theater_key_policy(action) {
            TheaterKeyPolicy::ExitThenPerform if already_on => TheaterKeyPolicy::ExitOnly,
            policy => policy,
        }
    }

    /// A held theater toggle key: winit repeats it, and each repeat would
    /// swap the whole layout. Other held keys (seek) keep repeating.
    pub(crate) fn is_repeated_theater_toggle(
        &self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
        repeat: bool,
    ) -> bool {
        repeat
            && crate::hotkeys::resolve_action(key, modifiers, &self.hotkey_config)
                == Some(HotkeyAction::ToggleTheater)
    }

    /// Close the overlay menus anchored to the bar. The bar slides away while
    /// the window is unfocused and would leave them hanging at the edge; the
    /// panel's own menu stays because the panel does not move.
    pub(crate) fn close_theater_bar_menus(&mut self) {
        use crate::app_message::{ContextMenuId, OpenMenu};
        if self.theater.active
            && matches!(
                self.open_menu,
                Some(
                    OpenMenu::Hamburger
                        | OpenMenu::PlayerModes
                        | OpenMenu::Context {
                            id: ContextMenuId::Strip,
                            ..
                        }
                )
            )
        {
            self.open_menu = None;
        }
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

    /// Whether Theater Mode's panel draws the black backdrop instead of the
    /// cover (the Cover Art setting).
    pub(crate) fn theater_cover_hidden(&self) -> bool {
        crate::theme::artwork_cover().hides_cover(true)
    }

    /// The lyrics layer for theater's panel: the Queue's, sized to the panel.
    pub(crate) fn theater_lyrics_panel_data(
        &self,
    ) -> Option<crate::widgets::lyrics_viewport::LyricsPanelData<'_>> {
        self.queue_lyrics_panel_data().map(|data| {
            crate::widgets::lyrics_viewport::LyricsPanelData {
                fit_to_panel: true,
                ..data
            }
        })
    }

    /// The now-playing cover theater shows: the playing station's art for
    /// radio, else the playing album's (frosted only while the lyrics layer
    /// shows). `None` means the placeholder.
    pub(crate) fn theater_now_playing_cover(&self) -> Option<&Handle> {
        if self.theater_cover_hidden() {
            return None;
        }
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
