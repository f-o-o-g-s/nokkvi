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
    keyboard, mouse, touch,
    widget::{button, column, container, row, text},
};

use crate::theme;

// ============================================================================
// Shared Library Context Menu Entry
// ============================================================================

/// Context menu entries shared across all library views (albums, songs, artists, genres, playlists).
/// Queue view has its own `QueueContextEntry` with queue-specific actions.
#[derive(Debug, Clone, Copy)]
pub enum LibraryContextEntry {
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
        LibraryContextEntry::AddToQueue,
        LibraryContextEntry::AddToPlaylist,
        LibraryContextEntry::Separator,
        LibraryContextEntry::GetInfo,
    ]
}

/// Library context menu entries with "Show in File Manager" (Songs, Albums, Artists views).
pub(crate) fn library_entries_with_folder() -> Vec<LibraryContextEntry> {
    vec![
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
}

pub(crate) fn radio_entries() -> Vec<RadioContextEntry> {
    vec![
        RadioContextEntry::Edit,
        RadioContextEntry::CopyStreamUrl,
        RadioContextEntry::Delete,
    ]
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
        RadioContextEntry::Delete => menu_button(
            Some("assets/icons/trash-2.svg"),
            "Delete Station",
            on_action(RadioContextEntry::Delete),
        ),
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
    T: Copy + 'a,
    Message: 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(slice::from_ref(&self.base));
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

impl<'a, T: Copy + 'a, Message: 'a> From<ContextMenu<'a, T, Message>> for Element<'a, Message> {
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
    T: Copy + 'a,
    Message: 'a,
{
    container(column(
        entries.iter().copied().map(|e| entry_view(e, Length::Fill)),
    ))
    .padding(4)
    .style(|_theme| container::Style {
        background: Some(theme::bg1().into()),
        border: iced::Border {
            width: 1.0,
            color: theme::accent_bright(),
            radius: theme::ui_border_radius(),
        },
        ..Default::default()
    })
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
    T: Copy + 'a,
    Message: 'a,
{
    if entries.is_empty() {
        return None;
    }

    // Always (re)build the menu Element — it's cheap and ensures the view
    // closure captures fresh data. We diff against the persisted `menu_tree`
    // so button widget state (is_pressed, hover) survives the view rebuild.
    let m = menu.get_or_insert_with(|| build_menu_element(entries, entry_view));
    if state.menu_tree.children.is_empty() {
        state.menu_tree = widget::Tree::new(&*m);
    } else {
        state.menu_tree.diff(&*m as &Element<'a, Message>);
    }

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

        let node = self
            .menu
            .as_widget_mut()
            .layout(&mut self.state.menu_tree, renderer, &limits);

        let padding = 5.0;
        let viewport = Rectangle::new(
            Point::new(padding, padding),
            Size::new(bounds.width - 2.0 * padding, bounds.height - 2.0 * padding),
        );
        let mut menu_bounds = Rectangle::new(self.position, node.size());

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

        node.move_to(menu_bounds.position())
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let cursor_over = cursor.position_over(layout.bounds());

        // Escape key → close
        if matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
        ) {
            shell.publish((self.on_open_change)(None));
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        // Click outside menu → emit close. Do NOT capture: a different menu's
        // trigger may be under the cursor, and iced dispatches overlays
        // before the widget tree, so the trigger's open emit arrives later
        // and wins (the parent's `SetOpenMenu` handler simply replaces the
        // value). For a click in genuinely empty space, only the close
        // emits, and the menu disappears next frame.
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. })
        ) && cursor_over.is_none()
        {
            shell.publish((self.on_open_change)(None));
            shell.request_redraw();
            return;
        }

        // Delegate to the menu content (buttons handle their own clicks)
        self.menu.as_widget_mut().update(
            &mut self.state.menu_tree,
            event,
            layout,
            cursor,
            renderer,
            shell,
            &layout.bounds(),
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
        self.menu.as_widget().draw(
            &self.state.menu_tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.menu.as_widget().mouse_interaction(
            &self.state.menu_tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }
}

// ============================================================================
// Menu Item Helpers
// ============================================================================

/// Minimum width for context menu items.
const MENU_MIN_WIDTH: f32 = 180.0;
const MENU_ICON_SIZE: f32 = 14.0;
const MENU_TEXT_SIZE: f32 = 13.0;

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
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => Some(theme::bg2().into()),
            _ => None,
        };
        button::Style {
            background: bg,
            text_color: theme::fg0(),
            border: iced::Border {
                radius: theme::ui_border_radius(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

/// Render a separator line for grouping menu items.
pub(crate) fn menu_separator<'a, Message: 'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme| container::Style {
            background: Some(theme::bg3().into()),
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
