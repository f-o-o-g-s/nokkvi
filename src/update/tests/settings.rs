//! Tests for settings dispatch update handlers.

use crate::test_helpers::*;

// ============================================================================
// Settings Dispatch (settings.rs)
// ============================================================================

#[test]
fn settings_general_strip_merged_mode_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    // Reset cache to a known state to avoid bleed from other tests touching globals.
    crate::theme::set_strip_merged_mode(false);
    assert!(!crate::theme::strip_merged_mode());

    let _ = app.handle_settings_general(
        "general.strip_merged_mode".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::strip_merged_mode());

    let _ = app.handle_settings_general(
        "general.strip_merged_mode".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::strip_merged_mode());
}

#[test]
fn settings_general_strip_show_labels_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_strip_show_labels(true);
    assert!(crate::theme::strip_show_labels());

    let _ = app.handle_settings_general(
        "general.strip_show_labels".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::strip_show_labels());

    let _ = app.handle_settings_general(
        "general.strip_show_labels".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::strip_show_labels());
}

#[test]
fn settings_general_strip_separator_updates_theme_cache() {
    use nokkvi_data::types::player_settings::StripSeparator;

    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_strip_separator(StripSeparator::Dot);
    assert!(matches!(
        crate::theme::strip_separator(),
        StripSeparator::Dot
    ));

    let _ = app.handle_settings_general(
        "general.strip_separator".to_string(),
        SettingValue::Enum {
            val: "Pipe |".to_string(),
            options: Vec::new(),
        },
    );
    assert!(matches!(
        crate::theme::strip_separator(),
        StripSeparator::Pipe
    ));

    let _ = app.handle_settings_general(
        "general.strip_separator".to_string(),
        SettingValue::Enum {
            val: "Dot ·".to_string(),
            options: Vec::new(),
        },
    );
    assert!(matches!(
        crate::theme::strip_separator(),
        StripSeparator::Dot
    ));
}

#[test]
fn settings_general_albums_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_albums_artwork_overlay(true);
    assert!(crate::theme::albums_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.albums_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::albums_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.albums_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::albums_artwork_overlay());
}

#[test]
fn settings_general_artists_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_artists_artwork_overlay(true);
    assert!(crate::theme::artists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.artists_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::artists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.artists_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::artists_artwork_overlay());
}

#[test]
fn settings_general_songs_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_songs_artwork_overlay(true);
    assert!(crate::theme::songs_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.songs_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::songs_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.songs_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::songs_artwork_overlay());
}

#[test]
fn settings_general_playlists_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_playlists_artwork_overlay(true);
    assert!(crate::theme::playlists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.playlists_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::playlists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.playlists_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::playlists_artwork_overlay());
}
