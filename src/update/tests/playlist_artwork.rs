//! Custom (user-uploaded) playlist artwork handler tests — assert observable
//! `ArtworkState` / `library.playlists` mutations (no `app_service` side
//! effects). Mirrors `radio_artwork.rs`.

use iced::widget::image;
use nokkvi_data::backend::playlists::PlaylistUIViewData;

use crate::{
    app_message::{CustomArtworkOutcome, MiniArt},
    test_helpers::test_app,
    views::{PlaylistsAction, PlaylistsMessage, playlists::PlaylistContextEntry},
};

fn handle(bytes: &[u8]) -> image::Handle {
    image::Handle::from_bytes(bytes.to_vec())
}

fn make_playlist(id: &str, name: &str, uploaded: bool) -> PlaylistUIViewData {
    PlaylistUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        comment: String::new(),
        duration: 0.0,
        song_count: 3,
        owner_name: String::new(),
        public: true,
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        artwork_album_ids: Vec::new(),
        uploaded_image: uploaded.then(|| "cover-ref".to_string()),
        is_smart: false,
        rules: None,
        evaluated_at: None,
        is_file_backed: false,
        sync: false,
        owner_id: String::new(),
        searchable_lower: name.to_lowercase(),
    }
}

/// Build a test app whose library holds one playlist `p1`.
fn playlist_app(uploaded: bool) -> crate::Nokkvi {
    let mut app = test_app();
    app.library
        .playlists
        .set_from_vec(vec![make_playlist("p1", "Road Trip", uploaded)]);
    app
}

/// Seed both custom-art caches for playlist `p1`.
fn seed_custom_art(app: &mut crate::Nokkvi) {
    app.artwork
        .playlist_custom_art
        .put("p1".into(), handle(b"mini"));
    app.artwork
        .playlist_custom_large_art
        .put("p1".into(), handle(b"large"));
}

// ============================================================================
// Message → action mapping (pure PlaylistsPage::update returns)
// ============================================================================

/// The panel-menu message carries the resolved (id, name) straight through.
#[test]
fn set_custom_artwork_message_maps_to_action() {
    let mut app = playlist_app(false);
    let playlists: Vec<_> = app.library.playlists.iter().cloned().collect();

    let (_task, action) = app.playlists_page.update(
        PlaylistsMessage::SetCustomArtwork("p1".into(), "Road Trip".into()),
        playlists.len(),
        &playlists,
    );

    assert!(
        matches!(action, PlaylistsAction::SetCustomArtwork(id, name)
            if id == "p1" && name == "Road Trip"),
        "SetCustomArtwork message must map to the SetCustomArtwork action"
    );
}

#[test]
fn reset_custom_artwork_message_maps_to_action() {
    let mut app = playlist_app(true);
    let playlists: Vec<_> = app.library.playlists.iter().cloned().collect();

    let (_task, action) = app.playlists_page.update(
        PlaylistsMessage::ResetCustomArtwork("p1".into(), "Road Trip".into()),
        playlists.len(),
        &playlists,
    );

    assert!(
        matches!(action, PlaylistsAction::ResetCustomArtwork(id, name)
            if id == "p1" && name == "Road Trip"),
        "ResetCustomArtwork message must map to the ResetCustomArtwork action"
    );
}

/// The ROW menu resolves the clicked index to the playlist at action time
/// (the existing per-row convention) and bubbles its (id, name).
#[test]
fn row_context_set_custom_artwork_resolves_clicked_playlist() {
    let mut app = playlist_app(false);
    let playlists: Vec<_> = app.library.playlists.iter().cloned().collect();

    let (_task, action) = app.playlists_page.update(
        PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::SetCustomArtwork),
        playlists.len(),
        &playlists,
    );

    assert!(
        matches!(action, PlaylistsAction::SetCustomArtwork(id, name)
            if id == "p1" && name == "Road Trip"),
        "row SetCustomArtwork must resolve the clicked playlist"
    );
}

#[test]
fn row_context_reset_artwork_resolves_clicked_playlist() {
    let mut app = playlist_app(true);
    let playlists: Vec<_> = app.library.playlists.iter().cloned().collect();

    let (_task, action) = app.playlists_page.update(
        PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::ResetArtwork),
        playlists.len(),
        &playlists,
    );

    assert!(
        matches!(action, PlaylistsAction::ResetCustomArtwork(id, name)
            if id == "p1" && name == "Road Trip"),
        "row ResetArtwork must resolve the clicked playlist"
    );
}

// ============================================================================
// Fetch-completion handlers
// ============================================================================

#[test]
fn custom_mini_loaded_stores_handle() {
    let mut app = test_app();
    let _ = app.handle_playlist_custom_mini_loaded(
        "p1".into(),
        Some("v1".into()),
        MiniArt::Loaded(handle(b"img")),
    );
    assert!(app.artwork.playlist_custom_art.contains(&"p1".to_string()));
}

#[test]
fn custom_mini_missing_stores_nothing() {
    let mut app = test_app();
    let _ =
        app.handle_playlist_custom_mini_loaded("p1".into(), Some("v1".into()), MiniArt::Missing);
    assert!(!app.artwork.playlist_custom_art.contains(&"p1".to_string()));
}

#[test]
fn custom_large_loaded_stores_into_large_cache_only() {
    let mut app = test_app();
    let _ = app.handle_playlist_custom_large_loaded("p1".into(), Some(handle(b"big")));
    assert!(
        app.artwork
            .playlist_custom_large_art
            .contains(&"p1".to_string())
    );
    assert!(!app.artwork.playlist_custom_art.contains(&"p1".to_string()));
}

// ============================================================================
// Set / Reset completions
// ============================================================================

/// A successful upload must drop any stale cached custom art (the refetch
/// carries a fresh cache-buster), optimistically mark the library row as
/// having an uploaded image (so display/menu gating flips without a full
/// list reload), and confirm with a success toast.
#[test]
fn playlist_custom_set_applied_clears_caches_marks_uploaded_and_toasts() {
    let mut app = playlist_app(false);
    seed_custom_art(&mut app);

    let _ = app.handle_playlist_custom_artwork_set(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::Applied,
    );

    assert!(!app.artwork.playlist_custom_art.contains(&"p1".to_string()));
    assert!(
        !app.artwork
            .playlist_custom_large_art
            .contains(&"p1".to_string())
    );
    assert!(
        app.library
            .playlists
            .iter()
            .find(|p| p.id == "p1")
            .and_then(|p| p.uploaded_image.as_ref())
            .is_some(),
        "the library row must be optimistically marked as having custom art"
    );
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Success
    );
}

/// A cancelled file picker is a silent no-op.
#[test]
fn playlist_custom_set_cancelled_is_silent_noop() {
    let mut app = playlist_app(false);
    seed_custom_art(&mut app);

    let _ = app.handle_playlist_custom_artwork_set(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::Cancelled,
    );

    assert!(app.artwork.playlist_custom_art.contains(&"p1".to_string()));
    assert!(
        app.library
            .playlists
            .iter()
            .find(|p| p.id == "p1")
            .is_some_and(|p| p.uploaded_image.is_none()),
        "cancel must not mark the row"
    );
    assert!(app.toast.toasts.is_empty(), "cancel must not toast");
}

/// A 403 refusal (uploads disabled / not the owner) surfaces the friendly
/// permission toast and leaves state alone.
#[test]
fn playlist_custom_set_forbidden_toasts_friendly_error() {
    let mut app = playlist_app(false);

    let _ = app.handle_playlist_custom_artwork_set(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::Failed(
            "Forbidden: API POST /api/playlist/p1/image failed with status 403: not owner".into(),
        ),
    );

    assert!(
        app.library
            .playlists
            .iter()
            .find(|p| p.id == "p1")
            .is_some_and(|p| p.uploaded_image.is_none()),
        "a failed upload must not mark the row"
    );
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

/// A successful reset clears the optimistic field (collage/quad instantly
/// return — their caches were never touched), drops the custom caches, and
/// confirms with a success toast.
#[test]
fn playlist_custom_reset_applied_clears_field_caches_and_toasts() {
    let mut app = playlist_app(true);
    seed_custom_art(&mut app);

    let _ = app.handle_playlist_custom_artwork_reset(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::Applied,
    );

    assert!(!app.artwork.playlist_custom_art.contains(&"p1".to_string()));
    assert!(
        !app.artwork
            .playlist_custom_large_art
            .contains(&"p1".to_string())
    );
    assert!(
        app.library
            .playlists
            .iter()
            .find(|p| p.id == "p1")
            .is_some_and(|p| p.uploaded_image.is_none()),
        "reset must clear the row's uploaded_image so the collage returns"
    );
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Success
    );
}

/// A failed reset keeps the field and caches (server still has the image)
/// and surfaces an error toast.
#[test]
fn playlist_custom_reset_failed_keeps_state_and_toasts_error() {
    let mut app = playlist_app(true);
    seed_custom_art(&mut app);

    let _ = app.handle_playlist_custom_artwork_reset(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::Failed("connection refused".into()),
    );

    assert!(app.artwork.playlist_custom_art.contains(&"p1".to_string()));
    assert!(
        app.library
            .playlists
            .iter()
            .find(|p| p.id == "p1")
            .is_some_and(|p| p.uploaded_image.is_some()),
        "a failed reset must keep the row marked"
    );
    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Error
    );
}

/// Playlist twin of the radio local-failure test: local pick/read failures
/// surface verbatim and never flow through the server-error classifiers.
#[test]
fn playlist_local_failure_toasts_verbatim_without_session_teardown() {
    let mut app = playlist_app(true);
    seed_custom_art(&mut app);
    let detail = "could not read /music/Unauthorized Bootlegs/cover.png: permission denied";

    let _ = app.handle_playlist_custom_artwork_set(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::LocalFailed(detail.into()),
    );

    assert!(app.artwork.playlist_custom_art.contains(&"p1".to_string()));
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
}

// ============================================================================
// Prefetch gates: pending, version awareness, negative cache
// ============================================================================

/// The viewport decision must skip ids with a fetch already in flight —
/// otherwise every scroll step re-dispatches duplicates for a cold viewport.
#[test]
fn custom_mini_decision_skips_in_flight_ids() {
    let app = {
        let mut app = playlist_app(true);
        app.artwork
            .playlist_custom_art_pending
            .insert("p1".to_string());
        app
    };
    assert!(
        app.playlist_custom_minis_to_fetch().is_empty(),
        "an in-flight id must not be re-queued"
    );
}

/// A cover replaced in the web UI bumps the playlist's `updated_at`; the
/// decision must treat a warmed-but-stale-version id as a MISS so the new
/// cover shows this session.
#[test]
fn custom_mini_decision_refetches_on_version_change() {
    let mut app = playlist_app(true); // updated_at = 2026-01-01T00:00:00Z
    app.artwork
        .playlist_custom_art
        .put("p1".into(), handle(b"old"));
    app.artwork
        .playlist_custom_art_versions
        .insert("p1".into(), Some("1999-01-01T00:00:00Z".into()));

    let to_fetch = app.playlist_custom_minis_to_fetch();
    assert_eq!(
        to_fetch.len(),
        1,
        "a warmed id whose recorded version no longer matches must refetch"
    );
    assert_eq!(to_fetch[0].0, "p1");

    // And with a MATCHING recorded version, the warm id stays skipped.
    app.artwork
        .playlist_custom_art_versions
        .insert("p1".into(), Some("2026-01-01T00:00:00Z".into()));
    assert!(
        app.playlist_custom_minis_to_fetch().is_empty(),
        "a warm, version-current id must not refetch"
    );
}

/// A persistently failing id (stale uploaded_image whose file is gone) must
/// not re-fire on every scroll step — unless its updated_at changes.
#[test]
fn custom_mini_decision_honors_negative_cache_until_version_changes() {
    let mut app = playlist_app(true); // updated_at = 2026-01-01T00:00:00Z
    app.artwork
        .playlist_custom_art_failed
        .insert("p1".into(), Some("2026-01-01T00:00:00Z".into()));
    assert!(
        app.playlist_custom_minis_to_fetch().is_empty(),
        "an id that failed at THIS version must not be re-queued"
    );

    // The failure was recorded at an OLDER version — a changed updated_at
    // bypasses the negative entry and re-attempts.
    app.artwork
        .playlist_custom_art_failed
        .insert("p1".into(), Some("1999-01-01T00:00:00Z".into()));
    assert_eq!(
        app.playlist_custom_minis_to_fetch().len(),
        1,
        "a changed updated_at must bypass the negative cache"
    );
}

/// Completions release the in-flight slot on success AND failure, record the
/// warming version on success, and negatively cache a Missing result.
#[test]
fn custom_mini_loaded_releases_pending_and_records_version() {
    let mut app = test_app();
    app.artwork
        .playlist_custom_art_pending
        .insert("p1".to_string());

    let _ = app.handle_playlist_custom_mini_loaded(
        "p1".into(),
        Some("v2".into()),
        MiniArt::Loaded(handle(b"img")),
    );

    assert!(
        !app.artwork.playlist_custom_art_pending.contains("p1"),
        "success must release the in-flight slot"
    );
    assert_eq!(
        app.artwork.playlist_custom_art_versions.get("p1"),
        Some(&Some("v2".to_string())),
        "success must record the version that warmed the slot"
    );
}

#[test]
fn custom_mini_missing_negative_caches_and_releases_pending() {
    let mut app = test_app();
    app.artwork
        .playlist_custom_art_pending
        .insert("p1".to_string());

    let _ =
        app.handle_playlist_custom_mini_loaded("p1".into(), Some("v2".into()), MiniArt::Missing);

    assert!(
        !app.artwork.playlist_custom_art_pending.contains("p1"),
        "failure must release the in-flight slot too (retry stays possible)"
    );
    assert_eq!(
        app.artwork.playlist_custom_art_failed.get("p1"),
        Some(&Some("v2".to_string())),
        "a Missing result must be negatively cached at its version"
    );
}

#[test]
fn custom_mini_transient_releases_pending_and_records_nothing() {
    let mut app = test_app();
    app.artwork
        .playlist_custom_art_pending
        .insert("p1".to_string());

    let _ =
        app.handle_playlist_custom_mini_loaded("p1".into(), Some("v2".into()), MiniArt::Transient);

    assert!(!app.artwork.playlist_custom_art_pending.contains("p1"));
    assert!(app.artwork.playlist_custom_art_versions.is_empty());
    assert!(app.artwork.playlist_custom_art_failed.is_empty());
}

/// The large-panel cache short-circuit must be version-aware too — a cached
/// large cover whose recorded (mini) version no longer matches the live
/// updated_at must refetch instead of serving the stale image forever.
#[test]
fn custom_large_short_circuit_is_version_aware() {
    let mut app = playlist_app(true); // updated_at = 2026-01-01T00:00:00Z
    app.artwork
        .playlist_custom_large_art
        .put("p1".into(), handle(b"big"));
    app.artwork
        .playlist_custom_art_versions
        .insert("p1".into(), Some("1999-01-01T00:00:00Z".into()));

    assert!(
        !app.playlist_custom_large_is_current("p1", "2026-01-01T00:00:00Z"),
        "a cached large cover at a stale version must NOT be served"
    );

    app.artwork
        .playlist_custom_art_versions
        .insert("p1".into(), Some("2026-01-01T00:00:00Z".into()));
    assert!(
        app.playlist_custom_large_is_current("p1", "2026-01-01T00:00:00Z"),
        "cached + version-current must serve from cache"
    );
}

// ============================================================================
// Optimistic updated_at bump on upload success
// ============================================================================

/// The optimistic update must bump `updated_at` alongside `uploaded_image`:
/// every LATER refetch (viewport prefetch, post-LRU-eviction large load)
/// re-sends the `_u=` cache-buster derived from it, and a stale value could
/// let an intermediary HTTP cache serve the pre-upload cover.
#[test]
fn playlist_custom_set_applied_bumps_updated_at_and_marks_pending() {
    let mut app = playlist_app(false); // updated_at = 2026-01-01T00:00:00Z
    app.artwork
        .playlist_custom_art_versions
        .insert("p1".into(), Some("stale".into()));
    app.artwork
        .playlist_custom_art_failed
        .insert("p1".into(), Some("stale".into()));

    let _ = app.handle_playlist_custom_artwork_set(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::Applied,
    );

    let p = app
        .library
        .playlists
        .iter()
        .find(|p| p.id == "p1")
        .expect("playlist present");
    assert_ne!(
        p.updated_at, "2026-01-01T00:00:00Z",
        "the optimistic update must bump updated_at so later refetches carry a fresh buster"
    );
    assert!(
        p.updated_at.contains('T'),
        "the bumped value must stay in the server's RFC 3339 shape: {}",
        p.updated_at
    );
    assert!(
        app.artwork.playlist_custom_art_pending.contains("p1"),
        "the post-upload mini refetch must be marked in flight"
    );
    assert!(
        !app.artwork.playlist_custom_art_versions.contains_key("p1"),
        "stale version records must be dropped with the caches"
    );
    assert!(
        !app.artwork.playlist_custom_art_failed.contains_key("p1"),
        "stale negative-cache records must be dropped with the caches"
    );
}

/// Reset drops the version/negative-cache records with the caches so a later
/// re-upload starts from a clean slate.
#[test]
fn playlist_custom_reset_applied_clears_version_records() {
    let mut app = playlist_app(true);
    seed_custom_art(&mut app);
    app.artwork
        .playlist_custom_art_versions
        .insert("p1".into(), Some("v".into()));
    app.artwork
        .playlist_custom_art_failed
        .insert("p1".into(), Some("v".into()));

    let _ = app.handle_playlist_custom_artwork_reset(
        "p1".into(),
        "Road Trip".into(),
        CustomArtworkOutcome::Applied,
    );

    assert!(!app.artwork.playlist_custom_art_versions.contains_key("p1"));
    assert!(!app.artwork.playlist_custom_art_failed.contains_key("p1"));
}
