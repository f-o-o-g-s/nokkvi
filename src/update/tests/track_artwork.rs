//! Track Artwork (GH #13): a song shows its album's cover, or with
//! `TrackArtwork::OwnCover` the art embedded in its own file, keyed
//! `mf-<song id>` on every song surface.

use nokkvi_data::types::player_settings::TrackArtwork;

use crate::{
    test_helpers::*,
    update::components::{ArtworkGate, plan_queue_row_minis},
    widgets::SlotListView,
};

fn handle() -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_bytes(vec![0u8; 64])
}

/// A queue row whose file carries its own art.
fn own_cover_song(id: &str) -> nokkvi_data::backend::queue::QueueSongUIViewData {
    let mut song = make_queue_song(id, "T", "A", "Alb");
    song.own_cover = Some(format!("mf-{id}"));
    song.artwork_url = format!("http://srv/art/album_{id}");
    song
}

fn playing(app: &mut crate::Nokkvi, songs: Vec<nokkvi_data::backend::queue::QueueSongUIViewData>) {
    app.scrobble.current_song_id = songs.first().map(|s| s.id.clone());
    app.library.queue_songs = songs;
}

#[test]
fn the_playing_cover_key_follows_the_setting() {
    let mut app = test_app();
    playing(&mut app, vec![own_cover_song("s1")]);

    assert_eq!(app.current_queue_song_cover_key(), Some("album_s1"));
    app.settings.track_artwork = TrackArtwork::OwnCover;
    assert_eq!(app.current_queue_song_cover_key(), Some("mf-s1"));
}

#[test]
fn a_song_without_its_own_art_keeps_the_album_cover() {
    let mut app = test_app();
    app.settings.track_artwork = TrackArtwork::OwnCover;
    playing(&mut app, vec![make_queue_song("s1", "T", "A", "Alb")]);

    assert_eq!(app.current_queue_song_cover_key(), Some("album_s1"));
}

/// The now-playing surfaces (mini player, accent, MilkDrop, Theater) read
/// the playing song's own cover, not the album's.
#[test]
fn the_playing_cover_reads_the_own_cover() {
    let mut app = test_app();
    playing(&mut app, vec![own_cover_song("s1")]);
    app.artwork.large_artwork.put("mf-s1".to_string(), handle());

    let cover = app.playing_cover().expect("a song is current");
    assert!(cover.large.is_none(), "Album Cover reads the album's key");

    app.settings.track_artwork = TrackArtwork::OwnCover;
    let cover = app.playing_cover().expect("a song is current");
    assert_eq!(cover.owner, "mf-s1");
    assert_eq!(cover.album_id, Some("mf-s1"));
    assert!(cover.large.is_some());
}

#[test]
fn the_now_playing_warm_fetches_the_own_cover() {
    let mut app = test_app();
    app.settings.track_artwork = TrackArtwork::OwnCover;
    playing(&mut app, vec![own_cover_song("s1")]);

    assert_eq!(app.now_playing_artwork_to_warm(), Some("mf-s1".to_string()));

    app.artwork.album_art.put("mf-s1".to_string(), handle());
    assert_eq!(app.now_playing_artwork_to_warm(), None);
}

/// Album Cover plans exactly what the queue fetched before the setting:
/// each row's album id through its prebuilt URL.
#[test]
fn queue_rows_under_album_cover_fetch_the_album_url() {
    let rows = vec![own_cover_song("s0"), make_queue_song("s1", "T", "A", "Alb")];
    let artwork = crate::state::ArtworkState::default();
    let plan = plan_queue_row_minis(
        &SlotListView::new(),
        &rows,
        &ArtworkGate::new(&artwork),
        TrackArtwork::AlbumCover,
    );

    let by_url: Vec<_> = plan
        .by_url
        .iter()
        .map(|(id, _, url)| (id.as_str(), url.as_str()))
        .collect();
    // `make_queue_song` rows carry no URL, so they plan nothing (as before).
    assert_eq!(by_url, [("album_s0", "http://srv/art/album_s0")]);
    assert!(plan.by_key.is_empty());
}

/// Own Cover fetches a row with its own art by its `mf-` key; a row
/// without keeps the album URL.
#[test]
fn queue_rows_under_own_cover_fetch_the_own_cover_by_key() {
    let mut bare = make_queue_song("s1", "T", "A", "Alb");
    bare.artwork_url = "http://srv/art/album_s1".to_string();
    let rows = vec![own_cover_song("s0"), bare];
    let artwork = crate::state::ArtworkState::default();
    let plan = plan_queue_row_minis(
        &SlotListView::new(),
        &rows,
        &ArtworkGate::new(&artwork),
        TrackArtwork::OwnCover,
    );

    let by_key: Vec<_> = plan.by_key.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(by_key, ["mf-s0"]);
    let by_url: Vec<_> = plan.by_url.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(by_url, ["album_s1"]);
}

/// An own cover already cached or on its way is not fetched again.
#[test]
fn a_warm_own_cover_is_not_planned_again() {
    let rows = vec![own_cover_song("s0"), own_cover_song("s1")];
    let mut artwork = crate::state::ArtworkState::default();
    // Cached the way `record_mini_art` caches it: the handle plus the
    // version it was fetched at.
    artwork.album_art.put("mf-s0".to_string(), handle());
    artwork.album_art_versions.insert("mf-s0".to_string(), None);
    artwork.album_art_pending.insert("mf-s1".to_string());
    let plan = plan_queue_row_minis(
        &SlotListView::new(),
        &rows,
        &ArtworkGate::new(&artwork),
        TrackArtwork::OwnCover,
    );

    assert!(plan.by_key.is_empty(), "{:?}", plan.by_key);
    assert!(plan.by_url.is_empty(), "{:?}", plan.by_url);
}
