//! Tests for settings dispatch update handlers.
//!
//! Interface-tab keys (`strip_*`, `*_artwork_overlay`, etc.) and the
//! migrated general-tab side-effect keys (`light_mode`, `local_music_path`,
//! `show_album_artists_only`, `artwork_resolution`, `verbose_config`) all
//! route through `define_settings!` in
//! `nokkvi_data::services::settings_tables`. The data crate owns the
//! dispatch + apply round-trip tests; the UI-side coverage below verifies
//! that `dispatch_settings_side_effect` translates each
//! [`SettingsSideEffect`] variant into the right user-visible follow-up:
//! toast pushed at the right level, `LoadArtists` / Tick task chained,
//! verbose-config TOML toast surfaced.
//!
//! The `artwork_overlay_*` tests below cover the `handle_player_settings_loaded`
//! path: when settings with `albums_artwork_overlay = false` (or any other
//! per-view variant) arrive, the corresponding process-global theme atomic must
//! be flipped. Each test saves the prior value and restores it on exit to avoid
//! bleeding state into the shared `UI_MODE` statics for parallel tests.

use nokkvi_data::{
    services::settings_tables::SettingsSideEffect,
    types::{player_settings::LivePlayerSettings, toast::ToastLevel, view_columns::ViewColumns},
};

use crate::test_helpers::*;

#[test]
fn side_effect_none_does_not_push_a_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::None);

    assert!(
        app.toast.toasts.is_empty(),
        "SettingsSideEffect::None must be a pure pass-through"
    );
}

#[test]
fn side_effect_toast_info_pushes_info_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Info,
        message: "hello".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Info);
    assert_eq!(app.toast.toasts[0].message, "hello");
}

#[test]
fn side_effect_toast_success_pushes_success_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Success,
        message: "done".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Success);
}

#[test]
fn side_effect_toast_warning_pushes_warning_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Warning,
        message: "heads up".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Warning);
}

#[test]
fn side_effect_toast_error_pushes_error_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Error,
        message: "boom".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Error);
}

#[test]
fn side_effect_load_artists_does_not_push_a_toast() {
    // The `Task<Message>` returned by `dispatch_settings_side_effect` is
    // opaque to unit tests, so we can only assert on observable state. The
    // important property is that emitting `LoadArtists` does NOT also push
    // a toast — that would be a UX regression on the show-album-artists
    // toggle, where the legacy arm intentionally ran silent.
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::LoadArtists);

    assert!(app.toast.toasts.is_empty());
}

#[test]
fn side_effect_set_light_mode_atomic_does_not_push_a_toast() {
    // The legacy `general.light_mode` arm only flipped the theme atomic and
    // wrote `config.toml`; it never surfaced a toast. The audit warns
    // against asserting on the process-global theme atomic in tests, so we
    // verify the silent contract: no toast surfaces on a side-effect run.
    let mut app = test_app();
    let prior_state = crate::theme::is_light_mode();

    let _task =
        app.dispatch_settings_side_effect(SettingsSideEffect::SetLightModeAtomic(!prior_state));

    assert!(
        app.toast.toasts.is_empty(),
        "SetLightModeAtomic must not push a toast; it only flips the atomic + config.toml"
    );

    // Restore the global atomic so this test does not bleed state into the
    // shared theme module that other tests in this binary may depend on.
    crate::theme::set_light_mode(prior_state);
}

#[test]
fn side_effect_write_verbose_config_enable_emits_success_toast() {
    // `WriteVerboseConfig { enabled: true }` writes the [visualizer]
    // section to `config.toml`. In a unit test the working directory points
    // at the repo, so the write may fail (file permissions / not present);
    // either way one toast (success or warn) MUST be pushed so the user
    // gets feedback.
    let mut app = test_app();

    let _task =
        app.dispatch_settings_side_effect(SettingsSideEffect::WriteVerboseConfig { enabled: true });

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "verbose_config toggle must push exactly one toast (success or warn)"
    );
    let level = app.toast.toasts[0].level;
    assert!(
        matches!(level, ToastLevel::Success | ToastLevel::Warning),
        "expected Success or Warning, got {level:?}"
    );
}

#[test]
fn side_effect_write_verbose_config_disable_emits_success_toast() {
    let mut app = test_app();

    let _task = app
        .dispatch_settings_side_effect(SettingsSideEffect::WriteVerboseConfig { enabled: false });

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "verbose_config toggle must push exactly one toast (success or warn)"
    );
    let level = app.toast.toasts[0].level;
    assert!(
        matches!(level, ToastLevel::Success | ToastLevel::Warning),
        "expected Success or Warning, got {level:?}"
    );
}

// ============================================================================
// Artwork overlay theme-atomic tests (B5)
// ============================================================================

/// Serializes the two tests that read-modify-restore the process-global
/// `albums_artwork_overlay` atomic. Both flip the same shared static, so under
/// parallel execution they race (one test's restore clobbers the other's
/// assertion). Holding this lock for the full duration of each test forces them
/// to run one at a time. `unwrap_or_else(|e| e.into_inner())` recovers a
/// poisoned lock so a panic in one test does not cascade into the other.
static OVERLAY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn player_settings_loaded_albums_artwork_overlay_false_clears_atomic() {
    let _guard = OVERLAY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    let prior = crate::theme::albums_artwork_overlay();

    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        albums_artwork_overlay: false,
        ..Default::default()
    });

    assert!(
        !crate::theme::albums_artwork_overlay(),
        "albums_artwork_overlay atomic must be false after loading settings with false"
    );

    // Restore to avoid bleeding state between tests.
    crate::theme::set_albums_artwork_overlay(prior);
}

#[test]
fn player_settings_loaded_albums_artwork_overlay_true_sets_atomic() {
    let _guard = OVERLAY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Force the atomic to false first so the test is self-contained regardless
    // of the initial global state (avoids false-pass when it's already true).
    crate::theme::set_albums_artwork_overlay(false);
    let mut app = test_app();

    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        albums_artwork_overlay: true,
        ..Default::default()
    });

    assert!(
        crate::theme::albums_artwork_overlay(),
        "albums_artwork_overlay atomic must be true after loading settings with true"
    );

    // Restore default (true — the real default from toml_settings.rs).
    crate::theme::set_albums_artwork_overlay(true);
}

#[test]
fn player_settings_loaded_artists_artwork_overlay_flips_atomic() {
    let mut app = test_app();
    let prior = crate::theme::artists_artwork_overlay();

    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        artists_artwork_overlay: false,
        ..Default::default()
    });
    assert!(!crate::theme::artists_artwork_overlay());

    crate::theme::set_artists_artwork_overlay(prior);
}

#[test]
fn player_settings_loaded_songs_artwork_overlay_flips_atomic() {
    let mut app = test_app();
    let prior = crate::theme::songs_artwork_overlay();

    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        songs_artwork_overlay: false,
        ..Default::default()
    });
    assert!(!crate::theme::songs_artwork_overlay());

    crate::theme::set_songs_artwork_overlay(prior);
}

#[test]
fn player_settings_loaded_playlists_artwork_overlay_flips_atomic() {
    let mut app = test_app();
    let prior = crate::theme::playlists_artwork_overlay();

    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        playlists_artwork_overlay: false,
        ..Default::default()
    });
    assert!(!crate::theme::playlists_artwork_overlay());

    crate::theme::set_playlists_artwork_overlay(prior);
}

// ============================================================================
// Themeable-flag mirror completeness guard (audit rank 8)
// ============================================================================
//
// `handle_player_settings_loaded` mirrors every settings-driven theme field
// (`settings.rounded_mode`, `settings.nav_layout`, …) into a process-global
// theme atomic via a 1:1 block of `crate::theme::set_*(settings.*)` calls
// (playback.rs). The 5 per-view `*_artwork_overlay` tests above each pin ONE
// field; this test is the all-fields completeness sweep. Adding a new themeable
// field to `LivePlayerSettings` + a getter/setter in `theme.rs` but FORGETTING
// the matching `set_*` mirror line leaves the new field unreflected on load —
// this test pre-seeds every atomic to the OPPOSITE of the value it will load,
// runs the handler, and asserts each getter now equals the loaded value. A
// missing mirror line leaves the stale (pre-seeded) value and the assertion
// fires, naming the exact field so the maintainer finds the dropped `set_*`.
//
// EXCLUDED: `light_mode`. The handler calls `set_light_mode` from
// `load_light_mode_from_config()` (config.toml), NOT `settings.light_mode` —
// it is not a `LivePlayerSettings` field, so it is correctly out of scope.
//
// LOSSY GETTERS AVOIDED: `is_rounded_mode()` collapses the 3-variant
// `RoundedMode` to a bool (PlayerOnly -> false), and `slot_row_height()`
// returns f32 pixels. The faithful getters `rounded_mode()` /
// `slot_row_height_variant()` are used instead.

#[test]
fn player_settings_loaded_mirrors_all_theme_atomics() {
    use nokkvi_data::types::player_settings::{
        ArtworkColumnMode, ArtworkStretchFit, NavDisplayMode, NavLayout, RoundedMode,
        SlotRowHeight, StripClickAction, StripSeparator, TrackInfoDisplay,
    };

    // Serialize against the 5 artwork-overlay tests: this sweep sets the same
    // process-global `*_artwork_overlay` atomics (plus ~22 others), so without
    // the shared lock one test's restore could clobber another's assertion.
    let _guard = OVERLAY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // --- Snapshot every mutated atomic so nothing bleeds into other tests. ---
    let prior_rounded_mode = crate::theme::rounded_mode();
    let prior_nav_layout = crate::theme::nav_layout();
    let prior_nav_display_mode = crate::theme::nav_display_mode();
    let prior_track_info_display = crate::theme::track_info_display();
    let prior_slot_row_height = crate::theme::slot_row_height_variant();
    let prior_opacity_gradient = crate::theme::is_opacity_gradient();
    let prior_slot_text_links = crate::theme::is_slot_text_links();
    let prior_horizontal_volume = crate::theme::is_horizontal_volume();
    let prior_font_family = crate::theme::font_family();
    let prior_strip_show_title = crate::theme::strip_show_title();
    let prior_strip_show_artist = crate::theme::strip_show_artist();
    let prior_strip_show_album = crate::theme::strip_show_album();
    let prior_strip_show_format_info = crate::theme::strip_show_format_info();
    let prior_strip_merged_mode = crate::theme::strip_merged_mode();
    let prior_strip_click_action = crate::theme::strip_click_action();
    let prior_strip_show_labels = crate::theme::strip_show_labels();
    let prior_strip_separator = crate::theme::strip_separator();
    let prior_albums_overlay = crate::theme::albums_artwork_overlay();
    let prior_artists_overlay = crate::theme::artists_artwork_overlay();
    let prior_songs_overlay = crate::theme::songs_artwork_overlay();
    let prior_playlists_overlay = crate::theme::playlists_artwork_overlay();
    let prior_artwork_column_mode = crate::theme::artwork_column_mode();
    let prior_artwork_column_stretch_fit = crate::theme::artwork_column_stretch_fit();
    let prior_artwork_column_width_pct = crate::theme::artwork_column_width_pct();
    let prior_artwork_auto_max_pct = crate::theme::artwork_auto_max_pct();
    let prior_artwork_vertical_height_pct = crate::theme::artwork_vertical_height_pct();

    // --- Pre-seed every atomic to a value DISTINCT from what we'll load. ---
    // Without this, a dropped `set_*` line could coincidentally already hold
    // the target value and the test would pass falsely. Each pre-seed is the
    // enum's #[default] / the opposite bool / a different float from the load.
    crate::theme::set_rounded_mode(RoundedMode::On);
    crate::theme::set_nav_layout(NavLayout::Top);
    crate::theme::set_nav_display_mode(NavDisplayMode::TextOnly);
    crate::theme::set_track_info_display(TrackInfoDisplay::Off);
    crate::theme::set_slot_row_height(SlotRowHeight::Default);
    crate::theme::set_opacity_gradient(false);
    crate::theme::set_slot_text_links(false);
    crate::theme::set_horizontal_volume(false);
    crate::theme::set_font_family("NokkviPreSeedDistinctFont".to_string());
    crate::theme::set_strip_show_title(false);
    crate::theme::set_strip_show_artist(false);
    crate::theme::set_strip_show_album(false);
    crate::theme::set_strip_show_format_info(false);
    crate::theme::set_strip_merged_mode(false);
    crate::theme::set_strip_click_action(StripClickAction::GoToQueue);
    crate::theme::set_strip_show_labels(false);
    crate::theme::set_strip_separator(StripSeparator::Dot);
    crate::theme::set_albums_artwork_overlay(false);
    crate::theme::set_artists_artwork_overlay(false);
    crate::theme::set_songs_artwork_overlay(false);
    crate::theme::set_playlists_artwork_overlay(false);
    crate::theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
    crate::theme::set_artwork_column_stretch_fit(ArtworkStretchFit::Cover);
    crate::theme::set_artwork_column_width_pct(0.99);
    crate::theme::set_artwork_auto_max_pct(0.99);
    crate::theme::set_artwork_vertical_height_pct(0.99);

    // --- Build a LivePlayerSettings with every themeable field non-default. ---
    // Floats compare exactly: the atomics store `f32::to_bits` and the getters
    // do `from_bits`, an exact round-trip, so `==` is safe here.
    let settings = LivePlayerSettings {
        rounded_mode: RoundedMode::PlayerOnly,
        nav_layout: NavLayout::Side,
        nav_display_mode: NavDisplayMode::IconsOnly,
        track_info_display: TrackInfoDisplay::TopBar,
        slot_row_height: SlotRowHeight::Spacious,
        opacity_gradient: true,
        slot_text_links: true,
        horizontal_volume: true,
        font_family: "NokkviTestSentinelFont".to_string(),
        strip_show_title: true,
        strip_show_artist: true,
        strip_show_album: true,
        strip_show_format_info: true,
        strip_merged_mode: true,
        strip_click_action: StripClickAction::CopyTrackInfo,
        strip_show_labels: true,
        strip_separator: StripSeparator::Pipe,
        albums_artwork_overlay: true,
        artists_artwork_overlay: true,
        songs_artwork_overlay: true,
        playlists_artwork_overlay: true,
        artwork_column_mode: ArtworkColumnMode::AlwaysVerticalStretched,
        artwork_column_stretch_fit: ArtworkStretchFit::Fill,
        artwork_column_width_pct: 0.37,
        artwork_auto_max_pct: 0.55,
        artwork_vertical_height_pct: 0.42,
        ..Default::default()
    };

    let mut app = test_app();
    let _ = app.handle_player_settings_loaded(settings.clone());

    // --- Assert every settings-driven getter now reflects the loaded value. ---
    // Each message names the field so a future skipped mirror line points the
    // maintainer at the exact missing `crate::theme::set_*` call in playback.rs.
    assert_eq!(
        crate::theme::rounded_mode(),
        settings.rounded_mode,
        "rounded_mode not mirrored (missing set_rounded_mode in handle_player_settings_loaded)"
    );
    assert_eq!(
        crate::theme::nav_layout(),
        settings.nav_layout,
        "nav_layout not mirrored (missing set_nav_layout)"
    );
    assert_eq!(
        crate::theme::nav_display_mode(),
        settings.nav_display_mode,
        "nav_display_mode not mirrored (missing set_nav_display_mode)"
    );
    assert_eq!(
        crate::theme::track_info_display(),
        settings.track_info_display,
        "track_info_display not mirrored (missing set_track_info_display)"
    );
    assert_eq!(
        crate::theme::slot_row_height_variant(),
        settings.slot_row_height,
        "slot_row_height not mirrored (missing set_slot_row_height)"
    );
    assert_eq!(
        crate::theme::is_opacity_gradient(),
        settings.opacity_gradient,
        "opacity_gradient not mirrored (missing set_opacity_gradient)"
    );
    assert_eq!(
        crate::theme::is_slot_text_links(),
        settings.slot_text_links,
        "slot_text_links not mirrored (missing set_slot_text_links)"
    );
    assert_eq!(
        crate::theme::is_horizontal_volume(),
        settings.horizontal_volume,
        "horizontal_volume not mirrored (missing set_horizontal_volume)"
    );
    assert_eq!(
        crate::theme::font_family(),
        settings.font_family,
        "font_family not mirrored (missing set_font_family)"
    );
    assert_eq!(
        crate::theme::strip_show_title(),
        settings.strip_show_title,
        "strip_show_title not mirrored (missing set_strip_show_title)"
    );
    assert_eq!(
        crate::theme::strip_show_artist(),
        settings.strip_show_artist,
        "strip_show_artist not mirrored (missing set_strip_show_artist)"
    );
    assert_eq!(
        crate::theme::strip_show_album(),
        settings.strip_show_album,
        "strip_show_album not mirrored (missing set_strip_show_album)"
    );
    assert_eq!(
        crate::theme::strip_show_format_info(),
        settings.strip_show_format_info,
        "strip_show_format_info not mirrored (missing set_strip_show_format_info)"
    );
    assert_eq!(
        crate::theme::strip_merged_mode(),
        settings.strip_merged_mode,
        "strip_merged_mode not mirrored (missing set_strip_merged_mode)"
    );
    assert_eq!(
        crate::theme::strip_click_action(),
        settings.strip_click_action,
        "strip_click_action not mirrored (missing set_strip_click_action)"
    );
    assert_eq!(
        crate::theme::strip_show_labels(),
        settings.strip_show_labels,
        "strip_show_labels not mirrored (missing set_strip_show_labels)"
    );
    assert_eq!(
        crate::theme::strip_separator(),
        settings.strip_separator,
        "strip_separator not mirrored (missing set_strip_separator)"
    );
    assert_eq!(
        crate::theme::albums_artwork_overlay(),
        settings.albums_artwork_overlay,
        "albums_artwork_overlay not mirrored (missing set_albums_artwork_overlay)"
    );
    assert_eq!(
        crate::theme::artists_artwork_overlay(),
        settings.artists_artwork_overlay,
        "artists_artwork_overlay not mirrored (missing set_artists_artwork_overlay)"
    );
    assert_eq!(
        crate::theme::songs_artwork_overlay(),
        settings.songs_artwork_overlay,
        "songs_artwork_overlay not mirrored (missing set_songs_artwork_overlay)"
    );
    assert_eq!(
        crate::theme::playlists_artwork_overlay(),
        settings.playlists_artwork_overlay,
        "playlists_artwork_overlay not mirrored (missing set_playlists_artwork_overlay)"
    );
    assert_eq!(
        crate::theme::artwork_column_mode(),
        settings.artwork_column_mode,
        "artwork_column_mode not mirrored (missing set_artwork_column_mode)"
    );
    assert_eq!(
        crate::theme::artwork_column_stretch_fit(),
        settings.artwork_column_stretch_fit,
        "artwork_column_stretch_fit not mirrored (missing set_artwork_column_stretch_fit)"
    );
    assert_eq!(
        crate::theme::artwork_column_width_pct(),
        settings.artwork_column_width_pct,
        "artwork_column_width_pct not mirrored (missing set_artwork_column_width_pct)"
    );
    assert_eq!(
        crate::theme::artwork_auto_max_pct(),
        settings.artwork_auto_max_pct,
        "artwork_auto_max_pct not mirrored (missing set_artwork_auto_max_pct)"
    );
    assert_eq!(
        crate::theme::artwork_vertical_height_pct(),
        settings.artwork_vertical_height_pct,
        "artwork_vertical_height_pct not mirrored (missing set_artwork_vertical_height_pct)"
    );

    // --- Restore every mutated atomic from the snapshot. ---
    crate::theme::set_rounded_mode(prior_rounded_mode);
    crate::theme::set_nav_layout(prior_nav_layout);
    crate::theme::set_nav_display_mode(prior_nav_display_mode);
    crate::theme::set_track_info_display(prior_track_info_display);
    crate::theme::set_slot_row_height(prior_slot_row_height);
    crate::theme::set_opacity_gradient(prior_opacity_gradient);
    crate::theme::set_slot_text_links(prior_slot_text_links);
    crate::theme::set_horizontal_volume(prior_horizontal_volume);
    crate::theme::set_font_family(prior_font_family);
    crate::theme::set_strip_show_title(prior_strip_show_title);
    crate::theme::set_strip_show_artist(prior_strip_show_artist);
    crate::theme::set_strip_show_album(prior_strip_show_album);
    crate::theme::set_strip_show_format_info(prior_strip_show_format_info);
    crate::theme::set_strip_merged_mode(prior_strip_merged_mode);
    crate::theme::set_strip_click_action(prior_strip_click_action);
    crate::theme::set_strip_show_labels(prior_strip_show_labels);
    crate::theme::set_strip_separator(prior_strip_separator);
    crate::theme::set_albums_artwork_overlay(prior_albums_overlay);
    crate::theme::set_artists_artwork_overlay(prior_artists_overlay);
    crate::theme::set_songs_artwork_overlay(prior_songs_overlay);
    crate::theme::set_playlists_artwork_overlay(prior_playlists_overlay);
    crate::theme::set_artwork_column_mode(prior_artwork_column_mode);
    crate::theme::set_artwork_column_stretch_fit(prior_artwork_column_stretch_fit);
    crate::theme::set_artwork_column_width_pct(prior_artwork_column_width_pct);
    crate::theme::set_artwork_auto_max_pct(prior_artwork_auto_max_pct);
    crate::theme::set_artwork_vertical_height_pct(prior_artwork_vertical_height_pct);
}

// ============================================================================
// Column-visibility restore wiring (audit rank 6)
// ============================================================================
//
// `handle_player_settings_loaded` must rebuild every page's `column_visibility`
// struct from the persisted `LivePlayerSettings` via the macro-generated
// `restore_from`. These tests pin (a) that the handler wires `restore_from` for
// all seven pages and reads the right per-column field, and (b) the total number
// of macro-owned column fields, so a future `*_show_*` column added to
// `ViewColumns` without a macro entry trips this guard instead of
// silently snapping back to default on load.

#[test]
fn player_settings_loaded_restores_all_column_visibility() {
    let mut app = test_app();

    // Every one of the 50 column `*_show_*` fields, set to an alternating
    // true/false pattern by declaration order so each adjacent pair differs.
    // Any assertion below fails if `restore_from` read the wrong field (the
    // realistic drift is a copy-pasted `@ token` matching a neighbor) or if the
    // handler skipped a page. The non-column `*_show_*` fields
    // (queue_show_default_playlist + 5 strip_show_*) are left at their defaults
    // via `..Default::default()`.
    let settings = LivePlayerSettings {
        view_columns: ViewColumns {
            // Queue
            queue_show_select: true,
            queue_show_index: false,
            queue_show_thumbnail: true,
            queue_show_stars: false,
            queue_show_album: true,
            queue_show_duration: false,
            queue_show_love: true,
            queue_show_plays: false,
            queue_show_genre: true,
            // Artists
            artists_show_select: true,
            artists_show_index: false,
            artists_show_thumbnail: true,
            artists_show_stars: false,
            artists_show_albumcount: true,
            artists_show_songcount: false,
            artists_show_plays: true,
            artists_show_love: false,
            // Genres
            genres_show_select: true,
            genres_show_index: false,
            genres_show_thumbnail: true,
            genres_show_albumcount: false,
            genres_show_songcount: true,
            // Playlists
            playlists_show_select: true,
            playlists_show_index: false,
            playlists_show_thumbnail: true,
            playlists_show_songcount: false,
            playlists_show_duration: true,
            playlists_show_updatedat: false,
            // Albums
            albums_show_select: true,
            albums_show_index: false,
            albums_show_thumbnail: true,
            albums_show_stars: false,
            albums_show_songcount: true,
            albums_show_plays: false,
            albums_show_love: true,
            // Songs
            songs_show_select: true,
            songs_show_index: false,
            songs_show_thumbnail: true,
            songs_show_stars: false,
            songs_show_album: true,
            songs_show_duration: false,
            songs_show_plays: true,
            songs_show_love: false,
            songs_show_genre: true,
            // Similar
            similar_show_select: true,
            similar_show_index: false,
            similar_show_thumbnail: true,
            similar_show_album: false,
            similar_show_duration: true,
            similar_show_love: false,
        },
        ..Default::default()
    };

    let _ = app.handle_player_settings_loaded(settings.clone());

    // Queue
    let q = app.queue_page.column_visibility;
    assert_eq!(q.select, settings.view_columns.queue_show_select);
    assert_eq!(q.index, settings.view_columns.queue_show_index);
    assert_eq!(q.thumbnail, settings.view_columns.queue_show_thumbnail);
    assert_eq!(q.stars, settings.view_columns.queue_show_stars);
    assert_eq!(q.album, settings.view_columns.queue_show_album);
    assert_eq!(q.duration, settings.view_columns.queue_show_duration);
    assert_eq!(q.love, settings.view_columns.queue_show_love);
    assert_eq!(q.plays, settings.view_columns.queue_show_plays);
    assert_eq!(q.genre, settings.view_columns.queue_show_genre);

    // Artists
    let a = app.artists_page.column_visibility;
    assert_eq!(a.select, settings.view_columns.artists_show_select);
    assert_eq!(a.index, settings.view_columns.artists_show_index);
    assert_eq!(a.thumbnail, settings.view_columns.artists_show_thumbnail);
    assert_eq!(a.stars, settings.view_columns.artists_show_stars);
    assert_eq!(a.albumcount, settings.view_columns.artists_show_albumcount);
    assert_eq!(a.songcount, settings.view_columns.artists_show_songcount);
    assert_eq!(a.plays, settings.view_columns.artists_show_plays);
    assert_eq!(a.love, settings.view_columns.artists_show_love);

    // Genres
    let g = app.genres_page.column_visibility;
    assert_eq!(g.select, settings.view_columns.genres_show_select);
    assert_eq!(g.index, settings.view_columns.genres_show_index);
    assert_eq!(g.thumbnail, settings.view_columns.genres_show_thumbnail);
    assert_eq!(g.albumcount, settings.view_columns.genres_show_albumcount);
    assert_eq!(g.songcount, settings.view_columns.genres_show_songcount);

    // Playlists
    let p = app.playlists_page.column_visibility;
    assert_eq!(p.select, settings.view_columns.playlists_show_select);
    assert_eq!(p.index, settings.view_columns.playlists_show_index);
    assert_eq!(p.thumbnail, settings.view_columns.playlists_show_thumbnail);
    assert_eq!(p.songcount, settings.view_columns.playlists_show_songcount);
    assert_eq!(p.duration, settings.view_columns.playlists_show_duration);
    assert_eq!(p.updatedat, settings.view_columns.playlists_show_updatedat);

    // Albums
    let al = app.albums_page.column_visibility;
    assert_eq!(al.select, settings.view_columns.albums_show_select);
    assert_eq!(al.index, settings.view_columns.albums_show_index);
    assert_eq!(al.thumbnail, settings.view_columns.albums_show_thumbnail);
    assert_eq!(al.stars, settings.view_columns.albums_show_stars);
    assert_eq!(al.songcount, settings.view_columns.albums_show_songcount);
    assert_eq!(al.plays, settings.view_columns.albums_show_plays);
    assert_eq!(al.love, settings.view_columns.albums_show_love);

    // Songs
    let s = app.songs_page.column_visibility;
    assert_eq!(s.select, settings.view_columns.songs_show_select);
    assert_eq!(s.index, settings.view_columns.songs_show_index);
    assert_eq!(s.thumbnail, settings.view_columns.songs_show_thumbnail);
    assert_eq!(s.stars, settings.view_columns.songs_show_stars);
    assert_eq!(s.album, settings.view_columns.songs_show_album);
    assert_eq!(s.duration, settings.view_columns.songs_show_duration);
    assert_eq!(s.plays, settings.view_columns.songs_show_plays);
    assert_eq!(s.love, settings.view_columns.songs_show_love);
    assert_eq!(s.genre, settings.view_columns.songs_show_genre);

    // Similar
    let si = app.similar_page.column_visibility;
    assert_eq!(si.select, settings.view_columns.similar_show_select);
    assert_eq!(si.index, settings.view_columns.similar_show_index);
    assert_eq!(si.thumbnail, settings.view_columns.similar_show_thumbnail);
    assert_eq!(si.album, settings.view_columns.similar_show_album);
    assert_eq!(si.duration, settings.view_columns.similar_show_duration);
    assert_eq!(si.love, settings.view_columns.similar_show_love);
}

#[test]
fn column_macro_covers_expected_field_count() {
    use std::mem::size_of;

    // Completeness guard. Each `*ColumnVisibility` is a struct of `bool` fields,
    // one per column the macro owns; `size_of` == field count (bool is 1 byte,
    // no padding for an all-bool struct). The sum across the 7 production
    // visibility structs MUST equal the number of column `*_show_*` fields the
    // macro restores. This guard trips when a column is ADDED or REMOVED in a
    // `define_view_columns!` invocation without updating the count below — the
    // realistic drift. It does NOT auto-detect a brand-new `ViewColumns`
    // `*_show_*` field that lacks a macro entry (a settings-only field leaves the
    // struct sizes unchanged); catching that direction relies on also adding the
    // column + its per-page round-trip assertion.
    //
    // 50 column fields total today (verified against
    // data/src/types/view_columns.rs). The 6 INTENTIONALLY-EXCLUDED
    // `*_show_*` fields are NOT columns and must NOT be counted here:
    //   - queue_show_default_playlist  (header chip, read directly in app_view)
    //   - strip_show_title / strip_show_artist / strip_show_album /
    //     strip_show_format_info / strip_show_labels  (metadata-strip toggles)
    const EXPECTED_COLUMN_FIELDS: usize = 50;

    let total = size_of::<crate::views::queue::QueueColumnVisibility>()
        + size_of::<crate::views::artists::ArtistsColumnVisibility>()
        + size_of::<crate::views::genres::GenresColumnVisibility>()
        + size_of::<crate::views::playlists::PlaylistsColumnVisibility>()
        + size_of::<crate::views::albums::AlbumsColumnVisibility>()
        + size_of::<crate::views::songs::SongsColumnVisibility>()
        + size_of::<crate::views::similar::SimilarColumnVisibility>();

    assert_eq!(
        total, EXPECTED_COLUMN_FIELDS,
        "column-visibility field count drifted: a column was added/removed in a \
         define_view_columns! invocation. Update EXPECTED_COLUMN_FIELDS and ensure \
         the matching LivePlayerSettings field + macro @ token exist (see comment)."
    );
}

#[test]
fn column_visibility_defaults_agree_with_live_player_settings_defaults() {
    // domain-types-3 guard: `LivePlayerSettings::default()` carries the real
    // shipped column defaults via the shared `ViewColumns::default()`, so the
    // pre-`PlayerSettingsLoaded` window and a `restore_from` over a default
    // settings struct must agree with each view's own macro-declared
    // `{View}ColumnVisibility::default()`. This pins the 4th drift surface
    // (the UI macro literal defaults in views/*/mod.rs) to the canonical
    // source of truth in data/src/types/view_columns.rs — retune a default in
    // one place without the other and this fails naming the view.
    let live = LivePlayerSettings::default();

    assert_eq!(
        crate::views::queue::QueueColumnVisibility::default(),
        crate::views::queue::QueueColumnVisibility::restore_from(&live),
        "Queue column defaults drifted from ViewColumns::default()"
    );
    assert_eq!(
        crate::views::albums::AlbumsColumnVisibility::default(),
        crate::views::albums::AlbumsColumnVisibility::restore_from(&live),
        "Albums column defaults drifted from ViewColumns::default()"
    );
    assert_eq!(
        crate::views::artists::ArtistsColumnVisibility::default(),
        crate::views::artists::ArtistsColumnVisibility::restore_from(&live),
        "Artists column defaults drifted from ViewColumns::default()"
    );
    assert_eq!(
        crate::views::genres::GenresColumnVisibility::default(),
        crate::views::genres::GenresColumnVisibility::restore_from(&live),
        "Genres column defaults drifted from ViewColumns::default()"
    );
    assert_eq!(
        crate::views::playlists::PlaylistsColumnVisibility::default(),
        crate::views::playlists::PlaylistsColumnVisibility::restore_from(&live),
        "Playlists column defaults drifted from ViewColumns::default()"
    );
    assert_eq!(
        crate::views::songs::SongsColumnVisibility::default(),
        crate::views::songs::SongsColumnVisibility::restore_from(&live),
        "Songs column defaults drifted from ViewColumns::default()"
    );
    assert_eq!(
        crate::views::similar::SimilarColumnVisibility::default(),
        crate::views::similar::SimilarColumnVisibility::restore_from(&live),
        "Similar column defaults drifted from ViewColumns::default()"
    );
}

#[test]
fn queue_and_songs_dropdown_order_is_pinned() {
    // Characterization pin: `dropdown_entries()` order == macro declaration
    // order, and for Queue/Songs the user-visible dropdown has always rendered
    // Genre right after Album (the pre-`dropdown_entries` hand-written vec!s).
    // A declaration reorder in queue/mod.rs or songs/mod.rs shows up here.
    let queue_labels: Vec<&'static str> = crate::views::queue::QueueColumnVisibility::default()
        .dropdown_entries()
        .into_iter()
        .map(|(_, label, _)| label)
        .collect();
    assert_eq!(
        queue_labels,
        [
            "Select",
            "Index",
            "Thumbnail",
            "Stars",
            "Album",
            "Genre",
            "Duration",
            "Love",
            "Plays",
        ]
    );

    let songs_labels: Vec<&'static str> = crate::views::songs::SongsColumnVisibility::default()
        .dropdown_entries()
        .into_iter()
        .map(|(_, label, _)| label)
        .collect();
    assert_eq!(
        songs_labels,
        [
            "Select",
            "Index",
            "Thumbnail",
            "Stars",
            "Album",
            "Genre",
            "Duration",
            "Plays",
            "Love",
        ]
    );
}

// ============================================================================
// Restore-defaults sentinel routing (Tier 0 #0.2)
// ============================================================================
//
// The Hotkeys tab "Restore Defaults" row uses the key `__restore_all_hotkeys`,
// which is parsed by `SentinelKind::from_key` and routed into
// `handle_restore_defaults` via the typed dispatch. That function must early-return
// `OpenResetHotkeysDialog` for the all-hotkeys sentinel rather than falling
// through to the HexColor scan path (which is intended for color-group resets
// like `__restore_bg` / `__restore_accent`).

#[test]
fn handle_restore_defaults_all_hotkeys_opens_reset_dialog() {
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app
        .settings_page
        .handle_restore_defaults("__restore_all_hotkeys");

    assert!(
        matches!(action, SettingsAction::OpenResetHotkeysDialog),
        "__restore_all_hotkeys must route to OpenResetHotkeysDialog, got {action:?}"
    );
}

#[test]
fn handle_restore_defaults_visualizer_opens_reset_dialog() {
    // Non-regression: __restore_visualizer must still open its dialog.
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app
        .settings_page
        .handle_restore_defaults("__restore_visualizer");

    assert!(
        matches!(action, SettingsAction::OpenResetVisualizerDialog),
        "__restore_visualizer must route to OpenResetVisualizerDialog, got {action:?}"
    );
}

#[test]
fn handle_restore_defaults_theme_returns_restore_color_group() {
    // Non-regression: __restore_theme must still return RestoreColorGroup
    // (with empty entries — the side effect is on-disk restore via presets).
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app.settings_page.handle_restore_defaults("__restore_theme");

    assert!(
        matches!(action, SettingsAction::RestoreColorGroup { .. }),
        "__restore_theme must route to RestoreColorGroup, got {action:?}"
    );
}

#[test]
fn handle_restore_defaults_color_group_with_no_cached_entries_returns_none() {
    // Non-regression: when called with a generic __restore_* color key
    // (e.g. __restore_bg) and no HexColor entries cached, the function
    // returns SettingsAction::None — the HexColor scan path is preserved.
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app.settings_page.handle_restore_defaults("__restore_bg");

    assert!(
        matches!(action, SettingsAction::None),
        "__restore_bg with no cached HexColor entries must return None, got {action:?}"
    );
}

// ============================================================================
// Sentinel-key dispatch characterization
// ============================================================================
//
// These tests pin the `__*` sentinel-key dispatch surface so changes to
// SentinelKind cannot silently change observable routing. They cover:
//
//   * `SentinelKind::from_key` parsing for each registered sentinel,
//   * `__preset_N` numeric index extraction (drives `ApplyPreset`),
//   * `handle_restore_defaults` with a populated `cached_entries` for
//     `__restore_bg` — exercises the HexColor scan path that returns a
//     non-empty `RestoreColorGroup`,
//   * the explicit non-sentinel-ness of `__toggle_*` keys (regular
//     ToggleSet keys that must NOT parse into `SentinelKind`).
//
// The `SentinelKind` enum itself owns from_key/to_key round-trip coverage
// in `settings/sentinel.rs`; these tests are the dispatch-level anchors.

#[test]
fn sentinel_action_logout_parses_to_logout() {
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(
        SentinelKind::from_key("__action_logout"),
        Some(SentinelKind::Logout)
    );
}

#[test]
fn sentinel_preset_zero_extracts_index() {
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(
        SentinelKind::from_key("__preset_0"),
        Some(SentinelKind::PresetTheme(0))
    );
}

#[test]
fn sentinel_preset_seven_extracts_index() {
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(
        SentinelKind::from_key("__preset_7"),
        Some(SentinelKind::PresetTheme(7))
    );
}

#[test]
fn sentinel_toggle_keys_are_not_sentinels() {
    // Regular ToggleSet keys — must NOT parse into SentinelKind, since they
    // route through `ToggleSetToggle`, not the EditActivate sentinel dispatch.
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(SentinelKind::from_key("__toggle_artwork_overlays"), None);
    assert_eq!(SentinelKind::from_key("__toggle_strip_fields"), None);
}

#[test]
fn handle_restore_defaults_bg_with_cached_entries_returns_non_empty_group() {
    // When `cached_entries` contains HexColor items in the "Background
    // Colors" category, __restore_bg must collect them into
    // `RestoreColorGroup { entries }` with a non-empty Vec.
    use nokkvi_data::types::setting_item::{SettingItem, SettingMeta};

    use crate::views::settings::SettingsAction;

    let mut app = test_app();

    // Seed cached_entries with the __restore_bg row + a HexColor row in the
    // same "Background Colors" category. The category match drives the
    // scan inside `handle_restore_defaults`.
    let restore_meta = SettingMeta::new("__restore_bg", "⟲ Restore Defaults", "Background Colors");
    let color_meta = SettingMeta::new(
        "dark.background.hard",
        "BG hard (dark)",
        "Background Colors",
    );
    app.settings_page.cached_entries = vec![
        SettingItem::text(restore_meta, "Press Enter", "Press Enter"),
        SettingItem::hex_color(color_meta, "#000000", "#1d2021"),
    ];

    let action = app.settings_page.handle_restore_defaults("__restore_bg");

    match action {
        SettingsAction::RestoreColorGroup { entries } => {
            assert_eq!(
                entries.len(),
                1,
                "__restore_bg must collect the one HexColor entry in its category"
            );
            assert_eq!(entries[0].0, "dark.background.hard");
            assert_eq!(entries[0].1, "#1d2021");
        }
        other => panic!("expected RestoreColorGroup, got {other:?}"),
    }
}

#[test]
fn player_settings_loaded_restores_full_active_playlist_context() {
    // Session restore must rebuild a COMPLETE banner context from persisted
    // settings — not a degraded `minimal` one. Regression guard: a restored
    // public playlist previously showed as "Private" with no duration/updated.
    let mut app = test_app();

    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        active_playlist_id: Some("pl_restore".to_string()),
        active_playlist_name: "Restored Mix".to_string(),
        active_playlist_comment: "comment".to_string(),
        active_playlist_duration: 4321.0,
        active_playlist_updated: "2026-05-27T20:19:59-06:00".to_string(),
        active_playlist_public: true,
        active_playlist_song_count: 42,
        ..Default::default()
    });

    let ctx = app
        .active_playlist_info
        .as_ref()
        .expect("restore must seed the active playlist context");
    assert_eq!(ctx.id, "pl_restore");
    assert_eq!(ctx.name, "Restored Mix");
    assert!(
        (ctx.duration_secs - 4321.0).abs() < f32::EPSILON,
        "duration restored"
    );
    assert_eq!(ctx.updated, "2026-05-27T20:19:59-06:00", "updated restored");
    assert!(ctx.public, "a public playlist must not restore as private");
    assert_eq!(ctx.song_count, 42, "song count restored");
}

#[test]
fn player_settings_loaded_without_active_playlist_clears_context() {
    let mut app = test_app();
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "stale".into(),
        "Stale".into(),
        String::new(),
    ));

    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        active_playlist_id: None,
        ..Default::default()
    });

    assert!(
        app.active_playlist_info.is_none(),
        "no persisted active playlist must clear the banner context"
    );
}
