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
        RadiosMessage, SimilarMessage, SongsMessage,
    },
    widgets::artwork_split_handle::DragEvent,
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

    /// If the message is the per-view artwork-column drag variant, return
    /// `Some(&ev)`. Returns `None` otherwise (including for views that do not
    /// carry the variant, e.g. Radios).
    fn extract_artwork_column_drag(&self) -> Option<&DragEvent>;

    /// If the message is the per-view always-vertical artwork drag variant,
    /// return `Some(&ev)`. Returns `None` otherwise.
    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent>;
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
    // Artwork split-handle drags must dispatch synchronously — they fire on
    // every cursor frame during a drag, and routing through `Task::done` would
    // add a frame of round-trip latency that's noticeable in the live preview.
    if let Some(ev) = msg.extract_artwork_column_drag() {
        return Some(handler.handle_artwork_column_drag(*ev));
    }
    if let Some(ev) = msg.extract_artwork_vertical_drag() {
        return Some(handler.handle_artwork_vertical_drag(*ev));
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

    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnVerticalDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
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

    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnVerticalDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
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

    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnVerticalDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
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

    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnVerticalDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
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

    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnVerticalDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
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

    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnVerticalDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
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

    // Radios has no artwork pane / drag handle, so both extractors are
    // permanently `None`. The trait method exists for uniformity with the
    // other views so `dispatch_view_chrome` can stay generic.
    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        None
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        None
    }
}

impl HasViewChrome for SimilarMessage {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
        if let Self::SetOpenMenu(next) = self {
            Some(next.clone())
        } else {
            None
        }
    }

    // SimilarMessage has no `Roulette` variant (results are ephemeral, no
    // roulette over the result list); the chrome arm is permanently false.
    fn is_roulette(&self) -> bool {
        false
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

    fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }

    fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
        if let Self::ArtworkColumnVerticalDrag(ev) = self {
            Some(ev)
        } else {
            None
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Characterization tests for the artwork-drag extractor methods on
    //! every per-view `HasViewChrome` impl. Each extractor must return
    //! `Some(&ev)` for its corresponding drag variant and `None` for an
    //! unrelated variant. Radios has no drag variants, so both extractors
    //! permanently return `None`.
    use super::*;

    const CHANGE: DragEvent = DragEvent::Change(0.42);
    const COMMIT: DragEvent = DragEvent::Commit(0.73);

    #[test]
    fn albums_extracts_column_drag() {
        let msg = AlbumsMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn albums_extracts_vertical_drag() {
        let msg = AlbumsMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn albums_unrelated_variant_returns_none() {
        let msg = AlbumsMessage::Roulette;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn artists_extracts_column_drag() {
        let msg = ArtistsMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn artists_extracts_vertical_drag() {
        let msg = ArtistsMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn artists_unrelated_variant_returns_none() {
        let msg = ArtistsMessage::Roulette;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn songs_extracts_column_drag() {
        let msg = SongsMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn songs_extracts_vertical_drag() {
        let msg = SongsMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn songs_unrelated_variant_returns_none() {
        let msg = SongsMessage::Roulette;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn genres_extracts_column_drag() {
        let msg = GenresMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn genres_extracts_vertical_drag() {
        let msg = GenresMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn genres_unrelated_variant_returns_none() {
        let msg = GenresMessage::Roulette;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn playlists_extracts_column_drag() {
        let msg = PlaylistsMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn playlists_extracts_vertical_drag() {
        let msg = PlaylistsMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn playlists_unrelated_variant_returns_none() {
        let msg = PlaylistsMessage::Roulette;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn queue_extracts_column_drag() {
        let msg = QueueMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn queue_extracts_vertical_drag() {
        let msg = QueueMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn queue_unrelated_variant_returns_none() {
        let msg = QueueMessage::Roulette;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn similar_extracts_column_drag() {
        let msg = SimilarMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn similar_extracts_vertical_drag() {
        let msg = SimilarMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn similar_unrelated_variant_returns_none() {
        let msg = SimilarMessage::NoOp;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn radios_has_no_drag_variants() {
        // Radios is the one slot-list view without an artwork pane; both
        // extractors are permanently `None` regardless of variant. Use a
        // benign variant (Roulette) to assert the contract.
        let msg = RadiosMessage::Roulette;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }
}
