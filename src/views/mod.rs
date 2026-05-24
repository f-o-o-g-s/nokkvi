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
pub(crate) mod sort_api;

// Re-export commonly used items
pub(crate) use albums::{AlbumsAction, AlbumsMessage, AlbumsPage, AlbumsViewData};
pub(crate) use artists::{ArtistsAction, ArtistsMessage, ArtistsPage, ArtistsViewData};
pub(crate) use browsing_panel::{BrowsingPanel, BrowsingPanelMessage, BrowsingView};
pub(crate) use genres::{GenresAction, GenresMessage, GenresPage, GenresViewData};
pub(crate) use login::{LoginAction, LoginMessage, LoginPage};
pub(crate) use playlists::{PlaylistsAction, PlaylistsMessage, PlaylistsPage, PlaylistsViewData};
pub(crate) use queue::{QueueAction, QueueMessage, QueuePage, QueueSortMode, QueueViewData};
pub(crate) use radios::{RadiosAction, RadiosMessage, RadiosPage, RadiosViewData};
pub(crate) use settings::{SettingsAction, SettingsMessage, SettingsPage, SettingsViewData};
pub(crate) use similar::{SimilarAction, SimilarMessage, SimilarPage, SimilarViewData};
pub(crate) use songs::{SongsAction, SongsMessage, SongsPage, SongsViewData};

use crate::{
    app_message::Message,
    widgets::{SlotListPageMessage, SlotListPageState, view_header::SortMode},
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

    /// Build a synthetic `SlotListSetOffset` message for this view at the given offset.
    ///
    /// Used by `handle_seek_settled` to trigger artwork prefetch after scrollbar
    /// seek settles — routes through the normal per-view SetOffset handler which
    /// drives the `LoadLargeArtwork` / `prefetch_album_artwork_tasks` path.
    ///
    /// Returns `None` for views that don't participate in the seek-settle
    /// artwork pipeline (e.g. Queue, Settings).
    fn synth_set_offset_message(&self, _offset: usize) -> Option<Message> {
        None
    }

    /// Wrap a [`SlotListPageMessage`] in this view's outer [`Message`] variant.
    ///
    /// The slot-list dispatch family (`handle_slot_list_navigate_up` /
    /// `_navigate_down` / `_set_offset` / `_activate_center` in `slot_list.rs`,
    /// `roulette_settle_play` in `roulette.rs`, and `handle_center_on_playing`
    /// in `hotkeys/navigation.rs`) routes the same `SlotListPageMessage` to
    /// each view via `view_page(view).map(|p| p.slot_list_message(msg))`. The
    /// no-default declaration makes "added a new ViewPage impl, forgot the
    /// SlotList wrapper" a compile error.
    ///
    /// Settings is not a slot-list view and does not implement `ViewPage`; its
    /// per-handler special case wraps directly in `SettingsMessage::SlotList*`
    /// variants instead.
    fn slot_list_message(&self, msg: SlotListPageMessage) -> Message;

    /// Whether this view renders artwork via `base_slot_list_layout`'s
    /// horizontal layout (i.e. passes `show_artwork_column: true` and
    /// resolves to `ArtworkOrientation::Horizontal`).
    ///
    /// `Nokkvi::elevated_artwork_extent` consults this to gate the
    /// artwork-elevation feature without a hand-maintained match on
    /// `View`. Default `false` means a new `ViewPage` impl opts out
    /// safely; override to `true` only when the view does in fact
    /// participate in the horizontal-artwork layout.
    fn uses_horizontal_artwork_column(&self) -> bool {
        false
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
    /// Navigate to Albums and auto-expand the album with this id, with no
    /// filter set. Dispatched by album-text clicks in Songs/Queue.
    NavigateAndExpandAlbum(String),
    /// Navigate to Artists and auto-expand the artist with this id, with
    /// no filter set. Dispatched by artist-text clicks in any view.
    NavigateAndExpandArtist(String),
    /// Navigate to Genres and auto-expand the genre with this id, with no
    /// filter set. Dispatched by genre-text clicks in Songs/Albums/Queue
    /// dynamic columns when the user has chosen Genre sort.
    NavigateAndExpandGenre(String),
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
/// - `SlotList(SlotListPageMessage::HoverEnterSlot)` / `HoverExitSlot` →
///   writes / idempotently clears `common.slot_list.hovered_slot`
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
///     slot_list_wrap: AlbumsMessage::SlotList,
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
        slot_list_wrap: $slot_list_wrap:path,
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
            $slot_list_wrap(crate::widgets::SlotListPageMessage::HoverEnterSlot(h)) => {
                $self.common.slot_list.hovered_slot = Some(h);
                Ok((Task::none(), $action_none))
            }
            $slot_list_wrap(crate::widgets::SlotListPageMessage::HoverExitSlot(h)) => {
                if $self.common.slot_list.hovered_slot == Some(h) {
                    $self.common.slot_list.hovered_slot = None;
                }
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

// ============================================================================
// define_view_columns! macro — generates a paired `{Name}Column` enum and
// `{Name}ColumnVisibility` struct (with `Default`, `get`, and `set`) from a
// single declaration. Eliminates the 4-site drift surface where each view
// duplicates the variant list across enum / struct / Default / get-match /
// set-match.
// ============================================================================

/// Generate a paired `{Name}Column` enum and `{Name}ColumnVisibility` struct
/// from a single declaration.
///
/// Each entry has the form `Variant: field_name = default_value`. The macro
/// emits the enum variant, the bool struct field, the `Default` impl entry,
/// and the `get` / `set` match arms in lockstep, so adding or renaming a
/// column is a one-site edit.
///
/// # Usage
/// ```ignore
/// define_view_columns! {
///     GenresColumn => GenresColumnVisibility {
///         Select: select = false,
///         Index: index = true,
///         Thumbnail: thumbnail = true,
///         AlbumCount: albumcount = true,
///         SongCount: songcount = true,
///     }
/// }
/// ```
#[allow(unused_macros)]
macro_rules! define_view_columns {
    // WITH setter annotations — production views that persist column visibility.
    // `=> $setter` maps each variant to its `SettingsManager` method name and
    // emits `impl ColumnPersist` so `Nokkvi::persist_column_visibility` can
    // dispatch without per-view boilerplate.
    (
        $col_enum:ident => $vis_struct:ident {
            $( $variant:ident : $field:ident = $default:expr => $setter:ident ),* $(,)?
        }
    ) => {
        $crate::views::define_view_columns!(
            $col_enum => $vis_struct { $( $variant : $field = $default ),* }
        );

        impl nokkvi_data::services::settings::ColumnPersist for $col_enum {
            fn apply_to_settings(
                self,
                sm: &mut nokkvi_data::services::settings::SettingsManager,
                value: bool,
            ) -> anyhow::Result<()> {
                match self {
                    $( Self::$variant => sm.$setter(value) ),*
                }
            }
        }
    };
    // WITHOUT setter annotations — test-only or non-persistent column enums.
    // Emits only the column enum, visibility struct, Default, get, and set.
    (
        $col_enum:ident => $vis_struct:ident {
            $( $variant:ident : $field:ident = $default:expr ),* $(,)?
        }
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $col_enum {
            $( $variant ),*
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $vis_struct {
            $( pub $field: bool ),*
        }

        impl Default for $vis_struct {
            fn default() -> Self {
                Self { $( $field: $default ),* }
            }
        }

        impl $vis_struct {
            pub fn get(&self, col: $col_enum) -> bool {
                match col {
                    $( $col_enum::$variant => self.$field ),*
                }
            }

            pub fn set(&mut self, col: $col_enum, value: bool) {
                match col {
                    $( $col_enum::$variant => self.$field = value ),*
                }
            }

            /// Flip a column's visibility and return the new value.
            ///
            /// Collapses the read-modify-write pattern that the seven
            /// `{View}Message::ToggleColumnVisible(col)` handlers
            /// (Albums/Artists/Genres/Playlists/Queue/Songs/Similar) used
            /// to spell out by hand, so they each become a single call
            /// that yields the bool needed for `ColumnVisibilityChanged`.
            pub fn toggle(&mut self, col: $col_enum) -> bool {
                let new_value = !self.get(col);
                self.set(col, new_value);
                new_value
            }
        }
    };
}

#[allow(unused_imports)]
pub(crate) use define_view_columns;

// ============================================================================
// impl_has_common_action! macro — implements `HasCommonAction` for a view's
// Action enum. Bakes in the always-present arms (SearchChanged,
// SortModeChanged, SortOrderChanged, RefreshViewData, NavigateAndFilter, None,
// catch-all `_ => ViewSpecific`) and accepts a declarative list of
// `NavigateAndExpand*` variants the enum carries. Pass `, no_center` to skip
// the `CenterOnPlaying` arm for views (e.g. Playlists) without that variant.
// ============================================================================

/// Implement `HasCommonAction` for a view's Action enum.
///
/// The always-present arms (SearchChanged, SortModeChanged, SortOrderChanged,
/// RefreshViewData, NavigateAndFilter, None, plus a catch-all
/// `_ => ViewSpecific`) are emitted unconditionally. `CenterOnPlaying` is
/// emitted by default; pass `, no_center` to skip it. Pass `, no_navigate_filter`
/// to skip the `NavigateAndFilter` arm and the variadic `NavigateAndExpand*`
/// list (for views like Radios that have neither). The variadic list adds one
/// match arm per `NavigateAndExpand*` variant the action carries — each such
/// variant must follow the `Variant(String)` shape and have a matching variant
/// on `CommonViewAction`.
///
/// # Usage
/// ```ignore
/// impl_has_common_action!(GenresAction { NavigateAndExpandArtist, NavigateAndExpandAlbum });
/// impl_has_common_action!(PlaylistsAction, no_center { NavigateAndExpandArtist });
/// impl_has_common_action!(RadiosAction, no_navigate_filter);
/// ```
#[allow(unused_macros)]
macro_rules! impl_has_common_action {
    ($action:ident { $( $variant:ident ),* $(,)? }) => {
        impl $crate::views::HasCommonAction for $action {
            fn as_common(&self) -> $crate::views::CommonViewAction {
                match self {
                    Self::SearchChanged(_) => $crate::views::CommonViewAction::SearchChanged,
                    Self::SortModeChanged(m) => $crate::views::CommonViewAction::SortModeChanged(*m),
                    Self::SortOrderChanged(a) => $crate::views::CommonViewAction::SortOrderChanged(*a),
                    Self::RefreshViewData => $crate::views::CommonViewAction::RefreshViewData,
                    Self::CenterOnPlaying => $crate::views::CommonViewAction::CenterOnPlaying,
                    Self::NavigateAndFilter(v, f) => {
                        $crate::views::CommonViewAction::NavigateAndFilter(*v, f.clone())
                    }
                    $( Self::$variant(id) => $crate::views::CommonViewAction::$variant(id.clone()), )*
                    Self::None => $crate::views::CommonViewAction::None,
                    _ => $crate::views::CommonViewAction::ViewSpecific,
                }
            }
        }
    };
    ($action:ident, no_center { $( $variant:ident ),* $(,)? }) => {
        impl $crate::views::HasCommonAction for $action {
            fn as_common(&self) -> $crate::views::CommonViewAction {
                match self {
                    Self::SearchChanged(_) => $crate::views::CommonViewAction::SearchChanged,
                    Self::SortModeChanged(m) => $crate::views::CommonViewAction::SortModeChanged(*m),
                    Self::SortOrderChanged(a) => $crate::views::CommonViewAction::SortOrderChanged(*a),
                    Self::RefreshViewData => $crate::views::CommonViewAction::RefreshViewData,
                    Self::NavigateAndFilter(v, f) => {
                        $crate::views::CommonViewAction::NavigateAndFilter(*v, f.clone())
                    }
                    $( Self::$variant(id) => $crate::views::CommonViewAction::$variant(id.clone()), )*
                    Self::None => $crate::views::CommonViewAction::None,
                    _ => $crate::views::CommonViewAction::ViewSpecific,
                }
            }
        }
    };
    ($action:ident, no_navigate_filter) => {
        impl $crate::views::HasCommonAction for $action {
            fn as_common(&self) -> $crate::views::CommonViewAction {
                match self {
                    Self::SearchChanged(_) => $crate::views::CommonViewAction::SearchChanged,
                    Self::SortModeChanged(m) => $crate::views::CommonViewAction::SortModeChanged(*m),
                    Self::SortOrderChanged(a) => $crate::views::CommonViewAction::SortOrderChanged(*a),
                    Self::RefreshViewData => $crate::views::CommonViewAction::RefreshViewData,
                    Self::CenterOnPlaying => $crate::views::CommonViewAction::CenterOnPlaying,
                    Self::None => $crate::views::CommonViewAction::None,
                    _ => $crate::views::CommonViewAction::ViewSpecific,
                }
            }
        }
    };
}

#[allow(unused_imports)]
pub(crate) use impl_has_common_action;

// ============================================================================
// Shared ViewData fragments — small reusable bundles of fields that recur
// across multiple `*ViewData<'a>` structs.
// ============================================================================

/// The three overlay-menu fields every library-view `*ViewData` carries:
/// the column-visibility checkbox dropdown's open flag + trigger bounds, and
/// the borrowed reference to the root `open_menu` state used by per-row and
/// artwork-panel context menus to resolve their own open/closed status.
///
/// Embedding this as `pub overlay: OverlayMenuViewData<'a>` in each
/// `*ViewData` keeps the per-view view-data structs honest about which fields
/// are part of the shared overlay-menu plumbing vs. view-specific data, and
/// lets construction sites in `app_view.rs` fold all three assignments into
/// one nested-struct literal.
///
/// Visibility matches the surrounding `*ViewData` structs (`pub`) so the
/// nested-field privacy lint doesn't fire even though the enclosing `views`
/// module is `pub(crate)`.
pub struct OverlayMenuViewData<'a> {
    /// Whether the column-visibility checkbox dropdown is open. Driven by
    /// `Nokkvi.open_menu` so a single root-level state enforces mutual
    /// exclusion with other overlay menus.
    pub column_dropdown_open: bool,
    /// Trigger bounds captured when the dropdown was opened. The overlay
    /// anchors below this rectangle.
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus and the artwork-panel context menu can resolve their own
    /// open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
}

// ============================================================================
// Shared closure factories — keep per-view view.rs/update.rs free of the
// `(f * total as f32) as usize` boilerplate by composing the routing layer
// (`{View}Message::SlotList(...)`) with the scroll-seek payload in one place.
// ============================================================================

/// Build the `on_seek` closure that `slot_list_view_with_scroll` /
/// `slot_list_view_with_drag` expect.
///
/// The scrollbar callback receives a normalized `f32` (`0.0..=1.0`) and the
/// per-view code historically translated that into a
/// `{View}Message::SlotList(SlotListPageMessage::ScrollSeek(offset))` —
/// 8 byte-identical sites across the slot-list views. The factory composes
/// the two layers (`SlotListPageMessage::ScrollSeek` → outer routing
/// constructor) so the call site collapses to `scroll_seek_msg(total,
/// {View}Message::SlotList)`.
pub(crate) fn scroll_seek_msg<F, M>(total: usize, ctor: F) -> impl Fn(f32) -> M
where
    F: Fn(SlotListPageMessage) -> M,
{
    move |f| ctor(SlotListPageMessage::ScrollSeek((f * total as f32) as usize))
}

/// `true` when the user toggle is on, OR the active sort matches one of the
/// `triggers` (so the column auto-shows). Used by the per-view
/// `*_stars_visible` / `*_plays_visible` / `songs_genre_visible` helpers,
/// each of which now collapses to a one-line `auto_show_on_sort(...)` call.
pub(crate) fn auto_show_on_sort<M: PartialEq>(
    sort_mode: M,
    user_visible: bool,
    triggers: &[M],
) -> bool {
    user_visible || triggers.contains(&sort_mode)
}

#[cfg(test)]
#[allow(unreachable_pub, dead_code)]
mod tests {
    use super::*;

    define_view_columns! {
        TestColumn => TestColumnVisibility {
            Foo: foo = false,
            Bar: bar = true,
            BazQux: bazqux = true,
        }
    }

    #[test]
    fn columns_default_matches_declaration() {
        let v = TestColumnVisibility::default();
        assert!(!v.foo);
        assert!(v.bar);
        assert!(v.bazqux);
    }

    #[test]
    fn columns_get_returns_field() {
        let v = TestColumnVisibility::default();
        assert!(!v.get(TestColumn::Foo));
        assert!(v.get(TestColumn::Bar));
        assert!(v.get(TestColumn::BazQux));
    }

    #[test]
    fn columns_set_mutates_field() {
        let mut v = TestColumnVisibility::default();
        v.set(TestColumn::Foo, true);
        assert!(v.foo);
        v.set(TestColumn::Bar, false);
        assert!(!v.bar);
        v.set(TestColumn::BazQux, false);
        assert!(!v.bazqux);
    }

    #[derive(Debug, Clone)]
    enum TestAction {
        SearchChanged(String),
        SortModeChanged(crate::widgets::view_header::SortMode),
        SortOrderChanged(bool),
        RefreshViewData,
        CenterOnPlaying,
        NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
        NavigateAndExpandArtist(String),
        ViewSpecificDoNotMatch,
        None,
    }

    impl_has_common_action!(TestAction {
        NavigateAndExpandArtist
    });

    #[test]
    fn has_common_action_search_changed_maps() {
        let a = TestAction::SearchChanged("q".to_string());
        assert!(matches!(a.as_common(), CommonViewAction::SearchChanged));
    }

    #[test]
    fn has_common_action_center_on_playing_maps_when_present() {
        let a = TestAction::CenterOnPlaying;
        assert!(matches!(a.as_common(), CommonViewAction::CenterOnPlaying));
    }

    #[test]
    fn has_common_action_navigate_and_expand_artist_maps_with_id() {
        let a = TestAction::NavigateAndExpandArtist("artist-id".to_string());
        match a.as_common() {
            CommonViewAction::NavigateAndExpandArtist(id) => assert_eq!(id, "artist-id"),
            _ => panic!("expected NavigateAndExpandArtist"),
        }
    }

    #[test]
    fn has_common_action_unknown_variant_falls_through_to_view_specific() {
        let a = TestAction::ViewSpecificDoNotMatch;
        assert!(matches!(a.as_common(), CommonViewAction::ViewSpecific));
    }

    #[test]
    fn has_common_action_none_maps() {
        let a = TestAction::None;
        assert!(matches!(a.as_common(), CommonViewAction::None));
    }

    #[derive(Debug, Clone)]
    enum TestActionNoCenter {
        SearchChanged(String),
        SortModeChanged(crate::widgets::view_header::SortMode),
        SortOrderChanged(bool),
        RefreshViewData,
        NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
        NavigateAndExpandArtist(String),
        ViewSpecificDoNotMatch,
        None,
    }

    impl_has_common_action!(
        TestActionNoCenter,
        no_center {
            NavigateAndExpandArtist
        }
    );

    #[test]
    fn has_common_action_no_center_compiles_and_maps_basics() {
        let a = TestActionNoCenter::SearchChanged("q".to_string());
        assert!(matches!(a.as_common(), CommonViewAction::SearchChanged));
        let b = TestActionNoCenter::None;
        assert!(matches!(b.as_common(), CommonViewAction::None));
    }

    #[test]
    fn has_common_action_no_center_falls_through_for_view_specific() {
        let a = TestActionNoCenter::ViewSpecificDoNotMatch;
        assert!(matches!(a.as_common(), CommonViewAction::ViewSpecific));
    }

    // ========================================================================
    // scroll_seek_msg — closure factory composes both routing layers
    // ========================================================================

    // SlotListPageMessage doesn't derive PartialEq (its variants carry types
    // like iced::keyboard::Modifiers and HoveredSlot that don't either), so
    // these stand-ins use plain Debug + manual pattern matching.

    /// Minimal stand-in for a per-view message enum carrying a SlotList variant.
    #[derive(Debug)]
    enum DummyAMessage {
        SlotList(SlotListPageMessage),
    }

    #[derive(Debug)]
    enum DummyBMessage {
        SlotList(SlotListPageMessage),
    }

    #[test]
    fn scroll_seek_msg_composes_for_view_a() {
        let f = scroll_seek_msg(100, DummyAMessage::SlotList);
        // f(0.5) should produce offset 50 — verifies the f32 → usize math.
        let msg = f(0.5);
        let DummyAMessage::SlotList(inner) = msg;
        match inner {
            SlotListPageMessage::ScrollSeek(offset) => assert_eq!(offset, 50),
            other => panic!("expected ScrollSeek(50), got {other:?}"),
        }
    }

    #[test]
    fn scroll_seek_msg_composes_for_view_b() {
        // Same helper, different outer constructor — verifies the closure
        // factory is generic over the routing layer (not hard-coded to one view).
        let f = scroll_seek_msg(200, DummyBMessage::SlotList);
        let msg = f(0.25);
        let DummyBMessage::SlotList(inner) = msg;
        match inner {
            SlotListPageMessage::ScrollSeek(offset) => assert_eq!(offset, 50),
            other => panic!("expected ScrollSeek(50), got {other:?}"),
        }
    }

    #[test]
    fn scroll_seek_msg_zero_fraction_yields_zero_offset() {
        let f = scroll_seek_msg(500, DummyAMessage::SlotList);
        let msg = f(0.0);
        let DummyAMessage::SlotList(inner) = msg;
        match inner {
            SlotListPageMessage::ScrollSeek(offset) => assert_eq!(offset, 0),
            other => panic!("expected ScrollSeek(0), got {other:?}"),
        }
    }

    // ========================================================================
    // auto_show_on_sort — auto-show helper for stars/plays/genre columns
    // ========================================================================

    #[test]
    fn auto_show_on_sort_returns_true_when_user_toggle_is_on() {
        // Even outside the trigger set, the user toggle wins.
        assert!(auto_show_on_sort(SortMode::Name, true, &[SortMode::Rating]));
    }

    #[test]
    fn auto_show_on_sort_returns_true_when_sort_matches_trigger() {
        // User toggle off, but sort = Rating → auto-shown.
        assert!(auto_show_on_sort(
            SortMode::Rating,
            false,
            &[SortMode::Rating]
        ));
    }

    #[test]
    fn auto_show_on_sort_returns_false_when_neither_toggle_nor_trigger() {
        assert!(!auto_show_on_sort(
            SortMode::Name,
            false,
            &[SortMode::Rating]
        ));
    }

    #[test]
    fn auto_show_on_sort_supports_multi_variant_triggers() {
        // Sanity check that a multi-element trigger list works (no view uses
        // this today, but the API allows it).
        let triggers = &[SortMode::Rating, SortMode::MostPlayed][..];
        assert!(auto_show_on_sort(SortMode::Rating, false, triggers));
        assert!(auto_show_on_sort(SortMode::MostPlayed, false, triggers));
        assert!(!auto_show_on_sort(SortMode::Name, false, triggers));
    }

    // ========================================================================
    // define_view_columns! — toggle() method (Task 3.B)
    // ========================================================================

    #[test]
    fn columns_toggle_flips_value_and_returns_new() {
        let mut v = TestColumnVisibility::default();
        // foo defaults to false → toggle returns true and flips the field.
        let new = v.toggle(TestColumn::Foo);
        assert!(new);
        assert!(v.foo);
        // bar defaults to true → toggle returns false.
        let new2 = v.toggle(TestColumn::Bar);
        assert!(!new2);
        assert!(!v.bar);
    }
}
