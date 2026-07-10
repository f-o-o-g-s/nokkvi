//! "Add to Mix" context-menu drift guards — one test per view lane, driven
//! through the FULL dispatcher (`app.update`) so each covers the whole chain:
//! view ContextMenuAction arm → per-view Action variant → root routing arm →
//! `add_seeds_to_mix`.
//!
//! Why per-view: albums/artists/genres/playlists resolve their menu entries
//! behind `_`/`matches!` catch-alls — forgetting a lane compiles green and
//! silently returns `Action::None`. These tests make that drift class red.

use nokkvi_data::types::{batch::BatchItem, trawl::TrawlSeedKind};

use crate::{
    Message,
    test_helpers::{make_album, make_artist, make_genre, make_song, test_app},
    widgets::context_menu::LibraryContextEntry,
};

fn crate_keys(app: &crate::Nokkvi) -> Vec<(TrawlSeedKind, String)> {
    app.trawl_crate
        .seeds
        .iter()
        .map(|s| {
            let (kind, id) = s.key();
            (kind, id.to_string())
        })
        .collect()
}

#[test]
fn albums_menu_add_to_mix_seeds_the_crate() {
    let mut app = test_app();
    crate::test_helpers::seed_albums(&mut app, vec![make_album("al1", "Loveless", "MBV")]);

    let _ = app.update(Message::Albums(
        crate::views::AlbumsMessage::ContextMenuAction(0, LibraryContextEntry::AddToMix),
    ));

    assert_eq!(
        crate_keys(&app),
        vec![(TrawlSeedKind::Album, "al1".to_string())],
        "album parent row seeds the crate"
    );
    assert_eq!(app.trawl_crate.seeds[0].label, "Loveless");
    assert_eq!(app.trawl_crate.seeds[0].sublabel, "MBV");
    assert!(
        app.toast
            .toasts
            .iter()
            .any(|t| t.message.contains("1 seed")),
        "menu adds toast a count"
    );
}

#[test]
fn artists_menu_add_to_mix_seeds_the_crate() {
    let mut app = test_app();
    crate::test_helpers::seed_artists(&mut app, vec![make_artist("ar1", "Burial")]);

    let _ = app.update(Message::Artists(
        crate::views::ArtistsMessage::ContextMenuAction(0, LibraryContextEntry::AddToMix),
    ));

    assert_eq!(
        crate_keys(&app),
        vec![(TrawlSeedKind::Artist, "ar1".to_string())]
    );
    assert_eq!(app.trawl_crate.seeds[0].label, "Burial");
    assert_eq!(app.trawl_crate.seeds[0].sublabel, "Artist");
}

#[test]
fn genres_menu_add_to_mix_seeds_the_crate() {
    let mut app = test_app();
    crate::test_helpers::seed_genres(&mut app, vec![make_genre("g1", "Phonk")]);

    let _ = app.update(Message::Genres(
        crate::views::GenresMessage::ContextMenuAction(0, LibraryContextEntry::AddToMix),
    ));

    assert_eq!(
        crate_keys(&app),
        vec![(TrawlSeedKind::Genre, "Phonk".to_string())],
        "genre seeds key on the NAME (batch pipeline contract)"
    );
    assert_eq!(app.trawl_crate.seeds[0].label, "Phonk");
}

#[test]
fn playlists_menu_add_to_mix_seeds_the_crate() {
    let mut app = test_app();
    let playlist = nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: "p1".to_string(),
        name: "Night Drive".to_string(),
        comment: String::new(),
        duration: 0.0,
        song_count: 42,
        owner_name: String::new(),
        public: false,
        updated_at: String::new(),
        artwork_album_ids: vec![],
        uploaded_image: None,
        searchable_lower: "night drive".to_string(),
    };
    app.library.playlists.append_page(vec![playlist], 1);

    let _ = app.update(Message::Playlists(
        crate::views::PlaylistsMessage::PlaylistContextAction(
            0,
            crate::views::playlists::PlaylistContextEntry::Library(LibraryContextEntry::AddToMix),
        ),
    ));

    assert_eq!(
        crate_keys(&app),
        vec![(TrawlSeedKind::Playlist, "p1".to_string())]
    );
    assert_eq!(app.trawl_crate.seeds[0].label, "Night Drive");
    assert_eq!(app.trawl_crate.seeds[0].sublabel, "42 songs");
}

#[test]
fn songs_menu_add_to_mix_seeds_the_crate() {
    let mut app = test_app();
    crate::test_helpers::seed_songs(&mut app, vec![make_song("s1", "Archangel", "Burial")]);

    let _ = app.update(Message::Songs(
        crate::views::SongsMessage::ContextMenuAction(0, LibraryContextEntry::AddToMix),
    ));

    assert_eq!(
        crate_keys(&app),
        vec![(TrawlSeedKind::Song, "s1".to_string())]
    );
    assert_eq!(app.trawl_crate.seeds[0].label, "Archangel");
    assert_eq!(app.trawl_crate.seeds[0].sublabel, "Burial");
}

#[test]
fn queue_menu_add_to_mix_seeds_the_crate() {
    let mut app = test_app();
    app.library.queue_songs = vec![crate::test_helpers::make_queue_song(
        "q1",
        "Nightcrawler",
        "Soudiere",
        "Trilogy",
    )];

    let _ = app.handle_queue(crate::views::QueueMessage::ContextMenuAction(
        0,
        crate::views::queue::QueueContextEntry::AddToMix,
    ));

    assert_eq!(
        crate_keys(&app),
        vec![(TrawlSeedKind::Song, "q1".to_string())]
    );
    assert_eq!(app.trawl_crate.seeds[0].label, "Nightcrawler");
    // The rebuilt Song must be playable: streaming keys on the id, and the
    // resolve path passes the embedded Song through untouched.
    match &app.trawl_crate.seeds[0].item {
        BatchItem::Song(song) => {
            assert_eq!(song.id, "q1");
            assert_eq!(song.artist, "Soudiere");
        }
        other => panic!("expected a Song seed, got {other:?}"),
    }
}

#[test]
fn duplicate_menu_add_toasts_already_in_the_mix() {
    let mut app = test_app();
    crate::test_helpers::seed_albums(&mut app, vec![make_album("al1", "Loveless", "MBV")]);

    let _ = app.update(Message::Albums(
        crate::views::AlbumsMessage::ContextMenuAction(0, LibraryContextEntry::AddToMix),
    ));
    app.toast.toasts.clear();
    let _ = app.update(Message::Albums(
        crate::views::AlbumsMessage::ContextMenuAction(0, LibraryContextEntry::AddToMix),
    ));

    assert_eq!(app.trawl_crate.len(), 1, "identity dedupe holds");
    assert!(
        app.toast
            .toasts
            .iter()
            .any(|t| t.message.contains("Already in the mix")),
        "duplicate add is acknowledged, not silent"
    );
}
