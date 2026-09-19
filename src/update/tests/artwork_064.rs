//! Navidrome 0.64 artwork: image info drives which covers are requested.
//!
//! Absent art (`imageAbsent: true`) is never requested by a surface that
//! holds the entity; everything else plans exactly as before.

use std::collections::{HashMap, HashSet};

use nokkvi_data::{
    backend::albums::AlbumUIViewData,
    types::{album::Album, artist::Artist},
};

use crate::{
    test_helpers::{make_artist, test_app},
    update::components::{
        album_prefetch_entry, expansion_child_album_ids, plan_album_artwork_fetches,
    },
    widgets::SlotListView,
};

fn album(id: &str, image: serde_json::Value) -> AlbumUIViewData {
    let mut v = serde_json::json!({ "id": id, "name": id, "updatedAt": "T1" });
    if let (Some(obj), Some(more)) = (v.as_object_mut(), image.as_object()) {
        obj.extend(more.clone());
    }
    let album: Album = serde_json::from_value(v).expect("album json");
    AlbumUIViewData::from_album(&album, "http://srv", "u=x&s=y&t=z")
}

fn absent() -> serde_json::Value {
    serde_json::json!({ "imageAbsent": true })
}

fn none() -> serde_json::Value {
    serde_json::json!({})
}

fn planned_ids(
    albums: &[AlbumUIViewData],
    failed: &HashMap<String, Option<String>>,
) -> Vec<String> {
    let slot_list = SlotListView::new();
    plan_album_artwork_fetches(
        &slot_list,
        albums,
        &HashSet::new(),
        &HashMap::new(),
        failed,
        album_prefetch_entry,
    )
    .into_iter()
    .map(|(id, _, _)| id)
    .collect()
}

// --- Skip absent art ---------------------------------------------------------

/// Two absent albums in the viewport plan no fetch; the others still do.
#[test]
fn absent_albums_plan_no_mini_fetch() {
    let albums = vec![
        album("a1", none()),
        album("a2", absent()),
        album("a3", none()),
        album("a4", absent()),
    ];
    assert_eq!(planned_ids(&albums, &HashMap::new()), vec!["a1", "a3"]);
}

/// A later reload that clears the flag plans the album again, and nothing
/// negative-cached it while it was absent.
#[test]
fn cleared_absent_flag_is_planned_again() {
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![album("a1", absent())]);
    assert!(planned_ids(&app.library.albums, &app.artwork.failed_art).is_empty());
    assert!(app.artwork.failed_art.is_empty(), "absent is not a failure");

    app.library.albums.set_from_vec(vec![album("a1", none())]);
    assert_eq!(
        planned_ids(&app.library.albums, &app.artwork.failed_art),
        vec!["a1"]
    );
}

/// An old-server album (no image keys) keeps today's URL and is planned.
#[test]
fn old_server_album_plans_as_before() {
    let a = album("a1", none());
    assert_eq!(
        a.artwork_url,
        nokkvi_data::utils::artwork_url::build_cover_art_url_with_timestamp(
            "a1",
            "http://srv",
            "u=x&s=y&t=z",
            Some(nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE),
            Some("T1"),
        )
    );
    assert_eq!(planned_ids(&[a], &HashMap::new()), vec!["a1"]);
}

/// Expansion children and Harbour shelves skip absent albums too.
#[test]
fn absent_albums_skip_expansion_and_shelf_warming() {
    let albums = vec![album("a1", none()), album("a2", absent())];
    let ids: Vec<String> = expansion_child_album_ids(&albums)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(ids, vec!["a1"]);

    let mut app = test_app();
    app.harbour.recently_added = albums;
    let ids: Vec<String> = app
        .harbour
        .shelf_album_art_triples()
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(ids, vec!["a1"]);
}

/// A centered absent album dispatches no large-art load and leaves no
/// loading marker; a normal album still starts its load.
#[test]
fn absent_album_centered_loads_no_large_art() {
    let mut app = test_app();
    app.library
        .albums
        .set_from_vec(vec![album("a1", absent()), album("a2", none())]);

    let _ = app.handle_load_large_artwork("a1".into());
    assert_eq!(app.artwork.loading_large_artwork, None);
    assert!(app.artwork.large_artwork.peek(&"a1".to_string()).is_none());

    let _ = app.handle_load_large_artwork("a2".into());
    assert_eq!(app.artwork.loading_large_artwork.as_deref(), Some("a2"));
}

fn artist(id: &str, absent: bool) -> nokkvi_data::backend::artists::ArtistUIViewData {
    let mut a = make_artist(id, id);
    a.image.image_absent = absent;
    a
}

/// Absent artists plan no `ar-` mini; the flag clearing plans it again.
#[test]
fn absent_artists_plan_no_mini_fetch() {
    let mut app = test_app();
    app.library.artists.set_from_vec(vec![
        artist("r1", false),
        artist("r2", true),
        artist("r3", false),
        artist("r4", true),
    ]);
    let ids = |app: &crate::Nokkvi| -> Vec<String> {
        app.artist_minis_to_fetch()
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };
    assert_eq!(ids(&app), vec!["r1", "r3"]);

    app.library
        .artists
        .set_from_vec(vec![artist("r2", false), artist("r4", true)]);
    assert_eq!(ids(&app), vec!["r2"]);
    assert!(app.artwork.failed_art.is_empty());
}

/// The 500px artist panel: absent → no load; an artist with an external
/// image and no absent flag is unaffected.
#[test]
fn absent_artist_panel_loads_nothing() {
    let mut app = test_app();
    let mut external = artist("r2", false);
    external.image_url = Some("https://img.example/r2.jpg".into());
    app.library
        .artists
        .set_from_vec(vec![artist("r1", true), external]);

    let _ = app.handle_load_artist_large_artwork("r1".into());
    assert_eq!(app.artwork.loading_large_artwork, None);

    let _ = app.handle_load_artist_large_artwork("r2".into());
    assert_eq!(app.artwork.loading_large_artwork.as_deref(), Some("r2"));
}

/// Harbour's artist shelves hold raw `Artist`s; absent ones warm nothing.
#[test]
fn harbour_absent_artists_are_not_warmed() {
    let mut app = test_app();
    let raw = |id: &str, absent: bool| -> Artist {
        let mut a: Artist =
            serde_json::from_value(serde_json::json!({ "id": id, "name": id })).expect("artist");
        a.image.image_absent = absent;
        a
    };
    app.harbour.most_played_artists = vec![raw("h1", false), raw("h2", true)];
    app.harbour.random_artist = Some(raw("h3", true));
    assert_eq!(app.harbour.shelf_artist_ids(), vec!["h1"]);
}

// --- The image hash is the version --------------------------------------------

mod version {
    use std::collections::{HashMap, HashSet};

    use nokkvi_data::types::image_info::{ImageInfo, artwork_version};

    use crate::update::components::{passive_artwork_version, should_refetch};

    const A: &str = "aaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbb";

    fn hashed(h: &str) -> ImageInfo {
        ImageInfo {
            image_hash: Some(h.to_owned()),
            ..ImageInfo::default()
        }
    }

    fn warm(version: Option<String>) -> (Vec<String>, HashMap<String, Option<String>>) {
        let ids = vec!["al-1".to_owned()];
        let mut versions = HashMap::new();
        versions.insert("al-1".to_owned(), version);
        (ids, versions)
    }

    fn refetch(
        ids: &[String],
        versions: &HashMap<String, Option<String>>,
        failed: &HashMap<String, Option<String>>,
        version: &Option<String>,
    ) -> bool {
        let cached: HashSet<&String> = ids.iter().collect();
        should_refetch(&cached, versions, failed, &"al-1".to_owned(), version)
    }

    #[test]
    fn a_valid_hash_wins_over_updated_at() {
        assert_eq!(artwork_version(&hashed(A), Some("T1")), Some(A.to_owned()));
        assert_eq!(
            artwork_version(&ImageInfo::default(), Some("T1")),
            Some("T1".to_owned()),
            "no hash (old server): updated_at as before"
        );
        assert_eq!(
            artwork_version(&hashed("not-a-hash"), Some("T1")),
            Some("T1".to_owned())
        );
        assert_eq!(artwork_version(&ImageInfo::default(), None), None);
    }

    /// Warmed at hash A, the row now says B: the cover changed.
    #[test]
    fn changed_hash_refetches() {
        let (ids, versions) = warm(artwork_version(&hashed(A), Some("T1")));
        let now = artwork_version(&hashed(B), Some("T1"));
        assert!(refetch(&ids, &versions, &HashMap::new(), &now));
    }

    /// Warmed at hash A; `updated_at` moved (a play-count bump) but the hash
    /// didn't: no refetch.
    #[test]
    fn same_hash_with_new_updated_at_does_not_refetch() {
        let (ids, versions) = warm(artwork_version(&hashed(A), Some("T1")));
        let now = artwork_version(&hashed(A), Some("T2"));
        assert!(!refetch(&ids, &versions, &HashMap::new(), &now));
    }

    /// Warmed before the hash was known: the first hash refetches once (the
    /// first fetch may have been the server's placeholder), then settles.
    #[test]
    fn first_hash_refetches_once() {
        let (ids, mut versions) = warm(artwork_version(&ImageInfo::default(), Some("T1")));
        let now = artwork_version(&hashed(A), Some("T1"));
        assert!(refetch(&ids, &versions, &HashMap::new(), &now));
        versions.insert("al-1".to_owned(), now.clone());
        assert!(!refetch(&ids, &versions, &HashMap::new(), &now));
    }

    /// Failed at hash A; the row now says B: re-attempt.
    #[test]
    fn failed_at_old_hash_reattempts() {
        let mut failed = HashMap::new();
        failed.insert("al-1".to_owned(), artwork_version(&hashed(A), None));
        let now = artwork_version(&hashed(B), None);
        assert!(refetch(&[], &HashMap::new(), &failed, &now));
        let same = artwork_version(&hashed(A), None);
        assert!(!refetch(&[], &HashMap::new(), &failed, &same));
    }

    /// Passive (song-keyed) surfaces stay id-only: `Song` carries no image
    /// info.
    #[test]
    fn passive_version_stays_none() {
        assert_eq!(passive_artwork_version(&Some("T1".to_owned())), None);
    }
}

/// The Albums view's prefetch entry and URL carry the hash on 0.64.
#[test]
fn album_rows_version_and_url_carry_the_hash() {
    let a = album("a1", serde_json::json!({ "imageHash": "0123456789abcdef" }));
    let (_, version, url) = album_prefetch_entry(&a);
    assert_eq!(version.as_deref(), Some("0123456789abcdef"));
    assert!(url.contains("id=al-a1_0123456789abcdef&"), "{url}");
    assert!(!url.contains("_u="), "{url}");

    let expansion = expansion_child_album_ids(std::slice::from_ref(&a));
    assert_eq!(expansion[0].1.as_deref(), Some("0123456789abcdef"));

    let mut app = test_app();
    app.harbour.recently_added = vec![a];
    assert_eq!(
        app.harbour.shelf_album_art_triples()[0].1.as_deref(),
        Some("0123456789abcdef")
    );
}

/// Artists gain a version: the hash, recorded through the shared gate.
#[test]
fn artist_minis_version_by_hash() {
    let mut app = test_app();
    let mut r1 = make_artist("r1", "R1");
    r1.image.image_hash = Some("0123456789abcdef".into());
    app.library.artists.set_from_vec(vec![r1]);
    assert_eq!(
        app.artist_minis_to_fetch(),
        vec![("r1".to_owned(), Some("0123456789abcdef".to_owned()))]
    );
    // Warmed at that hash: nothing more to fetch.
    app.artwork.album_art.put(
        "r1".into(),
        iced::widget::image::Handle::from_bytes(Vec::<u8>::new()),
    );
    app.artwork
        .album_art_versions
        .insert("r1".into(), Some("0123456789abcdef".into()));
    assert!(app.artist_minis_to_fetch().is_empty());
}

// --- Exact refresh after a background reload ----------------------------------

mod background_reload {
    use std::collections::HashSet;

    use nokkvi_data::backend::albums::AlbumUIViewData;

    use super::album;
    use crate::{
        app_message::MiniArt,
        test_helpers::{make_artist, test_app},
        update::components::{album_prefetch_entry, plan_album_artwork_fetches},
    };

    const A: &str = "aaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccc";

    fn hashed(id: &str, hash: &str, updated_at: &str) -> AlbumUIViewData {
        let mut a = album(id, serde_json::json!({ "imageHash": hash }));
        a.updated_at = Some(updated_at.to_owned());
        a
    }

    fn handle() -> iced::widget::image::Handle {
        iced::widget::image::Handle::from_bytes(Vec::<u8>::new())
    }

    /// Warm every row's mini at the version its prefetch entry records.
    fn warm(app: &mut crate::Nokkvi) {
        let entries: Vec<_> = app
            .library
            .albums
            .iter()
            .map(album_prefetch_entry)
            .collect();
        for (id, version, _) in entries {
            app.artwork.album_art_versions.insert(id.clone(), version);
            app.artwork.album_art.put(id, handle());
        }
    }

    /// What the Albums viewport prefetch would plan against the app's live
    /// cache state (the planning half `prefetch_artwork_tasks` runs after
    /// every load, background ones included).
    fn planned(app: &crate::Nokkvi) -> Vec<String> {
        let cached: HashSet<&String> = app.artwork.album_art.iter().map(|(k, _)| k).collect();
        plan_album_artwork_fetches(
            &app.albums_page.common.slot_list,
            &app.library.albums,
            &cached,
            &app.artwork.album_art_versions,
            &app.artwork.failed_art,
            album_prefetch_entry,
        )
        .into_iter()
        .map(|(id, _, _)| id)
        .collect()
    }

    fn reload(app: &mut crate::Nokkvi, rows: Vec<AlbumUIViewData>) -> Vec<String> {
        let n = rows.len();
        let _ = app.handle_albums_loaded(Ok(rows), n, true, Some("a1".into()));
        planned(app)
    }

    /// Unchanged hashes plan nothing; one changed hash plans exactly that id.
    #[test]
    fn reload_plans_only_the_changed_hash() {
        let mut app = test_app();
        app.library
            .albums
            .set_from_vec(vec![hashed("a1", A, "T1"), hashed("a2", B, "T1")]);
        warm(&mut app);
        assert!(reload(&mut app, vec![hashed("a1", A, "T1"), hashed("a2", B, "T1")]).is_empty());
        assert_eq!(
            reload(&mut app, vec![hashed("a1", A, "T1"), hashed("a2", C, "T1")]),
            vec!["a2"]
        );
    }

    /// A play-count bump moves `updatedAt`, not the hash: nothing refetches.
    #[test]
    fn reload_after_a_play_count_bump_plans_nothing() {
        let mut app = test_app();
        app.library.albums.set_from_vec(vec![hashed("a1", A, "T1")]);
        warm(&mut app);
        assert!(reload(&mut app, vec![hashed("a1", A, "T2")]).is_empty());
    }

    /// Old-server rows (no hash) with an unchanged `updatedAt` plan nothing,
    /// exactly as before.
    #[test]
    fn old_server_reload_plans_as_before() {
        let mut app = test_app();
        let old = |id: &str| album(id, serde_json::json!({}));
        app.library.albums.set_from_vec(vec![old("a1"), old("a2")]);
        warm(&mut app);
        assert!(reload(&mut app, vec![old("a1"), old("a2")]).is_empty());
    }

    /// Artists reload the same way: only a changed hash is planned.
    #[test]
    fn artist_reload_plans_only_the_changed_hash() {
        let mut app = test_app();
        let artist = |id: &str, hash: &str| {
            let mut a = make_artist(id, id);
            a.image.image_hash = Some(hash.to_owned());
            a
        };
        app.library
            .artists
            .set_from_vec(vec![artist("r1", A), artist("r2", B)]);
        for (id, version) in app.artist_minis_to_fetch() {
            app.artwork.album_art_versions.insert(id.clone(), version);
            app.artwork.album_art.put(id, handle());
        }
        let _ = app.handle_artists_loaded(
            Ok(vec![artist("r1", A), artist("r2", C)]),
            2,
            true,
            Some("r1".into()),
        );
        let ids: Vec<String> = app
            .artist_minis_to_fetch()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec!["r2"]);
    }

    /// Album `a1` with its large art cached at `large_hash` (as recorded
    /// when it was requested), its row now saying `row_hash`, and a mini
    /// arriving at `arriving`. Returns whether the large art survived.
    fn large_after_mini(
        large_hash: Option<&str>,
        row_hash: Option<&str>,
        arriving: Option<&str>,
    ) -> bool {
        let mut app = test_app();
        let row = match row_hash {
            Some(h) => hashed("a1", h, "T1"),
            None => album("a1", serde_json::json!({})),
        };
        app.library.albums.set_from_vec(vec![row]);
        app.artwork.album_art.put("a1".into(), handle());
        app.artwork.large_artwork.put("a1".into(), handle());
        if let Some(h) = large_hash {
            app.artwork
                .large_artwork_hashes
                .insert("a1".into(), h.to_owned());
        }
        let _ = app.handle_artwork_loaded(
            "a1".into(),
            arriving.map(str::to_owned),
            MiniArt::Loaded(handle()),
        );
        app.artwork.large_artwork.peek(&"a1".to_string()).is_some()
    }

    /// The row's hash moved on and the mini arrived at it: the cached large
    /// art shows the old cover, so it is dropped for a reload.
    #[test]
    fn changed_hash_drops_the_stale_large_art() {
        assert!(!large_after_mini(Some(A), Some(B), Some(B)));
    }

    /// Everything else keeps the large art: the same hash, an old server's
    /// timestamps (bit for bit today's behavior), a large fetched before any
    /// hash was known, or a stale surface refetching the OLD hash while the
    /// rows already say the new one (Harbour's previous-visit shelves).
    #[test]
    fn large_art_survives_everything_but_a_current_hash_change() {
        assert!(large_after_mini(Some(A), Some(A), Some(A)));
        assert!(large_after_mini(None, None, Some("2026-02-02T00:00:00Z")));
        assert!(large_after_mini(None, Some(A), Some(A)));
        assert!(large_after_mini(Some(B), Some(B), Some(A)), "stale mini");
    }

    /// A passive surface (queue, Songs) recording `None` in between must not
    /// hide the change from the large-art check.
    #[test]
    fn passive_none_in_between_still_drops_the_stale_large_art() {
        let mut app = test_app();
        app.library.albums.set_from_vec(vec![hashed("a1", A, "T1")]);
        app.artwork.album_art.put("a1".into(), handle());
        app.artwork.large_artwork.put("a1".into(), handle());
        app.artwork
            .large_artwork_hashes
            .insert("a1".into(), A.to_owned());
        // The queue re-warms the mini with its constant None version.
        let _ = app.handle_artwork_loaded("a1".into(), None, MiniArt::Loaded(handle()));
        assert!(app.artwork.large_artwork.peek(&"a1".to_string()).is_some());
        // The cover is replaced; the reload brings B and the mini refetches.
        app.library.albums.set_from_vec(vec![hashed("a1", B, "T1")]);
        let _ =
            app.handle_artwork_loaded("a1".into(), Some(B.to_owned()), MiniArt::Loaded(handle()));
        assert!(app.artwork.large_artwork.peek(&"a1".to_string()).is_none());
    }

    /// A large load records the hash it was requested at; a hashless load
    /// clears any stale record.
    #[test]
    fn large_load_records_its_hash() {
        let mut app = test_app();
        app.library.albums.set_from_vec(vec![hashed("a1", A, "T1")]);
        let _ = app.handle_load_large_artwork("a1".into());
        assert_eq!(
            app.artwork
                .large_artwork_hashes
                .get("a1")
                .map(String::as_str),
            Some(A)
        );
        app.library
            .albums
            .set_from_vec(vec![album("a1", serde_json::json!({}))]);
        let _ = app.handle_load_large_artwork("a1".into());
        assert!(!app.artwork.large_artwork_hashes.contains_key("a1"));
    }
}

/// A centered Harbour search album the server marked absent loads no large
/// art (its raw `Album` carries the flag).
#[test]
fn harbour_search_absent_album_loads_no_large_art() {
    let mut app = test_app();
    let raw: Album = serde_json::from_value(serde_json::json!({
        "id": "s1", "name": "S", "imageAbsent": true
    }))
    .expect("album");
    app.harbour.search_results = Some(nokkvi_data::types::library_search::LibrarySearchResults {
        albums: vec![raw],
        ..Default::default()
    });
    let _ = app.handle_load_large_artwork("s1".into());
    assert_eq!(app.artwork.loading_large_artwork, None);
}
