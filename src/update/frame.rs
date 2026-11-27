//! The per-frame tick (`Message::FrameTick`, from `iced::window::frames()`):
//! everything that animates at display refresh. The subscription runs only
//! while one of these steps has something to move (`Nokkvi::frames_wanted`,
//! below); each step still gates itself, so the work is trivial while its
//! feature is idle.

use std::time::Instant;

use iced::Task;
use nokkvi_data::types::player_settings::VisualizationMode;

use crate::{
    Nokkvi,
    app_message::Message,
    state::{
        AMBIENT_IDLE_AFTER, AMBIENT_INPUT_REARM, ChromeMotion, TICK_IDLE, TICK_LIVELY,
        TICK_LIVELY_HOLD, VISUALIZER_SETTLE,
    },
};

/// Run every per-frame step. Each one gates itself, so none waits on another's
/// early-out (the Lines boat's, say, when the visualizer is off).
pub(crate) fn handle_frame_tick(app: &mut Nokkvi, now: Instant) -> Task<Message> {
    app.stamp_transport(now);
    crate::update::theater::tick(app, now);
    // The Harbour Trawl scene's sea is procedural (a pure function of a phase
    // this step advances), so it is independent of the visualizer mode, the
    // `lines.boat` toggle, AND the audio-pause freeze: the scene keeps
    // breathing while the player is paused or stopped.
    super::boat::step_harbour_scene(app, now);
    step_lyrics_clock(app, now);
    publish_lyrics_center(app, now);
    super::boat::step_boat(app, now);
    Task::none()
}

/// Run the lyric clock on from the last position tick — the per-frame
/// playhead the word-level highlight reads (`LyricsState::clock_ms`). The
/// 10 Hz position tick anchors it; this step interpolates between anchors
/// while the transport runs, holds while paused, and never steps backwards
/// for a lagging tick (see `lyric_clock_ms`). Gated like the center: only a
/// matched sheet has a clock worth running.
fn step_lyrics_clock(app: &mut Nokkvi, now: Instant) {
    if !app.settings.lyrics_mode.is_active() || app.lyrics.matched_song_id.is_none() {
        return;
    }
    app.lyrics.clock_ms = crate::state::lyric_clock_ms(
        app.lyrics.clock_ms,
        app.lyrics.position_ms,
        app.lyrics.clock_anchor,
        app.lyrics.clock_running,
        now,
    );
}

/// Lyrics column center — the SINGLE publisher, for both kinds of sheet.
/// Animates in every visualizer mode, with NO pause gate.
///
/// Synced: ease toward the active line at display refresh, smoothing the
/// 100 ms position ticks into crossfade-tier motion. An in-flight glide
/// settles to its target even while paused, and a paused seek's snap lands
/// too: `frames_wanted` keeps frames coming until the published center
/// reaches [`lyrics_center_target`].
///
/// Plain: a pure function of the last tick's position, the track duration, the
/// line count and the user's wheel offset — so it holds still while paused and
/// jumps whole on a seek or a wheel notch.
fn publish_lyrics_center(app: &Nokkvi, now: Instant) {
    if let Some(pos) = lyrics_center_target(app, now) {
        crate::widgets::lyrics_viewport::set_lyrics_center(pos);
    }
}

/// Where the lyrics column center belongs at `now`, or `None` with no sheet to
/// place. [`publish_lyrics_center`] writes it; `frames_wanted` compares it with
/// the published one, so a jump that arrives while no frame runs (a paused
/// seek's snap, a wheel notch on a paused plain sheet) still gets its frame.
fn lyrics_center_target(app: &Nokkvi, now: Instant) -> Option<f32> {
    if !app.settings.lyrics_mode.is_active() || app.lyrics.matched_song_id.is_none() {
        return None;
    }
    Some(if app.lyrics.doc.synced {
        crate::widgets::lyrics_viewport::eased_center(
            app.lyrics.scroll_from,
            app.lyrics.scroll_to,
            app.lyrics.anim_start,
            app.lyrics.anim_duration_ms,
            now,
        )
    } else {
        crate::widgets::lyrics_viewport::drift_center(
            app.lyrics.position_ms,
            // The tick reports whole seconds; 0 means unknown, which
            // `drift_center` parks on rather than dividing by.
            app.playback.duration.saturating_mul(1000),
            app.lyrics.doc.lines.len(),
            app.lyrics.drift_offset,
        )
    })
}

/// How far (in lines) the published center may sit from its target and still
/// count as landed: well under a pixel at any row height, above f32 noise.
const LYRICS_CENTER_LANDED: f32 = 1e-3;

/// Frame pacing for the ambient Harbour scene: 60 fps.
pub(crate) const AMBIENT_FRAME: std::time::Duration = std::time::Duration::from_micros(16_667);

/// What the frame subscription runs ([`Nokkvi::frame_demand`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameDemand {
    /// Nothing moves: no frame messages.
    None,
    /// Only the ambient Harbour scene moves: a 60 fps timer.
    Ambient,
    /// Something tracks the music or the hand: every display frame.
    Display,
}

impl Nokkvi {
    /// Whether the display-rate frame subscription (`iced::window::frames()`)
    /// is wanted now. A frame message redraws the window and the redraw raises
    /// the next frame message, so left on it redraws at the monitor's refresh
    /// rate while nothing moves (17% of a core, paused on a 240 Hz display).
    /// It is on only while a step above has something to move: the transport
    /// runs (the glow, the lyric clock, the boat), a visualizer mode is on
    /// and the transport ran within its settle window (its shaders animate on
    /// their own clock, so a paused or stopped visualizer freezes once the
    /// bars have fallen), Theater Mode's chrome slides or is due to flip, the
    /// Harbour scene shows with the music playing or a present user (its sea
    /// breathes while paused, until the window loses focus or sits a minute
    /// without input), a lyrics glide or dissolve is in flight, or the lyrics
    /// column has a jump still to land (a paused seek). Everything else
    /// that animates (a hover wash, the centre flash, the marquee, the seek
    /// bar's and the scrollbar's fades, the roulette) requests its own frames,
    /// and the 10 Hz position tick redraws the rest. `subscription()` is
    /// re-read after every update, so a term that changes takes effect at the
    /// next message. A new per-frame step adds its term here.
    pub(crate) fn frames_wanted(&self) -> bool {
        self.frame_demand() != FrameDemand::None
    }

    /// Which frame source the subscription runs (`subscription()`): the
    /// display's own frames for anything that tracks the music or the hand
    /// (a visualizer, the lyric clock, Theater Mode, a lyrics glide, a Cover
    /// Beat swell), or a [`AMBIENT_FRAME`] timer when the Harbour scene is
    /// the only thing moving. Each frame rebuilds the whole view, and the
    /// ambient sea reads the same at 60 fps on a 120 or 240 Hz display.
    pub(crate) fn frame_demand(&self) -> FrameDemand {
        let now = Instant::now();
        if self.lyric_clock_running()
            || self.visualizer_settling(now)
            || self.theater_in_motion(now)
            || self.lyrics_in_motion(now)
        {
            FrameDemand::Display
        } else if self.harbour_scene_breathing(now) {
            FrameDemand::Ambient
        } else {
            FrameDemand::None
        }
    }

    /// A matched lyric sheet follows a running transport where it is drawn
    /// (the Queue cover or Theater Mode, with the layer shown): its word-level
    /// highlight reads the per-frame clock. Off that surface nothing reads
    /// it, and the clock is a function of the position tick's anchor, so it
    /// is right again on the first frame back. Nothing else needs a view
    /// rebuild per frame just because music plays: the seek bar, the marquee
    /// and the now-playing glow pace their own redraws, the visualizer and
    /// the Harbour scene have their own terms, and the Lines boat rides the
    /// visualizer's.
    fn lyric_clock_running(&self) -> bool {
        self.transport_running()
            && self.lyrics.matched_song_id.is_some()
            && self.lyrics_surface_visible()
            && self.lyrics_layer_wanted()
    }

    /// The transport is playing (not paused, not stopped).
    pub(crate) fn transport_running(&self) -> bool {
        self.playback.playing && !self.playback.paused
    }

    /// Note that the transport runs at `now`, for the idle gates below. Called
    /// from the frame step and the 10 Hz tick, so the stamp is at most a tick
    /// old when the transport stops.
    pub(crate) fn stamp_transport(&mut self, now: Instant) {
        if self.transport_running() {
            self.idle.transport_seen = Some(now);
        }
    }

    /// A visualizer mode is on and the transport ran within
    /// [`VISUALIZER_SETTLE`]: the bars and peaks fall and a swell settles
    /// after a pause or stop, then the picture holds still (MilkDrop
    /// included) until playback resumes, so a paused app costs no frames.
    fn visualizer_settling(&self, now: Instant) -> bool {
        self.settings.visualization_mode != VisualizationMode::Off
            && (self.transport_running() || self.idle.transport_within(now, VISUALIZER_SETTLE))
    }

    /// The Harbour scene shows and someone is there to watch it: the music
    /// plays, or the window has focus and saw input within
    /// [`AMBIENT_IDLE_AFTER`]. Otherwise the sea holds its last frame and
    /// picks up from it on the next input (its step clamps the gap).
    fn harbour_scene_breathing(&self, now: Instant) -> bool {
        self.harbour_scene_on_screen()
            && (self.transport_running()
                || (self.theater.window_focused && self.idle.input_within(now, AMBIENT_IDLE_AFTER)))
    }

    /// The 10 Hz tick's period: [`TICK_LIVELY`] while the transport runs or
    /// ran within [`TICK_LIVELY_HOLD`], any message other than the idle poll
    /// arrived within it, frames run or a toast shows; [`TICK_IDLE`]
    /// otherwise. Whatever starts playback (a click, a hotkey, MPRIS, IPC)
    /// arrives as a message, so the slow tick never delays a start.
    pub(crate) fn tick_interval(&self) -> std::time::Duration {
        let now = Instant::now();
        let lively = self.transport_running()
            || self.idle.transport_within(now, TICK_LIVELY_HOLD)
            || self.idle.event_within(now, TICK_LIVELY_HOLD)
            || self.frames_wanted()
            || self.toast.current().is_some();
        if lively { TICK_LIVELY } else { TICK_IDLE }
    }

    /// Whether the ambient-input listener runs: the Harbour scene could hold
    /// for idleness (it shows, nothing plays) and the last input is older than
    /// [`AMBIENT_INPUT_REARM`], so fresh input never raises a message per
    /// mouse move.
    pub(crate) fn ambient_input_wanted(&self) -> bool {
        self.harbour_scene_on_screen()
            && !self.transport_running()
            && !self.idle.input_within(Instant::now(), AMBIENT_INPUT_REARM)
    }

    /// Theater Mode's chrome sliding, or due to flip: `theater::tick` flips it
    /// when its target (hovered, a menu open, recent activity under Auto-Hide)
    /// disagrees with where it is, and the slide then eases for
    /// `SLIDE_DURATION`. A bar at rest, shown or hidden, needs no frame; the
    /// 10 Hz tick re-reads the subscription, so a hide deadline is met within
    /// a tick, and a mouse move raises its own update.
    fn theater_in_motion(&self, now: Instant) -> bool {
        if !self.theater.active {
            return false;
        }
        let chrome = self.theater.chrome;
        let since = match chrome {
            ChromeMotion::Shown { since, .. } | ChromeMotion::Hidden { since, .. } => since,
        };
        let sliding = now.saturating_duration_since(since) < super::theater::SLIDE_DURATION;
        let flip_due = super::theater::chrome_target(
            &self.theater,
            now,
            self.open_menu.is_some(),
            self.theater_controls(),
        ) != chrome.is_shown();
        sliding || flip_due
    }

    /// A sheet dissolving out, a synced sheet's glide still easing toward
    /// its line (`eased_center` settles at `anim_duration_ms`), or a center
    /// the frame tick has yet to publish: a snap has no motion, but while
    /// paused with the visualizer off its one frame comes from here (the sheet
    /// held the old line through every paused seek until playback resumed).
    fn lyrics_in_motion(&self, now: Instant) -> bool {
        // Off the surface nothing draws the column: a glide or snap that
        // happens there lands on the first frame back.
        if !self.settings.lyrics_mode.is_active() || !self.lyrics_surface_visible() {
            return false;
        }
        if self.lyrics.outgoing.is_some() {
            return true;
        }
        let gliding = self.lyrics.matched_song_id.is_some()
            && self.lyrics.doc.synced
            && self.lyrics.anim_duration_ms > 0
            && self.lyrics.anim_start.is_some_and(|start| {
                now.saturating_duration_since(start).as_millis()
                    < u128::from(self.lyrics.anim_duration_ms)
            });
        gliding
            || lyrics_center_target(self, now).is_some_and(|target| {
                (target - crate::widgets::lyrics_viewport::lyrics_center_pos()).abs()
                    > LYRICS_CENTER_LANDED
            })
    }
}
