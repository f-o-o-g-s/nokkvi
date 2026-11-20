//! Tests for the Discord Rich Presence handlers.

use crate::{app_message::Message, test_helpers::*};

#[test]
fn a_resolved_cover_is_remembered_for_its_album() {
    let mut app = test_app();
    assert!(app.discord.claim_cover("al1"));
    let _ = app.update(Message::DiscordCover {
        album_id: "al1".to_string(),
        url: Some("https://m.example/share/img/t?size=1200".to_string()),
    });
    assert_eq!(
        app.discord.cover_of("al1"),
        Some("https://m.example/share/img/t?size=1200")
    );
}

#[test]
fn a_cover_with_no_usable_link_is_still_marked_asked() {
    let mut app = test_app();
    assert!(app.discord.claim_cover("al1"));
    let _ = app.update(Message::DiscordCover {
        album_id: "al1".to_string(),
        url: None,
    });
    assert_eq!(app.discord.cover_of("al1"), None);
    assert!(
        !app.discord.claim_cover("al1"),
        "an album the server has no link for is not asked again every tick"
    );
}

#[test]
fn with_presence_off_a_tick_starts_no_worker_and_asks_for_no_cover() {
    let mut app = test_app();
    app.settings.discord_presence = false;
    app.settings.discord_app_id = "123".to_string();
    let request = app.sync_discord_presence(Some("s1"), "Album", 200, Some(10), true, false);
    assert!(request.is_none());
    assert!(!app.discord.is_running());
}

#[test]
fn presence_on_with_no_override_runs_under_nokkvis_app() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    app.settings.discord_app_id.clear();
    let _ = app.sync_discord_presence(Some("s1"), "Album", 200, Some(10), true, false);
    assert!(
        app.discord.is_running(),
        "an empty override must not keep presence off"
    );
}

#[test]
fn turning_presence_off_stops_the_worker() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    let _ = app.sync_discord_presence(Some("s1"), "Album", 200, Some(10), true, false);
    assert!(app.discord.is_running());
    app.settings.discord_presence = false;
    let _ = app.sync_discord_presence(Some("s1"), "Album", 200, Some(11), true, false);
    assert!(!app.discord.is_running());
}

#[test]
fn a_stale_tick_waits_for_the_next_one() {
    let mut app = test_app();
    app.settings.discord_presence = true;
    let request = app.sync_discord_presence(Some("s1"), "Album", 200, None, true, false);
    assert!(request.is_none());
    assert!(
        !app.discord.is_running(),
        "a stale tick must not even start the worker"
    );
}
