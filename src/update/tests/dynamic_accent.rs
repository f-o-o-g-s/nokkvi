//! Dynamic accent: the tick's bookkeeping (what is extracted, shown, held and
//! released). The fit itself is pinned in `theme::dynamic_accent`; the look
//! is the owner's to judge in the running app.
//!
//! `theme::set_dynamic_accent` writes process-global theme state, so every
//! test that lets a seed through takes `THEME_MODE_LOCK` and leaves the
//! global clear.

use iced::widget::image::Handle;

use super::SSE_SLOT_TEST_LOCK;
use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, Message, PlaybackMessage},
    state::ACCENT_HOLD_TICKS,
    test_helpers::*,
    theme::{self, AccentSeed, CoverPalette, THEME_MODE_LOCK},
};

fn png_of(rgb: [u8; 3]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 4, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

fn seed() -> AccentSeed {
    AccentSeed {
        lightness: 0.6,
        chroma: 0.2,
        hue: 0.5,
    }
}

fn tick(app: &mut Nokkvi) {
    let _ = app.update(Message::Playback(PlaybackMessage::Tick));
}

fn extracted(
    app: &mut Nokkvi,
    owner: &str,
    source: iced::advanced::image::Id,
    seed: Option<AccentSeed>,
) {
    let _ = app.update(Message::Artwork(ArtworkMessage::AccentExtracted {
        owner: owner.to_string(),
        source,
        palette: seed.map(CoverPalette::single),
    }));
}

/// A playing app with the setting on: songs s1 / s2 queued, s1 current, and
/// s1's album mini cached.
fn accent_app() -> (Nokkvi, iced::advanced::image::Id) {
    let mut app = test_app();
    app.settings.dynamic_accent = true;
    app.playback.playing = true;
    app.library.queue_songs = vec![
        make_queue_song("s1", "T", "A", "Al"),
        make_queue_song("s2", "T2", "A", "Al2"),
    ];
    app.scrobble.current_song_id = Some("s1".to_string());
    let handle = Handle::from_bytes(png_of([200, 30, 30]));
    let id = handle.id();
    app.artwork.album_art.put("album_s1".to_string(), handle);
    (app, id)
}

/// `accent_app` with s1's seed extracted and on screen.
fn shown_app() -> (Nokkvi, iced::advanced::image::Id) {
    let (mut app, id) = accent_app();
    tick(&mut app);
    extracted(&mut app, "album_s1", id, Some(seed()));
    tick(&mut app);
    assert!(
        app.dynamic_accent.applied,
        "fixture: the accent is on screen"
    );
    (app, id)
}

#[test]
fn the_setting_off_does_no_work() {
    let (mut app, _id) = accent_app();
    app.settings.dynamic_accent = false;
    tick(&mut app);
    assert_eq!(app.dynamic_accent.pending, None);
    assert_eq!(app.dynamic_accent.shown, None);
    assert!(!app.dynamic_accent.applied);
}

#[test]
fn the_playing_cover_is_read_once() {
    let (mut app, id) = accent_app();
    tick(&mut app);
    assert_eq!(
        app.dynamic_accent.pending,
        Some(("album_s1".to_string(), id)),
        "extraction requested"
    );
    tick(&mut app);
    assert_eq!(
        app.dynamic_accent.pending,
        Some(("album_s1".to_string(), id)),
        "requested once, not per tick"
    );
    assert!(
        !app.dynamic_accent.applied,
        "nothing shown before the seed lands"
    );
}

#[test]
fn an_extracted_seed_becomes_the_accent() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, id) = accent_app();
    tick(&mut app);
    extracted(&mut app, "album_s1", id, Some(seed()));
    assert_eq!(app.dynamic_accent.pending, None);
    tick(&mut app);
    assert!(app.dynamic_accent.applied);
    assert_eq!(
        app.dynamic_accent.shown,
        Some(("album_s1".to_string(), Some(id)))
    );
    assert_eq!(theme::dynamic_accent_seed(), Some(seed()));
    tick(&mut app);
    assert_eq!(app.dynamic_accent.pending, None, "not read again");

    theme::set_dynamic_accent(None);
}

#[test]
fn a_colorless_cover_keeps_the_theme_accent() {
    let (mut app, id) = accent_app();
    tick(&mut app);
    extracted(&mut app, "album_s1", id, None);
    tick(&mut app);
    assert!(!app.dynamic_accent.applied);
    assert_eq!(
        app.dynamic_accent.shown,
        Some(("album_s1".to_string(), Some(id))),
        "settled, so the cover is not read again"
    );
    tick(&mut app);
    assert_eq!(app.dynamic_accent.pending, None);
}

/// A skip can pass through a stopped state for a tick or two; the accent
/// holds through it, and only a real stop returns to the theme accent.
#[test]
fn stopping_returns_to_the_theme_accent_after_a_hold() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    app.playback.playing = false;
    app.playback.paused = false;
    for _ in 0..ACCENT_HOLD_TICKS {
        tick(&mut app);
        assert!(app.dynamic_accent.applied, "held through a brief stop");
    }
    tick(&mut app);
    assert!(!app.dynamic_accent.applied);
    assert_eq!(app.dynamic_accent.shown, None);
    assert_eq!(theme::dynamic_accent_seed(), None);
}

#[test]
fn pausing_keeps_the_accent() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    app.playback.playing = false;
    app.playback.paused = true;
    tick(&mut app);
    assert!(app.dynamic_accent.applied);

    theme::set_dynamic_accent(None);
}

/// A stop shorter than the hold, then play again: the accent never left.
#[test]
fn a_brief_stop_between_tracks_keeps_the_accent() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    app.playback.playing = false;
    tick(&mut app);
    tick(&mut app);
    app.playback.playing = true;
    tick(&mut app);
    assert!(app.dynamic_accent.applied);
    assert_eq!(app.dynamic_accent.idle_ticks, 0);

    theme::set_dynamic_accent(None);
}

/// Turning the setting off is immediate, with no hold.
#[test]
fn turning_the_setting_off_returns_to_the_theme_accent() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    app.settings.dynamic_accent = false;
    tick(&mut app);
    assert!(!app.dynamic_accent.applied);
    assert_eq!(theme::dynamic_accent_seed(), None);
}

/// A new track whose cover has not arrived keeps the previous accent for a
/// moment (one change, not two), and falls back to the theme accent if none
/// shows up. It requests nothing: playback already warms that cover.
#[test]
fn a_track_without_art_holds_then_falls_back() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    app.scrobble.current_song_id = Some("s2".to_string());
    for _ in 0..ACCENT_HOLD_TICKS {
        tick(&mut app);
        assert!(
            app.dynamic_accent.applied,
            "held while the cover may still arrive"
        );
    }
    tick(&mut app);
    assert!(
        !app.dynamic_accent.applied,
        "no cover: back to the theme accent"
    );
    assert_eq!(
        app.dynamic_accent.shown,
        Some(("album_s2".to_string(), None))
    );
    assert_eq!(theme::dynamic_accent_seed(), None);
}

#[test]
fn a_cover_arriving_during_the_hold_is_read() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    app.scrobble.current_song_id = Some("s2".to_string());
    tick(&mut app);
    let handle = Handle::from_bytes(png_of([30, 60, 200]));
    let id2 = handle.id();
    app.artwork.album_art.put("album_s2".to_string(), handle);
    tick(&mut app);
    assert_eq!(
        app.dynamic_accent.pending,
        Some(("album_s2".to_string(), id2))
    );
    assert!(
        app.dynamic_accent.applied,
        "the old accent stays until the new seed lands"
    );

    theme::set_dynamic_accent(None);
}

#[test]
fn a_replayed_album_recolors_from_the_cache() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, id) = shown_app();
    // Move to s2 (no art) and let the hold run out.
    app.scrobble.current_song_id = Some("s2".to_string());
    for _ in 0..=ACCENT_HOLD_TICKS {
        tick(&mut app);
    }
    assert!(!app.dynamic_accent.applied);
    // Back to s1: its seed is remembered.
    app.scrobble.current_song_id = Some("s1".to_string());
    tick(&mut app);
    assert!(app.dynamic_accent.applied, "recolored on the first tick");
    assert_eq!(app.dynamic_accent.pending, None, "without a second read");
    assert_eq!(
        app.dynamic_accent.shown,
        Some(("album_s1".to_string(), Some(id)))
    );

    theme::set_dynamic_accent(None);
}

/// The large cover landing after the mini was read must not re-read the same
/// album and nudge the accent mid-track.
#[test]
fn a_second_size_of_the_same_cover_is_not_read_again() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, id) = shown_app();
    app.artwork.large_artwork.put(
        "album_s1".to_string(),
        Handle::from_bytes(png_of([200, 30, 30])),
    );
    tick(&mut app);
    assert_eq!(app.dynamic_accent.pending, None);
    assert_eq!(
        app.dynamic_accent.shown,
        Some(("album_s1".to_string(), Some(id)))
    );

    theme::set_dynamic_accent(None);
}

/// A refreshed cover (a new handle replacing the one the seed came from) is
/// read again.
#[test]
fn a_replaced_cover_is_read_again() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    let fresh = Handle::from_bytes(png_of([30, 200, 60]));
    let fresh_id = fresh.id();
    app.artwork.album_art.put("album_s1".to_string(), fresh);
    tick(&mut app);
    assert_eq!(
        app.dynamic_accent.pending,
        Some(("album_s1".to_string(), fresh_id))
    );

    theme::set_dynamic_accent(None);
}

/// A result for a track that is no longer playing is remembered, not shown.
#[test]
fn a_late_result_for_another_track_is_not_shown() {
    let (mut app, id) = accent_app();
    tick(&mut app);
    app.scrobble.current_song_id = Some("s2".to_string());
    extracted(&mut app, "album_s1", id, Some(seed()));
    tick(&mut app);
    assert!(!app.dynamic_accent.applied);
    assert!(app.dynamic_accent.seeds.contains("album_s1"));
}

#[test]
fn logout_clears_the_accent_and_its_cache() {
    let _sse = SSE_SLOT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    let _ = app.reset_session_state();
    assert!(!app.dynamic_accent.applied);
    assert_eq!(app.dynamic_accent.shown, None);
    assert!(app.dynamic_accent.seeds.is_empty());
    assert_eq!(theme::dynamic_accent_seed(), None);
}

/// "Visualizer From Album Art" alone: the cover is read and the visualizer
/// colors follow it, while the accent stays the theme's.
#[test]
fn the_visualizer_can_follow_the_cover_without_the_accent() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, id) = accent_app();
    app.settings.dynamic_accent = false;
    app.settings.dynamic_visualizer = true;
    tick(&mut app);
    assert_eq!(
        app.dynamic_accent.pending,
        Some(("album_s1".to_string(), id)),
        "the cover is read for the visualizer alone"
    );
    extracted(&mut app, "album_s1", id, Some(seed()));
    tick(&mut app);
    assert!(app.dynamic_accent.applied);
    assert!(
        !theme::dynamic_accent_active(),
        "the accent stays the theme's"
    );
    assert!(theme::cover_milkdrop().is_some(), "the visualizer follows");

    theme::set_dynamic_accent(None);
}

/// Flipping either switch while a track plays re-applies what is on screen
/// at once, without reading the cover again; both off returns to the theme.
#[test]
fn flipping_a_switch_reapplies_without_a_new_read() {
    let _guard = THEME_MODE_LOCK.lock();
    let (mut app, _id) = shown_app();
    assert!(theme::cover_milkdrop().is_none(), "fixture: accent only");

    app.settings.dynamic_visualizer = true;
    tick(&mut app);
    assert_eq!(app.dynamic_accent.pending, None, "no second read");
    assert!(theme::dynamic_accent_active());
    assert!(theme::cover_milkdrop().is_some());

    app.settings.dynamic_accent = false;
    tick(&mut app);
    assert!(!theme::dynamic_accent_active());
    assert!(theme::cover_milkdrop().is_some());
    assert!(app.dynamic_accent.applied);

    app.settings.dynamic_visualizer = false;
    tick(&mut app);
    assert!(!app.dynamic_accent.applied);
    assert!(theme::cover_milkdrop().is_none());
}
