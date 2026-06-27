//! Radio-station artwork handler tests — assert observable `ArtworkState`
//! mutations (no `app_service` side effects).

use iced::widget::image;

use crate::{app_message::MiniArt, state, test_helpers::test_app};

fn handle(bytes: &[u8]) -> image::Handle {
    image::Handle::from_bytes(bytes.to_vec())
}

/// Build a test app playing a radio station with the given `coverArt` token.
/// The station is present in BOTH the library (for `has_logo` lookups) and
/// `active_playback`.
fn radio_app_with_cover(cover_art: Option<&str>) -> crate::Nokkvi {
    let mut app = test_app();
    let station = nokkvi_data::types::radio_station::RadioStation {
        id: "s1".into(),
        name: "Test".into(),
        stream_url: "http://stream".into(),
        home_page_url: None,
        cover_art: cover_art.map(str::to_string),
    };
    app.library.radio_stations = vec![station.clone()];
    app.active_playback = state::ActivePlayback::Radio(state::RadioPlaybackState {
        station,
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });
    app
}

#[test]
fn radio_art_loaded_stores_mini_handle() {
    let mut app = test_app();
    let _ = app.handle_radio_art_loaded("s1".into(), MiniArt::Loaded(handle(b"img")));
    assert!(app.artwork.radio_art.contains(&"s1".to_string()));
}

#[test]
fn radio_art_loaded_missing_stores_nothing() {
    let mut app = test_app();
    let _ = app.handle_radio_art_loaded("s1".into(), MiniArt::Missing);
    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
}

#[test]
fn radio_large_loaded_stores_into_large_cache_only() {
    let mut app = test_app();
    let _ = app.handle_radio_large_loaded("s1".into(), Some(handle(b"big")));
    assert!(app.artwork.radio_large_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
}

#[test]
fn icy_art_for_logoless_station_feeds_both_caches() {
    // No uploaded logo: the now-playing image is the station's only identity,
    // so it becomes both the large panel art AND the idle row thumbnail.
    let mut app = radio_app_with_cover(None);
    app.artwork
        .radio_icy_captured
        .insert("s1".into(), "http://np.jpg".into());
    let _ = app.handle_radio_icy_art_loaded(
        "s1".into(),
        "http://np.jpg".into(),
        Some(b"nowplaying".to_vec()),
    );
    assert!(app.artwork.radio_large_art.contains(&"s1".to_string()));
    assert!(app.artwork.radio_art.contains(&"s1".to_string()));
}

#[test]
fn icy_art_ignored_for_logo_station() {
    // An uploaded logo is the stable identity EVERYWHERE in-app; live ICY art
    // is dropped (it only drives MPRIS), so neither cache is touched.
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    app.artwork
        .radio_icy_captured
        .insert("s1".into(), "http://np.jpg".into());
    let _ = app.handle_radio_icy_art_loaded(
        "s1".into(),
        "http://np.jpg".into(),
        Some(b"nowplaying".to_vec()),
    );
    assert!(!app.artwork.radio_large_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
}

#[test]
fn icy_art_stale_out_of_order_completion_is_dropped() {
    // The dedup map holds the LATEST url (a newer track); a late completion for
    // a superseded url must not clobber the now-playing panel.
    let mut app = radio_app_with_cover(None);
    app.artwork
        .radio_icy_captured
        .insert("s1".into(), "http://track-B.jpg".into());
    let _ = app.handle_radio_icy_art_loaded(
        "s1".into(),
        "http://track-A.jpg".into(), // stale
        Some(b"old".to_vec()),
    );
    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_large_art.contains(&"s1".to_string()));
}

/// A "Refresh Artwork" clears `radio_icy_captured`; a capture fetch that was
/// already in flight when the refresh happened must be dropped on arrival — its
/// bytes feed neither cache (and, in production, are not persisted to disk),
/// so the cleared thumbnail is not resurrected.
#[test]
fn icy_art_dropped_after_capture_cleared_by_refresh() {
    let mut app = radio_app_with_cover(None);
    // Note: NO radio_icy_captured entry — a refresh just removed it.
    let _ = app.handle_radio_icy_art_loaded(
        "s1".into(),
        "http://np.jpg".into(),
        Some(b"nowplaying".to_vec()),
    );
    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_large_art.contains(&"s1".to_string()));
}

/// A failed fetch (`None` bytes) on a matching capture touches neither cache.
#[test]
fn icy_art_none_bytes_is_a_noop() {
    let mut app = radio_app_with_cover(None);
    app.artwork
        .radio_icy_captured
        .insert("s1".into(), "http://np.jpg".into());
    let _ = app.handle_radio_icy_art_loaded("s1".into(), "http://np.jpg".into(), None);
    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_large_art.contains(&"s1".to_string()));
}

#[test]
fn icy_capture_skips_logo_stations() {
    // A logo station never triggers an external ICY fetch.
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    assert!(
        app.maybe_capture_radio_icy_art(Some("http://cdn/cover.jpg".into()))
            .is_none()
    );
    assert!(app.artwork.radio_icy_captured.is_empty());
}

#[test]
fn icy_capture_without_session_is_not_recorded() {
    // [code-review finding 8] `test_app` has no `app_service`. A capture seen
    // while the backend session is absent (e.g. mid re-login) must NOT be
    // recorded in the dedup map — otherwise the url is permanently marked done
    // but never fetched, so no retry fires once the session is restored.
    let mut app = radio_app_with_cover(None);
    assert!(
        app.maybe_capture_radio_icy_art(Some("http://cdn/cover.jpg".into()))
            .is_none()
    );
    assert!(
        app.artwork.radio_icy_captured.is_empty(),
        "no session ⇒ url not recorded, so a later tick re-attempts"
    );
}

#[test]
fn icy_capture_rejects_non_http_and_empty_urls() {
    let mut app = radio_app_with_cover(None);
    assert!(app.maybe_capture_radio_icy_art(None).is_none());
    assert!(
        app.maybe_capture_radio_icy_art(Some(String::new()))
            .is_none()
    );
    assert!(
        app.maybe_capture_radio_icy_art(Some("ftp://x/y".into()))
            .is_none()
    );
    assert!(
        app.artwork.radio_icy_captured.is_empty(),
        "rejected urls must not be recorded"
    );
}

#[test]
fn icy_capture_ignored_when_not_playing_radio() {
    // Queue playback (default) has no active station to attribute art to.
    let mut app = test_app();
    assert!(
        app.maybe_capture_radio_icy_art(Some("http://cdn/cover.jpg".into()))
            .is_none()
    );
    assert!(app.artwork.radio_icy_captured.is_empty());
}

#[test]
fn hydrate_is_additive_and_seeds_dedup_map() {
    let mut app = test_app();
    // s1 already warmed this session (e.g. a live capture) — must be preserved.
    app.artwork.radio_art.put("s1".into(), handle(b"fresh"));

    let _ = app.handle_radio_art_hydrated(vec![
        ("s1".into(), "http://old/s1.jpg".into(), b"stale".to_vec()),
        ("s2".into(), "http://old/s2.jpg".into(), b"diskart".to_vec()),
    ]);

    // s1's warm handle is untouched; s2 is added from disk.
    assert!(app.artwork.radio_art.contains(&"s1".to_string()));
    assert!(app.artwork.radio_art.contains(&"s2".to_string()));
    // Both stations seed the dedup map so unchanged urls aren't re-fetched.
    assert_eq!(
        app.artwork.radio_icy_captured.get("s2").map(String::as_str),
        Some("http://old/s2.jpg")
    );
}

/// A station that now carries an uploaded logo must NOT have a stale remembered
/// stream thumbnail seeded into `radio_art` on hydrate — otherwise
/// `prefetch_radio_logo_tasks` (which skips ids already in `radio_art`) would
/// never fetch the new logo, leaving the station stuck on old art.
#[test]
fn hydrate_skips_radio_art_for_logo_stations() {
    // s1 is in the library WITH a logo token (it gained a logo since last run).
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));

    let _ = app.handle_radio_art_hydrated(vec![(
        "s1".into(),
        "http://old/s1.jpg".into(),
        b"stale-stream-art".to_vec(),
    )]);

    // The stale thumbnail is NOT seeded, so the logo prefetch isn't blocked.
    assert!(
        !app.artwork.radio_art.contains(&"s1".to_string()),
        "a logo station's remembered stream art must not block its logo fetch"
    );
}
