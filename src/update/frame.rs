//! The per-frame tick (`Message::FrameTick`, from `iced::window::frames()`):
//! everything that animates at display refresh. The subscription is always
//! on, so each step stays cheap while its feature is idle.

use std::time::Instant;

use iced::Task;

use crate::{Nokkvi, app_message::Message};

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
    publish_lyrics_center(app, now);
    super::boat::step_boat(app, now);
    Task::none()
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
    if !app.settings.lyrics_enabled || app.lyrics.matched_song_id.is_none() {
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
