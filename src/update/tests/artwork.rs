//! Tests for the artwork fetch dedup gates.
//!
//! The version-aware prefetch gate (N17) pins the invariant that a
//! server-side cover change (a different `updated_at`) makes an
//! already-warmed `album_art` slot a genuine prefetch MISS, so the passive
//! mini-thumbnail path re-fetches the new cover instead of serving the stale
//! one for the rest of the session. The large-art in-flight gate (bottom)
//! pins that re-centering a row whose large art is still loading sends no
//! second request.

use std::collections::{HashMap, HashSet};

use super::library::test_app_with_shell;
use crate::{
    app_message::{ArtworkMessage, MiniArt},
    test_helpers::{
        make_album, make_artist, make_queue_song, make_song, seed_albums, seed_artists, seed_songs,
        test_app,
    },
    update::{
        components::{passive_artwork_version, should_refetch},
        loader_target::{AlbumsTarget, LoaderTarget, SongsTarget},
    },
};

/// Empty negative cache — the default for the version-behaviour tests below
/// (they assert `should_refetch`'s membership/version logic, not the
/// negative-cache short-circuit, which has its own tests at the bottom).
fn no_failures() -> HashMap<String, Option<String>> {
    HashMap::new()
}

/// Build a `(cached_ids, versions)` pair the way a prefetch tick would: the
/// version map is kept in lockstep with the `album_art` keys.
fn seeded() -> (Vec<String>, HashMap<String, Option<String>>) {
    let ids = vec!["al-1".to_string()];
    let mut versions = HashMap::new();
    versions.insert("al-1".to_string(), Some("v1".to_string()));
    (ids, versions)
}

#[test]
fn prefetch_dedup_treats_changed_updated_at_as_cache_miss() {
    let (ids, versions) = seeded();
    let cached: HashSet<&String> = ids.iter().collect();

    // Same version → already warm → NOT a refetch.
    assert!(
        !should_refetch(
            &cached,
            &versions,
            &no_failures(),
            &"al-1".to_string(),
            &Some("v1".to_string())
        ),
        "unchanged cover must not re-fetch",
    );

    // Changed version → server cover changed → MUST refetch.
    assert!(
        should_refetch(
            &cached,
            &versions,
            &no_failures(),
            &"al-1".to_string(),
            &Some("v2".to_string())
        ),
        "a changed updated_at must be treated as a cache miss",
    );
}

#[test]
fn prefetch_dedup_unknown_id_is_a_miss() {
    let (ids, versions) = seeded();
    let cached: HashSet<&String> = ids.iter().collect();

    assert!(
        should_refetch(
            &cached,
            &versions,
            &no_failures(),
            &"al-unknown".to_string(),
            &Some("v1".to_string())
        ),
        "an id absent from album_art must always be a miss",
    );
}

#[test]
fn prefetch_dedup_evicted_handle_is_a_miss() {
    // Version recorded but the handle was evicted from album_art (capacity
    // skew): membership check, not just the version map, must catch it.
    let mut versions = HashMap::new();
    versions.insert("al-1".to_string(), Some("v1".to_string()));
    let cached: HashSet<&String> = HashSet::new(); // album_art is empty

    assert!(
        should_refetch(
            &cached,
            &versions,
            &no_failures(),
            &"al-1".to_string(),
            &Some("v1".to_string())
        ),
        "a version hit whose album_art handle was evicted must re-fetch",
    );
}

/// F4: a single-album queue must not re-fetch the same 80px thumbnail every
/// scroll tick. The bug was that the album_id-keyed version map was fed the
/// PER-SONG `updated_at`, which differs across the album's tracks; across
/// prefetch batches the recorded version oscillated, `should_refetch` kept
/// returning true, `album_art` was re-put, and `Handle::from_bytes` minted a
/// fresh texture for identical bytes → flicker.
///
/// The fix feeds a constant `None` as the version at the passive surfaces
/// (queue / song-mini / similar / editor), so every track of one album maps to
/// the same recorded version and the gate is stable. This test pins both
/// halves: (1) the post-fix contract — `None` on both ticks never oscillates,
/// and (2) the pre-fix oscillation at the pure `should_refetch` level, proving
/// WHY the call sites had to stop passing per-song versions (its semantics are
/// intentionally left unchanged).
#[test]
fn passive_single_album_queue_does_not_oscillate() {
    let album = "al-1".to_string();
    // Two tracks of the same album carrying DIFFERENT per-song updated_at —
    // exactly what `Song.updated_at` does per mediafile. After F4 the passive
    // closures discard these in favour of `passive_artwork_version()`, so the
    // recorded album-keyed version is constant regardless of which track's row
    // drives a given prefetch tick.
    let song_a_updated = Some("2026-01-01".to_string());
    let song_b_updated = Some("2026-02-02".to_string());

    // The passive surfaces feed THIS as the version, not the per-song value.
    // It must be the same for every track so the album-keyed gate stays warm.
    let passive_a = passive_artwork_version(&song_a_updated);
    let passive_b = passive_artwork_version(&song_b_updated);
    assert_eq!(
        passive_a, passive_b,
        "passive surfaces must feed an album-coherent version, identical across \
         tracks of one album — otherwise the album-keyed gate oscillates",
    );

    // Tick 1 (cold): nothing cached → miss; the loaded handler records the
    // passive version for song A's row.
    let cold: HashSet<&String> = HashSet::new();
    let mut versions: HashMap<String, Option<String>> = HashMap::new();
    assert!(
        should_refetch(&cold, &versions, &no_failures(), &album, &passive_a),
        "cold slot is a miss",
    );
    versions.insert(album.clone(), passive_a);

    // Tick 2 (warm): the next same-album row (song B, different per-song
    // updated_at) feeds its passive version → must NOT re-fetch.
    let warm_ids = [album.clone()];
    let warm: HashSet<&String> = warm_ids.iter().collect();
    assert!(
        !should_refetch(&warm, &versions, &no_failures(), &album, &passive_b),
        "a same-album row must not re-fetch once the album slot is version-warm",
    );

    // ── Pre-fix oscillation, documented at the pure gate level ──
    // Feeding the raw per-song versions (what the call sites USED to do)
    // re-fetches forever: tick 1 recorded song A's version, tick 2 arrives with
    // song B's. `should_refetch`'s own semantics are intentionally unchanged.
    let mut versions_song_a: HashMap<String, Option<String>> = HashMap::new();
    versions_song_a.insert(album.clone(), song_a_updated);
    assert!(
        should_refetch(
            &warm,
            &versions_song_a,
            &no_failures(),
            &album,
            &song_b_updated
        ),
        "pre-fix: differing per-song updated_at oscillates the album-keyed gate \
         (the bug F4 removes at the call sites)",
    );
}

/// Negative cache: once a cover fetch returns no image, `should_refetch` must
/// stop re-queuing that id at the SAME version even though it is absent from
/// `album_art` — otherwise every scroll/resize re-fetches a known-dead id.
#[test]
fn negative_cache_suppresses_refetch_of_a_known_failed_id() {
    let album = "al-dead".to_string();
    let versions: HashMap<String, Option<String>> = HashMap::new();
    let cached: HashSet<&String> = HashSet::new(); // never warmed

    // Cold + not failed → miss (would fetch).
    assert!(
        should_refetch(
            &cached,
            &versions,
            &no_failures(),
            &album,
            &Some("v1".to_string())
        ),
        "a never-tried id must be a miss",
    );

    // The loaded-handler recorded the failure at version v1.
    let mut failed: HashMap<String, Option<String>> = HashMap::new();
    failed.insert(album.clone(), Some("v1".to_string()));

    // Same id, same version, still absent from album_art → suppressed.
    assert!(
        !should_refetch(&cached, &versions, &failed, &album, &Some("v1".to_string())),
        "a known-failed id at the same version must not be re-fetched",
    );
}

/// A changed `updated_at` (server cover added) must bypass the negative entry
/// and re-attempt — otherwise a cover that later appears stays permanently
/// blank, since SSE reloads never clear the cache and the version bump is the
/// only re-attempt signal.
#[test]
fn negative_cache_is_bypassed_when_the_version_changes() {
    let album = "al-dead".to_string();
    let versions: HashMap<String, Option<String>> = HashMap::new();
    let cached: HashSet<&String> = HashSet::new();

    let mut failed: HashMap<String, Option<String>> = HashMap::new();
    failed.insert(album.clone(), Some("v1".to_string()));

    assert!(
        should_refetch(&cached, &versions, &failed, &album, &Some("v2".to_string())),
        "a bumped updated_at must re-attempt a previously-failed cover",
    );
}

// ============================================================================
// Large-art in-flight dedup
// ============================================================================

/// Re-centering an album whose large art is still loading sends no second
/// request. On a slow server a held arrow key at the end of the Albums list
/// stacked hundreds of full-size requests for the one centered album.
#[tokio::test]
async fn album_large_art_in_flight_is_not_requested_again() {
    let (mut app, db_path) = test_app_with_shell().await;

    assert!(
        app.handle_load_large_artwork("a1".into()).units() >= 1,
        "the first centering fetches"
    );
    assert_eq!(
        app.handle_load_large_artwork("a1".into()).units(),
        0,
        "a repeat while the fetch is in flight sends nothing"
    );
    assert!(
        app.handle_load_large_artwork("a2".into()).units() >= 1,
        "another album is not held up by the first"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// A failed fetch releases its claim, so the next centering retries.
#[tokio::test]
async fn album_large_art_failure_releases_the_claim() {
    let (mut app, db_path) = test_app_with_shell().await;

    let _ = app.handle_load_large_artwork("a1".into());
    let _ = app.handle_large_artwork_loaded("a1".into(), None);

    assert!(
        app.handle_load_large_artwork("a1".into()).units() >= 1,
        "a failed fetch must not block the retry"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// A cover replaced on the server while its old fetch is in flight is
/// requested at the new version; the gate holds back only exact repeats, so
/// it can never pin the old image in the cache.
#[tokio::test]
async fn album_large_art_at_a_new_version_is_requested_while_the_old_is_in_flight() {
    let (mut app, db_path) = test_app_with_shell().await;
    let mut album = make_album("a1", "Album", "Artist");
    album.updated_at = Some("v1".into());
    seed_albums(&mut app, vec![album.clone()]);

    let _ = app.handle_load_large_artwork("a1".into());
    album.updated_at = Some("v2".into());
    seed_albums(&mut app, vec![album]);

    assert!(
        app.handle_load_large_artwork("a1".into()).units() >= 1,
        "the new version goes out"
    );
    assert_eq!(
        app.handle_load_large_artwork("a1".into()).units(),
        0,
        "and is then held like any other in-flight fetch"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// The artist panel shares the album gate (same cache, same id space).
#[tokio::test]
async fn artist_large_art_in_flight_is_not_requested_again() {
    let (mut app, db_path) = test_app_with_shell().await;

    assert!(
        app.handle_load_artist_large_artwork("ar1".into()).units() >= 1,
        "the first centering fetches"
    );
    assert_eq!(
        app.handle_load_artist_large_artwork("ar1".into()).units(),
        0,
        "a repeat while the fetch is in flight sends nothing"
    );

    let _ = app.dispatch_artwork(ArtworkMessage::LargeArtistLoaded("ar1".into(), None));
    assert!(
        app.handle_load_artist_large_artwork("ar1".into()).units() >= 1,
        "a failed fetch must not block the retry"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// Without a session nothing is fetched, so nothing is claimed. Artwork state
/// is reset at logout, not at login, so a claim made on the login screen
/// would block that cover for the whole next session.
#[test]
fn large_art_without_a_session_claims_nothing() {
    let mut app = test_app();

    let _ = app.handle_load_large_artwork("a1".into());
    let _ = app.handle_load_artist_large_artwork("ar1".into());

    assert!(app.artwork.large_artwork_pending.is_empty());
}

// ============================================================================
// Thumbnail in-flight dedup
// ============================================================================
//
// The viewport prefetch re-runs on every scroll step and gated only on the
// thumbnails that had ARRIVED, so on a slow server every step re-requested
// every thumbnail still on its way (one was fetched 29 times in a demo-server
// session). Each surface now claims its ids in `album_art_pending` and the
// loaded handler releases them on every outcome.

fn album_with_art(id: &str) -> nokkvi_data::backend::albums::AlbumUIViewData {
    let mut album = make_album(id, "Album", "Artist");
    album.artwork_url = format!("http://srv/art/{id}");
    album
}

/// A second Albums viewport pass while the first pass's thumbnails are still
/// loading requests nothing; a settled fetch (any outcome) frees its id.
#[tokio::test]
async fn album_thumbnails_in_flight_are_not_requested_again() {
    let (mut app, db_path) = test_app_with_shell().await;
    seed_albums(
        &mut app,
        vec![
            album_with_art("a1"),
            album_with_art("a2"),
            album_with_art("a3"),
        ],
    );

    assert_eq!(
        AlbumsTarget::prefetch_artwork_tasks(&mut app).len(),
        3,
        "a cold viewport fetches every row"
    );
    assert!(
        AlbumsTarget::prefetch_artwork_tasks(&mut app).is_empty(),
        "a second pass while they load requests nothing"
    );

    let _ = app.handle_artwork_loaded("a2".into(), None, MiniArt::Transient);
    assert_eq!(
        AlbumsTarget::prefetch_artwork_tasks(&mut app).len(),
        1,
        "a settled fetch is planned again, the others stay held"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// The song-mini surfaces (Songs view, Similar) share the gate and release
/// through their own loaded handler.
#[tokio::test]
async fn song_thumbnails_in_flight_are_not_requested_again() {
    let (mut app, db_path) = test_app_with_shell().await;
    seed_songs(
        &mut app,
        vec![make_song("s1", "T", "A"), make_song("s2", "T", "A")],
    );

    assert_eq!(SongsTarget::prefetch_artwork_tasks(&mut app).len(), 2);
    assert!(
        SongsTarget::prefetch_artwork_tasks(&mut app).is_empty(),
        "a second pass while they load requests nothing"
    );

    let _ = app.handle_song_artwork_loaded("album_s1".into(), None, MiniArt::Transient);
    assert_eq!(
        SongsTarget::prefetch_artwork_tasks(&mut app).len(),
        1,
        "a settled fetch is planned again"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// The Artists viewport's `ar-` minis share the gate.
#[tokio::test]
async fn artist_thumbnails_in_flight_are_not_requested_again() {
    let (mut app, db_path) = test_app_with_shell().await;
    seed_artists(
        &mut app,
        vec![make_artist("ar1", "One"), make_artist("ar2", "Two")],
    );

    assert_eq!(app.prefetch_artist_mini_artwork_tasks().units(), 2);
    assert_eq!(
        app.prefetch_artist_mini_artwork_tasks().units(),
        0,
        "a second pass while they load requests nothing"
    );

    let _ = app.handle_artwork_loaded("ar1".into(), None, MiniArt::Transient);
    assert_eq!(
        app.prefetch_artist_mini_artwork_tasks().units(),
        1,
        "a settled fetch is planned again"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// Harbour's shelf covers go through the expansion fan-out, which also
/// claims what it sends.
#[tokio::test]
async fn harbour_shelf_covers_in_flight_are_not_requested_again() {
    let (mut app, db_path) = test_app_with_shell().await;
    app.harbour.recently_added = vec![album_with_art("a1"), album_with_art("a2")];

    assert_eq!(app.warm_harbour_artwork(0).units(), 2);
    assert_eq!(
        app.warm_harbour_artwork(0).units(),
        0,
        "a re-entry while they load requests nothing"
    );

    let _ = app.handle_artwork_loaded("a1".into(), None, MiniArt::Transient);
    assert_eq!(app.warm_harbour_artwork(0).units(), 1);

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// The song-change warm of the playing album stands down while a viewport
/// pass already has that cover on its way.
#[test]
fn now_playing_warm_skipped_while_the_album_is_in_flight() {
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.artwork.album_art_pending.insert("album_s1".to_string());

    assert!(app.now_playing_artwork_to_warm().is_none());
}

// ============================================================================
// A surface with no version to offer (the passive `None`) never disagrees
// ============================================================================

fn warm_app_at(version: Option<&str>) -> crate::Nokkvi {
    let mut app = test_app();
    let _ = app.record_mini_art(
        "al-1",
        &version.map(str::to_string),
        MiniArt::Loaded(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4])),
    );
    app
}

fn needs(app: &crate::Nokkvi, version: Option<&str>) -> bool {
    crate::update::components::ArtworkGate::new(&app.artwork)
        .needs_fetch(&"al-1".to_string(), &version.map(str::to_string))
}

/// The Albums view warmed the cover at its image hash; the Queue then asks
/// with the passive `None`. The two used to disagree forever: each switch
/// between the views re-downloaded every thumbnail both showed.
#[test]
fn a_passive_surface_takes_a_cover_the_albums_view_warmed() {
    let app = warm_app_at(Some("hash-1"));
    assert!(
        !needs(&app, passive_artwork_version(&None).as_deref()),
        "a surface with no version to compare takes the warm cover",
    );
    assert!(!needs(&app, Some("hash-1")));
    assert!(
        needs(&app, Some("hash-2")),
        "a changed cover still re-fetches"
    );
}

/// A passive surface re-warms an evicted cover: the version the album views
/// recorded for it stays, so they do not fetch it once more.
#[test]
fn a_passive_rewarm_keeps_the_version_the_albums_view_recorded() {
    let mut app = warm_app_at(Some("hash-1"));
    let _ = app.record_mini_art(
        "al-1",
        &None,
        MiniArt::Loaded(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4])),
    );
    assert!(!needs(&app, Some("hash-1")));
}

/// A cover only passive surfaces ever fetched has no version on record; the
/// Albums view fetches it once at its hash, and both sides agree from then on.
#[test]
fn a_cover_warmed_passively_is_fetched_once_at_its_hash() {
    let mut app = warm_app_at(None);
    assert!(needs(&app, Some("hash-1")));
    let _ = app.record_mini_art(
        "al-1",
        &Some("hash-1".to_string()),
        MiniArt::Loaded(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4])),
    );
    assert!(!needs(&app, Some("hash-1")));
    assert!(!needs(&app, None));
}

/// The same for a cover the server does not have: a failure recorded at a
/// hash stops a passive surface asking, and the passive failure does not
/// replace it.
#[test]
fn a_passive_surface_does_not_retry_a_cover_that_failed_at_a_hash() {
    let mut app = test_app();
    let _ = app.record_mini_art("al-1", &Some("hash-1".to_string()), MiniArt::Missing);
    assert!(!needs(&app, None));

    let _ = app.record_mini_art("al-1", &None, MiniArt::Missing);
    assert!(!needs(&app, Some("hash-1")));
    assert!(needs(&app, Some("hash-2")), "a changed version asks again");
}

// ---------------------------------------------------------------------------
// A full-size cover reaches the panel only once it is on the GPU
// ---------------------------------------------------------------------------

/// A cover: any handle will do, each with its own id.
fn cover() -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255])
}

fn shown(app: &crate::Nokkvi, id: &str) -> Option<iced::advanced::image::Id> {
    app.artwork
        .large_artwork
        .peek(&id.to_string())
        .map(iced::widget::image::Handle::id)
}

fn staged(app: &crate::Nokkvi, id: &str) -> Option<iced::advanced::image::Id> {
    app.artwork
        .large_staging
        .get(id)
        .map(iced::widget::image::Handle::id)
}

/// The renderer answers for the staged cover `handle`. A test cannot make an
/// allocation, so this is the answer without one (a picture that would not
/// decode, no window open): the panel is given the cover either way.
fn staging_ends(app: &mut crate::Nokkvi, id: &str, handle: &iced::widget::image::Handle) {
    let _ = app.dispatch_artwork(ArtworkMessage::LargeStaged {
        id: id.to_string(),
        handle_id: handle.id(),
        allocation: None,
    });
}

/// The renderer decodes a new image on a worker thread and draws nothing for
/// it meanwhile. A cover handed to the panel the moment its bytes arrived
/// left the panel empty for a frame or more, between the thumbnail and the
/// cover. It now waits out of the panel's cache, so the thumbnail stays up.
#[test]
fn a_cover_that_just_arrived_is_staged_and_the_panel_keeps_the_thumbnail() {
    let mut app = test_app();
    let cover = cover();
    let _ = app.handle_large_artwork_loaded("a1".into(), Some(cover.clone()));
    assert_eq!(shown(&app, "a1"), None, "not in the panel's cache yet");
    assert_eq!(staged(&app, "a1"), Some(cover.id()));

    staging_ends(&mut app, "a1", &cover);
    assert_eq!(shown(&app, "a1"), Some(cover.id()), "now the panel has it");
    assert_eq!(staged(&app, "a1"), None);
}

/// A cover shown again while it was no longer held on the GPU is allocated
/// again, and that answer arrives with nothing staged: it changes nothing.
#[test]
fn an_answer_with_nothing_staged_changes_nothing() {
    let mut app = test_app();
    let cover = cover();
    let _ = app.handle_large_artwork_loaded("a1".into(), Some(cover.clone()));
    staging_ends(&mut app, "a1", &cover);
    staging_ends(&mut app, "a1", &cover);
    assert_eq!(shown(&app, "a1"), Some(cover.id()));
    assert_eq!(staged(&app, "a1"), None);
}

/// A cover that changed on the server, or is being refreshed, is dropped
/// while it is staged: its answer must not put the old picture back. A newer
/// cover staged under the same id is not shown by the older one's answer.
#[test]
fn an_answer_for_a_cover_no_longer_staged_shows_nothing() {
    let mut app = test_app();
    let old = cover();
    let _ = app.handle_large_artwork_loaded("a1".into(), Some(old.clone()));
    app.artwork.drop_large_cover("a1");
    staging_ends(&mut app, "a1", &old);
    assert_eq!(shown(&app, "a1"), None);

    let new = cover();
    let _ = app.handle_large_artwork_loaded("a1".into(), Some(old.clone()));
    let _ = app.handle_large_artwork_loaded("a1".into(), Some(new.clone()));
    staging_ends(&mut app, "a1", &old);
    assert_eq!(shown(&app, "a1"), None, "the older cover's answer");
    assert_eq!(staged(&app, "a1"), Some(new.id()));
    staging_ends(&mut app, "a1", &new);
    assert_eq!(shown(&app, "a1"), Some(new.id()));
}

/// While its cover is staged an album or artist is centred again and again
/// (a held key, a reload): the bytes are here, so nothing is fetched.
#[test]
fn a_staged_cover_is_not_fetched_again() {
    let mut app = test_app();
    seed_albums(&mut app, vec![make_album("a1", "Album", "Artist")]);
    seed_artists(&mut app, vec![make_artist("ar1", "Artist")]);
    let _ = app.handle_large_artwork_loaded("a1".into(), Some(cover()));
    let _ = app.handle_large_artwork_loaded("ar1".into(), Some(cover()));

    let _ = app.handle_load_large_artwork("a1".into());
    assert_eq!(app.artwork.loading_large_artwork, None, "no album fetch");
    let _ = app.handle_load_artist_large_artwork("ar1".into());
    assert_eq!(app.artwork.loading_large_artwork, None, "no artist fetch");
}

/// A cover the panel already shows is delivered again whenever its row is
/// centred (the loader answers from the cache). It stays where it is: taking
/// it out to stage it would swap the panel back to the thumbnail.
#[test]
fn a_cover_already_shown_stays_shown_when_it_is_delivered_again() {
    let mut app = test_app();
    let cover = cover();
    let _ = app.handle_large_artwork_loaded("a1".into(), Some(cover.clone()));
    staging_ends(&mut app, "a1", &cover);

    let _ = app.handle_large_artwork_loaded("a1".into(), Some(cover.clone()));
    assert_eq!(shown(&app, "a1"), Some(cover.id()));
    assert_eq!(staged(&app, "a1"), None);
}

/// A manual Refresh Artwork brings a new picture: it is staged like any
/// other, the panel on the thumbnail until it is ready.
#[test]
fn a_refreshed_cover_is_staged_too() {
    let mut app = test_app();
    let cover = cover();
    let _ = app.handle_refresh_complete("a1".into(), None, Some(cover.clone()));
    assert_eq!(shown(&app, "a1"), None);
    assert_eq!(staged(&app, "a1"), Some(cover.id()));
}

// ---------------------------------------------------------------------------
// The panel's cover cache is bounded by bytes, not only by entries
// ---------------------------------------------------------------------------

/// A cover of `mib` MiB. Its bytes are never read, so the test pays for none
/// of them.
fn big_cover(mib: usize) -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_bytes(vec![0u8; mib * 1024 * 1024])
}

/// A full-size cover arrives and reaches the panel's cache.
fn cover_lands(app: &mut crate::Nokkvi, id: &str, cover: iced::widget::image::Handle) {
    let _ = app.handle_large_artwork_loaded(id.into(), Some(cover.clone()));
    staging_ends(app, id, &cover);
}

/// Navidrome answers nokkvi's square covers as PNG, about 1.3 MB each at
/// 1000 px, so the 200-entry cache held 255 MB after one long scroll. Covers
/// past the byte budget leave it, oldest first.
#[test]
fn the_panel_cache_lets_go_of_the_oldest_covers_over_its_byte_budget() {
    let mut app = test_app();
    for id in ["a1", "a2", "a3", "a4"] {
        cover_lands(&mut app, id, big_cover(20));
    }

    assert_eq!(shown(&app, "a1"), None, "the oldest gave way");
    for id in ["a2", "a3", "a4"] {
        assert!(shown(&app, id).is_some(), "{id} is still cached");
    }
}

/// The playing song's cover stays through any amount of browsing: the Queue
/// panel, Theater Mode and the lyrics backdrop read it the whole song long.
#[test]
fn the_playing_songs_cover_outlasts_the_covers_browsed_past_it() {
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("s1".to_string());
    cover_lands(&mut app, "album_s1", big_cover(20));

    for id in ["a2", "a3", "a4", "a5"] {
        cover_lands(&mut app, id, big_cover(20));
    }

    assert!(shown(&app, "album_s1").is_some(), "the playing cover stays");
    assert_eq!(shown(&app, "a2"), None, "the oldest browsed cover gave way");
    assert!(shown(&app, "a5").is_some());
}
