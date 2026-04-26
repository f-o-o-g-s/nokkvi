//! TEA-style page views — each view has a Page struct, Message enum, and Action enum
//!
//! Views: Albums, Artists, Songs, Genres, Playlists, Queue, Settings, Login.
//! `ViewPage` trait provides uniform hotkey dispatch. Each view implements it explicitly.
//! `impl_expansion_update!` deduplicates common expansion match arms.

pub(crate) mod albums;
pub(crate) mod artists;
pub(crate) mod browsing_panel;
pub(crate) mod expansion;
pub(crate) mod genres;
pub(crate) mod login;
pub(crate) mod playlists;
pub(crate) mod queue;
pub(crate) mod radios;
pub(crate) mod settings;
pub(crate) mod similar;
pub(crate) mod songs;

// Re-export commonly used items
pub(crate) use albums::{AlbumsAction, AlbumsColumn, AlbumsMessage, AlbumsPage, AlbumsViewData};
pub(crate) use artists::{
    ArtistsAction, ArtistsColumn, ArtistsMessage, ArtistsPage, ArtistsViewData,
};
pub(crate) use browsing_panel::{BrowsingPanel, BrowsingPanelMessage, BrowsingView};
pub(crate) use genres::{GenresAction, GenresMessage, GenresPage, GenresViewData};
pub(crate) use login::{LoginAction, LoginMessage, LoginPage};
pub(crate) use playlists::{PlaylistsAction, PlaylistsMessage, PlaylistsPage, PlaylistsViewData};
pub(crate) use queue::{
    QueueAction, QueueColumn, QueueMessage, QueuePage, QueueSortMode, QueueViewData,
};
pub(crate) use radios::{RadiosAction, RadiosMessage, RadiosPage, RadiosViewData};
pub(crate) use settings::{SettingsAction, SettingsMessage, SettingsPage, SettingsViewData};
pub(crate) use similar::{SimilarAction, SimilarMessage, SimilarPage, SimilarViewData};
pub(crate) use songs::{SongsAction, SongsColumn, SongsMessage, SongsPage, SongsViewData};

use crate::{
    app_message::Message,
    widgets::{SlotListPageState, view_header::SortMode},
};

// ============================================================================
// ViewPage trait — uniform interface for slot-list-based views
// ============================================================================

/// Uniform interface for slot-list-based views.
///
/// Provides hotkey handlers with a common API so they can operate on
/// any view without per-view `match self.current_view` arms.
pub(crate) trait ViewPage {
    /// Access the common slot list page state (search, sort, scroll, focus).
    fn common(&self) -> &SlotListPageState;
    /// Mutable access to the common slot list page state.
    fn common_mut(&mut self) -> &mut SlotListPageState;

    /// Whether this view has an active expansion that should be collapsed on Escape.
    fn is_expanded(&self) -> bool {
        false
    }
    /// Collapse the current expansion. Returns a `Message` if the view needs to emit one.
    fn collapse_expansion_message(&self) -> Option<Message> {
        None
    }

    /// The search input widget ID for this view (for focus operations).
    fn search_input_id(&self) -> &'static str;

    /// The `SortMode` options for cycling (Left/Right arrow).
    /// Returns `None` if this view doesn't support `SortMode` cycling (e.g., Queue uses `QueueSortMode`).
    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        None
    }

    /// Build a "sort mode selected" message for this view.
    /// Returns `None` if this view doesn't support `SortMode` (e.g., Queue uses `QueueSortMode`).
    fn sort_mode_selected_message(&self, _mode: SortMode) -> Option<Message> {
        None
    }

    /// Build a "toggle sort order" message for this view.
    fn toggle_sort_order_message(&self) -> Message;

    /// Build an "add center to queue" message, if supported.
    fn add_to_queue_message(&self) -> Option<Message> {
        None
    }

    /// Build an "expand center" message, if supported (Shift+Enter).
    fn expand_center_message(&self) -> Option<Message> {
        None
    }

    /// The `Message` to reload this view's data.
    fn reload_message(&self) -> Option<Message> {
        None
    }
}

// ============================================================================
// CommonViewAction — shared action variants across all slot-list-based views
// ============================================================================

/// Actions common to all slot-list-based views (except Queue, which has custom sort logic).
///
/// Each view's `Action` enum can classify itself into one of these variants,
/// allowing `Nokkvi::handle_common_view_action` to dispatch them generically
/// instead of repeating the same match arms in every handler.
pub(crate) enum CommonViewAction {
    /// Search text changed — triggers a reload of the view's data.
    SearchChanged,
    /// Sort mode changed — persists the new sort mode and triggers a reload.
    SortModeChanged(SortMode),
    /// Sort order changed — persists the new sort order and triggers a reload.
    SortOrderChanged(bool),
    /// Center the view on the currently playing track.
    CenterOnPlaying,
    /// User manually requested a data refresh, bypassing the local cache.
    RefreshViewData,
    /// Navigate to a different view and apply an ID filter.
    /// Used by inline link clicks (e.g. artist name → Artists view).
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
    /// No action — the view's update produced no effect.
    None,
    /// The action is view-specific and not handled generically.
    ViewSpecific,
}

/// Trait for Action enums that can classify themselves as a `CommonViewAction`.
///
/// Implement this on each view's Action enum to enable generic handling of
/// SearchChanged, SortModeChanged, SortOrderChanged, and None.
pub(crate) trait HasCommonAction {
    fn as_common(&self) -> CommonViewAction;
}

// (impl_view_page! macro removed — each view now has an explicit ViewPage impl)

// ============================================================================
// impl_expansion_update! macro — deduplicates common expansion view update arms
// ============================================================================

/// Handle the 7 common match arms shared by all expansion-based view `update()` methods.
///
/// Returns `Ok((Task, Action))` if the message was handled by a common arm,
/// or `Err(message)` if the caller should handle it as a view-specific message.
///
/// Common arms handled:
/// - ExpandCenter → delegates to `expansion.handle_expand_center()`
/// - CollapseExpansion → delegates to `expansion.collapse()`
/// - ChildrenLoaded(id, children) → delegates to `expansion.set_children()`
/// - SortModeSelected(mode) → delegates to `expansion.handle_sort_mode_selected()`
/// - ToggleSortOrder → delegates to `expansion.handle_toggle_sort_order()`
/// - SearchQueryChanged(query) → delegates to `expansion.handle_search_query_changed()`
/// - SearchFocused(focused) → delegates to `common.handle_search_focused()`
///
/// # Usage
/// ```ignore
/// let (cmd, action) = match super::impl_expansion_update!(
///     self, message, albums, total_items,
///     id_fn: |a| &a.id,
///     expand_center: AlbumsMessage::ExpandCenter => AlbumsAction::ExpandAlbum,
///     collapse: AlbumsMessage::CollapseExpansion,
///     children_loaded: AlbumsMessage::TracksLoaded,
///     sort_selected: AlbumsMessage::SortModeSelected => AlbumsAction::SortModeChanged,
///     toggle_sort: AlbumsMessage::ToggleSortOrder => AlbumsAction::SortOrderChanged,
///     search_changed: AlbumsMessage::SearchQueryChanged => AlbumsAction::SearchChanged,
///     search_focused: AlbumsMessage::SearchFocused,
///     action_none: AlbumsAction::None,
/// ) {
///     Ok(result) => result,
///     Err(msg) => match msg {
///         // Handle view-specific arms here
///         _ => (Task::none(), AlbumsAction::None),
///     },
/// };
/// ```
macro_rules! impl_expansion_update {
    (
        $self:expr, $message:expr, $items:expr, $total_items:expr,
        id_fn: $id_fn:expr,
        expand_center: $expand_msg:path => $expand_action:expr,
        collapse: $collapse_msg:path,
        children_loaded: $children_msg:path,
        sort_selected: $sort_msg:path => $sort_action:expr,
        toggle_sort: $toggle_msg:path => $sort_order_action:expr,
        search_changed: $search_msg:path => $search_action:expr,
        search_focused: $focus_msg:path,
        action_none: $action_none:expr $(,)?
    ) => {{
        // Try to match common expansion arms.
        // Returns Ok((Task, Action)) if handled, Err(message) to pass back for view-specific handling.
        #[allow(unreachable_patterns)]
        match $message {
            $expand_msg => {
                Ok(match $self.expansion.handle_expand_center($items, $id_fn, &mut $self.common) {
                    Some(id) => (Task::none(), $expand_action(id)),
                    None => (Task::none(), $action_none),
                })
            }
            $collapse_msg => {
                $self.expansion.collapse($items, $id_fn, &mut $self.common);
                Ok((Task::none(), $action_none))
            }
            $children_msg(id, children) => {
                $self.expansion.set_children(id, children, $items, &mut $self.common);
                Ok((Task::none(), $action_none))
            }
            $sort_msg(sort_mode) => {
                Ok(match $self.expansion.handle_sort_mode_selected(sort_mode, &mut $self.common) {
                    Some(vt) => (Task::none(), $sort_action(vt)),
                    None => (Task::none(), $action_none),
                })
            }
            $toggle_msg => {
                Ok(match $self.expansion.handle_toggle_sort_order(&mut $self.common) {
                    Some(ascending) => (Task::none(), $sort_order_action(ascending)),
                    None => (Task::none(), $action_none),
                })
            }
            $search_msg(query) => {
                Ok(match $self.expansion.handle_search_query_changed(query, $total_items, &mut $self.common) {
                    Some(q) => (Task::none(), $search_action(q)),
                    None => (Task::none(), $action_none),
                })
            }
            $focus_msg(focused) => {
                $self.common.handle_search_focused(focused);
                Ok((Task::none(), $action_none))
            }
            other => Err(other)
        }
    }};
}

pub(crate) use impl_expansion_update;

// ============================================================================
// Search Input IDs - Unique identifiers for each view's search field
// ============================================================================
// These constants ensure each view has a unique search input ID, preventing
// focus from transferring between views when switching. When adding a new
// view with a search field, add a constant here following the pattern:
// "{view_name}_search_input"

/// Search input ID for Albums view
pub(crate) const ALBUMS_SEARCH_ID: &str = "albums_search_input";

/// Search input ID for Queue view
pub(crate) const QUEUE_SEARCH_ID: &str = "queue_search_input";

/// Search input ID for Artists view
pub(crate) const ARTISTS_SEARCH_ID: &str = "artists_search_input";

/// Search input ID for Songs view
pub(crate) const SONGS_SEARCH_ID: &str = "songs_search_input";

/// Search input ID for Genres view
pub(crate) const GENRES_SEARCH_ID: &str = "genres_search_input";

/// Search input ID for Playlists view
pub(crate) const PLAYLISTS_SEARCH_ID: &str = "playlists_search_input";

/// Search input ID for Similar view
pub(crate) const SIMILAR_SEARCH_ID: &str = "similar_search_input";

/// Search input ID for Radios view
pub(crate) const RADIOS_SEARCH_ID: &str = "radios_search_input";
