//! Context Menu Widget
//!
//! A generic right-click context menu that wraps any child element.
//! Adapted from Halloy's `widget/context_menu.rs` for our use case.
//!
//! Usage:
//! ```ignore
//! context_menu(
//!     my_button_element,
//!     vec![Entry::Play, Entry::AddToQueue],
//!     |entry, length| entry.view(length, item_id),
//! )
//! ```

use std::slice;

use iced::{
    Element, Event, Length, Point, Rectangle, Size, Theme, Vector,
    advanced::{
        Layout, Shell, Widget, layout, overlay, renderer,
        widget::{self, tree},
    },
    mouse, touch,
    widget::{button, column, container, row, text},
};

use crate::{
    theme,
    widgets::{
        menu_constants::{
            MENU_ICON_SIZE, MENU_MIN_WIDTH, MENU_TEXT_SIZE, inflate_for_shadow_around_child,
            visible_menu_layout,
        },
        menu_dismiss,
    },
};

// ============================================================================
// Shared Library Context Menu Entry
// ============================================================================

/// Context menu entries shared across all library views (albums, songs, artists, genres, playlists).
/// Queue view has its own `QueueContextEntry` with queue-specific actions.
#[derive(Debug, Clone, Copy)]
pub enum LibraryContextEntry {
    /// Replace the queue with this collection/selection in a fresh random order
    /// and play from the top (one-shot Shuffle Play; never touches shuffle mode).
    ShufflePlay,
    AddToQueue,
    AddToPlaylist,
    Separator,
    GetInfo,
    ShowInFolder,
    /// Open Find Similar panel for this song/album/artist
    FindSimilar,
    /// Open Top Songs panel for this artist
    TopSongs,
    ReplaceQueueWithAllFound,
    AddAllFoundToQueue,
    AddAllFoundToPlaylist,
}

/// Standard library context menu entries list.
pub(crate) fn library_entries() -> Vec<LibraryContextEntry> {
    vec![
        LibraryContextEntry::ShufflePlay,
        LibraryContextEntry::AddToQueue,
        LibraryContextEntry::AddToPlaylist,
        LibraryContextEntry::Separator,
        LibraryContextEntry::GetInfo,
    ]
}

/// Library context menu entries with "Show in File Manager" (Songs, Albums, Artists views).
pub(crate) fn library_entries_with_folder() -> Vec<LibraryContextEntry> {
    vec![
        LibraryContextEntry::ShufflePlay,
        LibraryContextEntry::AddToQueue,
        LibraryContextEntry::AddToPlaylist,
        LibraryContextEntry::Separator,
        LibraryContextEntry::GetInfo,
        LibraryContextEntry::ShowInFolder,
    ]
}

/// Library context menu entries for Songs view (includes FindSimilar/TopSongs).
pub(crate) fn song_entries_with_folder() -> Vec<LibraryContextEntry> {
    vec![
        LibraryContextEntry::ShufflePlay,
        LibraryContextEntry::AddToQueue,
        LibraryContextEntry::AddToPlaylist,
        LibraryContextEntry::Separator,
        LibraryContextEntry::GetInfo,
        LibraryContextEntry::ShowInFolder,
        LibraryContextEntry::FindSimilar,
        LibraryContextEntry::TopSongs,
    ]
}

/// Library context menu entries for Artists view (includes TopSongs + FindSimilar).
pub(crate) fn artist_entries_with_folder() -> Vec<LibraryContextEntry> {
    vec![
        LibraryContextEntry::ShufflePlay,
        LibraryContextEntry::AddToQueue,
        LibraryContextEntry::AddToPlaylist,
        LibraryContextEntry::Separator,
        LibraryContextEntry::GetInfo,
        LibraryContextEntry::ShowInFolder,
        LibraryContextEntry::FindSimilar,
        LibraryContextEntry::TopSongs,
    ]
}

/// Library context menu entries for Similar/Top Songs view (includes batch actions).
pub(crate) fn similar_entries() -> Vec<LibraryContextEntry> {
    vec![
        LibraryContextEntry::ReplaceQueueWithAllFound,
        LibraryContextEntry::AddAllFoundToQueue,
        LibraryContextEntry::AddAllFoundToPlaylist,
        LibraryContextEntry::Separator,
        LibraryContextEntry::AddToQueue,
        LibraryContextEntry::AddToPlaylist,
        LibraryContextEntry::Separator,
        LibraryContextEntry::GetInfo,
        LibraryContextEntry::ShowInFolder,
        LibraryContextEntry::FindSimilar,
        LibraryContextEntry::TopSongs,
    ]
}

/// Render a standard library context menu entry.
pub(crate) fn library_entry_view<'a, Message: Clone + 'a>(
    entry: LibraryContextEntry,
    _length: Length,
    on_action: impl Fn(LibraryContextEntry) -> Message,
) -> Element<'a, Message> {
    match entry {
        LibraryContextEntry::ShufflePlay => menu_button(
            Some("assets/icons/shuffle.svg"),
            "Shuffle Play",
            on_action(LibraryContextEntry::ShufflePlay),
        ),
        LibraryContextEntry::AddToQueue => menu_button(
            Some("assets/icons/list-plus.svg"),
            "Add to Queue",
            on_action(LibraryContextEntry::AddToQueue),
        ),
        LibraryContextEntry::AddToPlaylist => menu_button(
            Some("assets/icons/list-music.svg"),
            "Add to Playlist",
            on_action(LibraryContextEntry::AddToPlaylist),
        ),
        LibraryContextEntry::Separator => menu_separator(),
        LibraryContextEntry::GetInfo => menu_button(
            Some("assets/icons/info.svg"),
            "Get Info",
            on_action(LibraryContextEntry::GetInfo),
        ),
        LibraryContextEntry::ShowInFolder => menu_button(
            Some("assets/icons/folder-open.svg"),
            "Show in File Manager",
            on_action(LibraryContextEntry::ShowInFolder),
        ),
        LibraryContextEntry::FindSimilar => menu_button(
            Some("assets/icons/radar.svg"),
            "Find Similar",
            on_action(LibraryContextEntry::FindSimilar),
        ),
        LibraryContextEntry::TopSongs => menu_button(
            Some("assets/icons/sparkles.svg"),
            "Top Songs",
            on_action(LibraryContextEntry::TopSongs),
        ),
        LibraryContextEntry::ReplaceQueueWithAllFound => menu_button(
            Some("assets/icons/circle-play.svg"),
            "Replace Queue with All Found",
            on_action(LibraryContextEntry::ReplaceQueueWithAllFound),
        ),
        LibraryContextEntry::AddAllFoundToQueue => menu_button(
            Some("assets/icons/list-tree.svg"),
            "Add All Found to Queue",
            on_action(LibraryContextEntry::AddAllFoundToQueue),
        ),
        LibraryContextEntry::AddAllFoundToPlaylist => menu_button(
            Some("assets/icons/library.svg"),
            "Create Playlist from All Found",
            on_action(LibraryContextEntry::AddAllFoundToPlaylist),
        ),
    }
}

// ============================================================================
// Strip Context Menu Entry (now-playing metadata strip)
// ============================================================================

/// Context menu entries for the track info strip (now-playing metadata).
#[derive(Debug, Clone, Copy)]
pub enum StripContextEntry {
    GoToQueue,
    GoToAlbum,
    GoToArtist,
    Separator,
    CopyTrackInfo,
    ToggleStar,
    ShowInFolder,
    FindSimilar,
    TopSongs,
}

/// Build strip context menu entries.
/// `has_local_path`: true if `local_music_path` is configured (shows "Show in File Manager").
pub(crate) fn strip_entries(has_local_path: bool) -> Vec<StripContextEntry> {
    let mut entries = vec![
        StripContextEntry::GoToQueue,
        StripContextEntry::GoToAlbum,
        StripContextEntry::GoToArtist,
        StripContextEntry::Separator,
        StripContextEntry::CopyTrackInfo,
        StripContextEntry::ToggleStar,
    ];
    entries.push(StripContextEntry::FindSimilar);
    entries.push(StripContextEntry::TopSongs);
    if has_local_path {
        entries.push(StripContextEntry::ShowInFolder);
    }
    entries
}

/// Render a strip context menu entry.
/// `is_starred`: whether the currently playing track is starred.
pub(crate) fn strip_entry_view<'a, Message: Clone + 'a>(
    entry: StripContextEntry,
    _length: Length,
    is_starred: bool,
    on_action: impl Fn(StripContextEntry) -> Message,
) -> Element<'a, Message> {
    match entry {
        StripContextEntry::GoToQueue => menu_button(
            Some("assets/icons/list-music.svg"),
            "Go to Queue",
            on_action(StripContextEntry::GoToQueue),
        ),
        StripContextEntry::GoToAlbum => menu_button(
            Some("assets/icons/disc-3.svg"),
            "Go to Album",
            on_action(StripContextEntry::GoToAlbum),
        ),
        StripContextEntry::GoToArtist => menu_button(
            Some("assets/icons/mic.svg"),
            "Go to Artist",
            on_action(StripContextEntry::GoToArtist),
        ),
        StripContextEntry::Separator => menu_separator(),
        StripContextEntry::CopyTrackInfo => menu_button(
            Some("assets/icons/copy.svg"),
            "Copy Track Info",
            on_action(StripContextEntry::CopyTrackInfo),
        ),
        StripContextEntry::ToggleStar => {
            let (icon, label) = if is_starred {
                ("assets/icons/star-filled.svg", "Unlove")
            } else {
                ("assets/icons/star.svg", "Love")
            };
            menu_button(Some(icon), label, on_action(StripContextEntry::ToggleStar))
        }
        StripContextEntry::ShowInFolder => menu_button(
            Some("assets/icons/folder-open.svg"),
            "Show in File Manager",
            on_action(StripContextEntry::ShowInFolder),
        ),
        StripContextEntry::FindSimilar => menu_button(
            Some("assets/icons/radar.svg"),
            "Find Similar",
            on_action(StripContextEntry::FindSimilar),
        ),
        StripContextEntry::TopSongs => menu_button(
            Some("assets/icons/star.svg"),
            "Top Songs",
            on_action(StripContextEntry::TopSongs),
        ),
    }
}

// ============================================================================
// Radio Context Menu Entry
// ============================================================================

/// Context menu entries for internet radio stations.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RadioContextEntry {
    Edit,
    Delete,
    CopyStreamUrl,
    /// Pick an image file and upload it as the station's custom logo.
    SetArtwork,
    /// Delete the uploaded logo server-side; automatic artwork (ICY
    /// now-playing / tower glyph) returns. Listed only when the station
    /// actually has an uploaded logo.
    ResetArtwork,
    RefreshArtwork,
}

/// Radio-station context entries. `has_custom_art` (the station's
/// `logo_cover_art().is_some()`) gates the "Reset Artwork" entry — there is
/// nothing to reset on a station without an uploaded logo.
pub(crate) fn radio_entries(has_custom_art: bool) -> Vec<RadioContextEntry> {
    let mut entries = vec![
        RadioContextEntry::Edit,
        RadioContextEntry::CopyStreamUrl,
        RadioContextEntry::SetArtwork,
    ];
    if has_custom_art {
        entries.push(RadioContextEntry::ResetArtwork);
    }
    entries.push(RadioContextEntry::RefreshArtwork);
    entries.push(RadioContextEntry::Delete);
    entries
}

pub(crate) fn radio_entry_view<'a, Message: Clone + 'a>(
    entry: RadioContextEntry,
    _length: Length,
    on_action: impl Fn(RadioContextEntry) -> Message,
) -> Element<'a, Message> {
    match entry {
        RadioContextEntry::Edit => menu_button(
            Some("assets/icons/pencil.svg"),
            "Edit Station",
            on_action(RadioContextEntry::Edit),
        ),
        RadioContextEntry::CopyStreamUrl => menu_button(
            Some("assets/icons/copy.svg"),
            "Copy Stream URL",
            on_action(RadioContextEntry::CopyStreamUrl),
        ),
        // The three artwork rows delegate to the PanelMenuEntry constructors
        // so the icon + label vocabulary has ONE definition shared with the
        // artwork-panel menus (and the playlist row menu) — a future rename
        // can't fork the wording between surfaces.
        RadioContextEntry::SetArtwork => {
            PanelMenuEntry::set_custom_artwork(on_action(RadioContextEntry::SetArtwork)).view()
        }
        RadioContextEntry::ResetArtwork => {
            PanelMenuEntry::reset_artwork(on_action(RadioContextEntry::ResetArtwork)).view()
        }
        RadioContextEntry::RefreshArtwork => {
            PanelMenuEntry::refresh_artwork(on_action(RadioContextEntry::RefreshArtwork)).view()
        }
        RadioContextEntry::Delete => menu_button(
            Some("assets/icons/trash-2.svg"),
            "Delete Station",
            on_action(RadioContextEntry::Delete),
        ),
    }
}

// ============================================================================
// Artwork-Panel Menu Entry
// ============================================================================

/// One entry of a view's large-artwork-panel right-click menu: a static icon +
/// label pair and the message it dispatches. The panel helpers in
/// `widgets::base_slot_list_layout` take a `Vec<PanelMenuEntry<Message>>` and
/// wrap the panel in a [`context_menu`] when the list is non-empty — views
/// declare their entries with the constructors below instead of the old
/// hardcoded single "Refresh Artwork" action.
#[derive(Debug, Clone)]
pub(crate) struct PanelMenuEntry<Message> {
    pub icon: &'static str,
    pub label: &'static str,
    pub message: Message,
}

impl<Message> PanelMenuEntry<Message> {
    /// "Refresh Artwork" — evict + re-fetch the entity's artwork.
    pub(crate) fn refresh_artwork(message: Message) -> Self {
        Self {
            icon: "assets/icons/refresh-cw.svg",
            label: "Refresh Artwork",
            message,
        }
    }

    /// "Set Custom Artwork…" — open the native file picker and upload.
    pub(crate) fn set_custom_artwork(message: Message) -> Self {
        Self {
            icon: "assets/icons/folder-open.svg",
            label: "Set Custom Artwork…",
            message,
        }
    }

    /// "Reset Artwork" — delete the custom image so the automatic art returns.
    pub(crate) fn reset_artwork(message: Message) -> Self {
        Self {
            icon: "assets/icons/rotate-ccw.svg",
            label: "Reset Artwork",
            message,
        }
    }

    /// Render this entry as a standard menu row. Consumes the entry (the
    /// message moves into the button).
    pub(crate) fn view<'a>(self) -> Element<'a, Message>
    where
        Message: Clone + 'a,
    {
        menu_button(Some(self.icon), self.label, self.message)
    }
}

// ============================================================================
// Helper: Resolve per-instance open state from the root menu coordinator
// ============================================================================

/// Returns `(is_open, open_position)` for a `context_menu` instance keyed by
/// `id`, derived from the root-level `Nokkvi.open_menu`. Use this at each call
/// site to drive the controlled `is_open` / `open_position` props.
pub(crate) fn open_state_for(
    open_menu: Option<&crate::app_message::OpenMenu>,
    id: &crate::app_message::ContextMenuId,
) -> (bool, Option<Point>) {
    match open_menu {
        Some(crate::app_message::OpenMenu::Context {
            id: open_id,
            position,
        }) if open_id == id => (true, Some(*position)),
        _ => (false, None),
    }
}

/// Resolve the `(is_open, open_position, on_change)` trio every artwork-panel
/// call site builds for `ContextMenuId::ArtworkPanel(view)`.
///
/// The returned closure maps the controlled-component callback (`Some(p)` →
/// open at `p`, `None` → close) into the view's per-page `SetOpenMenu` message
/// constructor. Each library view's `view.rs` previously inlined the same
/// 18-line block; this helper collapses it to a single `let` destructure.
pub(crate) fn artwork_panel_open_state<M>(
    view: crate::View,
    open_menu: Option<&crate::app_message::OpenMenu>,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> M + Clone,
) -> (bool, Option<Point>, impl Fn(Option<Point>) -> M + Clone) {
    let id = crate::app_message::ContextMenuId::ArtworkPanel(view);
    let (is_open, position) = open_state_for(open_menu, &id);
    let on_change = move |position: Option<Point>| match position {
        Some(p) => on_set_open_menu(Some(crate::app_message::OpenMenu::Context {
            id: id.clone(),
            position: p,
        })),
        None => on_set_open_menu(None),
    };
    (is_open, position, on_change)
}

// ============================================================================
// Public API
// ============================================================================

/// Create a context menu wrapping the given base element.
///
/// Controlled component: `is_open` and `open_position` are passed in by the
/// parent (derived from `Nokkvi.open_menu`), and the widget emits
/// `on_open_change(Some(cursor_pos))` when the user right-clicks the base or
/// `on_open_change(None)` when the menu should close. The parent stashes the
/// position into `OpenMenu::Context { id, position }` so the next render
/// passes it back here.
///
/// - `base` — the wrapped content (e.g., a slot list slot button)
/// - `entries` — menu items to show when opened
/// - `entry_view` — renders each entry into an `Element`; receives the entry
///   and a `Length` hint (use `Length::Fill` for full-width buttons)
/// - `is_open` — whether this widget instance owns the currently open menu
/// - `open_position` — anchor point for the overlay (passed in from parent)
/// - `on_open_change` — emitted with `Some(cursor_pos)` to request open or
///   `None` to request close
pub(crate) fn context_menu<'a, T, Message>(
    base: impl Into<Element<'a, Message>>,
    entries: Vec<T>,
    entry_view: impl Fn(T, Length) -> Element<'a, Message> + 'a,
    is_open: bool,
    open_position: Option<Point>,
    on_open_change: impl Fn(Option<Point>) -> Message + 'a,
) -> ContextMenu<'a, T, Message> {
    ContextMenu {
        base: base.into(),
        entries,
        entry_view: Box::new(entry_view),
        on_open_change: Box::new(on_open_change),
        is_open,
        open_position,
        menu: None,
    }
}

/// Wrap a slot-list row with a standard library context menu.
///
/// Handles `ContextMenuId::LibraryRow` construction, `open_state_for`, and the
/// `OpenMenu::Context` open/close messages in one place.
///
/// Pass the bare `XxxMessage::ContextMenuAction` tuple-variant constructor as
/// `on_context_action` — the helper supplies `item_index` as the first argument.
/// Pass `XxxMessage::SetOpenMenu` as `on_set_open_menu`.
pub(crate) fn wrap_library_row<'a, Message>(
    view: crate::View,
    item_index: usize,
    base: impl Into<Element<'a, Message>>,
    entries: Vec<LibraryContextEntry>,
    open_menu: Option<&'a crate::app_message::OpenMenu>,
    on_context_action: impl Fn(usize, LibraryContextEntry) -> Message + 'a,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> Message + 'a,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let cm_id = crate::app_message::ContextMenuId::LibraryRow { view, item_index };
    let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
    context_menu(
        base,
        entries,
        move |entry, length| {
            library_entry_view(entry, length, |e| on_context_action(item_index, e))
        },
        cm_open,
        cm_position,
        move |position| match position {
            Some(p) => on_set_open_menu(Some(crate::app_message::OpenMenu::Context {
                id: cm_id.clone(),
                position: p,
            })),
            None => on_set_open_menu(None),
        },
    )
    .into()
}

/// Like [`wrap_library_row`] for Similar/TopSongs rows, which use
/// `ContextMenuId::SimilarRow` instead of `LibraryRow`.
pub(crate) fn wrap_similar_row<'a, Message>(
    item_index: usize,
    base: impl Into<Element<'a, Message>>,
    entries: Vec<LibraryContextEntry>,
    open_menu: Option<&'a crate::app_message::OpenMenu>,
    on_context_action: impl Fn(usize, LibraryContextEntry) -> Message + 'a,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> Message + 'a,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let cm_id = crate::app_message::ContextMenuId::SimilarRow(item_index);
    let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
    context_menu(
        base,
        entries,
        move |entry, length| {
            library_entry_view(entry, length, |e| on_context_action(item_index, e))
        },
        cm_open,
        cm_position,
        move |position| match position {
            Some(p) => on_set_open_menu(Some(crate::app_message::OpenMenu::Context {
                id: cm_id.clone(),
                position: p,
            })),
            None => on_set_open_menu(None),
        },
    )
    .into()
}

/// Wrap a now-playing strip with the strip context menu (Go to Queue/Album/
/// Artist, Copy Track Info, Love/Unlove, Find Similar, Top Songs, optional
/// Show in File Manager).
///
/// Shared by the three strip placements (player-bar strip, top strip,
/// merged nav-bar strip), which differ only in their message type. Radio
/// playback gets NO context menu: `is_radio` returns the bare strip, exactly
/// preserving each placement's conditional widget-tree shape.
///
/// `on_action` / `on_set_open_menu` are fn pointers on purpose — all call
/// sites pass bare tuple-variant constructors (`*Message::StripContextAction`
/// / `*Message::SetOpenMenu`); loosen to `impl Fn + Clone` only if a future
/// caller needs captures.
pub(crate) fn wrap_strip_context_menu<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    is_radio: bool,
    has_local_path: bool,
    is_starred: bool,
    open_state: (bool, Option<Point>),
    on_action: fn(StripContextEntry) -> Message,
    on_set_open_menu: fn(Option<crate::app_message::OpenMenu>) -> Message,
) -> Element<'a, Message> {
    let base = base.into();
    if is_radio {
        return base;
    }
    let (is_open, position) = open_state;
    context_menu(
        base,
        strip_entries(has_local_path),
        move |entry, length| strip_entry_view(entry, length, is_starred, on_action),
        is_open,
        position,
        move |position| match position {
            Some(p) => on_set_open_menu(Some(crate::app_message::OpenMenu::Context {
                id: crate::app_message::ContextMenuId::Strip,
                position: p,
            })),
            None => on_set_open_menu(None),
        },
    )
    .into()
}

// ============================================================================
// Widget
// ============================================================================

pub struct ContextMenu<'a, T, Message> {
    base: Element<'a, Message>,
    entries: Vec<T>,
    entry_view: Box<dyn Fn(T, Length) -> Element<'a, Message> + 'a>,
    /// Emitted with `Some(cursor_pos)` to request open or `None` to request
    /// close. Pure state-change request — the actual open/close happens after
    /// the parent dispatches `Message::SetOpenMenu`.
    on_open_change: Box<dyn Fn(Option<Point>) -> Message + 'a>,
    /// Whether this widget instance owns the currently open menu (controlled
    /// by parent via `Nokkvi.open_menu`).
    is_open: bool,
    /// Anchor point for the overlay when open (passed in from parent).
    open_position: Option<Point>,
    /// Cached menu element, rebuilt when opening.
    menu: Option<Element<'a, Message>>,
}

/// Tree-state. Open/closed and position both live on the parent now; only the
/// overlay's persistent widget tree (button hover/press state) stays here.
#[derive(Debug)]
struct State {
    menu_tree: widget::Tree,
}

impl State {
    fn new() -> Self {
        Self {
            menu_tree: widget::Tree::empty(),
        }
    }
}

impl<'a, T, Message> Widget<Message, Theme, iced::Renderer> for ContextMenu<'a, T, Message>
where
    T: Clone + 'a,
    Message: 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(slice::from_mut(&mut self.base));
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Intercept right-click on our bounds to request open at the cursor
        // position. If a different context menu was already open elsewhere,
        // the parent's `SetOpenMenu` handler replaces it atomically.
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
        ) && let Some(cursor_pos) = cursor.position_over(layout.bounds())
        {
            let position = Point::new(cursor_pos.x + 5.0, cursor_pos.y + 5.0);
            shell.publish((self.on_open_change)(Some(position)));
            shell.request_redraw();
            // Capture so the child button doesn't also process the right-click.
            shell.capture_event();
            return;
        }

        // Forward to child
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.base.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        // Let the child provide its overlay first
        let base_state = tree.children.first_mut()?;
        let base_overlay =
            self.base
                .as_widget_mut()
                .overlay(base_state, layout, renderer, viewport, translation);

        let state = tree.state.downcast_mut::<State>();
        let our_overlay = if self.is_open
            && let Some(position) = self.open_position
        {
            build_overlay(
                state,
                &mut self.menu,
                &self.entries,
                &self.entry_view,
                &*self.on_open_change,
                position,
                translation,
            )
        } else {
            // Drop cached menu element + reset persisted tree so next open
            // starts fresh.
            self.menu = None;
            state.menu_tree = widget::Tree::empty();
            None
        };

        if base_overlay.is_none() && our_overlay.is_none() {
            None
        } else {
            Some(
                overlay::Group::with_children(
                    base_overlay.into_iter().chain(our_overlay).collect(),
                )
                .overlay(),
            )
        }
    }
}

impl<'a, T: Clone + 'a, Message: 'a> From<ContextMenu<'a, T, Message>> for Element<'a, Message> {
    fn from(menu: ContextMenu<'a, T, Message>) -> Self {
        Element::new(menu)
    }
}

// ============================================================================
// Overlay Builder
// ============================================================================

fn build_menu_element<'a, T, Message>(
    entries: &[T],
    entry_view: &(dyn Fn(T, Length) -> Element<'a, Message> + 'a),
) -> Element<'a, Message>
where
    T: Clone + 'a,
    Message: 'a,
{
    // Shared menu-panel chrome — see `widgets::menu_chrome`.
    container(column(
        entries.iter().cloned().map(|e| entry_view(e, Length::Fill)),
    ))
    .padding(4)
    .style(super::menu_chrome::container_style)
    .into()
}

fn build_overlay<'a, 'b, T, Message>(
    state: &'b mut State,
    menu: &'b mut Option<Element<'a, Message>>,
    entries: &[T],
    entry_view: &(dyn Fn(T, Length) -> Element<'a, Message> + 'a),
    on_open_change: &'b dyn Fn(Option<Point>) -> Message,
    position: Point,
    translation: Vector,
) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>>
where
    T: Clone + 'a,
    Message: 'a,
{
    if entries.is_empty() {
        return None;
    }

    // Always (re)build the menu Element — it's cheap and ensures the view
    // closure captures fresh data. We diff against the persisted `menu_tree`
    // so button widget state (is_pressed, hover) survives the view rebuild.
    let m = menu.get_or_insert_with(|| build_menu_element(entries, entry_view));
    // diff against the persisted `menu_tree` unconditionally: on a fresh (empty)
    // tree it allocates+populates the child state; on a populated tree it reconciles
    // while preserving button state (is_pressed, hover). Since iced's `Tree::new` no
    // longer eagerly populates children, the old `is_empty()` guard would otherwise
    // leave the overlay rendering against an empty child tree.
    state.menu_tree.diff(&mut *m as &mut Element<'a, Message>);

    menu.as_mut().map(|m| {
        overlay::Element::new(Box::new(MenuOverlay {
            menu: m,
            state,
            on_open_change,
            position: position + translation,
        }))
    })
}

// ============================================================================
// Menu Overlay
// ============================================================================

struct MenuOverlay<'a, 'b, Message> {
    menu: &'b mut Element<'a, Message>,
    state: &'b mut State,
    on_open_change: &'b dyn Fn(Option<Point>) -> Message,
    position: Point,
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for MenuOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds)
            .width(Length::Shrink)
            .height(Length::Shrink);

        let menu_node =
            self.menu
                .as_widget_mut()
                .layout(&mut self.state.menu_tree, renderer, &limits);

        let padding = 5.0;
        let viewport = Rectangle::new(
            Point::new(padding, padding),
            Size::new(bounds.width - 2.0 * padding, bounds.height - 2.0 * padding),
        );
        let mut menu_bounds = Rectangle::new(self.position, menu_node.size());

        // Clamp to viewport
        if menu_bounds.x < viewport.x {
            menu_bounds.x = viewport.x;
        } else if menu_bounds.x + menu_bounds.width > viewport.x + viewport.width {
            menu_bounds.x = viewport.x + viewport.width - menu_bounds.width;
        }

        if menu_bounds.y < viewport.y {
            menu_bounds.y = viewport.y;
        } else if menu_bounds.y + menu_bounds.height > viewport.y + viewport.height {
            menu_bounds.y = viewport.y + viewport.height - menu_bounds.height;
        }

        inflate_for_shadow_around_child(menu_node, menu_bounds.position())
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let menu_layout = visible_menu_layout(layout);
        let cursor_over = cursor.position_over(menu_layout.bounds());

        // Escape / outside-press dismissal — see `widgets::menu_dismiss` for
        // the capture semantics (outside presses deliberately stay
        // uncaptured). A press with no cursor position counts as outside.
        if menu_dismiss::handle_dismiss(
            event,
            shell,
            || menu_dismiss::press_began(event) && cursor_over.is_none(),
            || (self.on_open_change)(None),
        ) {
            return;
        }

        // Delegate to the menu content (buttons handle their own clicks)
        self.menu.as_widget_mut().update(
            &mut self.state.menu_tree,
            event,
            menu_layout,
            cursor,
            renderer,
            shell,
            &menu_layout.bounds(),
        );

        // Close menu after a button click inside it.
        // Must use ButtonReleased (not ButtonPressed) because iced buttons
        // fire on_press during the release event. Closing on press would
        // destroy the overlay before the button can emit its message.
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerLifted { .. })
        ) && cursor_over.is_some()
        {
            shell.publish((self.on_open_change)(None));
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let menu_layout = visible_menu_layout(layout);
        self.menu.as_widget().draw(
            &self.state.menu_tree,
            renderer,
            theme,
            style,
            menu_layout,
            cursor,
            &menu_layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let menu_layout = visible_menu_layout(layout);
        self.menu.as_widget().mouse_interaction(
            &self.state.menu_tree,
            menu_layout,
            cursor,
            &menu_layout.bounds(),
            renderer,
        )
    }
}

// ============================================================================
// Menu Item Helpers
// ============================================================================

/// Render a standard menu button with an optional icon.
///
/// Use this in your `entry_view` closure to render each menu entry.
pub(crate) fn menu_button<'a, Message: Clone + 'a>(
    icon_path: Option<&str>,
    label: &str,
    message: Message,
) -> Element<'a, Message> {
    use iced::widget::svg;

    let content: Element<'a, Message> = if let Some(icon) = icon_path {
        row![
            crate::embedded_svg::svg_widget(icon)
                .width(Length::Fixed(MENU_ICON_SIZE))
                .height(Length::Fixed(MENU_ICON_SIZE))
                .style(|_theme, _status| svg::Style {
                    color: Some(theme::fg1()),
                }),
            text(label.to_string())
                .size(MENU_TEXT_SIZE)
                .font(theme::ui_font())
                .color(theme::fg0()),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        text(label.to_string())
            .size(MENU_TEXT_SIZE)
            .font(theme::ui_font())
            .color(theme::fg0())
            .into()
    };

    button(
        container(content)
            .width(Length::Fill)
            .padding(iced::Padding {
                left: 8.0,
                right: 16.0,
                top: 0.0,
                bottom: 0.0,
            }),
    )
    .on_press(message)
    .padding(iced::Padding {
        top: 4.0,
        bottom: 4.0,
        left: 0.0,
        right: 0.0,
    })
    .width(Length::Fixed(MENU_MIN_WIDTH))
    .style(move |_theme, status| {
        // Hover/press fill = `bg2()` with `ui_radius_xs()` corners so the
        // highlight nests neatly inside the `ui_radius_md()` panel
        // outline in rounded mode (4 px vs 12 px concentric).
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => Some(theme::bg2().into()),
            _ => None,
        };
        button::Style {
            background: bg,
            text_color: theme::fg0(),
            border: iced::Border {
                radius: theme::ui_radius_xs(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

/// Render a separator line for grouping menu items.
///
/// Color matches `theme::border()` (the panel outline) so the
/// inter-section divider reads as a continuation of the chrome line —
/// matches the `hamburger_menu` separator vocabulary.
pub(crate) fn menu_separator<'a, Message: 'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        })
        .padding(iced::Padding {
            left: 8.0,
            right: 8.0,
            top: 2.0,
            bottom: 2.0,
        })
        .into()
}

#[cfg(test)]
mod tests {
    use iced::Point;

    use super::*;
    use crate::{
        View,
        app_message::{ContextMenuId, OpenMenu},
    };

    // --- open_state_for -----------------------------------------------------

    #[test]
    fn open_state_for_returns_closed_when_no_menu_open() {
        let id = ContextMenuId::ArtworkPanel(View::Albums);
        let (open, pos) = open_state_for(None, &id);
        assert!(!open);
        assert!(pos.is_none());
    }

    #[test]
    fn open_state_for_returns_open_when_id_matches() {
        let id = ContextMenuId::ArtworkPanel(View::Albums);
        let menu = OpenMenu::Context {
            id: id.clone(),
            position: Point::new(12.0, 34.0),
        };
        let (open, pos) = open_state_for(Some(&menu), &id);
        assert!(open);
        assert_eq!(pos, Some(Point::new(12.0, 34.0)));
    }

    #[test]
    fn open_state_for_returns_closed_when_id_differs() {
        let queried_id = ContextMenuId::ArtworkPanel(View::Albums);
        let other_menu = OpenMenu::Context {
            id: ContextMenuId::ArtworkPanel(View::Songs),
            position: Point::new(1.0, 2.0),
        };
        let (open, pos) = open_state_for(Some(&other_menu), &queried_id);
        assert!(!open);
        assert!(pos.is_none());
    }

    #[test]
    fn open_state_for_returns_closed_for_non_context_variant() {
        let id = ContextMenuId::ArtworkPanel(View::Queue);
        let hamburger = OpenMenu::Hamburger;
        let (open, pos) = open_state_for(Some(&hamburger), &id);
        assert!(!open);
        assert!(pos.is_none());
    }

    // --- radio_entries gating ------------------------------------------------

    /// "Set Custom Artwork…" is always offered; "Reset Artwork" only when the
    /// station actually has an uploaded logo (`logo_cover_art().is_some()`).
    #[test]
    fn radio_entries_gate_reset_on_custom_art() {
        let with = radio_entries(true);
        assert!(
            with.iter()
                .any(|e| matches!(e, RadioContextEntry::ResetArtwork)),
            "custom-art station must offer Reset Artwork"
        );
        assert!(
            with.iter()
                .any(|e| matches!(e, RadioContextEntry::SetArtwork)),
        );

        let without = radio_entries(false);
        assert!(
            !without
                .iter()
                .any(|e| matches!(e, RadioContextEntry::ResetArtwork)),
            "a station without custom art has nothing to reset"
        );
        assert!(
            without
                .iter()
                .any(|e| matches!(e, RadioContextEntry::SetArtwork)),
            "Set Custom Artwork… must always be offered"
        );
        // The pre-existing entries survive in both forms.
        for entries in [&with, &without] {
            for expected in ["Edit", "CopyStreamUrl", "RefreshArtwork", "Delete"] {
                let found = entries.iter().any(|e| {
                    matches!(
                        (e, expected),
                        (RadioContextEntry::Edit, "Edit")
                            | (RadioContextEntry::CopyStreamUrl, "CopyStreamUrl")
                            | (RadioContextEntry::RefreshArtwork, "RefreshArtwork")
                            | (RadioContextEntry::Delete, "Delete")
                    )
                });
                assert!(found, "missing {expected}");
            }
        }
    }

    // --- artwork_panel_open_state ------------------------------------------

    /// Tiny stand-in message type so the helper's generic-message bound
    /// stays exercised without dragging the real `*Message` enums into the
    /// test scope.
    #[derive(Debug, Clone, PartialEq)]
    enum TestMsg {
        Set(Option<OpenMenu>),
    }

    #[test]
    fn artwork_panel_open_state_reports_closed_when_no_menu_open() {
        let (open, pos, _on_change) = artwork_panel_open_state(View::Albums, None, TestMsg::Set);
        assert!(!open);
        assert!(pos.is_none());
    }

    #[test]
    fn artwork_panel_open_state_reports_open_for_matching_view() {
        let menu = OpenMenu::Context {
            id: ContextMenuId::ArtworkPanel(View::Albums),
            position: Point::new(7.0, 9.0),
        };
        let (open, pos, _on_change) =
            artwork_panel_open_state(View::Albums, Some(&menu), TestMsg::Set);
        assert!(open);
        assert_eq!(pos, Some(Point::new(7.0, 9.0)));
    }

    #[test]
    fn artwork_panel_open_state_reports_closed_for_different_view() {
        let other = OpenMenu::Context {
            id: ContextMenuId::ArtworkPanel(View::Songs),
            position: Point::new(0.0, 0.0),
        };
        let (open, pos, _on_change) =
            artwork_panel_open_state(View::Albums, Some(&other), TestMsg::Set);
        assert!(!open);
        assert!(pos.is_none());
    }

    #[test]
    fn artwork_panel_open_state_close_callback_emits_set_none() {
        let (_open, _pos, on_change) = artwork_panel_open_state(View::Albums, None, TestMsg::Set);
        assert_eq!(on_change(None), TestMsg::Set(None));
    }

    #[test]
    fn artwork_panel_open_state_open_callback_wraps_context_with_view() {
        let (_open, _pos, on_change) = artwork_panel_open_state(View::Queue, None, TestMsg::Set);
        let pt = Point::new(100.0, 200.0);
        let msg = on_change(Some(pt));
        let TestMsg::Set(payload) = msg;
        match payload {
            Some(OpenMenu::Context { id, position }) => {
                assert_eq!(id, ContextMenuId::ArtworkPanel(View::Queue));
                assert_eq!(position, pt);
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }
}
