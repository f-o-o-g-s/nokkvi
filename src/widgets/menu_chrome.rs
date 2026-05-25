//! Shared menu-panel chrome — the visual vocabulary used by every overlay
//! menu (hamburger, player-mode kebab, checkbox dropdown, context menu).
//!
//! Four sites previously open-coded the same recipe:
//!
//! - **`hamburger_menu`** and **`player_modes_menu`** paint directly with
//!   `renderer.fill_quad`, supplying an `iced::Border` + `iced::Shadow`.
//! - **`checkbox_dropdown`** and **`context_menu`** wrap their content in a
//!   styled `container` and rely on iced to draw the quad.
//!
//! The two shapes are byte-equivalent visually: `bg1()` fill, 1 px
//! `theme::border()` outline, `ui_radius_md()` corner, `MENU_SHADOW` halo.
//! Routing both shapes through this module's accessors makes it
//! impossible for one of the four sites to drift independently — a
//! future per-theme tweak to menu panels touches one body and lands
//! everywhere.

use iced::{Color, Theme, widget::container};

use crate::{theme, widgets::menu_constants::MENU_SHADOW};

/// Menu panel fill — the same `bg1()` swatch every overlay menu uses.
#[inline]
pub(crate) fn fill() -> Color {
    theme::bg1()
}

/// 1 px menu-panel border, themed via `theme::border()`.
#[inline]
pub(crate) fn border() -> iced::Border {
    iced::Border {
        width: 1.0,
        color: theme::border(),
        radius: theme::ui_radius_md(),
    }
}

/// `container::Style` for menu panels — drops straight into
/// `.style(menu_chrome::container_style)` call sites.
pub(crate) fn container_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(fill().into()),
        border: border(),
        shadow: MENU_SHADOW,
        ..Default::default()
    }
}
