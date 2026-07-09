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
        AlbumsMessage, ArtistsMessage, GenresMessage, HarbourMessage, PlaylistsMessage,
        QueueMessage, RadiosMessage, SimilarMessage, SongsMessage,
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

/// Generate a `HasViewChrome` impl for a per-view message enum.
///
/// `extract_set_open_menu` and `is_nav_action` are byte-identical across all
/// eight view-message types (every enum carries `SetOpenMenu(Option<OpenMenu>)`
/// and `SlotList(SlotListPageMessage)`); the three flags encode the per-view
/// variation axes:
/// - `roulette`: `yes` ⇒ `Self::Roulette` starts a roulette spin; `no` for
///   enums without the variant (Similar — results are ephemeral, no roulette
///   over the result list).
/// - `expand`: `yes` ⇒ `Self::CollapseExpansion | Self::ExpandCenter` trigger
///   the expand SFX (the four expansion views); `no` for the rest.
/// - `drag`: `yes` ⇒ extract `Self::ArtworkColumnDrag` /
///   `Self::ArtworkColumnVerticalDrag`; `no` for views without an artwork
///   pane (Radios). The trait methods still exist on those views so
///   `dispatch_view_chrome` can stay generic.
///
/// A flag typo cannot drift silently: `yes` on an enum missing the variant is
/// a hard compile error.
macro_rules! impl_view_chrome {
    (@roulette yes, $self:ident) => {
        matches!($self, Self::Roulette)
    };
    (@roulette no, $self:ident) => {
        false
    };
    (@expand yes, $self:ident) => {
        matches!($self, Self::CollapseExpansion | Self::ExpandCenter)
    };
    // Harbour collapses/expands its own sections via `ToggleSection` (a click on a
    // header) and `ExpandCenter` (Shift+Enter), not the shared `CollapseExpansion`.
    (@expand harbour, $self:ident) => {
        matches!($self, Self::ToggleSection(_) | Self::ExpandCenter)
    };
    (@expand no, $self:ident) => {
        false
    };
    (@drag yes, $self:ident, $variant:ident) => {
        if let Self::$variant(ev) = $self { Some(ev) } else { None }
    };
    (@drag no, $self:ident, $variant:ident) => {
        None
    };
    ($ty:ty { roulette: $roulette:tt, expand: $expand:tt, drag: $drag:tt }) => {
        impl HasViewChrome for $ty {
            fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>> {
                if let Self::SetOpenMenu(next) = self {
                    Some(next.clone())
                } else {
                    None
                }
            }

            fn is_roulette(&self) -> bool {
                impl_view_chrome!(@roulette $roulette, self)
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
                impl_view_chrome!(@expand $expand, self)
            }

            fn extract_artwork_column_drag(&self) -> Option<&DragEvent> {
                impl_view_chrome!(@drag $drag, self, ArtworkColumnDrag)
            }

            fn extract_artwork_vertical_drag(&self) -> Option<&DragEvent> {
                impl_view_chrome!(@drag $drag, self, ArtworkColumnVerticalDrag)
            }
        }
    };
}

impl_view_chrome!(AlbumsMessage {
    roulette: yes,
    expand: yes,
    drag: yes
});
impl_view_chrome!(ArtistsMessage {
    roulette: yes,
    expand: yes,
    drag: yes
});
impl_view_chrome!(SongsMessage {
    roulette: yes,
    expand: no,
    drag: yes
});
impl_view_chrome!(GenresMessage {
    roulette: yes,
    expand: yes,
    drag: yes
});
impl_view_chrome!(PlaylistsMessage {
    roulette: yes,
    expand: yes,
    drag: yes
});
impl_view_chrome!(QueueMessage {
    roulette: yes,
    expand: no,
    drag: yes
});
// Radios has no artwork pane / drag handle, so both extractors are
// permanently `None`.
impl_view_chrome!(RadiosMessage {
    roulette: yes,
    expand: no,
    drag: no
});
// SimilarMessage has no `Roulette` variant (results are ephemeral, no
// roulette over the result list); the chrome arm is permanently false.
impl_view_chrome!(SimilarMessage {
    roulette: no,
    expand: no,
    drag: yes
});
// Harbour is a slot-list view with an artwork pane but no roulette (its rows
// are a curated home list, not a shuffled library) and no inline expansion
// (sections toggle via `ToggleSection`, not the expand SFX).
impl_view_chrome!(HarbourMessage {
    roulette: no,
    expand: harbour,
    drag: yes
});

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
    fn harbour_extracts_column_drag() {
        let msg = HarbourMessage::ArtworkColumnDrag(CHANGE);
        assert_eq!(msg.extract_artwork_column_drag(), Some(&CHANGE));
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn harbour_extracts_vertical_drag() {
        let msg = HarbourMessage::ArtworkColumnVerticalDrag(COMMIT);
        assert_eq!(msg.extract_artwork_vertical_drag(), Some(&COMMIT));
        assert_eq!(msg.extract_artwork_column_drag(), None);
    }

    #[test]
    fn harbour_unrelated_variant_returns_none() {
        let msg = HarbourMessage::NoOp;
        assert_eq!(msg.extract_artwork_column_drag(), None);
        assert_eq!(msg.extract_artwork_vertical_drag(), None);
    }

    #[test]
    fn harbour_is_roulette_permanently_false() {
        // HarbourMessage has no `Roulette` variant.
        assert!(!HarbourMessage::NoOp.is_roulette());
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

    // ── Variation-axis guards (roulette / expand / nav / set-open-menu) ──

    #[test]
    fn albums_roulette_variant_is_roulette() {
        assert!(AlbumsMessage::Roulette.is_roulette());
    }

    #[test]
    fn similar_is_roulette_permanently_false() {
        // SimilarMessage has no `Roulette` variant; the chrome arm is
        // permanently false.
        assert!(!SimilarMessage::NoOp.is_roulette());
    }

    #[test]
    fn expansion_views_flag_expand_actions() {
        assert!(AlbumsMessage::CollapseExpansion.is_expand_action());
        assert!(AlbumsMessage::ExpandCenter.is_expand_action());
        assert!(ArtistsMessage::CollapseExpansion.is_expand_action());
        assert!(GenresMessage::ExpandCenter.is_expand_action());
        assert!(PlaylistsMessage::CollapseExpansion.is_expand_action());
        // Harbour toggles sections via ToggleSection (click) + ExpandCenter
        // (Shift+Enter), so both must play the expand SFX — not the enter sound.
        use crate::views::harbour::{HarbourMessage, HarbourSectionId};
        assert!(HarbourMessage::ToggleSection(HarbourSectionId::RecentlyPlayed).is_expand_action());
        assert!(HarbourMessage::ExpandCenter.is_expand_action());
        // A plain slot-list activation (Enter on an item) is NOT an expand action.
        assert!(
            !HarbourMessage::SlotList(crate::widgets::SlotListPageMessage::ActivateCenter(false))
                .is_expand_action()
        );
    }

    #[test]
    fn non_expansion_views_never_flag_expand_actions() {
        assert!(!SongsMessage::Roulette.is_expand_action());
        assert!(!QueueMessage::Roulette.is_expand_action());
        assert!(!RadiosMessage::Roulette.is_expand_action());
        assert!(!SimilarMessage::NoOp.is_expand_action());
    }

    #[test]
    fn nav_action_tracks_slot_list_navigation() {
        use crate::widgets::SlotListPageMessage;
        assert!(AlbumsMessage::SlotList(SlotListPageMessage::NavigateUp).is_nav_action());
        assert!(AlbumsMessage::SlotList(SlotListPageMessage::NavigateDown).is_nav_action());
        assert!(!AlbumsMessage::Roulette.is_nav_action());
    }

    #[test]
    fn set_open_menu_extracts_inner_payload() {
        assert_eq!(
            AlbumsMessage::SetOpenMenu(None).extract_set_open_menu(),
            Some(None)
        );
        assert_eq!(AlbumsMessage::Roulette.extract_set_open_menu(), None);
    }
}
