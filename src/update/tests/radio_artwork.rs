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

// ============================================================================
// Custom artwork (Set Custom Artwork… / Reset Artwork)
// ============================================================================

/// Right-click "Set Custom Artwork…" on a station row must bubble the
/// station to root as `RadiosAction::SetStationArtwork` (the root handler
/// owns the pick-file → upload side effects).
#[test]
fn set_station_artwork_message_maps_to_set_action() {
    let mut app = radio_app_with_cover(None);
    let station = app.library.radio_stations[0].clone();
    let stations = app.library.radio_stations.clone();

    let (_task, action) = app.radios_page.update(
        crate::views::RadiosMessage::SetStationArtwork(station),
        &stations,
    );

    assert!(
        matches!(action, crate::views::RadiosAction::SetStationArtwork(s) if s.id == "s1"),
        "SetStationArtwork message must map to the SetStationArtwork action"
    );
}

/// Right-click "Reset Artwork" must bubble `RadiosAction::ResetStationArtwork`.
#[test]
fn reset_station_artwork_message_maps_to_reset_action() {
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    let station = app.library.radio_stations[0].clone();
    let stations = app.library.radio_stations.clone();

    let (_task, action) = app.radios_page.update(
        crate::views::RadiosMessage::ResetStationArtwork(station),
        &stations,
    );

    assert!(
        matches!(action, crate::views::RadiosAction::ResetStationArtwork(s) if s.id == "s1"),
        "ResetStationArtwork message must map to the ResetStationArtwork action"
    );
}

/// Seed every in-memory artwork identity for station `s1`.
fn seed_station_art(app: &mut crate::Nokkvi) {
    app.artwork.radio_art.put("s1".into(), handle(b"mini"));
    app.artwork
        .radio_large_art
        .put("s1".into(), handle(b"large"));
    app.artwork
        .radio_icy_captured
        .insert("s1".into(), "http://np.jpg".into());
}

/// A successful upload must invalidate every cached identity for the station
/// (so the reloaded list's fresh coverArt token re-fetches the NEW image) and
/// confirm with a success toast.
#[test]
fn radio_custom_set_applied_clears_caches_and_toasts_success() {
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    seed_station_art(&mut app);
    let station = app.library.radio_stations[0].clone();

    let _ = app.handle_radio_custom_artwork_set(
        station,
        crate::app_message::CustomArtworkOutcome::Applied,
    );

    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_large_art.contains(&"s1".to_string()));
    // The ICY dedup record is deliberately KEPT on a custom-art SET: wiping
    // it would let the ~100ms playback tick immediately re-capture the
    // stream's now-playing art and mask the just-uploaded logo for the rest
    // of the session (worst case persisting it to RadioArtStore).
    assert!(
        app.artwork.radio_icy_captured.contains_key("s1"),
        "SET must preserve the ICY dedup record so the tick can't re-capture over the new logo"
    );
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Success
    );
}

/// A cancelled file picker is a silent no-op: no toast, caches untouched.
#[test]
fn radio_custom_set_cancelled_is_silent_noop() {
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    seed_station_art(&mut app);
    let station = app.library.radio_stations[0].clone();

    let _ = app.handle_radio_custom_artwork_set(
        station,
        crate::app_message::CustomArtworkOutcome::Cancelled,
    );

    assert!(app.artwork.radio_art.contains(&"s1".to_string()));
    assert!(app.artwork.radio_large_art.contains(&"s1".to_string()));
    assert!(app.toast.toasts.is_empty(), "cancel must not toast");
}

/// A 403 refusal surfaces the friendly permission toast and leaves the
/// cached art alone (nothing changed server-side).
#[test]
fn radio_custom_set_forbidden_toasts_friendly_error() {
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    seed_station_art(&mut app);
    let station = app.library.radio_stations[0].clone();

    let _ = app.handle_radio_custom_artwork_set(
        station,
        crate::app_message::CustomArtworkOutcome::Failed(
            "Forbidden: API POST /api/radio/s1/image failed with status 403: denied".into(),
        ),
    );

    assert!(app.artwork.radio_art.contains(&"s1".to_string()));
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Error
    );
    assert!(
        app.toast.toasts[0].message.contains("not allowed"),
        "403 must map to the friendly permission message, got: {}",
        app.toast.toasts[0].message
    );
}

/// A successful reset clears the caches (ICY/glyph returns until the fresh
/// list confirms) and confirms with a success toast.
#[test]
fn radio_custom_reset_applied_clears_caches_and_toasts_success() {
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    seed_station_art(&mut app);
    let station = app.library.radio_stations[0].clone();

    let _ = app.handle_radio_custom_artwork_reset(
        station,
        crate::app_message::CustomArtworkOutcome::Applied,
    );

    assert!(!app.artwork.radio_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_large_art.contains(&"s1".to_string()));
    assert!(!app.artwork.radio_icy_captured.contains_key("s1"));
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Success
    );
}

/// A failed reset keeps the cached art (server still has the image) and
/// surfaces an error toast.
#[test]
fn radio_custom_reset_failed_toasts_error_and_keeps_caches() {
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    seed_station_art(&mut app);
    let station = app.library.radio_stations[0].clone();

    let _ = app.handle_radio_custom_artwork_reset(
        station,
        crate::app_message::CustomArtworkOutcome::Failed("connection refused".into()),
    );

    assert!(app.artwork.radio_art.contains(&"s1".to_string()));
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Error
    );
}

/// A LOCAL pick/read failure must surface verbatim as a plain error toast —
/// NEVER through the server-error classifiers. The detail embeds the picked
/// path, so a folder literally named "Unauthorized Bootlegs" must not match
/// the 401 marker and log the user out (nor may "Forbidden" in a path map to
/// the permission message).
#[test]
fn radio_local_failure_toasts_verbatim_without_session_teardown() {
    let mut app = radio_app_with_cover(Some("ra-s1_18f0"));
    seed_station_art(&mut app);
    let station = app.library.radio_stations[0].clone();
    let detail = "could not read /music/Unauthorized Bootlegs/Forbidden Fruit.png: permission denied (os error 13)";

    let _ = app.handle_radio_custom_artwork_set(
        station,
        crate::app_message::CustomArtworkOutcome::LocalFailed(detail.into()),
    );

    // State untouched — nothing changed server-side.
    assert!(app.artwork.radio_art.contains(&"s1".to_string()));
    // Exactly one ERROR toast carrying the detail verbatim. A session
    // teardown would instead push the Info "Session expired…" toast.
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Error
    );
    assert!(
        app.toast.toasts[0].message.contains(detail),
        "local detail must surface verbatim, got: {}",
        app.toast.toasts[0].message
    );
    assert!(
        !app.toast.toasts[0].message.contains("Session expired"),
        "a local path substring must not trigger session teardown"
    );
    assert!(
        !app.toast.toasts[0].message.contains("not allowed"),
        "a local path substring must not hit the 403 classifier"
    );
}

// ============================================================================
// Station-list reload: snapshot refresh + sort-signature reset
// ============================================================================

fn station(
    id: &str,
    name: &str,
    cover: Option<&str>,
) -> nokkvi_data::types::radio_station::RadioStation {
    nokkvi_data::types::radio_station::RadioStation {
        id: id.into(),
        name: name.into(),
        stream_url: format!("http://stream/{id}"),
        home_page_url: None,
        cover_art: cover.map(str::to_string),
    }
}

/// The active radio playback's station is a clone snapshotted at play time;
/// a station-list reload (e.g. right after a custom-artwork upload) must
/// refresh it from the fresh list — otherwise the panel re-warm and the ICY
/// logo gate keep reading the stale (logo-less) coverArt for the whole play.
#[test]
fn station_list_reload_refreshes_active_playback_station_snapshot() {
    // Playing s1 while it had NO logo (the primary first-upload flow).
    let mut app = radio_app_with_cover(None);

    // The reload lands carrying the freshly-uploaded logo token.
    let _ = app.handle_radio_stations_loaded(Ok(vec![station("s1", "Test", Some("ra-s1_2"))]));

    assert_eq!(
        app.active_playback
            .radio_station()
            .and_then(|s| s.logo_cover_art()),
        Some("ra-s1_2"),
        "the play-time station snapshot must be refreshed from the reloaded list"
    );
}

/// A reload with an UNKNOWN playing station (filtered/deleted server-side)
/// leaves the snapshot untouched rather than clearing it.
#[test]
fn station_list_reload_keeps_snapshot_when_station_absent() {
    let mut app = radio_app_with_cover(Some("ra-s1_1"));

    let _ = app.handle_radio_stations_loaded(Ok(vec![station("other", "Other", None)]));

    assert_eq!(
        app.active_playback.radio_station().map(|s| s.id.as_str()),
        Some("s1"),
        "an absent station must not wipe the playback snapshot"
    );
}

/// Reloading the station list must re-sort it even when the count is
/// unchanged: `sort_radio_stations` short-circuits on its cached
/// `(ascending, len)` signature, and without a reset site the reloaded
/// server-ordered list silently replaced the sorted one (latent in the
/// pre-existing R-hotkey reload; hit constantly by the upload/reset reload).
#[test]
fn station_list_reload_resets_sort_signature_and_resorts() {
    let mut app = test_app();
    // Simulate a prior sorted load of 2 stations under the same settings.
    app.radios_page.last_sort_signature = Some((true, 2));

    // Server returns them OUT of name order (raw server order).
    let _ = app.handle_radio_stations_loaded(Ok(vec![
        station("s2", "Zebra FM", None),
        station("s1", "Alpha FM", None),
    ]));

    let names: Vec<&str> = app
        .library
        .radio_stations
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["Alpha FM", "Zebra FM"],
        "a reload with an unchanged count must still re-sort the fresh list"
    );
}
