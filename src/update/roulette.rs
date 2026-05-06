//! Roulette (slot-machine random pick) handler.
//!
//! Drives a fixed-viewport "wheel spin" through any slot-list view, lands on
//! a pre-rolled random index, and dispatches the view's normal play action.
//! Animation is fully time-derived from `RouletteState.position_at(now)` —
//! tick handlers are pure bookkeeping (advance offset, fire Tab SFX, detect
//! settle).
//!
//! Trigger: the "Roulette" entry appended to each view's sort dropdown emits
//! `Message::Roulette(RouletteMessage::Start(view))`. Cancel paths (Escape,
//! view switch) emit `Cancel`.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use iced::Task;
use nokkvi_data::audio::SfxType;
use tracing::{debug, trace};

use crate::{
    Nokkvi, View,
    app_message::{Message, RouletteMessage},
    state::RouletteState,
    views,
};

/// Below this item count the spin animation is too short to feel like a
/// roulette — just dispatch the play immediately.
const MIN_ITEMS_FOR_SPIN: usize = 3;

impl Nokkvi {
    pub(crate) fn handle_roulette_message(&mut self, msg: RouletteMessage) -> Task<Message> {
        match msg {
            RouletteMessage::Start(view) => self.handle_roulette_start(view),
            RouletteMessage::Tick(now) => self.handle_roulette_tick(now),
            RouletteMessage::Cancel => self.handle_roulette_cancel(),
        }
    }

    fn handle_roulette_start(&mut self, view: View) -> Task<Message> {
        // Reentrancy guard — second click while spinning is a no-op.
        if self.roulette.is_some() {
            return Task::none();
        }

        // Block plays during playlist edit mode the same way every other
        // play handler does.
        if let Some(task) = self.guard_play_action() {
            return task;
        }

        // Collapse any active inline expansion so the wheel spins through
        // a uniform parents-only list, never a mixed flattened set.
        match view {
            View::Albums => self.albums_page.expansion.clear(),
            View::Artists => self.artists_page.expansion.clear(),
            View::Genres => self.genres_page.expansion.clear(),
            View::Playlists => self.playlists_page.expansion.clear(),
            View::Queue | View::Songs | View::Radios | View::Settings => {}
        }

        // Clear any prior click-driven selection so the centered row gets
        // its center highlight as the spin advances. `build_slot_list_slots`
        // suppresses the center fallback when `selected_indices` is
        // non-empty, which would otherwise leave the highlight pinned to
        // whatever the user clicked before opening the dropdown.
        if let Some(page) = self.current_view_page_mut() {
            page.common_mut().clear_multi_selection();
        }

        let total_items = self.roulette_view_total(view);
        if total_items == 0 {
            return Task::none();
        }

        // Snapshot original offset before mutating it during the spin.
        let original_offset = self.roulette_view_viewport_offset(view).unwrap_or(0);

        let target_idx = pick_random_index(total_items);

        // Skip animation entirely for tiny lists — a 3-tick spin feels
        // anticlimactic and the user can't really tell anyway.
        if total_items < MIN_ITEMS_FOR_SPIN {
            debug!(
                "Roulette: {:?} has only {} items; settling immediately on idx {}",
                view, total_items, target_idx
            );
            return self.roulette_settle_play(view, target_idx, total_items);
        }

        let revolutions = revolutions_for(total_items);
        let forward_diff =
            (target_idx + total_items - (original_offset % total_items)) % total_items;
        let total_steps = revolutions * total_items + forward_diff;

        debug!(
            "Roulette start: view={:?} total={} target={} original_offset={} \
             revolutions={} steps={}",
            view, total_items, target_idx, original_offset, revolutions, total_steps
        );

        self.roulette = Some(RouletteState {
            view,
            total_items,
            original_offset,
            target_idx,
            total_steps,
            start_time: Instant::now(),
            last_offset: original_offset,
            last_sfx_at: None,
        });

        Task::none()
    }

    fn handle_roulette_tick(&mut self, now: Instant) -> Task<Message> {
        // Borrow split: we read state, may fire SFX (immutable on engine),
        // then update the slot list and the roulette bookkeeping.
        let Some(state) = self.roulette.as_ref() else {
            return Task::none();
        };
        let view = state.view;
        let total_items = state.total_items;
        let target_idx = state.target_idx;
        let last_offset = state.last_offset;
        let last_sfx_at = state.last_sfx_at;

        let (offset, settled) = state.position_at(now);

        if offset != last_offset {
            self.roulette_apply_offset(view, offset, total_items);

            let should_play_sfx = last_sfx_at.is_none_or(|t| {
                now.saturating_duration_since(t).as_millis() as u64
                    >= RouletteState::SFX_MIN_INTERVAL_MS
            });
            if should_play_sfx {
                self.sfx_engine.play(SfxType::Tab);
                if let Some(s) = self.roulette.as_mut() {
                    s.last_sfx_at = Some(now);
                }
            }
            if let Some(s) = self.roulette.as_mut() {
                s.last_offset = offset;
            }
        }

        if settled {
            trace!("Roulette settle on view={:?} idx={}", view, target_idx);
            self.sfx_engine.play(SfxType::Enter);
            self.roulette = None;
            return self.roulette_settle_play(view, target_idx, total_items);
        }

        Task::none()
    }

    fn handle_roulette_cancel(&mut self) -> Task<Message> {
        let Some(state) = self.roulette.take() else {
            return Task::none();
        };
        debug!(
            "Roulette cancelled on view={:?} (restoring offset {})",
            state.view, state.original_offset
        );
        self.sfx_engine.play(SfxType::Escape);
        self.roulette_apply_offset(state.view, state.original_offset, state.total_items);
        Task::none()
    }

    /// Total items currently displayed in `view` (matches what the user sees
    /// in the slot list). Used to size the spin and bound the random pick.
    fn roulette_view_total(&self, view: View) -> usize {
        match view {
            View::Queue => self.filter_queue_songs().len(),
            View::Songs => self.library.songs.len(),
            View::Albums => self.library.albums.len(),
            View::Artists => self.library.artists.len(),
            View::Genres => self.library.genres.len(),
            View::Playlists => self.library.playlists.len(),
            View::Radios => self.library.radio_stations.len(),
            View::Settings => 0,
        }
    }

    fn roulette_view_viewport_offset(&self, view: View) -> Option<usize> {
        match view {
            View::Queue => Some(self.queue_page.common.slot_list.viewport_offset),
            View::Songs => Some(self.songs_page.common.slot_list.viewport_offset),
            View::Albums => Some(self.albums_page.common.slot_list.viewport_offset),
            View::Artists => Some(self.artists_page.common.slot_list.viewport_offset),
            View::Genres => Some(self.genres_page.common.slot_list.viewport_offset),
            View::Playlists => Some(self.playlists_page.common.slot_list.viewport_offset),
            View::Radios => Some(self.radios_page.common.slot_list.viewport_offset),
            View::Settings => None,
        }
    }

    /// Move the slot list viewport on `view` to `offset`. Uses
    /// `handle_set_offset` (clears `selected_offset`, records scroll) so the
    /// transient scrollbar fades correctly while the wheel spins.
    pub(crate) fn roulette_apply_offset(&mut self, view: View, offset: usize, total_items: usize) {
        match view {
            View::Queue => {
                self.queue_page
                    .common
                    .handle_set_offset(offset, total_items);
            }
            View::Songs => {
                self.songs_page
                    .common
                    .handle_set_offset(offset, total_items);
            }
            View::Albums => {
                self.albums_page
                    .common
                    .handle_set_offset(offset, total_items);
            }
            View::Artists => {
                self.artists_page
                    .common
                    .handle_set_offset(offset, total_items);
            }
            View::Genres => {
                self.genres_page
                    .common
                    .handle_set_offset(offset, total_items);
            }
            View::Playlists => {
                self.playlists_page
                    .common
                    .handle_set_offset(offset, total_items);
            }
            View::Radios => {
                self.radios_page
                    .common
                    .handle_set_offset(offset, total_items);
            }
            View::Settings => {}
        }
    }

    /// Dispatch the per-view play action for the picked index. For Genres
    /// and Artists the user wants "load all songs, play a random one" —
    /// calls dedicated AppService methods. Other views just route through
    /// the page's normal `SlotListActivateCenter` play path.
    fn roulette_settle_play(
        &mut self,
        view: View,
        target_idx: usize,
        total_items: usize,
    ) -> Task<Message> {
        match view {
            View::Queue => {
                self.queue_page
                    .common
                    .handle_set_offset(target_idx, total_items);
                self.queue_page.common.slot_list.flash_center();
                Task::done(Message::Queue(views::QueueMessage::SlotListActivateCenter))
            }
            View::Songs => {
                self.songs_page
                    .common
                    .handle_set_offset(target_idx, total_items);
                self.songs_page.common.slot_list.flash_center();
                Task::done(Message::Songs(views::SongsMessage::SlotListActivateCenter))
            }
            View::Albums => {
                self.albums_page
                    .common
                    .handle_set_offset(target_idx, total_items);
                self.albums_page.common.slot_list.flash_center();
                Task::done(Message::Albums(
                    views::AlbumsMessage::SlotListActivateCenter,
                ))
            }
            View::Playlists => {
                self.playlists_page
                    .common
                    .handle_set_offset(target_idx, total_items);
                self.playlists_page.common.slot_list.flash_center();
                Task::done(Message::Playlists(
                    views::PlaylistsMessage::SlotListActivateCenter,
                ))
            }
            View::Radios => {
                self.radios_page
                    .common
                    .handle_set_offset(target_idx, total_items);
                self.radios_page.common.slot_list.flash_center();
                Task::done(Message::Radios(
                    views::RadiosMessage::SlotListActivateCenter,
                ))
            }
            View::Genres => {
                let Some(genre) = self.library.genres.get(target_idx) else {
                    return Task::none();
                };
                let name = genre.name.clone();
                self.clear_active_playlist();
                self.shell_action_task(
                    move |shell| async move { shell.play_genre_random(&name).await },
                    Message::SwitchView(View::Queue),
                    "play random song from genre",
                )
            }
            View::Artists => {
                let Some(artist) = self.library.artists.get(target_idx) else {
                    return Task::none();
                };
                let id = artist.id.clone();
                self.clear_active_playlist();
                self.shell_action_task(
                    move |shell| async move { shell.play_artist_random(&id).await },
                    Message::SwitchView(View::Queue),
                    "play random song from artist",
                )
            }
            View::Settings => Task::none(),
        }
    }
}

/// Pick a roughly-uniform random index in `0..total` without pulling in a
/// dedicated rand crate on the UI side. Subsec-nanosecond entropy is plenty
/// for "spin lands on something the user didn't predict" — visual roulette,
/// not security.
fn pick_random_index(total: usize) -> usize {
    if total <= 1 {
        return 0;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() as usize);
    nanos % total
}

/// How many full revolutions the wheel makes before settling. Smaller lists
/// need more revolutions to feel like a real spin (otherwise the wheel
/// barely moves).
fn revolutions_for(total_items: usize) -> usize {
    if total_items < 10 {
        6
    } else if total_items < 50 {
        4
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn state_at_origin(total_items: usize, original: usize, target: usize) -> RouletteState {
        let revs = revolutions_for(total_items);
        let forward_diff = (target + total_items - (original % total_items)) % total_items;
        RouletteState {
            view: View::Albums,
            total_items,
            original_offset: original,
            target_idx: target,
            total_steps: revs * total_items + forward_diff,
            start_time: Instant::now(),
            last_offset: original,
            last_sfx_at: None,
        }
    }

    #[test]
    fn position_at_zero_returns_original_offset() {
        let state = state_at_origin(100, 5, 73);
        let (offset, settled) = state.position_at(state.start_time);
        assert_eq!(offset, 5);
        assert!(!settled);
    }

    #[test]
    fn position_settles_on_target_after_full_duration() {
        let state = state_at_origin(100, 5, 73);
        let after = state.start_time
            + Duration::from_millis(
                RouletteState::MAIN_DURATION_MS
                    + RouletteState::FAKEOUT_PAUSE_MS
                    + RouletteState::FAKEOUT_TICK1_MS
                    + RouletteState::FAKEOUT_TICK2_MS
                    + 50,
            );
        let (offset, settled) = state.position_at(after);
        assert!(settled, "spin should be settled after main + fake-out");
        assert_eq!(offset, 73, "settled offset must equal target_idx");
    }

    #[test]
    fn position_during_pause_holds_near_miss() {
        let state = state_at_origin(100, 0, 50);
        let mid_pause =
            state.start_time + Duration::from_millis(RouletteState::MAIN_DURATION_MS + 100);
        let (offset, settled) = state.position_at(mid_pause);
        assert!(!settled);
        // Near-miss = total_steps - 2; modulo total_items lands two short of target.
        let near_miss = state.total_steps.saturating_sub(2);
        assert_eq!(
            offset,
            (state.original_offset + near_miss) % state.total_items
        );
    }

    #[test]
    fn fakeout_walks_one_step_before_settle() {
        let state = state_at_origin(100, 0, 50);
        let pre_settle = state.start_time
            + Duration::from_millis(
                RouletteState::MAIN_DURATION_MS
                    + RouletteState::FAKEOUT_PAUSE_MS
                    + RouletteState::FAKEOUT_TICK1_MS
                    + 10,
            );
        let (offset, settled) = state.position_at(pre_settle);
        assert!(!settled);
        let one_short = state.total_steps.saturating_sub(1);
        assert_eq!(
            offset,
            (state.original_offset + one_short) % state.total_items
        );
    }

    #[test]
    fn revolutions_scale_with_list_size() {
        assert_eq!(revolutions_for(5), 6);
        assert_eq!(revolutions_for(20), 4);
        assert_eq!(revolutions_for(500), 3);
    }

    #[test]
    fn pick_random_index_within_bounds() {
        for total in [1, 2, 5, 100, 10_000] {
            for _ in 0..20 {
                let idx = pick_random_index(total);
                assert!(idx < total, "idx {idx} must be < {total}");
            }
        }
    }

    #[test]
    fn pick_random_index_zero_total_is_zero() {
        assert_eq!(pick_random_index(0), 0);
    }
}
