//! Tests for the `Nokkvi.settings: PlayerSettings` substruct
//!
//! Pins the shape of the player-settings consolidation: the 18 persisted
//! fields that previously hung loose on `Nokkvi` now live inside the
//! `settings: PlayerSettings` substruct. UI-runtime flags (e.g.
//! `start_view_applied`, `pending_expand`) stay loose on `Nokkvi` and
//! must remain directly accessible.
//!
//! Also pins the EQ-modal sibling pattern: `eq_modal: EqModalState`
//! is a peer of `info_modal` / `about_modal`, replacing the 4 EQ-modal
//! fields that previously lived on `WindowState`.

use nokkvi_data::types::player_settings::{
    ArtworkResolution, EnterBehavior, LibraryPageSize, PlayerSettings,
};

use crate::test_helpers::*;

// ══════════════════════════════════════════════════════════════════════
//  PlayerSettings substruct — pin shape and load behavior
// ══════════════════════════════════════════════════════════════════════

#[test]
fn nokkvi_default_preserves_pre_substruct_field_values() {
    // Pins the pre-consolidation default values for the 18 mirror fields.
    //
    // `PlayerSettings` derives `Default`, which would zero every field
    // (scrobbling_enabled=false, scrobble_threshold=0.0, start_view="",
    // stable_viewport=false, auto_follow_playing=false). Before the
    // substruct refactor these five were hand-defaulted to non-zero
    // values on `Nokkvi`. `Nokkvi::default()` now restores them via
    // struct-update syntax; this test prevents that restoration from
    // silently disappearing.
    let app = test_app();
    let defaults = PlayerSettings::default();

    // Five fields hand-restored on Nokkvi (different from derive defaults).
    assert!(
        app.settings.scrobbling_enabled,
        "scrobbling_enabled must default to true"
    );
    assert!(
        (app.settings.scrobble_threshold - 0.50).abs() < f32::EPSILON,
        "scrobble_threshold must default to 0.50"
    );
    assert_eq!(
        app.settings.start_view, "Queue",
        "start_view must default to \"Queue\""
    );
    assert!(
        app.settings.stable_viewport,
        "stable_viewport must default to true"
    );
    assert!(
        app.settings.auto_follow_playing,
        "auto_follow_playing must default to true"
    );

    // The remaining 13 mirror fields follow PlayerSettings::default()
    // (all bool=false, all empty String/Option=None/Default enum).
    assert_eq!(
        app.settings.show_album_artists_only,
        defaults.show_album_artists_only
    );
    assert_eq!(
        app.settings.suppress_library_refresh_toasts,
        defaults.suppress_library_refresh_toasts
    );
    assert_eq!(app.settings.show_tray_icon, defaults.show_tray_icon);
    assert_eq!(app.settings.close_to_tray, defaults.close_to_tray);
    assert_eq!(app.settings.enter_behavior, defaults.enter_behavior);
    assert_eq!(app.settings.local_music_path, defaults.local_music_path);
    assert_eq!(app.settings.library_page_size, defaults.library_page_size);
    assert_eq!(
        app.settings.default_playlist_id,
        defaults.default_playlist_id
    );
    assert_eq!(
        app.settings.default_playlist_name,
        defaults.default_playlist_name
    );
    assert_eq!(
        app.settings.quick_add_to_playlist,
        defaults.quick_add_to_playlist
    );
    assert_eq!(
        app.settings.queue_show_default_playlist,
        defaults.queue_show_default_playlist
    );
    assert_eq!(app.settings.verbose_config, defaults.verbose_config);
    assert_eq!(app.settings.artwork_resolution, defaults.artwork_resolution);
}

#[test]
fn handle_player_settings_loaded_replaces_settings_substruct() {
    let mut app = test_app();

    // Construct non-default values for each of the 18 mirror fields. The
    // handler does additional work (engine pushes, column visibility,
    // theme atomics) that requires an app_service or theme state — those
    // side effects are tested elsewhere. Here we only pin that the
    // substruct itself ends up holding the loaded values.
    let settings = PlayerSettings {
        scrobbling_enabled: false,
        scrobble_threshold: 0.75,
        start_view: "Albums".to_string(),
        stable_viewport: false,
        auto_follow_playing: false,
        show_album_artists_only: true,
        suppress_library_refresh_toasts: true,
        show_tray_icon: true,
        close_to_tray: true,
        enter_behavior: EnterBehavior::AppendAndPlay,
        local_music_path: "/srv/music".to_string(),
        library_page_size: LibraryPageSize::Large,
        default_playlist_id: Some("pl-42".to_string()),
        default_playlist_name: "Favourites".to_string(),
        quick_add_to_playlist: true,
        queue_show_default_playlist: true,
        verbose_config: true,
        artwork_resolution: ArtworkResolution::High,
        ..PlayerSettings::default()
    };

    let _ = app.handle_player_settings_loaded(settings);

    assert!(!app.settings.scrobbling_enabled);
    assert!((app.settings.scrobble_threshold - 0.75).abs() < f32::EPSILON);
    assert_eq!(app.settings.start_view, "Albums");
    assert!(!app.settings.stable_viewport);
    assert!(!app.settings.auto_follow_playing);
    assert!(app.settings.show_album_artists_only);
    assert!(app.settings.suppress_library_refresh_toasts);
    assert!(app.settings.show_tray_icon);
    assert!(app.settings.close_to_tray);
    assert_eq!(app.settings.enter_behavior, EnterBehavior::AppendAndPlay);
    assert_eq!(app.settings.local_music_path, "/srv/music");
    assert_eq!(app.settings.library_page_size, LibraryPageSize::Large);
    assert_eq!(app.settings.default_playlist_id, Some("pl-42".to_string()));
    assert_eq!(app.settings.default_playlist_name, "Favourites");
    assert!(app.settings.quick_add_to_playlist);
    assert!(app.settings.queue_show_default_playlist);
    assert!(app.settings.verbose_config);
    assert_eq!(app.settings.artwork_resolution, ArtworkResolution::High);
}

#[test]
fn build_settings_view_data_reads_from_substruct() {
    let mut app = test_app();
    app.settings.start_view = "Songs".to_string();
    app.settings.stable_viewport = false;
    app.settings.auto_follow_playing = false;
    app.settings.show_album_artists_only = true;
    app.settings.suppress_library_refresh_toasts = true;
    app.settings.show_tray_icon = true;
    app.settings.close_to_tray = true;
    app.settings.verbose_config = true;
    app.settings.local_music_path = "/media/lib".to_string();
    app.settings.scrobbling_enabled = false;
    app.settings.scrobble_threshold = 0.33;
    app.settings.quick_add_to_playlist = true;
    app.settings.default_playlist_name = "MyPL".to_string();
    app.settings.queue_show_default_playlist = true;

    let data = app.build_settings_view_data();

    // GeneralSettingsData reflects substruct values
    assert_eq!(data.general.start_view.as_ref(), "Songs");
    assert!(!data.general.stable_viewport);
    assert!(!data.general.auto_follow_playing);
    assert!(data.general.show_album_artists_only);
    assert!(data.general.suppress_library_refresh_toasts);
    assert!(data.general.show_tray_icon);
    assert!(data.general.close_to_tray);
    assert!(data.general.verbose_config);
    assert_eq!(data.general.local_music_path.as_ref(), "/media/lib");

    // PlaybackSettingsData reflects substruct values
    assert!(!data.playback.scrobbling_enabled);
    assert!((data.playback.scrobble_threshold - 0.33).abs() < 1e-6);
    assert!(data.playback.quick_add_to_playlist);
    assert_eq!(data.playback.default_playlist_name.as_ref(), "MyPL");
    assert!(data.playback.queue_show_default_playlist);
}

#[test]
fn ui_runtime_flags_stay_loose_on_nokkvi() {
    let app = test_app();

    // Each of these field accesses is a compile-time pin: removing one
    // of the loose UI-runtime fields (or accidentally moving it into the
    // `settings` substruct) will fail to compile here, NOT silently in
    // a 1500-LOC update handler.
    let _: bool = app.start_view_applied;
    let _: bool = app.suppress_next_auto_center;
    let _: bool = app.pending_expand_center_only;
    let _: &Option<crate::state::PendingExpand> = &app.pending_expand;
    let _: &Option<crate::state::PendingTopPin> = &app.pending_top_pin;
    let _: &Option<String> = &app.server_version;

    // Spot-check the defaults so the test does work beyond field access.
    assert!(!app.start_view_applied);
    assert!(!app.suppress_next_auto_center);
    assert!(!app.pending_expand_center_only);
    assert!(app.pending_expand.is_none());
    assert!(app.pending_top_pin.is_none());
    assert!(app.server_version.is_none());
}

// ══════════════════════════════════════════════════════════════════════
//  EqModalState sibling extraction — pin shape and defaults
// ══════════════════════════════════════════════════════════════════════

#[test]
fn eq_modal_state_defaults_match_expected() {
    let state = crate::widgets::eq_modal::EqModalState::default();
    assert!(!state.open, "EqModalState.open must default to false");
    assert!(
        !state.save_mode,
        "EqModalState.save_mode must default to false"
    );
    assert!(
        state.save_name.is_empty(),
        "EqModalState.save_name must default to empty"
    );
    assert!(
        state.custom_presets.is_empty(),
        "EqModalState.custom_presets must default to empty Vec"
    );
}

#[test]
fn nokkvi_has_eq_modal_sibling_field() {
    // Compile-time pin: removing `eq_modal: EqModalState` from Nokkvi (or
    // accidentally moving it back into WindowState) will fail to compile
    // here, NOT silently leak EQ-modal state into the window-dimensions
    // catch-all.
    let app = test_app();
    let _: &crate::widgets::eq_modal::EqModalState = &app.eq_modal;
    let _: &crate::widgets::info_modal::InfoModalState = &app.info_modal;
    let _: &crate::widgets::about_modal::AboutModalState = &app.about_modal;

    // EQ-modal state lives on its own struct now, not under window.
    assert!(!app.eq_modal.open);
    assert!(!app.eq_modal.save_mode);
    assert!(app.eq_modal.save_name.is_empty());
    assert!(app.eq_modal.custom_presets.is_empty());

    // WindowState shape is now just dimensions + modifiers — pin the
    // remaining four fields so the field set doesn't quietly regrow.
    let _: f32 = app.window.width;
    let _: f32 = app.window.height;
    let _: f32 = app.window.scale_factor;
    let _: iced::keyboard::Modifiers = app.window.keyboard_modifiers;
}
