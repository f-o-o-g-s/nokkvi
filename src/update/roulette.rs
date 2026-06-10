//! Roulette (slot-machine random pick) handler.
//!
//! Drives a fixed-viewport "wheel spin" through any slot-list view. The
//! cruise phase runs at a constant rate indefinitely — the user presses
//! Enter to stop it, which rolls the landing target and arms the decel
//! walk. Animation is fully time-derived from `RouletteState.position_at`
//! — tick handlers are pure bookkeeping (advance offset, fire Tab SFX,
//! detect settle).
//!
//! Trigger: the "Roulette" entry appended to each view's sort dropdown
//! (or the bound hotkey, default Ctrl+R) emits
//! `Message::Roulette(RouletteMessage::Start(view))`. The Enter key during
//! a spin emits `RouletteMessage::Stop`. Cancel paths (Escape, view
//! switch) emit `Cancel`.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use iced::Task;
use nokkvi_data::audio::SfxType;
use tracing::{debug, trace};

use crate::{
    Nokkvi, View,
    app_message::{Message, NavigationMessage, RouletteMessage},
    state::{DecelArmed, DecelKeyframe, RouletteState},
};

/// Below this item count the spin animation is too short to feel like a
/// roulette — just dispatch the play immediately.
const MIN_ITEMS_FOR_SPIN: usize = 3;

/// Reference window the cruise rate is computed against. The rate is
/// chosen so the wheel walks `revolutions × total_items` positions over
/// this window, matching the visual velocity of the old time-bounded
/// cruise. With the cruise now indefinite, only the *rate* matters — the
/// window length is just the denominator that turns
/// `revolutions × total_items` into a positions-per-second figure.
const CRUISE_RATE_REFERENCE_WINDOW_MS: u64 = 1500;

/// Hard ceiling on cruise velocity. Without this, a 13 k-item library
/// would cruise at ~27 000 positions/sec — completely unreadable, each
/// 60 Hz frame jumping ~450 items. Cap so the eye can at least half-
/// perceive items strobing past during the cruise. 600 pos/sec ≈ 10
/// items per frame at 60 Hz, which still reads as fast motion blur but
/// shows enough texture to feel like a wheel rather than a uniform smear.
const CRUISE_RATE_MAX_POS_PER_SEC: u32 = 600;

/// Decel walk distance, in slot positions. The wheel decelerates over
/// `NATURAL_KEYFRAME_COUNT` clicks and lands `DECEL_WALK_*` positions
/// past the cruise-end offset. Tight bounds keep the per-click advance
/// readable on any library size (1–3 positions/click) — the previous
/// "1 revolution + walk-to-target" model gave ~800 positions/click for
/// a 13 k-song library, which read as teleportation, not deceleration.
const DECEL_WALK_MIN: usize = NATURAL_KEYFRAME_COUNT;
const DECEL_WALK_MAX: usize = NATURAL_KEYFRAME_COUNT * 3;

/// Decel phase length — how long the audible click cadence takes to
/// slow from ~20 Hz (cruise-rate-matching first click) down to ~1 Hz
/// (slot-machine final click). Jittered per spin.
const DECEL_DURATION_MIN_MS: u64 = 2400;
const DECEL_DURATION_MAX_MS: u64 = 3200;

/// Number of cubic-distributed keyframes in the natural-walk portion
/// of the decel phase. With 17 keyframes over 2400–3200 ms the click
/// holds escalate from ~47 ms (cruise-rate-matching) to ~1190 ms
/// (slot-machine final click). Pattern variations append 0–2 extra
/// keyframes after the natural walk for the final wobble.
const NATURAL_KEYFRAME_COUNT: usize = 17;

/// Minimum gap between viewport-artwork prefetch dispatches during the
/// spin. Matches the normal-scroll `seek_settled_timer` debounce so
/// the spin doesn't queue 60 prefetch batches per second — each batch
/// covers the visible viewport, dedup'd against the artwork LRU, and
/// new viewport positions are checked again 150 ms later. By settle,
/// every album the wheel scrolled past has had its thumbnail fetched.
const PREFETCH_MIN_INTERVAL_MS: u64 = 150;

impl Nokkvi {
    pub(crate) fn handle_roulette_message(&mut self, msg: RouletteMessage) -> Task<Message> {
        match msg {
            RouletteMessage::Start(view) => self.handle_roulette_start(view),
            RouletteMessage::Stop => self.handle_roulette_stop(),
            RouletteMessage::Tick(now) => self.handle_roulette_tick(now),
            RouletteMessage::Cancel => self.handle_roulette_cancel(),
        }
    }

    fn handle_roulette_start(&mut self, view: View) -> Task<Message> {
        // Reentrancy guard — second click while spinning is a no-op.
        if self.roulette.is_some() {
            return Task::none();
        }

        // Selecting "Roulette" from the sort dropdown closes it via the
        // overlay's on_select (not on_close), so drop the toolbar reveal-lock.
        if let Some(page) = self.view_page_mut(view) {
            page.common_mut().set_toolbar_dropdown_open(false);
        }

        // Block plays during playlist edit mode the same way every other
        // play handler does. Skip `enter_new_playback_context()` here — the
        // per-view settle dispatch (`roulette_settle_play`) routes through
        // each view's own play handler, which calls the helper when the play
        // replaces the queue. Calling it up-front would clear the loaded
        // playlist header for Queue-view roulette, which is an in-queue play.
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
            View::Queue | View::Songs | View::Radios | View::Settings | View::PlaylistEditor => {}
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

        // Skip animation entirely for tiny lists — a 3-tick spin feels
        // anticlimactic and the user can't really tell anyway. Roll the
        // target here since there's no cruise phase to await a Stop on.
        if total_items < MIN_ITEMS_FOR_SPIN {
            let mut rng = XorShift64::seeded_now();
            let target_idx = (rng.next() as usize) % total_items;
            debug!(
                "Roulette: {:?} has only {} items; settling immediately on idx {}",
                view, total_items, target_idx
            );
            return self.roulette_settle_play(view, target_idx, total_items);
        }

        let revolutions = revolutions_for(total_items);
        let cruise_pos_per_sec = cruise_rate_for(revolutions, total_items);

        debug!(
            "Roulette start: view={:?} total_items={} original_offset={} \
             cruise_pos_per_sec={} (revolutions={}) — awaiting Enter to stop",
            view, total_items, original_offset, cruise_pos_per_sec, revolutions
        );

        self.roulette = Some(RouletteState {
            view,
            total_items,
            original_offset,
            cruise_pos_per_sec,
            decel: None,
            start_time: Instant::now(),
            last_offset: original_offset,
            last_sfx_at: None,
            last_prefetch_at: None,
        });

        Task::none()
    }

    fn handle_roulette_stop(&mut self) -> Task<Message> {
        // Require an in-cruise spin. If decel is already armed the spin is
        // committed — second Stop press is a no-op.
        let Some(state) = self.roulette.as_ref() else {
            return Task::none();
        };
        if state.decel.is_some() {
            return Task::none();
        }

        let now = Instant::now();
        let total_items = state.total_items;
        let view = state.view;

        // Current cruise position becomes the starting offset of the decel
        // walk. position_at() in the cruise branch is the same computation
        // a fresh tick would have made — keeps the wheel visually continuous
        // across the cruise→decel transition.
        let (cruise_end_offset, _) = state.position_at(now);

        let mut rng = XorShift64::seeded_now();
        let pattern = FakeoutPattern::roll(&mut rng);
        let direction: i32 = if rng.next() & 1 == 0 { 1 } else { -1 };
        let decel_duration_ms = rng.range_inclusive(DECEL_DURATION_MIN_MS, DECEL_DURATION_MAX_MS);

        // Decel walks a small fixed-ish number of positions independent
        // of library size — the wheel coasts ~1–3 items per click,
        // ratcheting down from the cruise rate to a final visible step.
        // Randomness in "where it lands" lives in (a) when the user
        // pressed Enter (cruise position is timing-driven), and (b) this
        // jittered walk distance. Computing `natural_steps` directly
        // (rather than deriving from a freely-rolled `target_idx`) is
        // what keeps the per-click advance bounded: the old "roll a
        // random target across the whole library, walk to it" path gave
        // ~800 positions/click for 13 k items.
        let natural_steps =
            rng.range_inclusive(DECEL_WALK_MIN as u64, DECEL_WALK_MAX as u64) as usize;
        // Natural walk ends at `target - 1`, so target sits one position
        // beyond the walk's end. Wrap modulo total_items so tiny libraries
        // (which the walk may lap several times) still produce a valid index.
        let target_idx = (cruise_end_offset + natural_steps + 1) % total_items;

        let decel_keyframes = build_decel_keyframes(
            cruise_end_offset,
            target_idx,
            total_items,
            natural_steps,
            decel_duration_ms,
            pattern,
            direction,
            &mut rng,
        );

        debug!(
            "Roulette stop: view={:?} total_items={} target={} cruise_end_offset={} \
             natural_steps={} decel_duration_ms={} pattern={:?} direction={} keyframe_count={}",
            view,
            total_items,
            target_idx,
            cruise_end_offset,
            natural_steps,
            decel_duration_ms,
            pattern,
            direction,
            decel_keyframes.len()
        );

        // Commit decel. No Enter SFX here — the keypress kicks off the
        // decel walk, but the audible "Enter" is reserved for the final
        // landing on the picked song (fired by the settle branch in
        // handle_roulette_tick). The Tab clicks of the decel sequence are
        // what confirm the keypress registered. Reset last_sfx_at so the
        // first decel click isn't suppressed by the cruise-rattle throttle.
        if let Some(s) = self.roulette.as_mut() {
            s.decel = Some(DecelArmed {
                stop_time: now,
                target_idx,
                decel_keyframes,
            });
            s.last_sfx_at = None;
        }

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
        let last_offset = state.last_offset;
        let last_sfx_at = state.last_sfx_at;
        let last_prefetch_at = state.last_prefetch_at;

        let (offset, settled) = state.position_at(now);
        // Pull target_idx for the settle dispatch from the decel arm if
        // it's committed; settle never fires during cruise (`settled` is
        // always false in that branch).
        let target_idx = state.decel.as_ref().map(|a| a.target_idx);

        let mut prefetch_task: Option<Task<Message>> = None;
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

            // Dispatch viewport-artwork prefetch — without this, the
            // spin scrolls past slots whose thumbnails were never
            // requested and the slot list shows gray boxes until
            // settle. Throttled to ~150 ms so we don't queue 60
            // batches per second.
            let should_prefetch = last_prefetch_at.is_none_or(|t| {
                now.saturating_duration_since(t).as_millis() as u64 >= PREFETCH_MIN_INTERVAL_MS
            });
            if should_prefetch {
                let task = self.prefetch_viewport_artwork();
                if let Some(s) = self.roulette.as_mut() {
                    s.last_prefetch_at = Some(now);
                }
                prefetch_task = Some(task);
            }

            if let Some(s) = self.roulette.as_mut() {
                s.last_offset = offset;
            }
        }

        if settled {
            let idx = target_idx.unwrap_or(offset);
            trace!("Roulette settle on view={:?} idx={}", view, idx);
            self.sfx_engine.play(SfxType::Enter);
            self.roulette = None;
            let settle_task = self.roulette_settle_play(view, idx, total_items);
            return match prefetch_task {
                Some(p) => Task::batch([p, settle_task]),
                None => settle_task,
            };
        }

        prefetch_task.unwrap_or_else(Task::none)
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
            View::Settings | View::PlaylistEditor => 0,
        }
    }

    fn roulette_view_viewport_offset(&self, view: View) -> Option<usize> {
        self.view_page(view)
            .map(|p| p.common().slot_list.viewport_offset)
    }

    /// Move the slot list viewport on `view` to `offset`. Uses
    /// `handle_set_offset` (clears `selected_offset`, records scroll) so the
    /// transient scrollbar fades correctly while the wheel spins.
    pub(crate) fn roulette_apply_offset(&mut self, view: View, offset: usize, total_items: usize) {
        if let Some(page) = self.view_page_mut(view) {
            page.common_mut().handle_set_offset(offset, total_items);
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
            View::Genres => {
                let Some(genre) = self.library.genres.get(target_idx) else {
                    return Task::none();
                };
                let name = genre.name.clone();
                self.clear_active_playlist();
                self.shell_action_task(
                    move |shell| async move { shell.play_genre_random(&name).await },
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
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
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "play random song from artist",
                )
            }
            // No ViewPage and nothing to pick — unreachable in practice
            // (`roulette_view_total` returns 0 for both and
            // `handle_roulette_start` gates on total == 0), kept exhaustive
            // so a new view must place itself explicitly.
            View::Settings | View::PlaylistEditor => Task::none(),
            View::Queue | View::Songs | View::Albums | View::Playlists | View::Radios => {
                let Some(page) = self.view_page_mut(view) else {
                    return Task::none();
                };
                let common = page.common_mut();
                common.handle_set_offset(target_idx, total_items);
                common.slot_list.flash_center();
                Task::done(
                    page.slot_list_message(crate::widgets::SlotListPageMessage::ActivateCenter),
                )
            }
        }
    }
}

/// How many full revolutions the wheel makes per cruise-rate reference
/// window. Smaller lists need more "revolutions per window" to feel like
/// a real spin (otherwise the wheel barely moves at the same visual rate
/// big libraries cruise at).
fn revolutions_for(total_items: usize) -> usize {
    if total_items < 10 {
        6
    } else if total_items < 50 {
        4
    } else {
        3
    }
}

/// Constant cruise rate (positions per second) for the indefinite cruise
/// phase. Computed once at start so visual velocity stays steady through
/// the spin regardless of how long the user holds before pressing Enter.
/// Capped at `CRUISE_RATE_MAX_POS_PER_SEC` so massive libraries don't
/// turn the cruise into an unreadable smear (a 13 k-song library would
/// otherwise cruise at ~27 k positions/sec — ~450 items per 60 Hz frame).
fn cruise_rate_for(revolutions: usize, total_items: usize) -> u32 {
    let steps_per_window = (revolutions as u64).saturating_mul(total_items as u64);
    let rate = steps_per_window.saturating_mul(1000) / CRUISE_RATE_REFERENCE_WINDOW_MS;
    // Floor at 1 pos/sec so empty/degenerate inputs can't stall the spin.
    rate.clamp(1, CRUISE_RATE_MAX_POS_PER_SEC as u64) as u32
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

/// Shape of the final wobble after the natural-walk's long terminal
/// hold at `target - 1`. The natural walk is shared across all
/// patterns — only the tail keyframes (0–2 entries) and their explicit
/// holds differ per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeoutPattern {
    /// No wobble. Natural walk ends at `target - 1` with a long cubic
    /// hold, then the terminal keyframe settles straight onto target.
    /// Most common pick — lets the decel curve speak for itself.
    CleanLand,
    /// One extra keyframe at `target + direction` (overshoots target by
    /// one), held briefly, then settles. The "wheel went a touch too
    /// far" feel.
    Overshoot,
    /// Two extra keyframes: first at `target` itself (a false settle
    /// — the user thinks the wheel locked), then at `target + direction`
    /// (overshoot past), then settles. The slot-machine "wait, maybe
    /// this one… no THIS!" moment of doubt.
    FalseSettle,
}

impl FakeoutPattern {
    /// Roll a weighted pattern. CleanLand gets the largest share so
    /// the cubic decel's long terminal hold is the dominant flavor,
    /// with overshoot/false-settle as wobble variants.
    fn roll(rng: &mut XorShift64) -> Self {
        match rng.next() % 16 {
            0..=5 => Self::CleanLand,  // 6/16 = 37.5%
            6..=10 => Self::Overshoot, // 5/16 ≈ 31%
            _ => Self::FalseSettle,    // 5/16 ≈ 31%
        }
    }

    /// Number of extra keyframes the pattern appends after the natural
    /// walk and before the terminal keyframe.
    fn tail_count(self) -> usize {
        match self {
            Self::CleanLand => 0,
            Self::Overshoot => 1,
            Self::FalseSettle => 2,
        }
    }
}

/// Build the decel + fake-out keyframe sequence.
///
/// Layout:
/// 1. `NATURAL_KEYFRAME_COUNT` natural-walk keyframes with cubic-
///    distributed holds. Each advances `natural_steps / N` (with
///    remainder front-loaded so early clicks are slightly chunkier).
///    The walk lands at `target - 1`.
/// 2. 0–2 pattern tail keyframes with explicit jittered holds — the
///    overshoot/false-settle wobble after the natural walk's long
///    terminal hold.
/// 3. Terminal keyframe at `target_idx` (duration 0).
#[allow(clippy::too_many_arguments)]
fn build_decel_keyframes(
    cruise_end_offset: usize,
    target_idx: usize,
    total_items: usize,
    natural_steps: usize,
    decel_duration_ms: u64,
    pattern: FakeoutPattern,
    direction: i32,
    rng: &mut XorShift64,
) -> Vec<DecelKeyframe> {
    if total_items == 0 {
        return Vec::new();
    }
    let n = NATURAL_KEYFRAME_COUNT;
    let mut keyframes = Vec::with_capacity(n + pattern.tail_count() + 1);

    // Distribute natural_steps across `n` keyframes. base advance for
    // most; the first `remainder` keyframes advance `base + 1` so the
    // sum is exact. Front-loading the remainder means early (fast)
    // clicks are slightly chunkier than late (slow) clicks, which
    // reinforces the "audible slowdown" feel.
    let base = natural_steps / n;
    let remainder = natural_steps - base * n;

    let mut cumulative: usize = 0;
    for k in 0..n {
        let advance = if k < remainder { base + 1 } else { base };
        cumulative += advance;
        let offset = (cruise_end_offset + cumulative) % total_items;
        let duration_ms = cubic_hold_ms(k, n, decel_duration_ms);
        keyframes.push(DecelKeyframe {
            offset,
            duration_ms,
        });
    }

    // Pattern tail — positions and holds vary per variant.
    let tail_offsets = pattern_tail_offsets(pattern, target_idx, total_items, direction);
    let tail_holds_ms = pattern_tail_holds(pattern, rng);
    debug_assert_eq!(tail_offsets.len(), tail_holds_ms.len());
    for (offset, duration_ms) in tail_offsets.into_iter().zip(tail_holds_ms) {
        keyframes.push(DecelKeyframe {
            offset,
            duration_ms,
        });
    }

    // Terminal at target — settles on entry.
    keyframes.push(DecelKeyframe {
        offset: target_idx,
        duration_ms: 0,
    });

    keyframes
}

/// Hold for keyframe `k` of `n` total, derived from a cubic ease-out
/// time curve over `duration_ms`. Holds escalate monotonically from
/// ~D/N² (first) to ~D·∛(1/N) (last). Sums to `duration_ms` across
/// the sequence (up to per-entry truncation rounding error).
fn cubic_hold_ms(k: usize, n: usize, duration_ms: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let d = duration_ms as f64;
    let lo = 1.0 - (k as f64) / (n as f64);
    let hi = (1.0 - ((k + 1) as f64) / (n as f64)).max(0.0);
    let t_lo = d * (1.0 - lo.cbrt());
    let t_hi = d * (1.0 - hi.cbrt());
    (t_hi - t_lo).max(0.0).round() as u64
}

/// Absolute viewport offsets for the pattern tail keyframes, in walk
/// order. All offsets are computed modulo `total_items`.
fn pattern_tail_offsets(
    pattern: FakeoutPattern,
    target_idx: usize,
    total_items: usize,
    direction: i32,
) -> Vec<usize> {
    let off = |signed: i32| {
        let t = target_idx as i32 + signed;
        t.rem_euclid(total_items as i32) as usize
    };
    match pattern {
        FakeoutPattern::CleanLand => Vec::new(),
        FakeoutPattern::Overshoot => vec![off(direction)],
        FakeoutPattern::FalseSettle => vec![target_idx, off(direction)],
    }
}

/// Hold durations for the pattern tail keyframes, in walk order. Each
/// hold pulls an independent sample from a pattern-specific range so
/// consecutive plays don't lock onto identical wobble timing.
fn pattern_tail_holds(pattern: FakeoutPattern, rng: &mut XorShift64) -> Vec<u64> {
    match pattern {
        FakeoutPattern::CleanLand => Vec::new(),
        FakeoutPattern::Overshoot => vec![rng.range_inclusive(400, 700)],
        FakeoutPattern::FalseSettle => {
            vec![rng.range_inclusive(380, 550), rng.range_inclusive(450, 650)]
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Build an indefinite-cruise state (decel = None) for cruise-phase
    /// position tests. Uses the same rate the start handler would pick.
    fn cruise_state(total_items: usize, original: usize) -> RouletteState {
        let revs = revolutions_for(total_items);
        RouletteState {
            view: View::Albums,
            total_items,
            original_offset: original,
            cruise_pos_per_sec: cruise_rate_for(revs, total_items),
            decel: None,
            start_time: Instant::now(),
            last_offset: original,
            last_sfx_at: None,
            last_prefetch_at: None,
        }
    }

    /// Build a decel-armed state — cruise + Stop has fired and the decel
    /// walk is committed. Used by decel-phase position tests. Picks a
    /// walk wide enough to span original→target across at least one
    /// revolution; production uses a small fixed-ish walk (DECEL_WALK_*)
    /// and derives target from cruise_end + walk, but for these tests
    /// we want to exercise the "can hit any target" math path.
    fn decel_state(total_items: usize, original: usize, target: usize) -> RouletteState {
        let revs = revolutions_for(total_items);
        let natural_end = (target + total_items - 1) % total_items;
        let natural_steps =
            total_items + ((natural_end + total_items - (original % total_items)) % total_items);
        let mut rng = XorShift64(0xCAFE_BABE_CAFE_BABE);
        let decel_keyframes = build_decel_keyframes(
            original,
            target,
            total_items,
            natural_steps,
            2800,
            FakeoutPattern::CleanLand,
            1,
            &mut rng,
        );
        let now = Instant::now();
        RouletteState {
            view: View::Albums,
            total_items,
            original_offset: original,
            cruise_pos_per_sec: cruise_rate_for(revs, total_items),
            decel: Some(DecelArmed {
                stop_time: now,
                target_idx: target,
                decel_keyframes,
            }),
            start_time: now,
            last_offset: original,
            last_sfx_at: None,
            last_prefetch_at: None,
        }
    }

    /// Collect `n` decel-keyframe builds with the RNG re-seeded between
    /// calls. Stagger by 1 ms to keep the seed-mixer happy on virtualised
    /// CI where consecutive `SystemTime::now()` reads may land in the
    /// same nanosecond bucket.
    fn sample_builds(
        target: usize,
        total: usize,
        n: usize,
    ) -> Vec<(FakeoutPattern, Vec<DecelKeyframe>)> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let mut rng = XorShift64::seeded_now();
            let pattern = FakeoutPattern::roll(&mut rng);
            let direction: i32 = if rng.next() & 1 == 0 { 1 } else { -1 };
            let kfs = build_decel_keyframes(
                0,
                target,
                total,
                NATURAL_KEYFRAME_COUNT,
                2800,
                pattern,
                direction,
                &mut rng,
            );
            out.push((pattern, kfs));
            std::thread::sleep(Duration::from_millis(1));
        }
        out
    }

    #[test]
    fn position_at_zero_returns_original_offset() {
        let state = cruise_state(100, 5);
        let (offset, settled) = state.position_at(state.start_time);
        assert_eq!(offset, 5);
        assert!(!settled);
    }

    #[test]
    fn cruise_advances_at_constant_rate_indefinitely() {
        // After one cruise-rate reference window the wheel should have
        // walked revolutions × total_items positions. Mod total_items
        // for a 100-item list with 3 revolutions: 300 % 100 = 0, so the
        // offset returns to original.
        let state = cruise_state(100, 0);
        let one_window =
            state.start_time + Duration::from_millis(super::CRUISE_RATE_REFERENCE_WINDOW_MS);
        let (offset, settled) = state.position_at(one_window);
        assert!(!settled, "cruise never settles on its own");
        let revs = revolutions_for(100);
        let expected = (revs * 100) % 100;
        let diff = (offset as i64 - expected as i64).abs();
        assert!(
            diff <= 1,
            "after one ref window, offset {offset} should be near {expected} (diff <= 1)"
        );
    }

    #[test]
    fn cruise_never_settles_no_matter_how_long() {
        // Indefinite cruise: even after 60 seconds, position_at must not
        // report settled. This is the headline behavioural change — the
        // wheel waits for the user to press Enter.
        let state = cruise_state(50, 0);
        let way_later = state.start_time + Duration::from_secs(60);
        let (_, settled) = state.position_at(way_later);
        assert!(!settled);
    }

    #[test]
    fn decel_state_settles_on_target_after_full_duration() {
        let state = decel_state(100, 5, 73);
        let arm = state.decel.as_ref().unwrap();
        let total_decel: u64 = arm
            .decel_keyframes
            .iter()
            .take(arm.decel_keyframes.len() - 1)
            .map(|k| k.duration_ms)
            .sum();
        let after = arm.stop_time + Duration::from_millis(total_decel + 200);
        let (offset, settled) = state.position_at(after);
        assert!(settled, "spin should be settled after the full decel walk");
        assert_eq!(offset, 73, "settled offset must equal target_idx");
    }

    #[test]
    fn decel_state_starts_with_first_click() {
        // The decel branch enters the first keyframe immediately at
        // stop_time — there is no resting frame. Mirrors the old all-decel
        // behaviour: Enter triggers a click, not a pause.
        let state = decel_state(100, 7, 60);
        let arm = state.decel.as_ref().unwrap();
        let (offset, settled) = state.position_at(arm.stop_time);
        assert_eq!(offset, arm.decel_keyframes[0].offset);
        assert!(!settled);
    }

    #[test]
    fn decel_state_position_during_keyframe_hold_returns_that_keyframe() {
        let state = decel_state(100, 0, 50);
        let arm = state.decel.as_ref().unwrap();
        // Halfway through the first decel keyframe's hold.
        let half = arm.decel_keyframes[0].duration_ms / 2;
        let probe = arm.stop_time + Duration::from_millis(half);
        let (offset, settled) = state.position_at(probe);
        assert!(!settled);
        assert_eq!(offset, arm.decel_keyframes[0].offset);
    }

    #[test]
    fn cubic_hold_ms_escalates_monotonically() {
        let n = NATURAL_KEYFRAME_COUNT;
        let holds: Vec<u64> = (0..n).map(|k| cubic_hold_ms(k, n, 3000)).collect();
        for w in holds.windows(2) {
            assert!(
                w[1] >= w[0],
                "cubic holds must escalate monotonically: {holds:?}"
            );
        }
        // First hold should be ~50ms, last should be ~1190ms for N=17 D=3000.
        assert!(
            holds[0] < 80,
            "first hold should be cruise-rate-matching, got {}",
            holds[0]
        );
        assert!(
            holds[n - 1] > 900,
            "last hold should be slot-machine-slow, got {}",
            holds[n - 1]
        );
    }

    #[test]
    fn cubic_hold_ms_sums_close_to_duration() {
        // Per-keyframe truncation accumulates; total should be within
        // N rounding ulps of `duration_ms`.
        let n = NATURAL_KEYFRAME_COUNT;
        let total: u64 = (0..n).map(|k| cubic_hold_ms(k, n, 3000)).sum();
        let diff = (total as i64 - 3000).abs();
        assert!(
            diff <= (n as i64),
            "sum of cubic holds {total} should be within {n} ms of duration"
        );
    }

    #[test]
    fn build_decel_keyframes_terminal_is_target() {
        let mut rng = XorShift64(0x1234);
        let kfs = build_decel_keyframes(
            0,
            42,
            100,
            NATURAL_KEYFRAME_COUNT,
            2800,
            FakeoutPattern::CleanLand,
            1,
            &mut rng,
        );
        assert_eq!(kfs.last().map(|k| k.offset), Some(42));
        assert_eq!(kfs.last().map(|k| k.duration_ms), Some(0));
    }

    #[test]
    fn one_step_per_keyframe_when_natural_steps_equals_n() {
        // With natural_steps == N each keyframe should advance exactly
        // one position from the previous.
        let mut rng = XorShift64(0x5678);
        let kfs = build_decel_keyframes(
            10,
            10 + NATURAL_KEYFRAME_COUNT,
            1000,
            NATURAL_KEYFRAME_COUNT,
            2800,
            FakeoutPattern::CleanLand,
            1,
            &mut rng,
        );
        // First N keyframes are the natural walk; offsets should be
        // 11, 12, ..., 10 + N.
        for (i, kf) in kfs.iter().take(NATURAL_KEYFRAME_COUNT).enumerate() {
            assert_eq!(kf.offset, 11 + i);
        }
    }

    #[test]
    fn natural_walk_advances_sum_to_natural_steps() {
        // With natural_steps spread across N keyframes, the cumulative
        // advance from cruise_end_offset to the last natural-walk
        // keyframe must equal natural_steps. The caller computes
        // natural_steps so this offset lands at target-1.
        let mut rng = XorShift64(0x9ABC);
        let total = 1000;
        let original = 0;
        let target = 99;
        let revolutions = 3;
        let natural_end = (target + total - 1) % total; // 98
        let natural_steps =
            revolutions * total + (natural_end + total - (original % total)) % total;
        let kfs = build_decel_keyframes(
            original,
            target,
            total,
            natural_steps,
            2800,
            FakeoutPattern::CleanLand,
            1,
            &mut rng,
        );
        let last_natural = kfs[NATURAL_KEYFRAME_COUNT - 1];
        assert_eq!(
            last_natural.offset, natural_end,
            "natural walk's last keyframe must sit at target-1"
        );
    }

    #[test]
    fn pattern_tail_lengths_match_keyframe_counts() {
        let mut rng = XorShift64(0xDEAD);
        for pattern in [
            FakeoutPattern::CleanLand,
            FakeoutPattern::Overshoot,
            FakeoutPattern::FalseSettle,
        ] {
            let kfs = build_decel_keyframes(
                0,
                50,
                100,
                NATURAL_KEYFRAME_COUNT,
                2800,
                pattern,
                1,
                &mut rng,
            );
            assert_eq!(
                kfs.len(),
                NATURAL_KEYFRAME_COUNT + pattern.tail_count() + 1,
                "pattern {pattern:?} must produce N + tail + terminal keyframes"
            );
        }
    }

    #[test]
    fn false_settle_visits_target_as_non_terminal() {
        // FalseSettle's tail is [target, target+direction] — the first
        // tail keyframe is the iconic "false settle" hold ON target.
        let mut rng = XorShift64(0xBEEF);
        let kfs = build_decel_keyframes(
            0,
            50,
            100,
            NATURAL_KEYFRAME_COUNT,
            2800,
            FakeoutPattern::FalseSettle,
            1,
            &mut rng,
        );
        // Non-terminal keyframe at NATURAL_KEYFRAME_COUNT (first pattern
        // tail entry) should be target itself.
        assert_eq!(
            kfs[NATURAL_KEYFRAME_COUNT].offset, 50,
            "FalseSettle must hold on target as the first tail keyframe"
        );
        // Second tail entry should be target+direction.
        assert_eq!(kfs[NATURAL_KEYFRAME_COUNT + 1].offset, 51);
    }

    #[test]
    fn overshoot_jumps_past_target_then_settles() {
        let mut rng = XorShift64(0xF00D);
        let kfs = build_decel_keyframes(
            0,
            50,
            100,
            NATURAL_KEYFRAME_COUNT,
            2800,
            FakeoutPattern::Overshoot,
            1,
            &mut rng,
        );
        // Single tail keyframe at target+direction; then terminal at target.
        assert_eq!(kfs[NATURAL_KEYFRAME_COUNT].offset, 51);
        assert_eq!(kfs.last().map(|k| k.offset), Some(50));
    }

    #[test]
    fn keyframes_always_land_on_target() {
        for (_pattern, kfs) in sample_builds(42, 100, 60) {
            assert_eq!(
                kfs.last().map(|k| k.offset),
                Some(42),
                "every spin must terminate on target"
            );
        }
    }

    #[test]
    fn keyframes_stay_in_range_across_wrap() {
        // target = 0 — every offset must remain < total_items.
        for (_pattern, kfs) in sample_builds(0, 50, 40) {
            for kf in &kfs {
                assert!(kf.offset < 50, "every keyframe offset must stay in range");
            }
        }
    }

    #[test]
    fn all_three_patterns_appear_across_rolls() {
        // Across 200 rolls each pattern (weights 5/16, 5/16, 6/16) must
        // show up. P(zero of any one) < (10/16)^200 < 1e-39.
        let samples = sample_builds(50, 100, 200);
        let mut saw_clean = false;
        let mut saw_overshoot = false;
        let mut saw_false_settle = false;
        for (p, _) in &samples {
            match p {
                FakeoutPattern::CleanLand => saw_clean = true,
                FakeoutPattern::Overshoot => saw_overshoot = true,
                FakeoutPattern::FalseSettle => saw_false_settle = true,
            }
        }
        assert!(saw_clean, "CleanLand must be rolled across 200 spins");
        assert!(saw_overshoot, "Overshoot must be rolled across 200 spins");
        assert!(
            saw_false_settle,
            "FalseSettle must be rolled across 200 spins"
        );
    }

    #[test]
    fn revolutions_scale_with_list_size() {
        assert_eq!(revolutions_for(5), 6);
        assert_eq!(revolutions_for(20), 4);
        assert_eq!(revolutions_for(500), 3);
    }

    #[test]
    fn cruise_rate_scales_with_library_size() {
        // Bigger libraries cruise faster (more positions per second) so
        // each item still flickers by at a readable rate — the visual
        // velocity of the *items* stays similar rather than the visual
        // velocity of the *whole list*.
        let small = cruise_rate_for(revolutions_for(8), 8);
        let medium = cruise_rate_for(revolutions_for(50), 50);
        let large = cruise_rate_for(revolutions_for(500), 500);
        assert!(small > 0);
        assert!(medium > small);
        assert!(large > medium);
    }

    #[test]
    fn cruise_rate_clamps_at_ceiling_for_massive_libraries() {
        // Without the cap a 13 k-song library would cruise at ~27 k pos/sec,
        // 450 items per 60 Hz frame — perceptually a uniform smear. The
        // cap keeps the cruise readable as motion rather than blur.
        let huge = cruise_rate_for(revolutions_for(13_546), 13_546);
        assert_eq!(huge, super::CRUISE_RATE_MAX_POS_PER_SEC);

        let bigger = cruise_rate_for(revolutions_for(100_000), 100_000);
        assert_eq!(bigger, super::CRUISE_RATE_MAX_POS_PER_SEC);
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
