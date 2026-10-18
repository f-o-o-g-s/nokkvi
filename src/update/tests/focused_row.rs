//! The keys act on the highlighted row. Under Stable Viewport a click
//! highlights a row in place, off the viewport's centre, and a find-and-expand
//! pins its target to the top slot; either way the slot list draws the focus
//! marker (`selected_offset`) as the focused row, and Enter, Shift+A and the
//! artwork panel follow it. These pin the other keys to the same row.

use nokkvi_data::backend::{queue::QueueSongUIViewData, songs::SongUIViewData};

use crate::{
    Nokkvi, View,
    app_message::Message,
    test_helpers::*,
    views::{ArtistsMessage, QueueMessage, SongsMessage},
    widgets::SlotListPageMessage,
};

const ROWS: usize = 20;

fn songs() -> Vec<SongUIViewData> {
    (0..ROWS)
        .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "Artist"))
        .collect()
}

fn queue_songs() -> Vec<QueueSongUIViewData> {
    (0..ROWS)
        .map(|i| make_queue_song(&format!("s{i}"), &format!("Song {i}"), "Artist", "Album"))
        .collect()
}

fn app_on_artists() -> Nokkvi {
    let mut app = test_app();
    app.current_view = View::Artists;
    app.library.artists.set_from_vec(
        (0..ROWS)
            .map(|i| make_artist(&format!("a{i}"), &format!("Artist {i}")))
            .collect(),
    );
    app
}

/// Step the list down once, then click the row two below the viewport's
/// centre (Stable Viewport highlights it in place). Returns the clicked
/// index and the viewport centre, which must differ for the test to mean
/// anything.
fn click_off_centre(
    app: &mut Nokkvi,
    wrap: fn(SlotListPageMessage) -> Message,
    total: usize,
    viewport_centre: impl Fn(&Nokkvi) -> Option<usize>,
) -> (usize, usize) {
    let _ = app.update(wrap(SlotListPageMessage::NavigateDown));
    let centre = viewport_centre(app).expect("a viewport centre");
    let clicked = centre + 2;
    assert!(clicked < total);
    let _ = app.update(wrap(SlotListPageMessage::SetOffset(
        clicked,
        iced::keyboard::Modifiers::default(),
    )));
    (clicked, centre)
}

fn last_toast(app: &Nokkvi) -> String {
    app.toast
        .toasts
        .back()
        .map(|toast| toast.message.clone())
        .unwrap_or_default()
}

#[test]
fn the_star_key_stars_the_highlighted_artist() {
    let mut app = app_on_artists();
    let (clicked, centre) = click_off_centre(
        &mut app,
        |m| Message::Artists(ArtistsMessage::SlotList(m)),
        ROWS,
        |app| {
            app.artists_page
                .common
                .get_center_item_index(app.library.artists.len())
        },
    );
    let _ = app.handle_toggle_star();
    let toast = last_toast(&app);
    assert!(
        toast.ends_with(&format!("Artist {clicked}")),
        "starred the highlighted row, not the centre (Artist {centre}): {toast}"
    );
}

#[test]
fn the_rating_keys_rate_the_highlighted_song() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.library.songs.set_from_vec(songs());
    let (clicked, _) = click_off_centre(
        &mut app,
        |m| Message::Songs(SongsMessage::SlotList(m)),
        ROWS,
        |app| {
            app.songs_page
                .common
                .get_center_item_index(app.library.songs.len())
        },
    );
    let _ = app.handle_increase_rating();
    let toast = last_toast(&app);
    assert!(
        toast.contains(&format!("Song {clicked} - Artist")),
        "rated the highlighted row: {toast}"
    );
}

#[test]
fn get_info_opens_the_highlighted_song() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.library.songs.set_from_vec(songs());
    let (clicked, _) = click_off_centre(
        &mut app,
        |m| Message::Songs(SongsMessage::SlotList(m)),
        ROWS,
        |app| {
            app.songs_page
                .common
                .get_center_item_index(app.library.songs.len())
        },
    );
    let _ = app.handle_get_info();
    let title = app
        .info_modal
        .item
        .as_ref()
        .map(|item| item.title().to_string());
    assert_eq!(title, Some(format!("Song {clicked}")));
}

fn app_on_queue() -> Nokkvi {
    let mut app = test_app();
    app.current_view = View::Queue;
    app.library.queue_songs = queue_songs();
    app
}

fn click_queue_off_centre(app: &mut Nokkvi) -> (usize, usize) {
    click_off_centre(
        app,
        |m| Message::Queue(QueueMessage::SlotList(m)),
        ROWS,
        |app| {
            app.queue_page
                .common
                .get_center_item_index(app.library.queue_songs.len())
        },
    )
}

#[test]
fn the_star_key_stars_the_highlighted_queue_song() {
    let mut app = app_on_queue();
    let (clicked, _) = click_queue_off_centre(&mut app);
    let _ = app.handle_toggle_star();
    let toast = last_toast(&app);
    assert!(
        toast.ends_with(&format!("Song {clicked}")),
        "starred the highlighted row: {toast}"
    );
}

#[test]
fn ctrl_d_removes_the_highlighted_queue_row() {
    let mut app = app_on_queue();
    let (clicked, centre) = click_queue_off_centre(&mut app);
    let _ = app.handle_remove_from_queue();
    let ids: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(ids.len(), ROWS - 1, "one row removed");
    assert!(
        !ids.contains(&format!("s{clicked}").as_str()),
        "the highlighted row went"
    );
    assert!(
        ids.contains(&format!("s{centre}").as_str()),
        "the centre row stayed"
    );
}

#[test]
fn shift_down_moves_the_highlighted_queue_row() {
    let mut app = app_on_queue();
    let (clicked, _) = click_queue_off_centre(&mut app);
    let _ = app.handle_move_track(false);
    assert_eq!(
        app.library.queue_songs[clicked + 1].id,
        format!("s{clicked}"),
        "the highlighted row moved one place down"
    );
    assert_eq!(
        app.queue_page.common.get_queue_target_indices(ROWS),
        vec![clicked + 1],
        "the focus followed the row, and Shift+A would add it"
    );
}
