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
    assert_eq!(app.artist_minis_to_fetch(), vec!["r1", "r3"]);

    app.library
        .artists
        .set_from_vec(vec![artist("r2", false), artist("r4", true)]);
    assert_eq!(app.artist_minis_to_fetch(), vec!["r2"]);
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
