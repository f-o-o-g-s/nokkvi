//! Observable-state tests for the synced-lyrics pipeline: active-line
//! tracking (incl. pre-roll), the song-change promote-or-clear, and the
//! stale-load guard. `app_service` is `None` in `test_app`, so `shell_task`
//! dispatches are inert — results are driven synthetically via
//! `handle_lyrics_loader`, exactly as the async pipeline would deliver them.

use std::sync::Arc;

use nokkvi_data::types::lyrics::{LrcDocument, LrcLine, LyricsIndex};

use crate::{
    app_message::{LyricsLoaderMessage, PlaybackStateUpdate},
    test_helpers::*,
};

fn timed_doc(times_ms: &[u32]) -> LrcDocument {
    LrcDocument {
        lines: times_ms
            .iter()
            .map(|&time_ms| LrcLine {
                time_ms,
                text: format!("line@{time_ms}"),
                words: vec![],
            })
            .collect(),
        synced: true,
    }
}

fn update_for(song_id: &str, position_ms: u32) -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        position: position_ms / 1000,
        position_ms,
        duration: 200,
        playing: true,
        paused: false,
        title: "Test Song".to_string(),
        artist: "Test Artist".to_string(),
        album: "Test Album".to_string(),
        art_url: None,
        random: false,
        repeat: false,
        repeat_queue: false,
        consume: false,
        current_index: Some(0),
        current_entry_id: Some(0),
        song_id: Some(song_id.to_string()),
        format_suffix: "flac".to_string(),
        sample_rate: 44100,
        current_stream_bit_perfect: false,
        bitrate: 1411,
        live_icy_metadata: None,
        bpm: None,
    }
}

/// Seed a resolved doc for `song_id` as the currently-matched track.
fn seed_matched(app: &mut crate::Nokkvi, song_id: &str, doc: LrcDocument) {
    app.lyrics.enabled = true;
    app.lyrics.doc = doc;
    app.lyrics.matched_song_id = Some(song_id.to_string());
    app.scrobble.current_song_id = Some(song_id.to_string());
}

#[test]
fn active_line_tracks_position_with_pre_roll() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[5_000, 10_000, 15_000]));

    // Pre-roll: before the first timestamp NOTHING is active (221 corpus
    // files start >60s in — highlighting line 1 early would be dishonest).
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_000));
    assert_eq!(app.lyrics.active_index, None);

    let _ = app.handle_playback_state_updated(update_for("song_1", 5_000));
    assert_eq!(app.lyrics.active_index, Some(0));

    let _ = app.handle_playback_state_updated(update_for("song_1", 12_345));
    assert_eq!(app.lyrics.active_index, Some(1));
    assert_eq!(app.lyrics.position_ms, 12_345);

    // Within the same line: unchanged.
    let _ = app.handle_playback_state_updated(update_for("song_1", 14_999));
    assert_eq!(app.lyrics.active_index, Some(1));

    let _ = app.handle_playback_state_updated(update_for("song_1", 15_000));
    assert_eq!(app.lyrics.active_index, Some(2));
}

#[test]
fn song_change_cold_path_clears() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));
    app.lyrics.active_index = Some(0);
    let epoch_before = app.lyrics.load_epoch;

    // No prefetch parked → the transition clears synchronously.
    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.active_index, None);
    assert_eq!(app.lyrics.matched_song_id, None);
    assert_ne!(
        app.lyrics.load_epoch, epoch_before,
        "clear must bump the epoch"
    );
}

#[test]
fn song_change_promotes_pending_next() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));
    app.lyrics.pending_next = Some(("song_2".to_string(), timed_doc(&[0, 2_000])));

    // The prefetched doc swaps in synchronously — no blank gap, and the
    // recompute in the same pass lands the active line for the new track.
    let _ = app.handle_playback_state_updated(update_for("song_2", 100));
    assert_eq!(app.lyrics.matched_song_id.as_deref(), Some("song_2"));
    assert_eq!(app.lyrics.doc.lines.len(), 2);
    assert_eq!(app.lyrics.active_index, Some(0));
    assert!(app.lyrics.pending_next.is_none());
}

#[test]
fn loaded_applies_for_current() {
    let mut app = test_app();
    app.lyrics.enabled = true;
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.lyrics.position_ms = 6_000;

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[5_000, 10_000])),
        epoch: app.lyrics.load_epoch,
    });
    assert_eq!(app.lyrics.matched_song_id.as_deref(), Some("song_1"));
    assert_eq!(app.lyrics.doc.lines.len(), 2);
    // Active line recomputed immediately from the stored position.
    assert_eq!(app.lyrics.active_index, Some(0));
}

#[test]
fn stale_load_rejected_wrong_song() {
    let mut app = test_app();
    app.lyrics.enabled = true;
    app.scrobble.current_song_id = Some("song_2".to_string());

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[5_000])),
        epoch: app.lyrics.load_epoch,
    });
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.matched_song_id, None);
}

#[test]
fn stale_load_rejected_wrong_epoch() {
    let mut app = test_app();
    app.lyrics.enabled = true;
    app.scrobble.current_song_id = Some("song_1".to_string());

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[5_000])),
        epoch: app.lyrics.load_epoch.wrapping_add(1),
    });
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.matched_song_id, None);
}

#[test]
fn unsynced_doc_is_no_match() {
    let mut app = test_app();
    app.lyrics.enabled = true;
    app.scrobble.current_song_id = Some("song_1".to_string());

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(LrcDocument {
            lines: vec![LrcLine {
                time_ms: 0,
                text: "plain".into(),
                words: vec![],
            }],
            synced: false,
        }),
        epoch: app.lyrics.load_epoch,
    });
    // Identity recorded (no re-fire loop), doc honestly empty.
    assert_eq!(app.lyrics.matched_song_id.as_deref(), Some("song_1"));
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.active_index, None);
}

#[test]
fn prefetch_parks_only_non_current_synced() {
    let mut app = test_app();
    app.lyrics.enabled = true;
    app.scrobble.current_song_id = Some("song_1".to_string());

    // A prefetch for the CURRENT track is useless — dropped.
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[0])),
    });
    assert!(app.lyrics.pending_next.is_none());

    // A synced doc for the NEXT track parks.
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: "song_2".to_string(),
        doc: Box::new(timed_doc(&[0])),
    });
    assert_eq!(
        app.lyrics.pending_next.as_ref().map(|(id, _)| id.as_str()),
        Some("song_2")
    );
}

#[test]
fn index_ready_sets_index() {
    let mut app = test_app();
    assert!(app.lyrics.index.is_none());
    let _ = app.handle_lyrics_index_ready(Arc::new(LyricsIndex::default()));
    assert!(app.lyrics.index.is_some());
}

#[test]
fn over_cover_viz_coexists_with_lyrics() {
    // The visualizer must NOT yield to the lyrics layer (owner-directed: both
    // hero surfaces render at once — the panel stacks the viz between the
    // lyrics scrim and the haloed text). Seed a REAL visualizer + an
    // over-cover mode (in `test_app` the ctor leaves `visualizer = None`, so
    // a naive assertion would pass vacuously).
    let mut app = test_app();
    app.visualizer = Some(crate::widgets::visualizer::Visualizer::new(
        192,
        app.visualizer_config.clone(),
    ));
    app.engine.visualization_mode = nokkvi_data::types::player_settings::VisualizationMode::Scope;
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));

    let (viz, _boat) = app.over_cover_overlays();
    assert!(
        viz.is_some(),
        "over-cover visualizer must render alongside active lyrics"
    );
}

#[test]
fn queue_lyrics_panel_data_gated_on_enabled() {
    let mut app = test_app();
    assert!(app.queue_lyrics_panel_data().is_none());

    app.lyrics.enabled = true;
    let data = app
        .queue_lyrics_panel_data()
        .expect("enabled + queue playback");
    // Empty doc → the faded no-match message is set.
    assert!(data.empty_message.is_some());
    assert!(data.lines.is_empty());

    app.lyrics.doc = timed_doc(&[1_000]);
    app.lyrics.active_index = Some(0);
    let data = app.queue_lyrics_panel_data().expect("still enabled");
    assert_eq!(data.lines.len(), 1);
    assert!(data.empty_message.is_none());
    assert_eq!(data.active_index, Some(0));
}

#[test]
fn toggle_lyrics_flips_live_mirror() {
    // The arm must flip the LIVE mirror synchronously — the async persist is
    // inert in test_app (app_service is None), so asserting the persisted
    // setting here would be unreachable by design.
    let mut app = test_app();
    assert!(!app.lyrics.enabled);

    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
    assert!(app.lyrics.enabled);
    assert!(
        app.settings.lyrics_enabled,
        "live settings union mirrors the flip"
    );

    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
    assert!(!app.lyrics.enabled);
}

#[test]
fn toggle_lyrics_off_clears_doc() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));
    assert!(!app.lyrics.doc.lines.is_empty());

    // enabled=true (from seed) → toggling turns lyrics OFF and clears.
    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
    assert!(!app.lyrics.enabled);
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.matched_song_id, None);
}

// ---------------------------------------------------------------------------
// Motion (C2): the boat tick publishes the eased center to a process-global
// atomic, so these tests serialize on a local lock and reset it in setup —
// cargo runs tests concurrently in one process and unserialized global-atomic
// tests are flaky by construction (the THEME_MODE_LOCK precedent).
// ---------------------------------------------------------------------------

static LYRICS_MOTION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn motion_setup(app: &mut crate::Nokkvi, times_ms: &[u32]) {
    seed_matched(app, "song_1", timed_doc(times_ms));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
}

#[test]
fn glide_advances_on_boat_tick_regardless_of_visualizer_mode() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    motion_setup(&mut app, &[1_000, 2_000, 3_000]);
    // Not in Lines mode (test_app default is Bars) — the boat's own
    // `visible` early-return fires, so this proves the lyrics glide runs
    // BEFORE the boat's mode early-outs rather than riding on them.
    assert_ne!(
        app.engine.visualization_mode,
        nokkvi_data::types::player_settings::VisualizationMode::Lines
    );

    // Land on line 0 (pre-roll → snap), then advance to line 1 (glide).
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_000));
    let now = std::time::Instant::now();
    let _ = crate::update::boat::handle_boat_tick(&mut app, now);
    assert_eq!(crate::widgets::lyrics_viewport::lyrics_center_pos(), 0.0);

    let _ = app.handle_playback_state_updated(update_for("song_1", 2_000));
    assert!(app.lyrics.anim_duration_ms > 0, "one-line advance glides");
    let start = app.lyrics.anim_start.expect("glide armed");

    // Mid-glide: strictly between the endpoints.
    let mid = start + std::time::Duration::from_millis(u64::from(app.lyrics.anim_duration_ms) / 4);
    let _ = crate::update::boat::handle_boat_tick(&mut app, mid);
    let pos = crate::widgets::lyrics_viewport::lyrics_center_pos();
    assert!(
        pos > 0.0 && pos < 1.0,
        "mid-glide center must be between lines, got {pos}"
    );

    // Past the duration (even if playback paused meanwhile): settled on target.
    let done =
        start + std::time::Duration::from_millis(u64::from(app.lyrics.anim_duration_ms) + 50);
    let _ = crate::update::boat::handle_boat_tick(&mut app, done);
    assert_eq!(crate::widgets::lyrics_viewport::lyrics_center_pos(), 1.0);
}

#[test]
fn seek_sized_jump_snaps_first_tick() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    motion_setup(
        &mut app,
        &[1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000, 8_000],
    );

    let _ = app.handle_playback_state_updated(update_for("song_1", 1_000));
    let _ = crate::update::boat::handle_boat_tick(&mut app, std::time::Instant::now());
    // Jump 0 → 7 (delta 7 > 4): snap — the very next tick lands on target.
    let _ = app.handle_playback_state_updated(update_for("song_1", 8_000));
    assert_eq!(app.lyrics.anim_duration_ms, 0, "seek-sized jump must snap");
    let _ = crate::update::boat::handle_boat_tick(&mut app, std::time::Instant::now());
    assert_eq!(crate::widgets::lyrics_viewport::lyrics_center_pos(), 7.0);
}

#[test]
fn fast_line_gap_caps_glide_duration() {
    // Observable purely via state — no atomics needed. Lines 200ms apart must
    // glide well under the settle time so dense passages never smear.
    let lines = timed_doc(&[1_000, 1_200, 1_400]).lines;
    let capped = crate::update::lyrics::lyrics_glide_duration(&lines, 0);
    assert!(capped < crate::update::lyrics::LYRICS_SETTLE_MS);
    assert!(capped >= crate::update::lyrics::LYRICS_MIN_GLIDE_MS);
    // Last line (no next): full settle.
    assert_eq!(
        crate::update::lyrics::lyrics_glide_duration(&lines, 2),
        crate::update::lyrics::LYRICS_SETTLE_MS
    );
}

#[test]
fn crossfade_transition_parks_outgoing_sheet() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    app.engine.crossfade_enabled = true;
    app.engine.crossfade_duration_secs = 7;

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    let outgoing = app
        .lyrics
        .outgoing
        .as_ref()
        .expect("old sheet parked for the dissolve");
    assert_eq!(outgoing.doc.lines.len(), 2);
    assert_eq!(outgoing.duration_ms, 7_000);
    // The current doc cleared for the cold-path resolve underneath.
    assert!(app.lyrics.doc.lines.is_empty());
}

#[test]
fn no_dissolve_when_crossfade_off() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));
    app.engine.crossfade_enabled = false;

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(
        app.lyrics.outgoing.is_none(),
        "hard swap when crossfade is off"
    );
}

#[test]
fn finished_dissolve_expires_on_tick() {
    let mut app = test_app();
    app.lyrics.enabled = true;
    app.lyrics.outgoing = Some(crate::state::OutgoingLyrics {
        doc: timed_doc(&[1_000]),
        center: 0.0,
        started: std::time::Instant::now() - std::time::Duration::from_secs(30),
        duration_ms: 5_000,
    });
    app.scrobble.current_song_id = Some("song_2".to_string());

    let _ = app.handle_playback_state_updated(update_for("song_2", 1_000));
    assert!(
        app.lyrics.outgoing.is_none(),
        "elapsed dissolve must be dropped"
    );
}

#[test]
fn blur_ready_stores_and_clears_pending() {
    use nokkvi_data::types::player_settings::LyricsBackdropBlur;
    let mut app = test_app();
    let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
    let source_id = handle.id();
    app.artwork.lyrics_blur_pending = Some(("album_a".to_string(), LyricsBackdropBlur::Medium));

    let _ = app.handle_lyrics_blur_ready(
        "album_a".to_string(),
        LyricsBackdropBlur::Medium,
        source_id,
        Some(handle),
    );
    assert!(app.artwork.lyrics_blur_pending.is_none(), "guard released");
    let cached = app.artwork.lyrics_blurred.as_ref().expect("blur stored");
    assert_eq!(cached.album_id, "album_a");
    assert_eq!(cached.level, LyricsBackdropBlur::Medium);
    assert!(cached.handle.is_some());
}

#[test]
fn blurred_cover_resolver_gates_on_track_level_and_toggle() {
    use nokkvi_data::types::player_settings::LyricsBackdropBlur;
    let mut app = test_app();
    // Playing track s1 resolves to album_s1 (make_queue_song derives it).
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.lyrics.enabled = true;
    app.settings.lyrics_backdrop_blur = LyricsBackdropBlur::Medium;
    // The sharp source lives in the large LRU; the cache references ITS id
    // (the resolver's staleness check compares against the live handle).
    let source = iced::widget::image::Handle::from_rgba(1, 1, vec![9, 9, 9, 255]);
    let source_id = source.id();
    app.artwork
        .large_artwork
        .put("album_s1".to_string(), source);
    let blurred = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
    app.artwork.lyrics_blurred = Some(crate::state::LyricsBlurredCover {
        album_id: "album_s1".to_string(),
        source_id,
        level: LyricsBackdropBlur::Medium,
        handle: Some(blurred),
    });

    assert!(
        app.lyrics_blurred_cover_for_view().is_some(),
        "matching (album, source, level) with lyrics on must resolve the frost"
    );

    // A refreshed cover (new source handle id): stale blur must NOT display.
    let refreshed = iced::widget::image::Handle::from_rgba(1, 1, vec![7, 7, 7, 255]);
    app.artwork
        .large_artwork
        .put("album_s1".to_string(), refreshed);
    assert!(
        app.lyrics_blurred_cover_for_view().is_none(),
        "a blur of the OLD cover must fall back sharp after a refresh"
    );
    // Restore the matching source for the remaining gate checks.
    let source2 = iced::widget::image::Handle::from_rgba(1, 1, vec![9, 9, 9, 255]);
    let source2_id = source2.id();
    app.artwork
        .large_artwork
        .put("album_s1".to_string(), source2);
    if let Some(cached) = app.artwork.lyrics_blurred.as_mut() {
        cached.source_id = source2_id;
    }
    assert!(app.lyrics_blurred_cover_for_view().is_some());

    // A different blur level than the cache was built at: sharp fallback.
    app.settings.lyrics_backdrop_blur = LyricsBackdropBlur::Heavy;
    assert!(app.lyrics_blurred_cover_for_view().is_none());

    // Blur off: never resolves, regardless of cache.
    app.settings.lyrics_backdrop_blur = LyricsBackdropBlur::Off;
    assert!(app.lyrics_blurred_cover_for_view().is_none());

    // Lyrics toggled off: the cover goes back to sharp.
    app.settings.lyrics_backdrop_blur = LyricsBackdropBlur::Medium;
    app.lyrics.enabled = false;
    assert!(app.lyrics_blurred_cover_for_view().is_none());
}

#[test]
fn foreign_doc_never_scanned_after_clear() {
    // The recompute is guarded on matched_song_id == current: after a cold
    // clear, the (empty) doc must not produce an active line for the new song.
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));
    let _ = app.handle_playback_state_updated(update_for("song_2", 50_000));
    assert_eq!(app.lyrics.active_index, None);
}
