//! Observable-state tests for the synced-lyrics pipeline: active-line
//! tracking (incl. pre-roll), the song-change promote-or-clear, and the
//! stale-load guard. `app_service` is `None` in `test_app`, so `shell_task`
//! dispatches are inert — results are driven synthetically via
//! `handle_lyrics_loader`, exactly as the async pipeline would deliver them.

use std::sync::Arc;

use nokkvi_data::types::{
    lyrics::{LrcDocument, LrcLine, LyricsIndex},
    player_settings::{LyricsAlignment, LyricsMode},
};

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

/// A plain (untimed) sheet as the server hands it over: `synced: false`, every
/// stamp 0, the server's line order.
fn plain_doc(line_count: usize) -> LrcDocument {
    LrcDocument {
        lines: (0..line_count)
            .map(|i| LrcLine {
                time_ms: 0,
                text: format!("plain line {i}"),
                words: vec![],
            })
            .collect(),
        synced: false,
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
        seekable: true,
        live_transcode: false,
        live_icy_metadata: None,
        bpm: None,
        seek_epoch: 0,
        next_track_epoch: 0,
        station_id: None,
    }
}

/// Seed a resolved doc for `song_id` as the currently-matched track.
fn seed_matched(app: &mut crate::Nokkvi, song_id: &str, doc: LrcDocument) {
    app.settings.lyrics_mode = LyricsMode::On;
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
    let epoch_before = app.lyrics.load_epoch.current();

    // No prefetch parked → the transition clears synchronously.
    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.active_index, None);
    assert_eq!(app.lyrics.matched_song_id, None);
    assert_ne!(
        app.lyrics.load_epoch.current(),
        epoch_before,
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
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.lyrics.position_ms = 6_000;

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[5_000, 10_000])),
        epoch: app.lyrics.load_epoch.current(),
    });
    assert_eq!(app.lyrics.matched_song_id.as_deref(), Some("song_1"));
    assert_eq!(app.lyrics.doc.lines.len(), 2);
    // Active line recomputed immediately from the stored position.
    assert_eq!(app.lyrics.active_index, Some(0));
}

#[test]
fn stale_load_rejected_wrong_song() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_2".to_string());

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[5_000])),
        epoch: app.lyrics.load_epoch.current(),
    });
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.matched_song_id, None);
}

#[test]
fn stale_load_rejected_wrong_epoch() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[5_000])),
        epoch: app.lyrics.load_epoch.current().wrapping_add(1),
    });
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.matched_song_id, None);
}

#[test]
fn plain_doc_is_kept_with_no_active_line() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.lyrics.position_ms = 50_000;

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(plain_doc(3)),
        epoch: app.lyrics.load_epoch.current(),
    });
    assert_eq!(app.lyrics.matched_song_id.as_deref(), Some("song_1"));
    assert_eq!(app.lyrics.doc.lines.len(), 3, "plain lyrics are kept");
    assert!(!app.lyrics.doc.synced);
    // No line of an untimed sheet is known to be current. A time search over
    // its all-zero stamps would name the LAST line.
    assert_eq!(app.lyrics.active_index, None);
}

#[test]
fn empty_doc_is_still_the_no_match() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(LrcDocument::default()),
        epoch: app.lyrics.load_epoch.current(),
    });
    // Identity recorded (no re-fire loop), doc honestly empty.
    assert_eq!(app.lyrics.matched_song_id.as_deref(), Some("song_1"));
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.active_index, None);
}

#[test]
fn a_plain_sheet_never_gains_an_active_line_on_a_tick() {
    // The trap: every stamp is 0, so a time search would return the LAST line
    // from the first tick and highlight it for the whole track.
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(5));

    for position_ms in [0, 100_000, 199_000] {
        let _ = app.handle_playback_state_updated(update_for("song_1", position_ms));
        assert_eq!(
            app.lyrics.active_index, None,
            "no active line at {position_ms} ms"
        );
        assert_eq!(
            app.lyrics.position_ms, position_ms,
            "the plain sheet still follows the clock"
        );
    }
}

#[test]
fn prefetch_parks_any_non_current_sheet() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());

    // A prefetch for the CURRENT track is useless — dropped.
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[0])),
    });
    assert!(app.lyrics.pending_next.is_none());
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: "song_1".to_string(),
        doc: Box::new(plain_doc(3)),
    });
    assert!(
        app.lyrics.pending_next.is_none(),
        "a plain prefetch for the current track is dropped too"
    );

    // A synced doc for the NEXT track parks.
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: "song_2".to_string(),
        doc: Box::new(timed_doc(&[0])),
    });
    assert_eq!(
        app.lyrics.pending_next.as_ref().map(|(id, _)| id.as_str()),
        Some("song_2")
    );

    // So does a plain one — it is a real sheet now, not a no-match.
    app.lyrics.pending_next = None;
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: "song_3".to_string(),
        doc: Box::new(plain_doc(4)),
    });
    assert_eq!(
        app.lyrics.pending_next.as_ref().map(|(id, _)| id.as_str()),
        Some("song_3")
    );

    // An EMPTY doc is still a no-match and never parks.
    app.lyrics.pending_next = None;
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: "song_4".to_string(),
        doc: Box::new(LrcDocument::default()),
    });
    assert!(app.lyrics.pending_next.is_none());
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
        std::sync::Arc::default(),
    ));
    app.settings.visualization_mode = nokkvi_data::types::player_settings::VisualizationMode::Scope;
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));

    let (viz, _boat) = app.over_cover_overlays();
    assert!(
        viz.is_some(),
        "over-cover visualizer must render alongside active lyrics"
    );
}

#[test]
fn dispatch_bumps_epoch_so_concurrent_resolves_cant_co_win() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());
    let before = app.lyrics.load_epoch.current();

    // Each dispatch supersedes the last: the older epoch loses the stale guard.
    let _ = app.dispatch_lyrics_resolve("song_1".to_string());
    let e1 = app.lyrics.load_epoch.current();
    assert_eq!(e1, before.wrapping_add(1));

    let _ = app.dispatch_lyrics_resolve("song_1".to_string());
    assert_eq!(app.lyrics.load_epoch.current(), e1.wrapping_add(1));

    // A Loaded carrying the SUPERSEDED epoch is rejected (can't overwrite).
    app.lyrics.doc = timed_doc(&[1_000]);
    app.lyrics.matched_song_id = Some("song_1".to_string());
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(LrcDocument::default()),
        epoch: e1, // stale
    });
    assert_eq!(
        app.lyrics.doc.lines.len(),
        1,
        "a stale-epoch no-match must not erase the rendered doc"
    );
}

#[test]
fn index_ready_redrives_a_pre_index_no_match() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());
    // A no-match landed before the index existed: matched == current, doc empty.
    app.lyrics.matched_song_id = Some("song_1".to_string());
    app.lyrics.doc = LrcDocument::default();
    let before = app.lyrics.load_epoch.current();

    // The index lands → re-drive fires despite matched_song_id == current
    // (the guard that used to defeat it), observable via the epoch bump.
    let _ = app.handle_lyrics_index_ready(Arc::new(LyricsIndex::default()));
    assert_eq!(
        app.lyrics.load_epoch.current(),
        before.wrapping_add(1),
        "empty doc + index landing must re-dispatch"
    );

    // With a rendered doc already present, the index landing does NOT re-drive.
    let mut app2 = test_app();
    app2.settings.lyrics_mode = LyricsMode::On;
    app2.scrobble.current_song_id = Some("song_1".to_string());
    app2.lyrics.matched_song_id = Some("song_1".to_string());
    app2.lyrics.doc = timed_doc(&[1_000]);
    let before2 = app2.lyrics.load_epoch.current();
    let _ = app2.handle_lyrics_index_ready(Arc::new(LyricsIndex::default()));
    assert_eq!(
        app2.lyrics.load_epoch.current(),
        before2,
        "a resolved doc must not be re-driven"
    );
}

#[test]
fn library_change_redrives_an_unresolved_track() {
    use crate::services::navidrome_sse::LibraryChange;

    fn wildcard() -> LibraryChange {
        LibraryChange {
            album_ids: vec![],
            artist_ids: vec![],
            song_ids: vec![],
            playlist_ids: vec![],
            genre_ids: vec![],
            is_wildcard: true,
        }
    }

    // A rescan reaches nokkvi ONLY as a wildcard event (per-song ids come from
    // annotation writes). A track showing the no-match state gets a second
    // look, so lyrics embedded by the rescan appear without a restart.
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.settings.lyrics_mode = LyricsMode::On;
    app.current_view = crate::View::Queue;
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.lyrics.matched_song_id = Some("song_1".to_string());
    app.lyrics.doc = LrcDocument::default();
    let before = app.lyrics.load_epoch.current();

    let _ = app.handle_library_changed(wildcard());
    assert_eq!(
        app.lyrics.load_epoch.current(),
        before.wrapping_add(1),
        "a wildcard change must re-drive a resolved no-match"
    );

    // A song-scoped change is an ANNOTATION write (scrobble, star, rating) —
    // it cannot change lyrics, so it must not re-drive. Re-driving would run
    // the whole chain, third-party LRCLIB request included, on every scrobble
    // point and every rating keypress for the track being watched.
    let mut annotated = test_app();
    annotated.screen = crate::Screen::Home;
    annotated.settings.lyrics_mode = LyricsMode::On;
    annotated.current_view = crate::View::Queue;
    annotated.scrobble.current_song_id = Some("song_1".to_string());
    annotated.lyrics.matched_song_id = Some("song_1".to_string());
    annotated.lyrics.doc = LrcDocument::default();
    let before_annotated = annotated.lyrics.load_epoch.current();
    let _ = annotated.handle_library_changed(LibraryChange {
        song_ids: vec!["song_1".to_string()],
        is_wildcard: false,
        ..wildcard()
    });
    assert_eq!(
        annotated.lyrics.load_epoch.current(),
        before_annotated,
        "an annotation write must not re-drive the lyrics chain"
    );

    // A sheet already showing is left alone — no flicker, no refetch.
    let mut app2 = test_app();
    app2.screen = crate::Screen::Home;
    app2.settings.lyrics_mode = LyricsMode::On;
    app2.current_view = crate::View::Queue;
    app2.scrobble.current_song_id = Some("song_1".to_string());
    app2.lyrics.matched_song_id = Some("song_1".to_string());
    app2.lyrics.doc = timed_doc(&[1_000]);
    let before2 = app2.lyrics.load_epoch.current();
    let _ = app2.handle_library_changed(wildcard());
    assert_eq!(
        app2.lyrics.load_epoch.current(),
        before2,
        "a rendered sheet must not be re-driven"
    );
}

#[test]
fn extensions_probe_landing_redrives_an_unresolved_track() {
    use crate::app_message::Message;
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());
    // A no-match landed while the probe was pending (server channel skipped;
    // the incomplete miss was deliberately not cached backend-side).
    app.lyrics.matched_song_id = Some("song_1".to_string());
    let before = app.lyrics.load_epoch.current();

    // The probe responds advertising songLyrics → the current track gets its
    // second look (observable via the dispatch epoch bump).
    let _ = app.update(Message::OpenSubsonicExtensionsFetched(Some(vec![
        "songLyrics".to_string(),
    ])));
    assert!(app.supports_song_lyrics(), "probe stored");
    assert_eq!(
        app.lyrics.load_epoch.current(),
        before.wrapping_add(1),
        "probe landing must re-dispatch the unresolved track"
    );

    // A FAILED probe (None) stores nothing and re-drives nothing.
    let mut app2 = test_app();
    app2.settings.lyrics_mode = LyricsMode::On;
    app2.scrobble.current_song_id = Some("song_1".to_string());
    let before2 = app2.lyrics.load_epoch.current();
    let _ = app2.update(Message::OpenSubsonicExtensionsFetched(None));
    assert_eq!(app2.lyrics.load_epoch.current(), before2);
}

#[test]
fn clear_resets_position_so_cold_docs_scan_from_zero() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[5_000, 200_000]));
    app.lyrics.position_ms = 200_000;

    // Skip: the cold path clears. Position must reset so the next doc isn't
    // scanned against the old track's deep position.
    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert_eq!(app.lyrics.position_ms, 0);

    // A doc landing for the new track scans from 0 → pre-roll (None), not a
    // wrong-line snap deep in the sheet.
    app.scrobble.current_song_id = Some("song_2".to_string());
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_2".to_string(),
        doc: Box::new(timed_doc(&[5_000, 200_000])),
        epoch: app.lyrics.load_epoch.current(),
    });
    assert_eq!(app.lyrics.active_index, None, "cold doc starts at pre-roll");
}

#[test]
fn queue_lyrics_panel_data_gated_on_enabled_and_loaded_track() {
    let mut app = test_app();
    assert!(app.queue_lyrics_panel_data().is_none());

    // Enabled but transport idle (stopped / never played): no panel — the
    // scrim must not paint over browsing artwork. This holds even with a
    // current_song_id, which login seeds from the restored queue and a
    // CLI/MPRIS stop retains (the queue cursor survives stop()).
    app.settings.lyrics_mode = LyricsMode::On;
    assert!(
        app.queue_lyrics_panel_data().is_none(),
        "no loaded track → no lyrics layer"
    );
    app.scrobble.current_song_id = Some("song_1".to_string());
    assert!(
        app.queue_lyrics_panel_data().is_none(),
        "a seeded/stopped current_song_id alone must not raise the layer"
    );

    // A track is audibly live (paused counts) but its resolve hasn't landed
    // (matched_song_id None): the panel shows, but the no-match message is
    // suppressed (a resolve may be in flight — a false negative here flashes
    // on every skip).
    app.playback.playing = true;
    let data = app.queue_lyrics_panel_data().expect("loaded track");
    assert!(data.lines.is_empty());
    assert!(
        data.empty_message.is_none(),
        "no false negative before the resolve lands"
    );

    // A landed no-match for the current track (matched == current, empty doc):
    // now the message is the resolved verdict.
    app.lyrics.matched_song_id = Some("song_1".to_string());
    assert_eq!(
        app.queue_lyrics_panel_data().expect("loaded").empty_message,
        Some("No lyrics for this track"),
        "resolved no-match shows the message"
    );

    app.lyrics.doc = timed_doc(&[1_000]);
    app.lyrics.active_index = Some(0);
    let data = app.queue_lyrics_panel_data().expect("still enabled");
    assert_eq!(data.lines.len(), 1);
    assert!(data.empty_message.is_none());
    assert_eq!(data.active_index, Some(0));
    assert!(data.synced, "a timed sheet renders as synced");

    // The word-level highlight reads the lyric clock and the setting through
    // the same panel data.
    app.lyrics.clock_ms = 1_234;
    app.settings.lyrics_word_highlight =
        nokkvi_data::types::player_settings::LyricsWordHighlight::Word;
    let data = app.queue_lyrics_panel_data().expect("still enabled");
    assert_eq!(data.clock_ms, 1_234);
    assert_eq!(
        data.word_highlight,
        nokkvi_data::types::player_settings::LyricsWordHighlight::Word
    );

    // A plain sheet renders with `synced == false`: no accent line, the flat
    // band falloff, and the drift center instead of the glide.
    app.lyrics.doc = plain_doc(4);
    app.lyrics.active_index = None;
    let data = app.queue_lyrics_panel_data().expect("plain sheet renders");
    assert_eq!(data.lines.len(), 4);
    assert!(!data.synced);
    assert_eq!(data.active_index, None);
    assert!(data.empty_message.is_none());

    // Paused keeps the layer; a stop (both transport flags false) drops it
    // even though current_song_id survives.
    app.playback.playing = false;
    app.playback.paused = true;
    assert!(
        app.queue_lyrics_panel_data().is_some(),
        "paused keeps lyrics"
    );
    app.playback.paused = false;
    assert!(
        app.queue_lyrics_panel_data().is_none(),
        "stop-after-play must drop the layer despite the surviving song id"
    );
}

/// Pausing keeps the Queue cover on the playing track while the lyrics layer
/// is up. An unlocked panel follows the centered row, and the view's
/// `cover_shows_now_playing` gate then drops the sheet whenever that row is
/// not the playing track. Without lyrics a paused cover still follows the
/// centered row, and a stop never locks.
#[test]
fn paused_cover_stays_on_the_playing_track_under_the_lyrics_layer() {
    let locked = |app: &crate::Nokkvi| {
        app.build_queue_view_data(crate::widgets::base_slot_list_layout::ArtworkBleed::NONE)
            .cover_locks_to_playing
    };
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));

    app.playback.playing = true;
    assert!(locked(&app), "playing locks the cover");

    // A real pause keeps `playing` and raises `paused`.
    app.playback.paused = true;
    assert!(
        locked(&app),
        "paused under the lyrics layer keeps the playing track's cover"
    );

    app.settings.lyrics_mode = LyricsMode::Off;
    assert!(
        !locked(&app),
        "paused without lyrics follows the centered row"
    );

    app.settings.lyrics_mode = LyricsMode::On;
    app.playback.playing = false;
    app.playback.paused = false;
    assert!(!locked(&app), "stopped follows the centered row");
}

#[test]
fn toggle_lyrics_flips_live_mirror() {
    // The arm must flip the LIVE mirror synchronously — the async persist is
    // inert in test_app (app_service is None), so asserting the persisted
    // setting here would be unreachable by design.
    let mut app = test_app();
    assert_eq!(app.settings.lyrics_mode, LyricsMode::Off);

    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
    assert_eq!(app.settings.lyrics_mode, LyricsMode::On);
    assert_eq!(
        app.settings.lyrics_mode,
        LyricsMode::On,
        "live settings union mirrors the flip"
    );

    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
    assert_eq!(app.settings.lyrics_mode, LyricsMode::Off);
}

#[test]
fn toggle_lyrics_off_clears_doc() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));
    assert!(!app.lyrics.doc.lines.is_empty());

    // enabled=true (from seed) → toggling turns lyrics OFF and clears.
    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
    assert_eq!(app.settings.lyrics_mode, LyricsMode::Off);
    assert!(app.lyrics.doc.lines.is_empty());
    assert_eq!(app.lyrics.matched_song_id, None);
}

/// Lyrics switched by a settings load (the Settings view's row writes
/// through `handle_player_settings_loaded`, and so does a config.toml hot
/// reload): the same edges as the player-bar toggle.
fn load_lyrics_setting(app: &mut crate::Nokkvi, mode: LyricsMode) {
    let mut settings = app.settings.clone();
    settings.lyrics_mode = mode;
    let _ = app.handle_player_settings_loaded(settings);
}

#[test]
fn lyrics_off_from_a_settings_load_clears_the_sheet() {
    // The handler mirrors the process-global theme atomics.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_500));
    assert!(app.lyrics.clock_ms > 0, "the sheet's clock runs");

    load_lyrics_setting(&mut app, LyricsMode::Off);

    assert_eq!(app.settings.lyrics_mode, LyricsMode::Off);
    assert!(app.lyrics.doc.lines.is_empty(), "the sheet is cleared");
    assert_eq!(app.lyrics.matched_song_id, None);
    assert_eq!(app.lyrics.clock_ms, 0, "and its clock");
}

#[test]
fn lyrics_back_on_after_a_song_change_shows_no_stale_sheet() {
    // Off from Settings or config.toml, the song changes (with lyrics off
    // nothing clears on the change), on again: the old song's sheet and
    // clock used to come back over the new song.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_500));

    load_lyrics_setting(&mut app, LyricsMode::Off);
    let _ = app.handle_playback_state_updated(update_for("song_2", 500));
    load_lyrics_setting(&mut app, LyricsMode::On);

    assert_eq!(app.settings.lyrics_mode, LyricsMode::On);
    assert!(app.lyrics.doc.lines.is_empty(), "no stale sheet");
    assert_ne!(app.lyrics.matched_song_id.as_deref(), Some("song_1"));
}

#[test]
fn a_settings_load_that_keeps_lyrics_on_keeps_the_sheet() {
    // Every settings write reloads the whole set: an unrelated row must not
    // wipe the sheet on screen.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));

    load_lyrics_setting(&mut app, LyricsMode::On);

    assert_eq!(app.lyrics.doc.lines.len(), 2);
    assert_eq!(app.lyrics.matched_song_id.as_deref(), Some("song_1"));
}

// ---------------------------------------------------------------------------
// Motion (C2): the frame tick publishes the eased center to a process-global
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
fn glide_advances_on_frame_tick_regardless_of_visualizer_mode() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    motion_setup(&mut app, &[1_000, 2_000, 3_000]);
    // Not in Lines mode (test_app default is Bars) — the boat's own
    // `visible` early-return fires, so this proves the lyrics glide runs
    // BEFORE the boat's mode early-outs rather than riding on them.
    assert_ne!(
        app.settings.visualization_mode,
        nokkvi_data::types::player_settings::VisualizationMode::Lines
    );

    // Land on line 0 (pre-roll → snap), then advance to line 1 (glide).
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_000));
    let now = std::time::Instant::now();
    let _ = crate::update::frame::handle_frame_tick(&mut app, now);
    assert_eq!(crate::widgets::lyrics_viewport::lyrics_center_pos(), 0.0);

    let _ = app.handle_playback_state_updated(update_for("song_1", 2_000));
    assert!(app.lyrics.anim_duration_ms > 0, "one-line advance glides");
    let start = app.lyrics.anim_start.expect("glide armed");

    // Mid-glide: strictly between the endpoints.
    let mid = start + std::time::Duration::from_millis(u64::from(app.lyrics.anim_duration_ms) / 4);
    let _ = crate::update::frame::handle_frame_tick(&mut app, mid);
    let pos = crate::widgets::lyrics_viewport::lyrics_center_pos();
    assert!(
        pos > 0.0 && pos < 1.0,
        "mid-glide center must be between lines, got {pos}"
    );

    // Past the duration (even if playback paused meanwhile): settled on target.
    let done =
        start + std::time::Duration::from_millis(u64::from(app.lyrics.anim_duration_ms) + 50);
    let _ = crate::update::frame::handle_frame_tick(&mut app, done);
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
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    // Jump 0 → 7 (delta 7 > 4): snap — the very next tick lands on target.
    let _ = app.handle_playback_state_updated(update_for("song_1", 8_000));
    assert_eq!(app.lyrics.anim_duration_ms, 0, "seek-sized jump must snap");
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
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
fn plain_sheet_drifts_with_playback() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(11));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);

    // `update_for` reports a 200 s track, so the 11-line sheet walks 0 -> 10.
    let tick_to = |app: &mut crate::Nokkvi, position_ms| {
        let _ = app.handle_playback_state_updated(update_for("song_1", position_ms));
        let _ = crate::update::frame::handle_frame_tick(app, std::time::Instant::now());
        crate::widgets::lyrics_viewport::lyrics_center_pos()
    };

    assert_eq!(tick_to(&mut app, 0), 0.0, "starts on the first line");
    assert_eq!(tick_to(&mut app, 100_000), 5.0, "halfway sits mid-sheet");
    assert_eq!(tick_to(&mut app, 200_000), 10.0, "ends on the last line");
}

#[test]
fn plain_sheet_jumps_in_one_step_on_a_seek() {
    // The center is a pure function of the last tick, so a seek arrives whole
    // rather than gliding — no easing in v1.
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(11));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);

    let _ = app.handle_playback_state_updated(update_for("song_1", 10_000));
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    assert_eq!(crate::widgets::lyrics_viewport::lyrics_center_pos(), 0.5);

    let _ = app.handle_playback_state_updated(update_for("song_1", 150_000));
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    assert_eq!(crate::widgets::lyrics_viewport::lyrics_center_pos(), 7.5);
}

#[test]
fn plain_sheet_parks_when_the_duration_is_unknown() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(11));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);

    let mut update = update_for("song_1", 100_000);
    update.duration = 0;
    let _ = app.handle_playback_state_updated(update);
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    assert_eq!(
        crate::widgets::lyrics_viewport::lyrics_center_pos(),
        0.0,
        "a zero duration parks at the offset instead of dividing by zero"
    );
}

#[test]
fn a_synced_sheet_after_a_plain_one_follows_its_own_glide() {
    // The published center is a process-global atomic. A stale drift value
    // from the previous plain sheet must not fling the synced column.
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(60));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
    let _ = app.handle_playback_state_updated(update_for("song_1", 200_000));
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    assert_eq!(crate::widgets::lyrics_viewport::lyrics_center_pos(), 59.0);

    // A synced track follows. Its own tick retargets and its own frame tick
    // republishes, so the drift value is gone by the first rendered frame.
    seed_matched(&mut app, "song_2", timed_doc(&[1_000, 2_000, 3_000]));
    app.lyrics.active_index = None;
    let _ = app.handle_playback_state_updated(update_for("song_2", 1_000));
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    assert_eq!(
        crate::widgets::lyrics_viewport::lyrics_center_pos(),
        0.0,
        "the synced glide owns the center again"
    );
}

#[test]
fn drift_offset_resets_on_clear_and_promote() {
    let mut state = crate::state::LyricsState {
        drift_offset: 4.0,
        ..Default::default()
    };
    state.clear();
    assert_eq!(state.drift_offset, 0.0, "a song change unscrolls the sheet");

    state.drift_offset = 4.0;
    state.pending_next = Some(("song_2".to_string(), plain_doc(3)));
    assert!(state.promote_next("song_2"));
    assert_eq!(
        state.drift_offset, 0.0,
        "the promoted sheet starts unscrolled"
    );
}

// ---------------------------------------------------------------------------
// The mouse wheel over a plain sheet
// ---------------------------------------------------------------------------

/// Send one wheel step as the widget would, through the root handler.
fn wheel(app: &mut crate::Nokkvi, delta_lines: f32) {
    let _ = app.handle_queue(crate::views::QueueMessage::LyricsWheel(delta_lines));
}

/// The center the next rendered frame would show.
fn published_center(app: &mut crate::Nokkvi) -> f32 {
    let _ = crate::update::frame::handle_frame_tick(app, std::time::Instant::now());
    crate::widgets::lyrics_viewport::lyrics_center_pos()
}

#[test]
fn wheel_scrolls_a_plain_sheet_and_the_drift_carries_on() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(11));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
    let _ = app.handle_playback_state_updated(update_for("song_1", 0));
    assert_eq!(published_center(&mut app), 0.0);

    wheel(&mut app, 2.0);
    assert_eq!(app.lyrics.drift_offset, 2.0);
    assert_eq!(published_center(&mut app), 2.0, "the sheet moved");

    // The drift carries on from where the user left it.
    let _ = app.handle_playback_state_updated(update_for("song_1", 100_000));
    assert_eq!(published_center(&mut app), 7.0, "drift + offset");
}

#[test]
fn wheel_is_clamped_when_written_with_no_hidden_overshoot() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(11));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);

    // At the start, a huge backward shove stores no hidden debt: ONE forward
    // notch then moves the sheet immediately.
    let _ = app.handle_playback_state_updated(update_for("song_1", 0));
    wheel(&mut app, -1_000.0);
    assert_eq!(published_center(&mut app), 0.0);
    wheel(&mut app, 2.0);
    assert_eq!(published_center(&mut app), 2.0, "no backward debt to repay");

    // Same at the end.
    let _ = app.handle_playback_state_updated(update_for("song_1", 200_000));
    wheel(&mut app, 1_000.0);
    assert_eq!(published_center(&mut app), 10.0);
    wheel(&mut app, -2.0);
    assert_eq!(published_center(&mut app), 8.0, "no forward debt to repay");
}

#[test]
fn wheel_is_ignored_for_a_synced_or_empty_sheet() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    wheel(&mut app, 3.0);
    assert_eq!(
        app.lyrics.drift_offset, 0.0,
        "a synced sheet follows its own line cursor"
    );

    app.lyrics.doc = LrcDocument::default();
    wheel(&mut app, 3.0);
    assert_eq!(app.lyrics.drift_offset, 0.0, "nothing to scroll");
}

#[test]
fn wheel_offset_survives_ticks_and_seeks_but_not_a_song_change() {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(11));
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
    let _ = app.handle_playback_state_updated(update_for("song_1", 0));
    wheel(&mut app, 2.0);

    // Ordinary ticks and a seek leave the manual scroll alone.
    let _ = app.handle_playback_state_updated(update_for("song_1", 20_000));
    assert_eq!(app.lyrics.drift_offset, 2.0);
    let _ = app.handle_playback_state_updated(update_for("song_1", 150_000));
    assert_eq!(app.lyrics.drift_offset, 2.0, "a seek keeps the scroll");

    // A song change does not.
    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert_eq!(app.lyrics.drift_offset, 0.0);
}

#[test]
fn wheel_leaves_the_queue_untouched() {
    let mut app = test_app();
    app.library.queue_songs = (0..20)
        .map(|i| make_queue_song(&format!("s{i}"), "T", "A", "Al"))
        .collect();
    seed_matched(&mut app, "s0", plain_doc(11));
    app.queue_page.common.slot_list.viewport_offset = 5;
    app.queue_page.common.slot_list.selected_indices.insert(3);

    wheel(&mut app, 2.0);
    assert_eq!(
        app.queue_page.common.slot_list.viewport_offset, 5,
        "the wheel scrolls the lyric sheet, not the queue"
    );
    assert!(
        app.queue_page
            .common
            .slot_list
            .selected_indices
            .contains(&3),
        "the selection is untouched"
    );
}

#[test]
fn a_song_change_parks_the_column_so_the_next_dissolve_is_honest() {
    // The pre-roll trap. A new sheet fires no retarget while `active_index` is
    // None on both sides of the compare, so a surviving glide target would have
    // the frame tick keep publishing the PREVIOUS track's last line through the
    // new sheet's pre-roll. `park_outgoing` snapshots that atomic at the next
    // transition, and `draw` places the parked sheet at it — a short sheet
    // parked at line 40 culls every line and the dissolve shows nothing.
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    app.settings.crossfade_enabled = true;
    app.settings.crossfade_duration_secs = 7;

    // Track A runs deep into a long sheet.
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000, 3_000]));
    app.lyrics.scroll_from = 40.0;
    app.lyrics.scroll_to = 40.0;
    crate::widgets::lyrics_viewport::set_lyrics_center(40.0);

    // Skip to track B, whose short sheet is all pre-roll for now.
    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert_eq!(app.lyrics.scroll_to, 0.0, "the glide target is parked");
    assert!(app.lyrics.anim_start.is_none());

    app.scrobble.current_song_id = Some("song_2".to_string());
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_2".to_string(),
        doc: Box::new(timed_doc(&[12_000, 14_000])),
        epoch: app.lyrics.load_epoch.current(),
    });
    assert_eq!(app.lyrics.active_index, None, "still pre-roll");
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    assert_eq!(
        crate::widgets::lyrics_viewport::lyrics_center_pos(),
        0.0,
        "pre-roll must publish line 0, not the previous track's line"
    );

    // Skip again inside B's pre-roll: its sheet parks where it was SHOWN.
    let _ = app.handle_playback_state_updated(update_for("song_3", 0));
    let outgoing = app.lyrics.outgoing.as_ref().expect("B parked");
    assert_eq!(
        outgoing.center, 0.0,
        "the dissolve starts where the user was looking"
    );
}

#[test]
fn a_sheet_resolved_mid_track_renders_in_place_on_its_first_frame() {
    // Toggling Lyrics on at 2:00 (or the enter-Queue kick): `clear()` zeroed
    // the playhead and only the tick refreshes it, one tick from now. Without
    // a seed the plain sheet draws at line 0 for a few frames, then jumps.
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.playback.position = 100;
    app.playback.duration = 200;
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);

    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(plain_doc(11)),
        epoch: app.lyrics.load_epoch.current(),
    });
    assert_eq!(app.lyrics.position_ms, 100_000, "seeded from the transport");
    let _ = crate::update::frame::handle_frame_tick(&mut app, std::time::Instant::now());
    assert_eq!(
        crate::widgets::lyrics_viewport::lyrics_center_pos(),
        5.0,
        "the first frame already sits mid-sheet"
    );

    // The tick's exact milliseconds are never coarsened back to whole seconds.
    let _ = app.handle_playback_state_updated(update_for("song_1", 100_500));
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(plain_doc(11)),
        epoch: app.lyrics.load_epoch.current(),
    });
    assert_eq!(app.lyrics.position_ms, 100_500);
}

#[test]
fn toggling_lyrics_off_drops_a_dissolve_in_flight() {
    // Otherwise toggling back on inside the crossfade window re-renders the
    // PREVIOUS track's sheet fading out over the current one.
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.lyrics.outgoing = Some(crate::state::OutgoingLyrics {
        doc: timed_doc(&[1_000]),
        synced: true,
        center: 0.0,
        started: std::time::Instant::now(),
        duration_ms: 12_000,
    });

    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
    assert_eq!(app.settings.lyrics_mode, LyricsMode::Off);
    assert!(
        app.lyrics.outgoing.is_none(),
        "switching lyrics off takes the dissolve with it"
    );
}

#[test]
fn crossfade_transition_parks_a_plain_sheet_too() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", plain_doc(6));
    app.settings.crossfade_enabled = true;
    app.settings.crossfade_duration_secs = 7;

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    let outgoing = app
        .lyrics
        .outgoing
        .as_ref()
        .expect("a plain sheet dissolves out like a synced one");
    assert_eq!(outgoing.doc.lines.len(), 6);
    assert!(
        !outgoing.synced,
        "the parked sheet remembers its kind so it keeps its own falloff"
    );
    assert_eq!(outgoing.duration_ms, 7_000);
}

#[test]
fn crossfade_transition_parks_outgoing_sheet() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    app.settings.crossfade_enabled = true;
    app.settings.crossfade_duration_secs = 7;

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
    app.settings.crossfade_enabled = false;

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(
        app.lyrics.outgoing.is_none(),
        "hard swap when crossfade is off"
    );
}

#[test]
fn finished_dissolve_expires_on_tick() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.lyrics.outgoing = Some(crate::state::OutgoingLyrics {
        doc: timed_doc(&[1_000]),
        synced: true,
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
    app.settings.lyrics_mode = LyricsMode::On;
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

    // MilkDrop replaces the cover: nothing to frost.
    app.settings.lyrics_backdrop_blur = LyricsBackdropBlur::Medium;
    assert!(app.lyrics_blurred_cover_for_view().is_some());
    app.settings.visualization_mode =
        nokkvi_data::types::player_settings::VisualizationMode::Milkdrop;
    assert!(app.lyrics_blurred_cover_for_view().is_none());
    app.settings.visualization_mode = nokkvi_data::types::player_settings::VisualizationMode::Bars;

    // Lyrics toggled off: the cover goes back to sharp.
    app.settings.lyrics_mode = LyricsMode::Off;
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

// ---------------------------------------------------------------------------
// The lyric clock: the per-frame playhead the word-level highlight reads. The
// position tick anchors it ten times a second; the frame tick runs it on
// between anchors while the transport runs, holds it while paused, and never
// lets a lagging tick drag it backwards.
// ---------------------------------------------------------------------------

fn ms(n: u64) -> std::time::Duration {
    std::time::Duration::from_millis(n)
}

/// One frame tick. The whole tick also publishes the column center into the
/// process-global atomic the motion tests above read, so it runs under their
/// lock (held for the call only: the clock assertions never read the center).
fn frame(app: &mut crate::Nokkvi, at: std::time::Instant) {
    let _guard = LYRICS_MOTION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = crate::update::frame::handle_frame_tick(app, at);
}

#[test]
fn lyric_clock_runs_on_between_position_ticks_while_playing() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_000));
    let anchored = app
        .lyrics
        .clock_anchor
        .expect("the position tick anchors the clock");
    assert!(app.lyrics.clock_running);

    frame(&mut app, anchored + ms(60));
    assert_eq!(app.lyrics.clock_ms, 1_060);
    frame(&mut app, anchored + ms(90));
    assert_eq!(app.lyrics.clock_ms, 1_090);
}

#[test]
fn lyric_clock_holds_while_paused() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    let mut paused = update_for("song_1", 1_000);
    paused.paused = true;
    let _ = app.handle_playback_state_updated(paused);
    let anchored = app.lyrics.clock_anchor.expect("anchored even while paused");
    assert!(!app.lyrics.clock_running);

    frame(&mut app, anchored + ms(500));
    assert_eq!(app.lyrics.clock_ms, 1_000, "a paused clock does not run on");
}

#[test]
fn lyric_clock_never_steps_back_for_a_lagging_tick() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_000));
    let first = app.lyrics.clock_anchor.expect("anchored");
    frame(&mut app, first + ms(80));
    assert_eq!(app.lyrics.clock_ms, 1_080);

    // The next tick's position round-trip lagged the real playhead: it says
    // 1050 while the clock already shows 1080. The clock holds rather than
    // flickering backwards, then runs on once the new anchor catches up.
    let _ = app.handle_playback_state_updated(update_for("song_1", 1_050));
    let second = app.lyrics.clock_anchor.expect("re-anchored");
    frame(&mut app, second);
    assert_eq!(app.lyrics.clock_ms, 1_080, "held, not dragged back to 1050");
    frame(&mut app, second + ms(10));
    assert_eq!(app.lyrics.clock_ms, 1_080, "still held: 1050 + 10 < 1080");
    frame(&mut app, second + ms(60));
    assert_eq!(app.lyrics.clock_ms, 1_110, "caught up: 1050 + 60");
}

#[test]
fn lyric_clock_snaps_back_on_a_seek() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 60_000]));
    let _ = app.handle_playback_state_updated(update_for("song_1", 50_000));
    let first = app.lyrics.clock_anchor.expect("anchored");
    frame(&mut app, first + ms(40));
    assert_eq!(app.lyrics.clock_ms, 50_040);

    // A real seek backwards falls far behind the clock: the clock follows it.
    let _ = app.handle_playback_state_updated(update_for("song_1", 10_000));
    let second = app.lyrics.clock_anchor.expect("re-anchored");
    frame(&mut app, second + ms(20));
    assert_eq!(app.lyrics.clock_ms, 10_020);
}

#[test]
fn song_change_resets_the_lyric_clock() {
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    let _ = app.handle_playback_state_updated(update_for("song_1", 50_000));
    let anchored = app.lyrics.clock_anchor.expect("anchored");
    frame(&mut app, anchored + ms(30));
    assert_eq!(app.lyrics.clock_ms, 50_030);

    // The cold path clears the sheet: the clock goes with it, so the next
    // track's sheet can't read the previous track's playhead for a frame.
    let _ = app.handle_playback_state_updated(update_for("song_2", 500));
    assert_eq!(app.lyrics.clock_ms, 0);
    assert_eq!(app.lyrics.clock_anchor, None);
    assert!(!app.lyrics.clock_running);
}

#[test]
fn a_sheet_landing_mid_track_starts_its_clock_at_the_transport() {
    // A resolve that lands mid-track seeds `position_ms` from the whole-second
    // transport (see `Loaded`); the first frame reads the clock from there
    // rather than from zero, so the first word lit is the right one.
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.playback.position = 61;
    let epoch = app.lyrics.load_epoch.current();
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: "song_1".to_string(),
        doc: Box::new(timed_doc(&[1_000, 2_000])),
        epoch,
    });
    assert_eq!(app.lyrics.position_ms, 61_000);
    frame(&mut app, std::time::Instant::now());
    assert_eq!(app.lyrics.clock_ms, 61_000);
}

// ── Auto: the layer only for songs that have lyrics ─────────────────────────

/// Auto on the Queue with `song_id` playing and its lookup not landed yet.
fn auto_app(song_id: &str) -> crate::Nokkvi {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::Auto;
    app.current_view = crate::View::Queue;
    app.scrobble.current_song_id = Some(song_id.to_string());
    app.playback.playing = true;
    app
}

/// The lookup for `song_id` lands under the live epoch.
fn land(app: &mut crate::Nokkvi, song_id: &str, doc: LrcDocument) {
    let epoch = app.lyrics.load_epoch.current();
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::Loaded {
        song_id: song_id.to_string(),
        doc: Box::new(doc),
        epoch,
    });
}

/// The next song's lookup lands ahead of the track change.
fn prefetch(app: &mut crate::Nokkvi, song_id: &str, doc: LrcDocument) {
    let _ = app.handle_lyrics_loader(LyricsLoaderMessage::PrefetchLoaded {
        song_id: song_id.to_string(),
        doc: Box::new(doc),
    });
}

fn layer_up(app: &crate::Nokkvi) -> bool {
    app.queue_lyrics_panel_data().is_some()
}

fn toggle(app: &mut crate::Nokkvi) {
    let _ = app.handle_player_bar(crate::widgets::PlayerBarMessage::ToggleLyrics);
}

#[test]
fn auto_covers_the_art_only_once_the_song_has_lyrics() {
    let mut app = auto_app("song_1");
    assert!(
        !layer_up(&app),
        "the cover stays plain while the lookup runs"
    );
    assert!(!app.lyrics_layer_wanted(), "and the button is dark");

    land(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    assert!(layer_up(&app), "synced lyrics raise the layer");
    assert!(app.lyrics_layer_wanted(), "and light the button");
}

#[test]
fn auto_counts_a_plain_sheet_as_lyrics() {
    let mut app = auto_app("song_1");
    land(&mut app, "song_1", plain_doc(4));
    assert!(layer_up(&app));
}

#[test]
fn auto_leaves_a_song_without_lyrics_on_the_plain_cover() {
    let mut app = auto_app("song_1");
    land(&mut app, "song_1", LrcDocument::default());
    assert!(!layer_up(&app), "no empty overlay for a resolved no-match");
    assert!(!app.lyrics_layer_wanted());
}

#[test]
fn auto_hides_the_layer_again_for_the_next_song_without_lyrics() {
    let mut app = auto_app("song_1");
    land(&mut app, "song_1", timed_doc(&[1_000]));
    assert!(layer_up(&app));

    // No prefetch: the next song's lookup runs after the change.
    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(!layer_up(&app), "the next song starts on the plain cover");
    land(&mut app, "song_2", LrcDocument::default());
    assert!(!layer_up(&app), "and stays on it without lyrics");
}

#[test]
fn auto_shows_a_prefetched_sheet_at_the_track_change() {
    let mut app = auto_app("song_1");
    land(&mut app, "song_1", timed_doc(&[1_000]));
    prefetch(&mut app, "song_2", timed_doc(&[0, 2_000]));

    let _ = app.handle_playback_state_updated(update_for("song_2", 100));
    assert!(layer_up(&app), "the decision is ready when the song starts");
}

#[test]
fn auto_looks_the_song_up_on_a_lyrics_surface_only() {
    let mut app = auto_app("song_1");
    let before = app.lyrics.load_epoch.current();
    let _ = app.lyrics_kick_if_unresolved();
    assert_ne!(
        app.lyrics.load_epoch.current(),
        before,
        "Auto looks the playing song up while the layer is hidden"
    );

    app.current_view = crate::View::Albums;
    let before = app.lyrics.load_epoch.current();
    let _ = app.lyrics_kick_if_unresolved();
    assert_eq!(
        app.lyrics.load_epoch.current(),
        before,
        "nothing is looked up off the Queue and Theater Mode"
    );
}

#[test]
fn toggle_in_auto_hides_lyrics_for_the_current_song_only() {
    let mut app = auto_app("song_1");
    land(&mut app, "song_1", timed_doc(&[1_000]));

    toggle(&mut app);
    assert!(!layer_up(&app), "Toggle Lyrics hides what Auto showed");
    assert!(!app.lyrics_layer_wanted(), "and darkens the button");
    assert_eq!(
        app.settings.lyrics_mode,
        LyricsMode::Auto,
        "the setting stays Auto"
    );
    assert!(
        !app.lyrics.doc.lines.is_empty(),
        "the sheet stays loaded for a second press"
    );

    prefetch(&mut app, "song_2", timed_doc(&[0, 2_000]));
    let _ = app.handle_playback_state_updated(update_for("song_2", 100));
    assert!(layer_up(&app), "the next song with lyrics shows them again");
    assert_eq!(app.settings.lyrics_mode, LyricsMode::Auto);
}

#[test]
fn toggle_in_auto_shows_the_empty_overlay_on_a_song_without_lyrics() {
    let mut app = auto_app("song_1");
    land(&mut app, "song_1", LrcDocument::default());

    toggle(&mut app);
    let data = app
        .queue_lyrics_panel_data()
        .expect("Toggle Lyrics raises the layer");
    assert_eq!(data.empty_message, Some("No lyrics for this track"));
    assert!(app.lyrics_layer_wanted(), "and lights the button");

    toggle(&mut app);
    assert!(!layer_up(&app), "a second press hides it again");
    assert_eq!(app.settings.lyrics_mode, LyricsMode::Auto);
}

#[test]
fn toggle_in_auto_on_the_next_song_without_lyrics_is_auto_again() {
    // Shown on a lyric-less song, then the next lyric-less song: hidden.
    let mut app = auto_app("song_1");
    land(&mut app, "song_1", LrcDocument::default());
    toggle(&mut app);
    assert!(layer_up(&app));

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    land(&mut app, "song_2", LrcDocument::default());
    assert!(!layer_up(&app));
}

#[test]
fn a_lyrics_mode_change_drops_the_toggle_override() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = auto_app("song_1");
    app.start_view_applied = true;
    land(&mut app, "song_1", timed_doc(&[1_000]));
    toggle(&mut app);
    assert!(!layer_up(&app));

    load_lyrics_setting(&mut app, LyricsMode::On);
    assert!(
        layer_up(&app),
        "On shows lyrics whatever was toggled in Auto"
    );
    load_lyrics_setting(&mut app, LyricsMode::Auto);
    assert!(
        layer_up(&app),
        "back in Auto, the song's lyrics decide again"
    );
}

#[test]
fn toggle_in_on_and_off_still_flips_the_setting() {
    let mut app = test_app();
    app.settings.lyrics_mode = LyricsMode::On;
    toggle(&mut app);
    assert_eq!(app.settings.lyrics_mode, LyricsMode::Off);
    toggle(&mut app);
    assert_eq!(app.settings.lyrics_mode, LyricsMode::On);
}

#[test]
fn auto_drops_the_dissolve_when_the_next_song_starts_uncovered() {
    let mut app = auto_app("song_1");
    app.settings.crossfade_enabled = true;
    app.settings.crossfade_duration_secs = 7;
    land(&mut app, "song_1", timed_doc(&[1_000, 2_000]));

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(
        app.lyrics.outgoing.is_none(),
        "with no layer up, a parked sheet would surface mid-fade when song 2's lyrics land"
    );
}

#[test]
fn auto_dissolves_into_a_prefetched_sheet() {
    let mut app = auto_app("song_1");
    app.settings.crossfade_enabled = true;
    app.settings.crossfade_duration_secs = 7;
    land(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    prefetch(&mut app, "song_2", timed_doc(&[0, 2_000]));

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(
        app.lyrics.outgoing.is_some(),
        "both sheets are up, so the old one dissolves as in On"
    );
}

#[test]
fn auto_never_dissolves_a_sheet_toggle_lyrics_hid() {
    let mut app = auto_app("song_1");
    app.settings.crossfade_enabled = true;
    app.settings.crossfade_duration_secs = 7;
    land(&mut app, "song_1", timed_doc(&[1_000, 2_000]));
    toggle(&mut app);
    prefetch(&mut app, "song_2", timed_doc(&[0, 2_000]));

    let _ = app.handle_playback_state_updated(update_for("song_2", 0));
    assert!(
        app.lyrics.outgoing.is_none(),
        "the hidden sheet must not fade back in over song 2"
    );
}

/// The Lyrics: Alignment row and a config.toml edit both land through a
/// settings load; the layer the Queue cover and Theater Mode draw carries the
/// new alignment from the next frame.
#[test]
fn a_settings_load_sets_the_alignment_the_lyrics_layer_draws_with() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = test_app();
    seed_matched(&mut app, "song_1", timed_doc(&[1_000]));
    app.playback.playing = true;
    assert_eq!(
        app.queue_lyrics_panel_data().expect("layer up").align,
        LyricsAlignment::Center,
        "Center until the setting says otherwise"
    );

    for align in [
        LyricsAlignment::Left,
        LyricsAlignment::Right,
        LyricsAlignment::Center,
    ] {
        let mut settings = app.settings.clone();
        settings.lyrics_alignment = align;
        let _ = app.handle_player_settings_loaded(settings);
        assert_eq!(
            app.queue_lyrics_panel_data().expect("layer up").align,
            align
        );
    }
}
