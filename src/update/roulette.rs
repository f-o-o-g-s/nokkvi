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
    state::{FakeoutKeyframe, RouletteState},
    views,
};

/// Below this item count the spin animation is too short to feel like a
/// roulette — just dispatch the play immediately.
const MIN_ITEMS_FOR_SPIN: usize = 3;

/// Total roulette duration budget (main spin + fake-out walk). Variable
/// per spin so consecutive plays don't feel mechanically identical.
const TOTAL_DURATION_MIN_MS: u64 = 4400;
const TOTAL_DURATION_MAX_MS: u64 = 5400;
/// Floor on the eased main spin alone. Ensures even a long fake-out
/// can't squeeze the visible deceleration below something legible.
const MAIN_DURATION_FLOOR_MS: u64 = 2800;
/// Cruise phase length — how long the wheel spins at constant velocity
/// before deceleration begins. Jittered per spin so consecutive plays
/// don't lock onto an identical "spin up, slow down" cadence.
const CRUISE_DURATION_MIN_MS: u64 = 1300;
const CRUISE_DURATION_MAX_MS: u64 = 1700;

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

        // Single PRNG seeded once per spin so every random choice (target
        // index, fake-out pattern, direction, per-keyframe holds, total
        // duration) shares entropy without correlating — a multi-call
        // SystemTime approach can land on the same nanosecond bucket on
        // fast hardware and produce visibly repeating spins.
        let mut rng = XorShift64::seeded_now();

        let target_idx = (rng.next() as usize) % total_items;

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
        let fakeout_keyframes = build_fakeout_keyframes(target_idx, total_items, &mut rng);
        // Main spin lands on the first fake-out keyframe, not directly on
        // `target_idx` — the keyframe walk handles the last few rows with
        // timed pauses for the slot-machine wobble. When the rolled
        // pattern is "no fake-out" the first (and only) keyframe is
        // `target_idx` itself, so the eased spin lands directly on target.
        let near_miss_offset = fakeout_keyframes.first().map_or(target_idx, |k| k.offset);
        let forward_diff =
            (near_miss_offset + total_items - (original_offset % total_items)) % total_items;
        let main_spin_steps = revolutions * total_items + forward_diff;

        // Total budget jittered per spin; main spin claims whatever's
        // left after the fake-out, with a floor so a chatty fake-out
        // can't squeeze the deceleration into something abrupt.
        let total_duration_ms = rng.range_inclusive(TOTAL_DURATION_MIN_MS, TOTAL_DURATION_MAX_MS);
        let fakeout_total_ms: u64 = fakeout_keyframes
            .iter()
            .take(fakeout_keyframes.len().saturating_sub(1))
            .map(|k| k.duration_ms)
            .sum();
        let main_duration_ms = total_duration_ms
            .saturating_sub(fakeout_total_ms)
            .max(MAIN_DURATION_FLOOR_MS);
        let cruise_duration_ms =
            rng.range_inclusive(CRUISE_DURATION_MIN_MS, CRUISE_DURATION_MAX_MS);

        debug!(
            "Roulette start: view={:?} total_items={} target={} original_offset={} \
             revolutions={} main_spin_steps={} main_duration_ms={} \
             cruise_duration_ms={} fakeout_total_ms={} keyframes={:?}",
            view,
            total_items,
            target_idx,
            original_offset,
            revolutions,
            main_spin_steps,
            main_duration_ms,
            cruise_duration_ms,
            fakeout_total_ms,
            fakeout_keyframes
        );

        self.roulette = Some(RouletteState {
            view,
            total_items,
            original_offset,
            target_idx,
            main_duration_ms,
            cruise_duration_ms,
            main_spin_steps,
            fakeout_keyframes,
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
                Task::done(Message::Playlists(views::PlaylistsMessage::SlotList(
                    crate::widgets::SlotListPageMessage::ActivateCenter,
                )))
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

/// Tiny xorshift64* PRNG. Not cryptographic — just enough variety for
/// the roulette's visual roll. One seed feeds every random choice in a
/// spin (pattern, direction, holds, durations) so the choices share
/// entropy without correlating across calls to `SystemTime::now()` that
/// would otherwise land in the same nanosecond bucket on fast hardware.
struct XorShift64(u64);

impl XorShift64 {
    fn seeded_now() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        // Splash an arbitrary mixer in case the clock is low-resolution
        // — a zero seed is a fixed point for xorshift, so guard against it.
        let seed = nanos ^ 0x9E37_79B9_7F4A_7C15;
        Self(if seed == 0 { 1 } else { seed })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Inclusive range `[lo, hi]`. Returns `lo` if `hi <= lo`.
    fn range_inclusive(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        let span = hi - lo + 1;
        lo + (self.next() % span)
    }
}

/// One of the available fake-out patterns. Each names a distinct shape
/// for how the wheel approaches `target_idx` after the eased main spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeoutPattern {
    /// No wobble — the eased spin decelerates straight onto the target.
    /// Picking this occasionally (~20%) keeps the slot-machine feel from
    /// becoming predictable.
    None,
    /// Single near-miss tick: `target ± 1`, then settle.
    Single,
    /// Two ticks marching toward the target: `target ± 2`, `target ± 1`,
    /// then settle. Same direction throughout.
    Double,
    /// Overshoot bounce: pass the target by 1, hold, retreat past target
    /// by 1 in the opposite direction, hold, settle.
    Overshoot,
    /// Three ticks zigzagging across the target before settling.
    Zigzag,
}

impl FakeoutPattern {
    /// Roll a weighted pattern. None gets ~20% so the wheel sometimes
    /// just decelerates cleanly; the wobble patterns share the rest.
    fn roll(rng: &mut XorShift64) -> Self {
        // 16 buckets so the weights stay readable as integer ratios.
        match rng.next() % 16 {
            0..=2 => Self::None,        // 3/16 ≈ 19%
            3..=6 => Self::Single,      // 4/16 = 25%
            7..=9 => Self::Double,      // 3/16 ≈ 19%
            10..=12 => Self::Overshoot, // 3/16 ≈ 19%
            _ => Self::Zigzag,          // 3/16 ≈ 19%
        }
    }
}

/// Build the post-spin fake-out walk. Picks a random pattern + direction
/// per spin and individually jitters every keyframe's hold duration so
/// the audible tick spacing varies within a single fake-out as well as
/// across consecutive spins. The final keyframe is always at
/// `target_idx` and signals settle.
fn build_fakeout_keyframes(
    target_idx: usize,
    total_items: usize,
    rng: &mut XorShift64,
) -> Vec<FakeoutKeyframe> {
    if total_items == 0 {
        return Vec::new();
    }

    let pattern = FakeoutPattern::roll(rng);
    let direction: i32 = if rng.next() & 1 == 0 { 1 } else { -1 };

    // Per-pattern hold-duration ranges. Each non-terminal keyframe pulls
    // an independent sample from its band so two ticks within the same
    // fake-out audibly differ — a uniform 200 ms across every keyframe
    // sounds robotic.
    let signed_pattern: Vec<(i32, (u64, u64))> = match pattern {
        FakeoutPattern::None => vec![],
        FakeoutPattern::Single => vec![(direction, (220, 380))],
        FakeoutPattern::Double => vec![(direction * 2, (170, 280)), (direction, (160, 280))],
        FakeoutPattern::Overshoot => vec![(direction, (160, 260)), (-direction, (200, 320))],
        FakeoutPattern::Zigzag => vec![
            (direction * 2, (130, 210)),
            (-direction, (130, 200)),
            (direction, (170, 260)),
        ],
    };

    let total = total_items as i32;
    let mut keyframes: Vec<FakeoutKeyframe> = signed_pattern
        .into_iter()
        .map(|(signed, (lo, hi))| {
            let abs = (target_idx as i32 + signed).rem_euclid(total) as usize;
            FakeoutKeyframe {
                offset: abs,
                duration_ms: rng.range_inclusive(lo, hi),
            }
        })
        .collect();
    // Terminal keyframe always lands on the target. Its `duration_ms`
    // is unused — `position_at` reports settled the moment we enter it.
    keyframes.push(FakeoutKeyframe {
        offset: target_idx,
        duration_ms: 0,
    });
    keyframes
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, time::Duration};

    use super::*;

    /// Build a state with a deterministic single-tick fake-out, so
    /// position-tracking tests can assert exact offsets without
    /// depending on the random pattern roll.
    fn state_with_fixed_fakeout(
        total_items: usize,
        original: usize,
        target: usize,
    ) -> RouletteState {
        let revs = revolutions_for(total_items);
        let near_miss = (target + total_items - 1) % total_items;
        let forward_diff = (near_miss + total_items - (original % total_items)) % total_items;
        let main_spin_steps = revs * total_items + forward_diff;
        RouletteState {
            view: View::Albums,
            total_items,
            original_offset: original,
            target_idx: target,
            main_duration_ms: 4000,
            cruise_duration_ms: 1500,
            main_spin_steps,
            fakeout_keyframes: vec![
                FakeoutKeyframe {
                    offset: near_miss,
                    duration_ms: 220,
                },
                FakeoutKeyframe {
                    offset: target,
                    duration_ms: 0,
                },
            ],
            start_time: Instant::now(),
            last_offset: original,
            last_sfx_at: None,
        }
    }

    /// Sample `n` keyframe builds. Stagger calls by 1 ms purely as
    /// defense against same-nanosecond clock reads on virtualised CI;
    /// the xorshift seed mixer should already protect against fixed
    /// points.
    fn sample_keyframes(target: usize, total: usize, n: usize) -> Vec<Vec<FakeoutKeyframe>> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let mut rng = XorShift64::seeded_now();
            out.push(build_fakeout_keyframes(target, total, &mut rng));
            std::thread::sleep(Duration::from_millis(1));
        }
        out
    }

    #[test]
    fn position_at_zero_returns_original_offset() {
        let state = state_with_fixed_fakeout(100, 5, 73);
        let (offset, settled) = state.position_at(state.start_time);
        assert_eq!(offset, 5);
        assert!(!settled);
    }

    #[test]
    fn position_settles_on_target_after_full_duration() {
        let state = state_with_fixed_fakeout(100, 5, 73);
        let total_fakeout: u64 = state
            .fakeout_keyframes
            .iter()
            .take(state.fakeout_keyframes.len() - 1)
            .map(|k| k.duration_ms)
            .sum();
        let after =
            state.start_time + Duration::from_millis(state.main_duration_ms + total_fakeout + 50);
        let (offset, settled) = state.position_at(after);
        assert!(settled, "spin should be settled after main + fake-out");
        assert_eq!(offset, 73, "settled offset must equal target_idx");
    }

    #[test]
    fn position_during_first_keyframe_holds_near_miss() {
        let state = state_with_fixed_fakeout(100, 0, 50);
        let mid_keyframe = state.start_time + Duration::from_millis(state.main_duration_ms + 100);
        let (offset, settled) = state.position_at(mid_keyframe);
        assert!(!settled);
        assert_eq!(offset, state.fakeout_keyframes[0].offset);
    }

    #[test]
    fn fakeout_can_be_skipped_entirely() {
        // None weight is ~3/16, so across 200 rolls the probability of
        // never producing a no-fakeout spin is vanishingly small.
        let samples = sample_keyframes(50, 100, 200);
        let saw_no_fakeout = samples
            .iter()
            .any(|kf| kf.len() == 1 && kf.last().map(|k| k.offset) == Some(50));
        assert!(
            saw_no_fakeout,
            "rolling 200 spins must occasionally produce no fake-out"
        );
    }

    #[test]
    fn fakeout_overshoots_and_undershoots_across_rolls() {
        let samples = sample_keyframes(50, 100, 200);
        let mut saw_overshoot = false;
        let mut saw_undershoot = false;
        for kf in &samples {
            // Skip terminal keyframe (always = target).
            for k in &kf[..kf.len() - 1] {
                if k.offset > 50 {
                    saw_overshoot = true;
                }
                if k.offset < 50 {
                    saw_undershoot = true;
                }
            }
        }
        assert!(saw_overshoot, "fake-out should sometimes overshoot target");
        assert!(
            saw_undershoot,
            "fake-out should sometimes undershoot target"
        );
    }

    #[test]
    fn fakeout_always_settles_on_target() {
        for kf in sample_keyframes(42, 100, 50) {
            assert_eq!(
                kf.last().map(|k| k.offset),
                Some(42),
                "every fake-out must terminate on target"
            );
        }
    }

    #[test]
    fn fakeout_keyframes_wrap_around_list_boundaries() {
        // target = 0 with negative offsets must wrap to total_items - 1,
        // not produce a value >= total_items or panic.
        for kf in sample_keyframes(0, 50, 50) {
            for k in &kf {
                assert!(k.offset < 50, "every keyframe offset must stay in range");
            }
        }
    }

    #[test]
    fn fakeout_keyframe_holds_are_individually_jittered() {
        // Within multi-keyframe patterns the per-keyframe holds must
        // sample independently — otherwise every tick at the end sounds
        // metronomic. Across 80 rolls we should observe at least one
        // multi-keyframe fake-out whose non-terminal holds are distinct.
        let samples = sample_keyframes(50, 100, 80);
        let saw_distinct_holds = samples.iter().any(|kf| {
            if kf.len() < 3 {
                return false;
            }
            let holds: HashSet<u64> = kf[..kf.len() - 1].iter().map(|k| k.duration_ms).collect();
            holds.len() >= 2
        });
        assert!(
            saw_distinct_holds,
            "multi-keyframe fake-outs must sometimes have distinct per-tick holds"
        );
    }

    #[test]
    fn revolutions_scale_with_list_size() {
        assert_eq!(revolutions_for(5), 6);
        assert_eq!(revolutions_for(20), 4);
        assert_eq!(revolutions_for(500), 3);
    }

    #[test]
    fn xorshift_never_emits_zero_after_nonzero_seed() {
        // xorshift's fixed point is 0; the seed mixer guarantees we
        // start off it. Drawing many values must never settle on stuck
        // zero.
        let mut rng = XorShift64::seeded_now();
        for _ in 0..1000 {
            assert_ne!(rng.next(), 0, "xorshift should never emit zero");
        }
    }
}
