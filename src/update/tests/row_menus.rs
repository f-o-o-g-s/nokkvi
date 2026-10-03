//! Row-menu coverage: every entry a row's right-click menu offers must
//! produce an action, for every row kind of the four expandable views.
//!
//! The menus and their handlers are separate lists (the renderer offers
//! entries, `update` matches them), so an offered entry without a handler
//! arm compiles green and does nothing. These tests read the same lists the
//! renderers do (`*Page::parent_menu` / `child_menu`, and for Playlists
//! `playlist_context_entries` / `playlist_child_entries`) and fail on any
//! entry that resolves to `Action::None`.

use crate::{
    test_helpers::{make_album, make_artist, make_genre, make_song},
    views::{
        AlbumsAction, AlbumsMessage, AlbumsPage, ArtistsAction, ArtistsMessage, ArtistsPage,
        GenresAction, GenresMessage, GenresPage, PlaylistsAction, PlaylistsMessage, PlaylistsPage,
        playlists::{
            PlaylistContextEntry,
            view::{PlaylistRowFlags, playlist_context_entries},
        },
    },
    widgets::context_menu::{LibraryContextEntry, playlist_child_entries},
};

/// Flattened index of the parent row and of its first expanded child.
const PARENT_ROW: usize = 0;
const CHILD_ROW: usize = 1;

/// The entries a click can fire (separators are not buttons).
fn clickable(entries: Vec<LibraryContextEntry>) -> impl Iterator<Item = LibraryContextEntry> {
    entries
        .into_iter()
        .filter(|e| !matches!(e, LibraryContextEntry::Separator))
}

#[test]
fn every_albums_row_menu_entry_has_an_action() {
    let albums = vec![make_album("a1", "Record", "Band")];
    let rows = [
        (PARENT_ROW, AlbumsPage::parent_menu()),
        (CHILD_ROW, AlbumsPage::child_menu()),
    ];
    for (row, offered) in rows {
        for entry in clickable(offered) {
            let mut page = AlbumsPage::new();
            page.expansion.expanded_id = Some("a1".into());
            page.expansion.children = vec![make_song("s1", "Track", "Band")];

            let (_, action) = page.update(
                AlbumsMessage::ContextMenuAction(row, entry),
                albums.len(),
                &albums,
            );
            assert!(
                !matches!(action, AlbumsAction::None),
                "Albums row {row} offers {entry:?}, which does nothing"
            );
        }
    }
}

#[test]
fn every_artists_row_menu_entry_has_an_action() {
    let artists = vec![make_artist("ar1", "Band")];
    let rows = [
        (PARENT_ROW, ArtistsPage::parent_menu()),
        (CHILD_ROW, ArtistsPage::child_menu()),
    ];
    for (row, offered) in rows {
        for entry in clickable(offered) {
            let mut page = ArtistsPage::new();
            page.expansion.expanded_id = Some("ar1".into());
            page.expansion.children = vec![make_album("a1", "Record", "Band")];

            let (_, action) = page.update(
                ArtistsMessage::ContextMenuAction(row, entry),
                artists.len(),
                &artists,
            );
            assert!(
                !matches!(action, ArtistsAction::None),
                "Artists row {row} offers {entry:?}, which does nothing"
            );
        }
    }
}

#[test]
fn every_genres_row_menu_entry_has_an_action() {
    let genres = vec![make_genre("g1", "Rock")];
    let rows = [
        (PARENT_ROW, GenresPage::parent_menu()),
        (CHILD_ROW, GenresPage::child_menu()),
    ];
    for (row, offered) in rows {
        for entry in clickable(offered) {
            let mut page = GenresPage::new();
            page.expansion.expanded_id = Some("g1".into());
            page.expansion.children = vec![make_album("a1", "Record", "Band")];

            let (_, action) = page.update(
                GenresMessage::ContextMenuAction(row, entry),
                genres.len(),
                &genres,
            );
            assert!(
                !matches!(action, GenresAction::None),
                "Genres row {row} offers {entry:?}, which does nothing"
            );
        }
    }
}

#[test]
fn every_playlists_row_menu_entry_has_an_action() {
    let playlists = vec![super::playlists::playlist_row("p1", "Mix", false)];
    let fresh_page = || {
        let mut page = PlaylistsPage::new();
        page.expansion.expanded_id = Some("p1".into());
        page.expansion.children = vec![make_song("s1", "Track", "Band")];
        page
    };

    // Parent rows: every combination of the flags that gate the menu.
    for bits in 0..8u8 {
        let flags = PlaylistRowFlags {
            has_custom_art: bits & 1 != 0,
            is_smart: bits & 2 != 0,
            is_owned: bits & 4 != 0,
        };
        for entry in playlist_context_entries(flags) {
            if matches!(entry, PlaylistContextEntry::Separator) {
                continue;
            }
            let (_, action) = fresh_page().update(
                PlaylistsMessage::PlaylistContextAction(PARENT_ROW, entry),
                playlists.len(),
                &playlists,
            );
            assert!(
                !matches!(action, PlaylistsAction::None),
                "Playlists parent row ({flags:?}) offers {entry:?}, which does nothing"
            );
        }
    }

    // Child rows, under a regular and under a smart parent.
    for parent_is_smart in [false, true] {
        for entry in clickable(playlist_child_entries(parent_is_smart)) {
            let (_, action) = fresh_page().update(
                PlaylistsMessage::ContextMenuAction(CHILD_ROW, entry),
                playlists.len(),
                &playlists,
            );
            assert!(
                !matches!(action, PlaylistsAction::None),
                "Playlists child row (smart parent: {parent_is_smart}) offers {entry:?}, \
                 which does nothing"
            );
        }
    }
}
