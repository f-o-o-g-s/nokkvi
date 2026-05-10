//! Handler prologue extraction: `HasViewChrome` trait + `dispatch_view_chrome` free function.
//!
//! Every `handle_*()` function in `src/update/` began with 2–3 identical blocks:
//! 1. `SetOpenMenu` early-return — intercept before page state sees the message.
//! 2. `Roulette` early-return — forward to the root roulette spin.
//! 3. `play_view_sfx(nav_flag, expand_flag)` — trigger nav/expand sound effects.
//!
//! `dispatch_view_chrome` encodes that shared prologue once.  Each view message type
//! implements `HasViewChrome` to describe which of its variants fall into each category.

use iced::Task;

use crate::{
    app_message::{Message, OpenMenu, RouletteMessage},
    views::{
        AlbumsMessage, ArtistsMessage, GenresMessage, PlaylistsMessage, QueueMessage,
        RadiosMessage, SongsMessage,
    },
};

// ─── Trait ───────────────────────────────────────────────────────────────────

pub(crate) trait HasViewChrome {
    /// If the message is a `SetOpenMenu` variant, return `Some(inner)`.
    /// Returns `None` for every other variant.
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>>;

    /// Returns `true` when the message is the `Roulette` variant.
    fn is_roulette(&self) -> bool;

    /// Returns `true` when the message triggers a navigation sound effect
    /// (i.e. `SlotListNavigateUp` or `SlotListNavigateDown`).
    fn is_nav_action(&self) -> bool;

    /// Returns `true` when the message triggers an expand/collapse sound effect
    /// (i.e. `CollapseExpansion` or `ExpandCenter`).
    fn is_expand_action(&self) -> bool;
}

// ─── Free function ───────────────────────────────────────────────────────────

/// Run the shared handler prologue for a view message.
///
/// Returns `Some(task)` when the message was intercepted as a "chrome" action
/// (SetOpenMenu or Roulette); the caller should return that task immediately.
/// Returns `None` when the message is a normal page action — the caller
/// continues to the page's `update()`.  The SFX call (if applicable) is
/// executed before returning `None` so the caller does not need to call it.
pub(crate) fn dispatch_view_chrome<M: HasViewChrome>(
    handler: &mut crate::Nokkvi,
    msg: &M,
    view: crate::View,
) -> Option<Task<Message>> {
    if let Some(menu) = msg.extract_set_open_menu() {
        return Some(Task::done(Message::SetOpenMenu(menu)));
    }
    if msg.is_roulette() {
        return Some(Task::done(Message::Roulette(RouletteMessage::Start(view))));
    }
    handler.play_view_sfx(msg.is_nav_action(), msg.is_expand_action());
    None
}

// ─── Trait implementations ───────────────────────────────────────────────────

impl HasViewChrome for AlbumsMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    fn is_roulette(&self) -> bool {
        matches!(self, Self::Roulette)
    }

    fn is_nav_action(&self) -> bool {
        matches!(
            self,
            Self::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp
                    | crate::widgets::SlotListPageMessage::NavigateDown
            )
        )
    }

    fn is_expand_action(&self) -> bool {
        matches!(self, Self::CollapseExpansion | Self::ExpandCenter)
    }
}

impl HasViewChrome for ArtistsMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    fn is_roulette(&self) -> bool {
        matches!(self, Self::Roulette)
    }

    fn is_nav_action(&self) -> bool {
        matches!(
            self,
            Self::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp
                    | crate::widgets::SlotListPageMessage::NavigateDown
            )
        )
    }

    fn is_expand_action(&self) -> bool {
        matches!(self, Self::CollapseExpansion | Self::ExpandCenter)
    }
}

impl HasViewChrome for SongsMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    fn is_roulette(&self) -> bool {
        matches!(self, Self::Roulette)
    }

    fn is_nav_action(&self) -> bool {
        matches!(
            self,
            Self::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp
                    | crate::widgets::SlotListPageMessage::NavigateDown
            )
        )
    }

    fn is_expand_action(&self) -> bool {
        false
    }
}

impl HasViewChrome for GenresMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    fn is_roulette(&self) -> bool {
        matches!(self, Self::Roulette)
    }

    fn is_nav_action(&self) -> bool {
        matches!(
            self,
            Self::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp
                    | crate::widgets::SlotListPageMessage::NavigateDown
            )
        )
    }

    fn is_expand_action(&self) -> bool {
        matches!(self, Self::CollapseExpansion | Self::ExpandCenter)
    }
}

impl HasViewChrome for PlaylistsMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    fn is_roulette(&self) -> bool {
        matches!(self, Self::Roulette)
    }

    fn is_nav_action(&self) -> bool {
        matches!(
            self,
            Self::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp
                    | crate::widgets::SlotListPageMessage::NavigateDown
            )
        )
    }

    fn is_expand_action(&self) -> bool {
        matches!(self, Self::CollapseExpansion | Self::ExpandCenter)
    }
}

impl HasViewChrome for QueueMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    fn is_roulette(&self) -> bool {
        matches!(self, Self::Roulette)
    }

    fn is_nav_action(&self) -> bool {
        matches!(
            self,
            Self::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp
                    | crate::widgets::SlotListPageMessage::NavigateDown
            )
        )
    }

    fn is_expand_action(&self) -> bool {
        false
    }
}

impl HasViewChrome for RadiosMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    fn is_roulette(&self) -> bool {
        matches!(self, Self::Roulette)
    }

    fn is_nav_action(&self) -> bool {
        matches!(
            self,
            Self::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp
                    | crate::widgets::SlotListPageMessage::NavigateDown
            )
        )
    }

    fn is_expand_action(&self) -> bool {
        false
    }
}
