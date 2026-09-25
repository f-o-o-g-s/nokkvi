//! Theater Mode state: the now-playing layout that hides the slot list, the
//! toolbar and the nav, leaving the cover, the over-cover visualizer and the
//! lyrics on screen. Transient by design: never persisted, never entered on
//! the Login screen. Entry and exit go through `Nokkvi::enter_theater` /
//! `Nokkvi::exit_theater` (`update/theater.rs`).

use std::time::Instant;

/// Theater Mode's root-owned state (`Nokkvi.theater`). Manual `Default`
/// because of the `Instant`s and because `window_focused` starts `true`
/// (a `false` start would keep the bar hidden until the first focus event).
#[derive(Debug)]
pub struct TheaterState {
    /// The theater layout replaces the home layout while this is set.
    pub active: bool,
    /// Last mouse move / wheel / press / key press while active. `None` after
    /// the window loses focus, so the bar returns on the next real activity
    /// rather than on refocus alone.
    pub last_activity: Option<Instant>,
    /// The cursor is on the transient bar (its `mouse_area` enter / exit). Cleared on unfocus and exit, whose `on_exit` may never fire.
    pub bar_hovered: bool,
    /// The cursor is on the corner exit icon riding above the bar.
    pub corner_hovered: bool,
    /// OS window focus, mirrored from `WindowFocused` / `WindowUnfocused`.
    pub window_focused: bool,
    /// Where the chrome is going, since when, and from what offset it left.
    pub chrome: ChromeMotion,
    /// The window's mode before Theater Fills the Screen went fullscreen;
    /// restored (and consumed) on exit.
    pub prior_window_mode: Option<iced::window::Mode>,
    /// The mode the last exit asked the window to return to, and when. On
    /// Wayland the mode query answers from the compositor's last reply, so a
    /// re-entry right after an exit can read the fullscreen being undone.
    pub restore_sent: Option<(iced::window::Mode, Instant)>,
}

impl Default for TheaterState {
    fn default() -> Self {
        Self {
            active: false,
            last_activity: None,
            bar_hovered: false,
            corner_hovered: false,
            window_focused: true,
            chrome: ChromeMotion::Shown {
                since: Instant::now(),
                from: 0.0,
            },
            prior_window_mode: None,
            restore_sent: None,
        }
    }
}

/// The chrome's slide. `from` is the slide offset (0 = fully shown, 1 =
/// fully hidden) at the moment of the flip, so a reversal mid-slide eases
/// from where the bar is rather than from an endpoint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChromeMotion {
    Shown { since: Instant, from: f32 },
    Hidden { since: Instant, from: f32 },
}

impl ChromeMotion {
    /// Whether the chrome is headed on screen.
    pub fn is_shown(self) -> bool {
        matches!(self, Self::Shown { .. })
    }
}
