//! The per-frame tick (`Message::FrameTick`, from `iced::window::frames()`):
//! everything that animates at display refresh. The subscription runs only
//! while one of these steps has something to move (`Nokkvi::frames_wanted`,
//! below); each step still gates itself, so the work is trivial while its
//! feature is idle.

use std::time::Instant;

use iced::Task;
use nokkvi_data::types::player_settings::VisualizationMode;

use crate::{Nokkvi, app_message::Message, state::ChromeMotion};

/// Run every per-frame step. Each one gates itself, so none waits on another's
/// early-out (the Lines boat's, say, when the visualizer is off).
pub(crate) fn handle_frame_tick(app: &mut Nokkvi, now: Instant) -> Task<Message> {
    crate::update::theater::tick(app, now);
    step_now_playing_glow(app, now);
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

/// Drive the now-playing breathing glow off the frame tick so it stays smooth
/// at any display refresh rate (a fixed-interval timer steps visibly on high-Hz
/// displays). Animates in every visualizer mode; frozen while paused/stopped.
fn step_now_playing_glow(app: &Nokkvi, now: Instant) {
    if app.playback.playing && !app.playback.paused {
        let phase = (now.duration_since(app.glow_epoch).as_secs_f32()
            / crate::widgets::slot_list::GLOW_PERIOD_SECS)
            .fract();
        crate::widgets::slot_list::set_now_playing_phase(phase);
    }
}

/// Lyrics column center — the SINGLE publisher, for both kinds of sheet.
/// Animates in every visualizer mode, with NO pause gate.
///
/// Synced: ease toward the active line at display refresh, smoothing the
/// 100 ms position ticks into crossfade-tier motion. An in-flight glide
/// settles to its target even while paused (the retarget only fires on real
/// line changes, which don't happen while paused).
///
/// Plain: a pure function of the last tick's position, the track duration, the
/// line count and the user's wheel offset — so it holds still while paused and
/// jumps whole on a seek or a wheel notch.
fn publish_lyrics_center(app: &Nokkvi, now: Instant) {
    if !app.settings.lyrics_mode.is_active() || app.lyrics.matched_song_id.is_none() {
        return;
    }
    let pos = if app.lyrics.doc.synced {
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
    };
    crate::widgets::lyrics_viewport::set_lyrics_center(pos);
}

impl Nokkvi {
    /// Whether the display-rate frame subscription (`iced::window::frames()`)
    /// is wanted now. A frame message redraws the window and the redraw raises
    /// the next frame message, so left on it redraws at the monitor's refresh
    /// rate while nothing moves (17% of a core, paused on a 240 Hz display).
    /// It is on only while a step above has something to move: the transport
    /// runs (the glow, the lyric clock, the boat), a visualizer mode is on
    /// (its shaders animate on their own clock), Theater Mode's chrome slides
    /// or is due to flip, the Harbour scene shows (its sea breathes while
    /// paused) or a lyrics glide or dissolve is in flight. Everything else
    /// that animates (a hover wash, the centre flash, the marquee, the seek
    /// bar's and the scrollbar's fades, the roulette) requests its own frames,
    /// and the 10 Hz position tick redraws the rest. `subscription()` is
    /// re-read after every update, so a term that changes takes effect at the
    /// next message. A new per-frame step adds its term here.
    pub(crate) fn frames_wanted(&self) -> bool {
        let transport_running = self.playback.playing && !self.playback.paused;
        let visualizer_on = self.settings.visualization_mode != VisualizationMode::Off;
        let now = Instant::now();
        transport_running
            || visualizer_on
            || self.theater_in_motion(now)
            || self.harbour_scene_on_screen()
            || self.lyrics_in_motion(now)
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

    /// A sheet dissolving out, or a synced sheet's glide still easing toward
    /// its line (`eased_center` settles at `anim_duration_ms`; a snap has no
    /// motion), with the layer drawn at all.
    fn lyrics_in_motion(&self, now: Instant) -> bool {
        if !self.settings.lyrics_mode.is_active() {
            return false;
        }
        if self.lyrics.outgoing.is_some() {
            return true;
        }
        self.lyrics.matched_song_id.is_some()
            && self.lyrics.doc.synced
            && self.lyrics.anim_duration_ms > 0
            && self.lyrics.anim_start.is_some_and(|start| {
                now.saturating_duration_since(start).as_millis()
                    < u128::from(self.lyrics.anim_duration_ms)
            })
    }
}
