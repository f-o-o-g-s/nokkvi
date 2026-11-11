//! The artist expansion grouped by release type: section header rows among
//! the album rows, and everything that reads those rows.

use nokkvi_data::{
    backend::albums::AlbumUIViewData,
    types::{
        batch::{BatchItem, BatchPayload},
        discography::{DiscographyGroup, ReleaseType},
        trawl::TrawlSeedKind,
    },
};

use crate::{
    Nokkvi, View,
    test_helpers::{make_album, make_artist, seed_artists, test_app},
    update::hotkeys::CenterItem,
    views::{ArtistsAction, ArtistsMessage, ArtistsPage, DiscographyRow},
    widgets::{SlotListPageMessage, context_menu::LibraryContextEntry},
};

/// The artist every test expands: the middle one of three, so flattened
/// indices differ from artist indices on both sides of the expansion.
const ARTIST: &str = "ar1";

/// Flattened rows with [`mixed`] loaded under [`ARTIST`]:
/// `ar0, ar1, #Albums, lp-new, lp-old, #EPs, ep-new, ep-old, #Singles, single, ar2`.
const ALBUMS_HEADER: usize = 2;
const LP_NEW: usize = 3;
const EPS_HEADER: usize = 5;
const EP_NEW: usize = 6;
const SINGLES_HEADER: usize = 8;
const SINGLE: usize = 9;

pub(super) fn typed(id: &str, types: &[ReleaseType], year: u32) -> AlbumUIViewData {
    let mut album = make_album(id, id, "One");
    album.album_artist_ids = vec![ARTIST.to_string()];
    album.release_types = types.to_vec();
    album.year = Some(year);
    album
}

/// Two albums, two EPs and a single, in the server's newest-first order.
pub(super) fn mixed() -> Vec<AlbumUIViewData> {
    vec![
        typed("ep-new", &[ReleaseType::Ep], 2020),
        typed("lp-new", &[ReleaseType::Album], 2016),
        typed("single", &[ReleaseType::Single], 2012),
        typed("ep-old", &[ReleaseType::Ep], 2009),
        typed("lp-old", &[ReleaseType::Album], 2003),
    ]
}

/// An app on the Artists view with three artists and `albums` loaded under
/// the middle one, through the real `AlbumsLoaded` path.
pub(super) fn app_with_albums(albums: Vec<AlbumUIViewData>) -> Nokkvi {
    let mut app = test_app();
    app.current_view = View::Artists;
    seed_artists(
        &mut app,
        vec![
            make_artist("ar0", "Zero"),
            make_artist(ARTIST, "One"),
            make_artist("ar2", "Two"),
        ],
    );
    let _ = app.handle_artists(ArtistsMessage::AlbumsLoaded(ARTIST.to_string(), albums));
    app
}

/// The expanded artist's rows as short strings: `#EPs` for a header, the id
/// for an album.
pub(super) fn shape(app: &Nokkvi) -> Vec<String> {
    app.artists_page
        .expansion
        .children
        .iter()
        .map(|row| match row {
            DiscographyRow::Section(header) => format!("#{}", header.group.label()),
            DiscographyRow::Album(album) => album.id.clone(),
        })
        .collect()
}

/// Put the keyboard focus on flattened row `idx` (a viewport move, as the
/// navigation keys do it).
pub(super) fn centre(app: &mut Nokkvi, idx: usize) {
    let total = app
        .artists_page
        .expansion
        .flattened_len(&app.library.artists);
    app.artists_page.common.handle_set_offset(idx, total);
    assert_eq!(
        app.artists_page.common.get_center_item_index(total),
        Some(idx)
    );
}

#[test]
fn a_mixed_discography_loads_under_section_headers() {
    let app = app_with_albums(mixed());
    assert_eq!(
        shape(&app),
        [
            "#Albums", "lp-new", "lp-old", "#EPs", "ep-new", "ep-old", "#Singles", "single"
        ]
    );
    // Headers are real rows: the list is three artists plus eight children.
    assert_eq!(
        app.artists_page
            .expansion
            .flattened_len(&app.library.artists),
        11
    );
}

#[test]
fn an_artist_with_only_plain_albums_loads_as_before() {
    let plain = vec![
        typed("b", &[ReleaseType::Album], 2010),
        typed("a", &[], 2005),
    ];
    let app = app_with_albums(plain);
    assert_eq!(shape(&app), ["b", "a"]);
}

#[test]
fn grouping_off_loads_the_servers_flat_list() {
    let mut app = app_with_albums(Vec::new());
    app.artists_page.group_by_release_type = false;
    let artists = app.library.artists.to_vec();
    let _ = app.artists_page.update(
        ArtistsMessage::AlbumsLoaded(ARTIST.to_string(), mixed()),
        artists.len(),
        &artists,
    );
    assert_eq!(
        shape(&app),
        ["ep-new", "lp-new", "single", "ep-old", "lp-old"]
    );
}

#[test]
fn a_centred_header_shows_the_expanded_artist_in_the_panel() {
    let mut app = app_with_albums(mixed());
    for row in [ALBUMS_HEADER, EPS_HEADER, EP_NEW] {
        centre(&mut app, row);
        assert_eq!(
            app.centred_artist_id().as_deref(),
            Some(ARTIST),
            "row {row}"
        );
    }
    // Past the expansion the next artist takes over.
    centre(&mut app, 10);
    assert_eq!(app.centred_artist_id().as_deref(), Some("ar2"));
}

#[test]
fn navigation_steps_onto_a_header_like_any_row() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, 1);
    let _ = app.handle_artists(ArtistsMessage::SlotList(SlotListPageMessage::NavigateDown));
    let total = app
        .artists_page
        .expansion
        .flattened_len(&app.library.artists);
    assert_eq!(
        app.artists_page.common.get_center_item_index(total),
        Some(ALBUMS_HEADER)
    );
    assert!(matches!(
        app.center_item(),
        Some(CenterItem::Named(name)) if name == "Albums"
    ));
}

#[test]
fn the_star_and_rating_keys_find_nothing_on_a_header() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, EPS_HEADER);
    assert!(app.get_center_item_info().is_none());
    assert!(matches!(
        app.center_item(),
        Some(CenterItem::Named(name)) if name == "EPs"
    ));

    // The album under it is still rateable.
    centre(&mut app, EP_NEW);
    let info = app.get_center_item_info().expect("an album row");
    assert_eq!(info.id, "ep-new");
}

#[test]
fn get_info_on_a_header_opens_nothing() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, EPS_HEADER);
    let _ = app.handle_get_info();
    assert!(!app.info_modal.visible);

    centre(&mut app, EP_NEW);
    let _ = app.handle_get_info();
    assert!(app.info_modal.visible);
}

#[test]
fn enter_on_an_album_under_a_header_plays_that_album() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, SINGLE);
    let artists = app.library.artists.to_vec();
    let (_, action) = app.artists_page.update(
        ArtistsMessage::SlotList(SlotListPageMessage::ActivateCenter(false)),
        artists.len(),
        &artists,
    );
    assert!(
        matches!(action, ArtistsAction::PlayAlbum(ref id, false) if id == "single"),
        "{action:?}"
    );
}

#[test]
fn album_numbers_count_albums_only() {
    let app = app_with_albums(mixed());
    let artists = &app.library.artists;
    let label = |row: usize| app.artists_page.album_sub_index_label(row, artists);
    // The expanded artist is the list's second row.
    assert_eq!(label(LP_NEW), "2.1");
    assert_eq!(label(LP_NEW + 1), "2.2");
    assert_eq!(label(EP_NEW), "2.3");
    assert_eq!(label(SINGLE), "2.5");
}

#[test]
fn a_flat_list_numbers_its_albums_as_before() {
    let plain = vec![
        typed("b", &[ReleaseType::Album], 2010),
        typed("a", &[], 2005),
    ];
    let app = app_with_albums(plain);
    let artists = &app.library.artists;
    for row in [2, 3] {
        assert_eq!(
            app.artists_page.album_sub_index_label(row, artists),
            app.artists_page
                .expansion
                .child_sub_index_label(row, artists, |a| &a.id),
        );
    }
}

#[test]
fn star_and_rating_sync_reach_an_album_in_a_folded_section() {
    let mut app = test_app();
    app.artists_page
        .collapsed_sections
        .insert(DiscographyGroup::Eps);
    seed_artists(&mut app, vec![make_artist(ARTIST, "One")]);
    let _ = app.handle_artists(ArtistsMessage::AlbumsLoaded(ARTIST.to_string(), mixed()));
    assert_eq!(
        shape(&app),
        ["#Albums", "lp-new", "lp-old", "#EPs", "#Singles", "single"]
    );

    let _ = app.handle_album_starred_status_updated("ep-old".to_string(), true);
    let _ = app.handle_album_rating_updated("ep-old".to_string(), 4);
    let _ = app.handle_album_starred_status_updated("lp-new".to_string(), true);

    let album = |id: &str| {
        app.artists_page
            .expanded_albums()
            .find(|a| a.id == id)
            .cloned()
            .expect("the album is held")
    };
    assert!(album("ep-old").is_starred);
    assert_eq!(album("ep-old").rating, Some(4));
    assert!(album("lp-new").is_starred);
    assert!(!album("ep-new").is_starred);
    // Folded albums stay known to the artwork bookkeeping too.
    assert!(app.held_albums().any(|(id, _)| id == "ep-old"));
}

// ── Header actions ──────────────────────────────────────────────────────────

/// Run one page message against the app's artists, returning the action.
fn page_update(app: &mut Nokkvi, message: ArtistsMessage) -> ArtistsAction {
    let artists = app.library.artists.to_vec();
    let (_, action) = app.artists_page.update(message, artists.len(), &artists);
    action
}

fn slot(message: SlotListPageMessage) -> ArtistsMessage {
    ArtistsMessage::SlotList(message)
}

/// The album ids a batch names, in order; panics on any other item.
fn album_ids(payload: &BatchPayload) -> Vec<&str> {
    payload
        .items
        .iter()
        .map(|item| match item {
            BatchItem::Album(id) => id.as_str(),
            other => panic!("expected an album, got {other:?}"),
        })
        .collect()
}

fn centre_index(app: &Nokkvi) -> Option<usize> {
    let total = app
        .artists_page
        .expansion
        .flattened_len(&app.library.artists);
    app.artists_page.common.get_center_item_index(total)
}

#[test]
fn enter_on_a_header_plays_its_section_in_list_order() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, EPS_HEADER);
    for force in [false, true] {
        let action = page_update(&mut app, slot(SlotListPageMessage::ActivateCenter(force)));
        let ArtistsAction::PlaySelection(payload, forced) = action else {
            panic!("Enter on a header plays the section, got {action:?}");
        };
        assert_eq!(album_ids(&payload), ["ep-new", "ep-old"]);
        assert_eq!(forced, force);
    }
}

#[test]
fn enter_on_a_folded_header_still_plays_its_section() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, EPS_HEADER);
    let _ = page_update(&mut app, ArtistsMessage::ExpandCenter);
    let action = page_update(&mut app, slot(SlotListPageMessage::ActivateCenter(false)));
    let ArtistsAction::PlaySelection(payload, false) = action else {
        panic!("Enter on a folded header plays the section, got {action:?}");
    };
    assert_eq!(album_ids(&payload), ["ep-new", "ep-old"]);
}

#[test]
fn shift_enter_on_a_header_folds_and_unfolds_its_section() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, EPS_HEADER);

    let action = page_update(&mut app, ArtistsMessage::ExpandCenter);
    assert!(matches!(action, ArtistsAction::None), "{action:?}");
    assert_eq!(
        shape(&app),
        ["#Albums", "lp-new", "lp-old", "#EPs", "#Singles", "single"]
    );
    // The artist stays expanded and the header stays where it was.
    assert_eq!(
        app.artists_page.expansion.expanded_id.as_deref(),
        Some(ARTIST)
    );
    assert_eq!(centre_index(&app), Some(EPS_HEADER));
    assert_eq!(
        app.artists_page.common.slot_list.viewport_offset,
        EPS_HEADER
    );

    let _ = page_update(&mut app, ArtistsMessage::ExpandCenter);
    assert_eq!(
        shape(&app),
        [
            "#Albums", "lp-new", "lp-old", "#EPs", "ep-new", "ep-old", "#Singles", "single"
        ]
    );
    assert_eq!(centre_index(&app), Some(EPS_HEADER));
}

#[test]
fn shift_enter_on_an_album_under_a_header_still_drills_into_albums() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, EP_NEW);
    let action = page_update(&mut app, ArtistsMessage::ExpandCenter);
    assert!(
        matches!(action, ArtistsAction::NavigateAndExpandAlbum(ref id) if id == "ep-new"),
        "{action:?}"
    );
}

#[test]
fn a_chevron_click_folds_a_section_without_moving_the_viewport() {
    let mut app = app_with_albums(mixed());
    // The viewport sits on the first album; the click is two rows below.
    centre(&mut app, LP_NEW);
    // A stale selection inside the section must not survive the fold.
    let _ = page_update(&mut app, slot(SlotListPageMessage::SelectionToggle(EP_NEW)));

    let _ = app.handle_artists(ArtistsMessage::ToggleSection(EPS_HEADER));

    assert_eq!(
        shape(&app),
        ["#Albums", "lp-new", "lp-old", "#EPs", "#Singles", "single"]
    );
    assert_eq!(app.artists_page.common.slot_list.viewport_offset, LP_NEW);
    // The focus lands on the header that was clicked, as a plain click
    // would leave it, and nothing else stays selected.
    assert_eq!(centre_index(&app), Some(EPS_HEADER));
    assert!(app.artists_page.common.activation_batch().is_none());
}

#[test]
fn folding_the_last_section_clamps_the_viewport() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, 10); // the last artist, below the expansion
    let _ = app.handle_artists(ArtistsMessage::ToggleSection(ALBUMS_HEADER));
    let total = app
        .artists_page
        .expansion
        .flattened_len(&app.library.artists);
    assert_eq!(total, 9);
    assert!(app.artists_page.common.slot_list.viewport_offset < total);
}

#[test]
fn toggling_an_album_row_or_an_artist_changes_nothing() {
    let mut app = app_with_albums(mixed());
    let before = shape(&app);
    for row in [0, 1, LP_NEW, 10, 99] {
        let _ = app.handle_artists(ArtistsMessage::ToggleSection(row));
        assert_eq!(shape(&app), before, "row {row}");
    }
    assert!(app.artists_page.collapsed_sections.is_empty());
}

#[test]
fn a_fold_is_remembered_for_the_next_artist_and_a_reload() {
    let mut app = app_with_albums(mixed());
    let _ = app.handle_artists(ArtistsMessage::ToggleSection(EPS_HEADER));
    assert!(
        app.artists_page
            .collapsed_sections
            .contains(&DiscographyGroup::Eps)
    );

    // A background reload of the same artist keeps the fold...
    let _ = app.handle_artists(ArtistsMessage::AlbumsLoaded(ARTIST.to_string(), mixed()));
    assert_eq!(
        shape(&app),
        ["#Albums", "lp-new", "lp-old", "#EPs", "#Singles", "single"]
    );
    // ...and so does the next artist's expansion.
    let next_artists = || -> Vec<AlbumUIViewData> {
        mixed()
            .into_iter()
            .map(|mut album| {
                album.album_artist_ids = vec!["ar2".to_string()];
                album
            })
            .collect()
    };
    let _ = app.handle_artists(ArtistsMessage::CollapseExpansion);
    let _ = app.handle_artists(ArtistsMessage::AlbumsLoaded(
        "ar2".to_string(),
        next_artists(),
    ));
    assert_eq!(
        shape(&app),
        ["#Albums", "lp-new", "lp-old", "#EPs", "#Singles", "single"]
    );

    // Unfolding forgets it. Flattened: ar0, ar1, ar2, #Albums, 2 albums, #EPs.
    let _ = app.handle_artists(ArtistsMessage::ToggleSection(6));
    assert!(app.artists_page.collapsed_sections.is_empty());
    let _ = app.handle_artists(ArtistsMessage::AlbumsLoaded(
        "ar2".to_string(),
        next_artists(),
    ));
    assert_eq!(shape(&app).len(), 8);
}

#[test]
fn the_queue_hotkey_adds_the_centred_headers_section() {
    let mut app = app_with_albums(mixed());
    centre(&mut app, ALBUMS_HEADER);
    let action = page_update(&mut app, slot(SlotListPageMessage::AddCenterToQueue));
    let ArtistsAction::AddBatchToQueue(payload) = action else {
        panic!("Shift+A on a header adds the section, got {action:?}");
    };
    assert_eq!(album_ids(&payload), ["lp-new", "lp-old"]);
}

#[test]
fn a_selected_header_stands_for_its_albums_once_each() {
    let mut app = app_with_albums(mixed());
    // An EP, its header, the Singles header and the next artist, ticked out
    // of order; the batch follows display order.
    for row in [EP_NEW, 10, SINGLES_HEADER, EPS_HEADER] {
        let _ = page_update(&mut app, slot(SlotListPageMessage::SelectionToggle(row)));
    }
    let action = page_update(&mut app, slot(SlotListPageMessage::ActivateCenter(false)));
    let ArtistsAction::PlaySelection(payload, false) = action else {
        panic!("Enter with a selection plays it, got {action:?}");
    };
    let keys: Vec<_> = payload
        .items
        .iter()
        .map(nokkvi_data::types::trawl::batch_item_key)
        .collect();
    assert_eq!(
        keys,
        [
            (TrawlSeedKind::Album, "ep-new"),
            (TrawlSeedKind::Album, "ep-old"),
            (TrawlSeedKind::Album, "single"),
            (TrawlSeedKind::Artist, "ar2"),
        ]
    );
}

#[test]
fn every_section_header_menu_entry_acts_on_the_section() {
    for folded in [false, true] {
        for entry in ArtistsPage::section_menu() {
            let mut app = app_with_albums(mixed());
            if folded {
                let _ = app.handle_artists(ArtistsMessage::ToggleSection(EPS_HEADER));
            }
            let action = page_update(
                &mut app,
                ArtistsMessage::ContextMenuAction(EPS_HEADER, entry),
            );
            let want = ["ep-new", "ep-old"];
            match (entry, &action) {
                (LibraryContextEntry::ShufflePlay, ArtistsAction::PlayBatch(payload, true))
                | (LibraryContextEntry::AddToQueue, ArtistsAction::AddBatchToQueue(payload))
                | (
                    LibraryContextEntry::AddToPlaylist,
                    ArtistsAction::AddBatchToPlaylist(payload),
                ) => assert_eq!(album_ids(payload), want, "{entry:?} folded={folded}"),
                (LibraryContextEntry::AddToMix, ArtistsAction::AddBatchToMix(seeds)) => {
                    let ids: Vec<_> = seeds
                        .iter()
                        .map(|seed| nokkvi_data::types::trawl::batch_item_key(&seed.item).1)
                        .collect();
                    assert_eq!(ids, want, "{entry:?} folded={folded}");
                }
                _ => panic!("header menu entry {entry:?} gave {action:?}"),
            }
        }
    }
}

#[test]
fn the_header_menu_offers_only_entries_a_section_can_take() {
    assert!(matches!(
        ArtistsPage::section_menu().as_slice(),
        [
            LibraryContextEntry::ShufflePlay,
            LibraryContextEntry::AddToQueue,
            LibraryContextEntry::AddToPlaylist,
            LibraryContextEntry::AddToMix,
        ]
    ));
    // The entries it leaves out do nothing on a header.
    let mut app = app_with_albums(mixed());
    for entry in [
        LibraryContextEntry::GetInfo,
        LibraryContextEntry::ShowInFolder,
        LibraryContextEntry::FindSimilar,
        LibraryContextEntry::TopSongs,
        LibraryContextEntry::SimilarArtists,
    ] {
        let action = page_update(
            &mut app,
            ArtistsMessage::ContextMenuAction(EPS_HEADER, entry),
        );
        assert!(
            matches!(action, ArtistsAction::None),
            "{entry:?}: {action:?}"
        );
    }
}

#[test]
fn star_and_rating_clicks_on_a_header_do_nothing() {
    let mut app = app_with_albums(mixed());
    for message in [
        ArtistsMessage::ClickToggleStar(EPS_HEADER),
        ArtistsMessage::ClickSetRating(EPS_HEADER, 4),
    ] {
        let action = page_update(&mut app, message);
        assert!(matches!(action, ArtistsAction::None), "{action:?}");
    }
}
