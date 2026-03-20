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
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Create a context menu wrapping the given base element.
///
/// - `base` — the wrapped content (e.g., a slot list slot button)
/// - `entries` — menu items to show when opened
/// - `entry_view` — renders each entry into an `Element`; receives the entry
///   and a `Length` hint (use `Length::Fill` for full-width buttons)
pub(crate) fn context_menu<'a, T, Message>(
    base: impl Into<Element<'a, Message>>,
    entries: Vec<T>,
    entry_view: impl Fn(T, Length) -> Element<'a, Message> + 'a,
) -> ContextMenu<'a, T, Message> {
    ContextMenu {
        base: base.into(),
        entries,
        entry_view: Box::new(entry_view),
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
    /// Cached menu element, rebuilt when opening.
    menu: Option<Element<'a, Message>>,
}

#[derive(Debug)]
struct State {
    status: Status,
    menu_tree: widget::Tree,
}

impl State {
    fn new() -> Self {
        Self {
            status: Status::Closed,
            menu_tree: widget::Tree::empty(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Status {
    Closed,
    Open { position: Point },
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
        // Intercept right-click on our bounds to toggle the menu.
        let is_right_click = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
        );

        if is_right_click {
            let state = tree.state.downcast_mut::<State>();
            let prev = state.status;

            if let Some(cursor_pos) = cursor.position_over(layout.bounds()) {
                // Open at cursor position (with small offset)
                state.status = Status::Open {
                    position: Point::new(cursor_pos.x + 5.0, cursor_pos.y + 5.0),
                };
                if prev != state.status {
                    shell.request_redraw();
                }
                // Capture the event so the child button doesn't also process it
                shell.capture_event();
                return;
            } else if matches!(prev, Status::Open { .. }) {
                // Click outside while open → close
                state.status = Status::Closed;
                shell.request_redraw();
            }
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
        let base_state = tree.children.first_mut().unwrap();
        let base_overlay =
            self.base
                .as_widget_mut()
                .overlay(base_state, layout, renderer, viewport, translation);

        // Build our overlay if open
        let state = tree.state.downcast_mut::<State>();
        let our_overlay = build_overlay(
            state,
            &mut self.menu,
            &self.entries,
            &self.entry_view,
            translation,
        );

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
    translation: Vector,
) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>>
where
    T: Copy + 'a,
    Message: 'a,
{
    if entries.is_empty() {
        return None;
    }

    match state.status {
        Status::Open { .. } => {
            // Always (re)build the menu Element — it's cheap and ensures
            // the view closure captures fresh data.
            // BUT we must diff it against the *existing* menu_tree so that
            // button widget state (e.g. is_pressed) survives view rebuilds.
            let m = menu.get_or_insert_with(|| build_menu_element(entries, entry_view));
            if state.menu_tree.children.is_empty() {
                // First open: no existing tree, create one fresh
                state.menu_tree = widget::Tree::new(&*m);
            } else {
                // View was rebuilt (menu field reset to None) but tree
                // state persists from the previous frame. Diff preserves
                // widget state like Button::is_pressed across frames.
                state.menu_tree.diff(&*m as &Element<'a, Message>);
            }
        }
        Status::Closed => {
            *menu = None;
            // Reset the tree so next open creates a fresh one
            state.menu_tree = widget::Tree::empty();
            return None;
        }
    }

    if let Status::Open { position } = state.status {
        menu.as_mut().map(|m| {
            overlay::Element::new(Box::new(MenuOverlay {
                menu: m,
                state,
                position: position + translation,
            }))
        })
    } else {
        None
    }
}

// ============================================================================
// Menu Overlay
// ============================================================================

struct MenuOverlay<'a, 'b, Message> {
    menu: &'b mut Element<'a, Message>,
    state: &'b mut State,
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
            self.state.status = Status::Closed;
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        // Click outside menu → close
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. })
        ) && cursor_over.is_none()
        {
            self.state.status = Status::Closed;
            shell.capture_event();
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
            self.state.status = Status::Closed;
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
