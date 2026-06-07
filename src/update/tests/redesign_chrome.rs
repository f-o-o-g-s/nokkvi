//! Chrome-math regression tests for the redesign branch's new
//! `TrackInfoDisplay` × `NavLayout` matrix.
//!
//! `chrome_height_with_header()` (`src/widgets/slot_list.rs`) is the single
//! source of truth driving the slot-count math in every page view. Six of
//! its fan-outs depend on the active strip mode and nav layout, and an
//! off-by-one in any of them silently drops a partial slot at the bottom
//! of the list. The matrix below pins every combination so future agents
//! adding a new mode or layout fail loudly instead of silently shifting
//! one row by one pixel.
//!
//! Tests acquire `crate::theme::THEME_MODE_LOCK` to serialize against
//! other tests that mutate `UI_MODE` atomics, and restore the original
//! atomics on exit.
//!
//! Also pins `Nokkvi::mini_player_artwork`: in `MiniPlayer` mode the
//! resolver returns the cached large-artwork handle; in every other
//! `TrackInfoDisplay` mode it short-circuits to `None` (so other strip
//! modes don't pay the per-frame queue walk).

use nokkvi_data::types::player_settings::{NavLayout, RoundedMode, TrackInfoDisplay};

use crate::{
    test_helpers::{make_queue_song, test_app},
    theme::{
        THEME_MODE_LOCK, is_rounded_for_player, is_rounded_mode, nav_layout, rounded_mode,
        set_nav_layout, set_rounded_mode, set_track_info_display, track_info_display,
    },
    widgets::{
        player_bar::player_bar_height,
        slot_list::{chrome_height_with_header, view_header_chrome},
        track_info_strip::STRIP_HEIGHT_WITH_SEPARATOR,
    },
};

/// Snapshot relevant UI_MODE atomics so a test that mutates them leaves
/// the global state exactly as it found it — neighboring tests in the
/// same binary read the baseline.
struct UiModeGuard {
    saved_tid: TrackInfoDisplay,
    saved_nav: NavLayout,
    saved_rounded: RoundedMode,
}

impl UiModeGuard {
    fn snapshot() -> Self {
        Self {
            saved_tid: track_info_display(),
            saved_nav: nav_layout(),
            saved_rounded: rounded_mode(),
        }
    }
}

impl Drop for UiModeGuard {
    fn drop(&mut self) {
        set_track_info_display(self.saved_tid);
        set_nav_layout(self.saved_nav);
        set_rounded_mode(self.saved_rounded);
    }
}

fn expected_chrome(tid: TrackInfoDisplay, layout: NavLayout) -> f32 {
    let nav = match layout {
        NavLayout::Top => crate::theme::nav_bar_height(),
        NavLayout::Side | NavLayout::None => 0.0,
    };
    let player = if tid == TrackInfoDisplay::PlayerBar {
        // Base + 1 px top separator + strip-with-its-own-separator.
        72.0 + 1.0 + STRIP_HEIGHT_WITH_SEPARATOR
    } else if tid == TrackInfoDisplay::MiniPlayer {
        // MiniPlayer is the capsule layout: 1px separator + capsule scrub (20) +
        // 1px separator + artwork content row (56) = 78 — taller than the base.
        78.0
    } else {
        72.0
    };
    let strip = match (tid, layout) {
        (TrackInfoDisplay::TopBarUnder, NavLayout::Top) => STRIP_HEIGHT_WITH_SEPARATOR,
        (
            TrackInfoDisplay::TopBar | TrackInfoDisplay::TopBarUnder,
            NavLayout::Side | NavLayout::None,
        ) => STRIP_HEIGHT_WITH_SEPARATOR,
        _ => 0.0,
    };
    nav + player + view_header_chrome() + strip
}

#[test]
fn chrome_matrix_flat_mode_pins_every_combination() {
    let _guard = THEME_MODE_LOCK.lock();
    let _restore = UiModeGuard::snapshot();
    set_rounded_mode(RoundedMode::Off);

    let layouts = [NavLayout::Top, NavLayout::Side, NavLayout::None];
    let modes = [
        TrackInfoDisplay::Off,
        TrackInfoDisplay::PlayerBar,
        TrackInfoDisplay::TopBar,
        TrackInfoDisplay::TopBarUnder,
        TrackInfoDisplay::MiniPlayer,
    ];

    for layout in layouts {
        for mode in modes {
            set_nav_layout(layout);
            set_track_info_display(mode);
            let got = chrome_height_with_header();
            let expected = expected_chrome(mode, layout);
            assert!(
                (got - expected).abs() < f32::EPSILON,
                "chrome drifted at ({mode:?}, {layout:?}): got {got}, expected {expected}",
            );
        }
    }
}

#[test]
fn chrome_matrix_rounded_mode_swaps_nav_height_only() {
    // Rounded mode adds 12 px of padding around the nav-bar pill — every
    // other component is mode-invariant. Pinning the delta here catches
    // a future agent extending nav_bar_height with extra mode-conditional
    // chrome without updating the slot-count math.
    let _guard = THEME_MODE_LOCK.lock();
    let _restore = UiModeGuard::snapshot();

    set_nav_layout(NavLayout::Top);
    set_track_info_display(TrackInfoDisplay::Off);

    set_rounded_mode(RoundedMode::On);
    let rounded = chrome_height_with_header();

    set_rounded_mode(RoundedMode::Off);
    let flat = chrome_height_with_header();

    assert!(
        (rounded - flat - 12.0).abs() < f32::EPSILON,
        "rounded-vs-flat chrome delta drifted: rounded={rounded}, flat={flat}",
    );

    // PlayerOnly mode rounds the bottom playback chrome but leaves the
    // nav bar (and the rest of the UI) flat. Chrome height must match
    // the fully-flat baseline, not the fully-rounded one.
    set_rounded_mode(RoundedMode::PlayerOnly);
    let player_only = chrome_height_with_header();
    assert!(
        (player_only - flat).abs() < f32::EPSILON,
        "PlayerOnly must leave nav-bar height flat: got {player_only}, expected {flat}",
    );
}

#[test]
fn rounded_predicates_split_global_from_player_scope() {
    // Pin the contract that distinguishes the three rounded modes: `On`
    // rounds everything, `PlayerOnly` rounds only the bottom playback
    // chrome, `Off` rounds nothing. Future agents adding a new mode here
    // fail loudly instead of silently rewiring one predicate.
    let _guard = THEME_MODE_LOCK.lock();
    let _restore = UiModeGuard::snapshot();

    set_rounded_mode(RoundedMode::Off);
    assert!(!is_rounded_mode());
    assert!(!is_rounded_for_player());

    set_rounded_mode(RoundedMode::On);
    assert!(is_rounded_mode());
    assert!(is_rounded_for_player());

    set_rounded_mode(RoundedMode::PlayerOnly);
    assert!(
        !is_rounded_mode(),
        "global rounded must be false in PlayerOnly"
    );
    assert!(
        is_rounded_for_player(),
        "player-scope rounded must be true in PlayerOnly",
    );
}

#[test]
fn top_bar_under_strip_only_adds_height_in_top_nav() {
    // TopBarUnder is a Top-nav-only feature: in Side/None layouts it
    // falls through to the same above-content position as TopBar (so the
    // chrome height stays equal to TopBar's). In Top-nav layout it adds
    // its own row beneath the nav bar.
    let _guard = THEME_MODE_LOCK.lock();
    let _restore = UiModeGuard::snapshot();
    set_rounded_mode(RoundedMode::Off);

    set_nav_layout(NavLayout::Top);
    set_track_info_display(TrackInfoDisplay::Off);
    let top_off = chrome_height_with_header();
    set_track_info_display(TrackInfoDisplay::TopBarUnder);
    let top_under = chrome_height_with_header();
    assert!(
        (top_under - top_off - STRIP_HEIGHT_WITH_SEPARATOR).abs() < f32::EPSILON,
        "TopBarUnder in top-nav must add STRIP_HEIGHT_WITH_SEPARATOR; got delta {}",
        top_under - top_off,
    );

    set_nav_layout(NavLayout::Side);
    set_track_info_display(TrackInfoDisplay::TopBar);
    let side_top_bar = chrome_height_with_header();
    set_track_info_display(TrackInfoDisplay::TopBarUnder);
    let side_top_under = chrome_height_with_header();
    assert!(
        (side_top_bar - side_top_under).abs() < f32::EPSILON,
        "TopBar and TopBarUnder must have identical chrome in side-nav (both render above content)",
    );
}

#[test]
fn player_bar_strip_adds_top_separator_plus_strip_with_separator() {
    // Pins the fix from `fix(player-bar): use STRIP_HEIGHT_WITH_SEPARATOR
    // + drop -1px fudge` — switching from `Off` to `PlayerBar` must grow
    // player_bar_height() by 1 (top separator) + STRIP_HEIGHT_WITH_SEPARATOR.
    let _guard = THEME_MODE_LOCK.lock();
    let _restore = UiModeGuard::snapshot();
    set_track_info_display(TrackInfoDisplay::Off);
    let off = player_bar_height();
    set_track_info_display(TrackInfoDisplay::PlayerBar);
    let on = player_bar_height();
    let expected = 1.0 + STRIP_HEIGHT_WITH_SEPARATOR;
    assert!(
        (on - off - expected).abs() < f32::EPSILON,
        "PlayerBar strip delta drifted: got {} expected {expected}",
        on - off,
    );
}

// ---------------------------------------------------------------------------
// MiniPlayer artwork resolver — pins that `Nokkvi::mini_player_artwork`
// is gated on `TrackInfoDisplay::MiniPlayer` and short-circuits to None
// in every other mode.

#[test]
fn mini_player_artwork_only_when_mini_player_mode_active() {
    let _guard = THEME_MODE_LOCK.lock();
    let _restore = UiModeGuard::snapshot();

    let mut app = test_app();
    let song_id = "s1";
    let song = make_queue_song(song_id, "T", "A", "Alb");
    let album_id = song.album_id.clone();
    app.scrobble.current_song_id = Some(song_id.to_string());
    app.library.queue_songs.push(song);
    let handle = iced::widget::image::Handle::from_bytes(vec![0u8; 64]);
    app.artwork.large_artwork.put(album_id, handle);

    // Mini-player mode: resolver returns the cached handle.
    set_track_info_display(TrackInfoDisplay::MiniPlayer);
    assert!(
        app.mini_player_artwork().is_some(),
        "MiniPlayer mode must surface the cached large artwork",
    );

    // Every other strip mode short-circuits before walking the queue.
    for mode in [
        TrackInfoDisplay::Off,
        TrackInfoDisplay::PlayerBar,
        TrackInfoDisplay::TopBar,
        TrackInfoDisplay::TopBarUnder,
    ] {
        set_track_info_display(mode);
        assert!(
            app.mini_player_artwork().is_none(),
            "{mode:?}: mini-player artwork must be gated on TrackInfoDisplay::MiniPlayer",
        );
    }
}

#[test]
fn mini_player_artwork_none_without_current_song() {
    let _guard = THEME_MODE_LOCK.lock();
    let _restore = UiModeGuard::snapshot();
    set_track_info_display(TrackInfoDisplay::MiniPlayer);

    let app = test_app();
    assert!(
        app.scrobble.current_song_id.is_none(),
        "test_app baseline has no current song",
    );
    assert!(
        app.mini_player_artwork().is_none(),
        "no current song => resolver returns None even in MiniPlayer mode",
    );
}
