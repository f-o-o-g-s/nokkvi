//! Tests for the Discord Rich Presence handlers.

use crate::{app_message::Message, state::CoverAnswer, test_helpers::*};

#[test]
fn a_tick_built_for_the_item_before_a_switch_is_skipped() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    let request = app.sync_discord_presence(Some("s1"), "Album", 200, Some(10), false, true, false);
    assert!(request.is_none());
    assert!(
        !app.discord.is_running(),
        "a tick describing the previous item must not reach Discord"
    );
}

#[test]
fn the_cover_is_asked_for_only_once_the_song_has_played_a_moment() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
    // The first tick starts the worker; covers are asked from the next one.
    let _ = app.sync_discord_presence(Some("s1"), "Al", 180, Some(0), true, true, false);
    let early = app.sync_discord_presence(Some("s1"), "Al", 180, Some(1), true, true, false);
    assert!(
        early.is_none(),
        "a skip straight past the song asks nothing"
    );
    let settled = app.sync_discord_presence(Some("s1"), "Al", 180, Some(5), true, true, false);
    assert!(settled.is_some());
    let again = app.sync_discord_presence(Some("s1"), "Al", 180, Some(6), true, true, false);
    assert!(again.is_none(), "one ask per album");
}

#[test]
fn a_private_cover_link_ends_the_asking() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    app.library.queue_songs = vec![
        make_queue_song("s1", "T", "A", "Al"),
        make_queue_song("s2", "T2", "A", "Al2"),
    ];
    // The first tick starts the worker; covers are asked from the next one.
    let _ = app.sync_discord_presence(Some("s1"), "Al", 180, Some(4), true, true, false);
    assert!(
        app.sync_discord_presence(Some("s1"), "Al", 180, Some(5), true, true, false)
            .is_some()
    );
    let _ = app.update(Message::DiscordCover {
        album_id: "album_s1".to_string(),
        answer: CoverAnswer::PrivateLink,
    });
    assert!(
        app.sync_discord_presence(Some("s2"), "Al2", 180, Some(5), true, true, false)
            .is_none(),
        "a tailnet server's links are all private: no more getAlbumInfo2"
    );
}

#[test]
fn a_resolved_cover_is_remembered_for_its_album() {
    let mut app = test_app();
    assert!(app.discord.claim_cover("al1", 0));
    let _ = app.update(Message::DiscordCover {
        album_id: "al1".to_string(),
        answer: CoverAnswer::Link("https://m.example/share/img/t?size=1200".to_string()),
    });
    assert_eq!(
        app.discord.cover_of("al1"),
        Some("https://m.example/share/img/t?size=1200")
    );
}

#[test]
fn a_cover_with_no_usable_link_is_still_marked_asked() {
    let mut app = test_app();
    assert!(app.discord.claim_cover("al1", 0));
    let _ = app.update(Message::DiscordCover {
        album_id: "al1".to_string(),
        answer: CoverAnswer::NoLink,
    });
    assert_eq!(app.discord.cover_of("al1"), None);
    assert!(
        !app.discord.claim_cover("al1", i64::MAX),
        "an album the server has no link for is not asked again every tick"
    );
}

/// A failed lookup (a timeout, a server error) is not the album's answer: the
/// song still playing asks again once the wait is over. It used to count as
/// "no link", and the album showed nokkvi's icon until logout.
#[test]
fn a_failed_cover_lookup_is_not_remembered_as_no_cover() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
    let _ = app.sync_discord_presence(Some("s1"), "Al", 180, Some(4), true, true, false);
    assert!(
        app.sync_discord_presence(Some("s1"), "Al", 180, Some(5), true, true, false)
            .is_some()
    );
    let _ = app.update(Message::DiscordCover {
        album_id: "album_s1".to_string(),
        answer: CoverAnswer::Failed,
    });
    assert!(
        app.sync_discord_presence(Some("s1"), "Al", 180, Some(6), true, true, false)
            .is_none(),
        "the failing server is not asked again on the very next tick"
    );
    assert!(
        app.discord.claim_cover("album_s1", i64::MAX),
        "but the album is still to be asked about"
    );
}

#[test]
fn with_presence_off_a_tick_starts_no_worker_and_asks_for_no_cover() {
    let mut app = test_app();
    app.settings.discord_presence = false;
    app.settings.discord_app_id = "123".to_string();
    let request = app.sync_discord_presence(Some("s1"), "Album", 200, Some(10), true, true, false);
    assert!(request.is_none());
    assert!(!app.discord.is_running());
}

#[test]
fn presence_on_with_no_override_runs_under_nokkvis_app() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    app.settings.discord_app_id.clear();
    let _ = app.sync_discord_presence(Some("s1"), "Album", 200, Some(10), true, true, false);
    assert!(
        app.discord.is_running(),
        "an empty override must not keep presence off"
    );
}

#[test]
fn turning_presence_off_stops_the_worker() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    let _ = app.sync_discord_presence(Some("s1"), "Album", 200, Some(10), true, true, false);
    assert!(app.discord.is_running());
    app.settings.discord_presence = false;
    let _ = app.sync_discord_presence(Some("s1"), "Album", 200, Some(11), true, true, false);
    assert!(!app.discord.is_running());
}

#[test]
fn a_stale_tick_waits_for_the_next_one() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    let request = app.sync_discord_presence(Some("s1"), "Album", 200, None, true, true, false);
    assert!(request.is_none());
    assert!(
        !app.discord.is_running(),
        "a stale tick must not even start the worker"
    );
}
