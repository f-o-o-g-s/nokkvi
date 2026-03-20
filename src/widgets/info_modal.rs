//! Info Modal Widget
//!
//! A reusable overlay modal that displays item properties in a Feishin-style
//! two-column table. Used for the "Get Info" context menu action.
//!
//! Layout:
//!   - Left column: muted `text` label (fixed width)
//!   - Right column: read-only `text_editor` — borderless, transparent background,
//!     allows free-form mouse selection and Ctrl+C within each cell.
//!   - Thin horizontal separator between rows (like Feishin's `withRowBorders`)
//!   - Multi-line values (Comment, Path, etc.) expand their row naturally.
//!   - Mutation actions are silently discarded to keep text read-only.
//!   - Hovering a row reveals a per-row copy button that copies "Label: Value".

use std::collections::HashSet;

use iced::{
    Alignment, Element, Length,
    widget::{
        button, column, container, mouse_area, opaque, row, scrollable, space, svg, text,
        text_editor,
    },
};
use nokkvi_data::types::info_modal::InfoModalItem;

use crate::theme;

// =============================================================================
// State & Messages
// =============================================================================

/// Values longer than this (in chars) get truncated and a chevron toggle.
const LONG_VALUE_THRESHOLD: usize = 120;
/// How many chars to show when a long value is collapsed.
const TRUNCATED_LENGTH: usize = 120;

/// State for the info modal overlay.
#[derive(Debug, Default)]
pub struct InfoModalState {
    pub visible: bool,
    pub item: Option<InfoModalItem>,
    /// Cached property rows (label, value), computed once when the modal opens.
    pub cached_properties: Vec<(String, String)>,
    /// One text_editor::Content per property row (for the value column).
    /// Content reflects current expanded/collapsed state.
    pub value_editors: Vec<text_editor::Content>,
    /// Which rows have values exceeding LONG_VALUE_THRESHOLD.
    pub long_rows: Vec<bool>,
    /// Rows the user has explicitly expanded.
    pub expanded_rows: HashSet<usize>,
    /// Which row the mouse is currently hovering over (for the per-row copy button).
    pub hovered_row: Option<usize>,
}

impl InfoModalState {
    /// Open the modal with the given item.
    pub fn open(&mut self, item: InfoModalItem) {
        self.cached_properties = item.properties();
        self.expanded_rows.clear();
        self.long_rows = self
            .cached_properties
            .iter()
            .map(|(_, v)| v.len() > LONG_VALUE_THRESHOLD || v.contains('\n'))
            .collect();
        self.value_editors = self
            .cached_properties
            .iter()
            .enumerate()
            .map(|(i, (_, value))| {
                let text = if self.long_rows[i] {
                    truncate_to_word_boundary(value, TRUNCATED_LENGTH).to_string() + "…"
                } else {
                    value.clone()
                };
                text_editor::Content::with_text(&text)
            })
            .collect();
        self.visible = true;
        self.item = Some(item);
    }

    /// Toggle expand/collapse for a single row.
    pub fn toggle_row(&mut self, idx: usize) {
        if self.expanded_rows.contains(&idx) {
            self.expanded_rows.remove(&idx);
        } else {
            self.expanded_rows.insert(idx);
        }
        // Rebuild just this row's editor content.
        if let Some((_, value)) = self.cached_properties.get(idx) {
            let is_expanded = self.expanded_rows.contains(&idx);
            let text = if !is_expanded {
                truncate_to_word_boundary(value, TRUNCATED_LENGTH).to_string() + "…"
            } else {
                value.clone()
            };
            if let Some(editor) = self.value_editors.get_mut(idx) {
                *editor = text_editor::Content::with_text(&text);
            }
        }
    }

    /// Close and reset the modal.
    pub fn close(&mut self) {
        self.visible = false;
        self.item = None;
        self.cached_properties.clear();
        self.value_editors.clear();
        self.long_rows.clear();
        self.expanded_rows.clear();
        self.hovered_row = None;
    }
}

/// Truncate a string to at most `max_chars` characters, breaking at a word boundary.
fn truncate_to_word_boundary(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }
    let bytes = &s.as_bytes()[..max_chars.min(s.len())];
    if let Some(pos) = bytes.iter().rposition(|&b| b == b' ') {
        &s[..pos]
    } else {
        let mut end = max_chars.min(s.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Messages emitted by the info modal.
#[derive(Debug, Clone)]
pub enum InfoModalMessage {
    /// Open the modal with a given item
    Open(Box<InfoModalItem>),
    /// User closed the modal (Escape, X button, or backdrop click)
    Close,
    /// Copy all properties to clipboard
    CopyAll,
    /// Mouse entered/left a property row — drives the per-row copy button.
    RowHovered(Option<usize>),
    /// Copy a single row's "Label: Value" to the clipboard.
    CopyRow(usize),
    /// Toggle expand/collapse for a long-value row.
    ToggleRowExpanded(usize),
    /// Open a URL in the system browser.
    OpenUrl(String),
    /// Open the item's containing folder in the file manager.
    OpenFolder(String),
    /// A text_editor action for a specific value row (index, action).
    EditorAction(usize, text_editor::Action),
}

// =============================================================================
// Helpers
// =============================================================================

/// Returns `true` if the value looks like a standalone URL.
/// Used to decide whether to render the value as a clickable link.
fn is_url(value: &str) -> bool {
    let v = value.trim();
    (v.starts_with("http://") || v.starts_with("https://")) && !v.contains('\n') && !v.contains(' ')
}

// =============================================================================
// View
// =============================================================================

/// Modal dialog width
const MODAL_WIDTH: f32 = 760.0;
/// Maximum modal content height before scrolling
const MODAL_MAX_HEIGHT: f32 = 620.0;
/// Font sizes
const TITLE_SIZE: f32 = 16.0;
const LABEL_SIZE: f32 = 12.0;
const VALUE_SIZE: f32 = 13.0;
/// Fixed width for the label column in pixels
const LABEL_COL_WIDTH: f32 = 140.0;

/// Render the info modal overlay. Returns `None` if not visible.
pub(crate) fn info_modal_overlay<'a>(
    state: &'a InfoModalState,
) -> Option<Element<'a, InfoModalMessage>> {
    if !state.visible {
        return None;
    }

    let item = state.item.as_ref()?;

    // ── Header: [Title  Type  ·····  📋  X] ─────────────────────
    let title_text = text(item.title())
        .size(TITLE_SIZE)
        .font(theme::ui_font())
        .color(theme::fg0());

    let type_label = match item {
        InfoModalItem::Song { .. } => "Song",
        InfoModalItem::Album { .. } => "Album",
        InfoModalItem::Artist { .. } => "Artist",
        InfoModalItem::Playlist { .. } => "Playlist",
    };
    let type_badge = text(type_label)
        .size(11)
        .font(theme::ui_font())
        .color(theme::fg4());

    let close_button = button(
        crate::embedded_svg::svg_widget("assets/icons/x.svg")
            .width(16)
            .height(16)
            .style(|_theme, _status| svg::Style {
                color: Some(theme::fg3()),
            }),
    )
    .on_press(InfoModalMessage::Close)
    .padding(iced::Padding {
        top: 2.0,
        bottom: 2.0,
        left: 6.0,
        right: 6.0,
    })
    .style(|_theme, _status| button::Style {
        background: None,
        border: iced::Border::default(),
        ..Default::default()
    });

    let copy_button = button(
        crate::embedded_svg::svg_widget("assets/icons/copy.svg")
            .width(14)
            .height(14)
            .style(|_theme, _status| svg::Style {
                color: Some(theme::fg3()),
            }),
    )
    .on_press(InfoModalMessage::CopyAll)
    .padding(iced::Padding {
        top: 2.0,
        bottom: 2.0,
        left: 6.0,
        right: 6.0,
    })
    .style(|_theme, _status| button::Style {
        background: None,
        border: iced::Border::default(),
        ..Default::default()
    });

    // Only show folder button when we have a resolvable local path
    let folder_path = item.folder_path();
    let header = if let Some(ref fp) = folder_path {
        let folder_path_msg = fp.clone();
        let folder_button = button(
            crate::embedded_svg::svg_widget("assets/icons/folder-open.svg")
                .width(14)
                .height(14)
                .style(|_theme, _status| svg::Style {
                    color: Some(theme::fg3()),
                }),
        )
        .on_press(InfoModalMessage::OpenFolder(folder_path_msg))
        .padding(iced::Padding {
            top: 2.0,
            bottom: 2.0,
            left: 6.0,
            right: 6.0,
        })
        .style(|_theme, _status| button::Style {
            background: None,
            border: iced::Border::default(),
            ..Default::default()
        });
        row![
            title_text,
            type_badge,
            space::horizontal(),
            folder_button,
            copy_button,
            close_button
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    } else {
        row![
            title_text,
            type_badge,
            space::horizontal(),
            copy_button,
            close_button
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };

    let header_sep = separator_line();

    // ── Property table rows ──────────────────────────────────────
    let mut rows: Vec<Element<'_, InfoModalMessage>> = Vec::new();

    let n = state.cached_properties.len();
    for (idx, (label, _)) in state.cached_properties.iter().enumerate() {
        let Some(content) = state.value_editors.get(idx) else {
            continue;
        };

        let is_long = state.long_rows.get(idx).copied().unwrap_or(false);
        let is_expanded = state.expanded_rows.contains(&idx);

        // Plain label cell — same for all rows
        let label_cell = container(
            text(label.as_str())
                .size(LABEL_SIZE)
                .font(theme::ui_font())
                .color(theme::fg4()),
        )
        .width(Length::Fixed(LABEL_COL_WIDTH))
        .align_y(Alignment::Start)
        .padding(iced::Padding {
            top: 4.0,
            bottom: 4.0,
            left: 0.0,
            right: 8.0,
        });

        // Value cell — URL → clickable link; long → editor + spoiler; else → plain editor
        let value = state
            .cached_properties
            .get(idx)
            .map_or("", |(_, v)| v.as_str());

        let value_cell: Element<'_, InfoModalMessage> = if is_url(value) {
            let url = value.trim().to_string();
            let url_copy = url.clone();
            button(
                text(url)
                    .size(VALUE_SIZE)
                    .font(theme::ui_font())
                    .color(theme::accent_bright()),
            )
            .on_press(InfoModalMessage::OpenUrl(url_copy))
            .padding(iced::Padding {
                top: 4.0,
                bottom: 4.0,
                left: 0.0,
                right: 0.0,
            })
            .style(|_theme, status| {
                let opacity = match status {
                    button::Status::Hovered | button::Status::Pressed => 0.75,
                    _ => 1.0,
                };
                let mut color = theme::accent_bright();
                color.a = opacity;
                button::Style {
                    background: None,
                    border: iced::Border {
                        // Underline-like bottom border
                        width: 0.0,
                        ..Default::default()
                    },
                    text_color: color,
                    ..Default::default()
                }
            })
            .into()
        } else if is_long {
            let chevron_icon = if is_expanded {
                "assets/icons/chevron-up.svg"
            } else {
                "assets/icons/chevron-down.svg"
            };
            let spoiler_btn = button(
                row![
                    crate::embedded_svg::svg_widget(chevron_icon)
                        .width(11)
                        .height(11)
                        .style(|_theme, _status| svg::Style {
                            color: Some(theme::accent_bright()),
                        }),
                    text(if is_expanded {
                        "Show less"
                    } else {
                        "Show more"
                    })
                    .size(11)
                    .font(theme::ui_font())
                    .color(theme::accent_bright()),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .on_press(InfoModalMessage::ToggleRowExpanded(idx))
            .padding(iced::Padding {
                top: 2.0,
                bottom: 4.0,
                left: 0.0,
                right: 0.0,
            })
            .style(|_theme, _status| button::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            });

            // Value cell editor (defined before value_cell branch to avoid borrow issues)
            let editor = text_editor(content)
                .on_action(move |action| InfoModalMessage::EditorAction(idx, action))
                .size(VALUE_SIZE)
                .font(theme::ui_font())
                .height(Length::Shrink)
                .padding(iced::Padding {
                    top: 4.0,
                    bottom: 4.0,
                    left: 0.0,
                    right: 0.0,
                })
                .style(|_theme, _status| text_editor::Style {
                    background: iced::Background::Color(iced::Color::TRANSPARENT),
                    border: iced::Border {
                        width: 0.0,
                        radius: theme::ui_border_radius(),
                        color: iced::Color::TRANSPARENT,
                    },
                    placeholder: theme::fg4(),
                    value: theme::fg1(),
                    selection: {
                        let mut c = theme::accent_bright();
                        c.a = 0.3;
                        c
                    },
                });

            column![editor, spoiler_btn].into()
        } else {
            let editor = text_editor(content)
                .on_action(move |action| InfoModalMessage::EditorAction(idx, action))
                .size(VALUE_SIZE)
                .font(theme::ui_font())
                .height(Length::Shrink)
                .padding(iced::Padding {
                    top: 4.0,
                    bottom: 4.0,
                    left: 0.0,
                    right: 0.0,
                })
                .style(|_theme, _status| text_editor::Style {
                    background: iced::Background::Color(iced::Color::TRANSPARENT),
                    border: iced::Border {
                        width: 0.0,
                        radius: theme::ui_border_radius(),
                        color: iced::Color::TRANSPARENT,
                    },
                    placeholder: theme::fg4(),
                    value: theme::fg1(),
                    selection: {
                        let mut c = theme::accent_bright();
                        c.a = 0.3;
                        c
                    },
                });
            editor.into()
        };

        let property_row = {
            // Fixed-width copy button slot — copy icon when hovered, empty space otherwise.
            // Using a fixed-width container keeps the value column width stable.
            let copy_slot: Element<'_, InfoModalMessage> = if state.hovered_row == Some(idx) {
                button(
                    crate::embedded_svg::svg_widget("assets/icons/copy.svg")
                        .width(12)
                        .height(12)
                        .style(|_theme, _status| svg::Style {
                            color: Some(theme::fg3()),
                        }),
                )
                .on_press(InfoModalMessage::CopyRow(idx))
                .padding(iced::Padding {
                    top: 4.0,
                    bottom: 4.0,
                    left: 4.0,
                    right: 2.0,
                })
                .style(|_theme, _status| button::Style {
                    background: None,
                    border: iced::Border::default(),
                    ..Default::default()
                })
                .into()
            } else {
                container(space::horizontal())
                    .width(Length::Fixed(26.0))
                    .into()
            };

            let inner = row![label_cell, value_cell, copy_slot]
                .width(Length::Fill)
                .align_y(Alignment::Start);

            mouse_area(inner)
                .on_enter(InfoModalMessage::RowHovered(Some(idx)))
                .on_exit(InfoModalMessage::RowHovered(None))
        };

        rows.push(property_row.into());

        // Row separator (skip after last row)
        if idx + 1 < n {
            rows.push(row_separator());
        }
    }

    let table = column(rows).width(Length::Fill).padding(iced::Padding {
        right: 16.0,
        ..Default::default()
    });

    // ── Scrollable table ─────────────────────────────────────────
    let scrollable_table = scrollable(table)
        .height(Length::Shrink)
        .style(|_theme, status| {
            let rail = scrollable::Rail {
                background: Some(theme::bg2().into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                scroller: scrollable::Scroller {
                    background: theme::fg4().into(),
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                },
            };

            let hovered_rail = scrollable::Rail {
                scroller: scrollable::Scroller {
                    background: theme::accent_bright().into(),
                    ..rail.scroller
                },
                ..rail
            };

            match status {
                scrollable::Status::Active { .. } => scrollable::Style {
                    container: container::Style::default(),
                    vertical_rail: rail,
                    horizontal_rail: rail,
                    gap: None,
                    auto_scroll: scrollable::AutoScroll {
                        background: iced::Color::TRANSPARENT.into(),
                        border: Default::default(),
                        shadow: Default::default(),
                        icon: iced::Color::TRANSPARENT,
                    },
                },
                scrollable::Status::Hovered {
                    is_vertical_scrollbar_hovered,
                    is_horizontal_scrollbar_hovered,
                    ..
                } => scrollable::Style {
                    container: container::Style::default(),
                    vertical_rail: if is_vertical_scrollbar_hovered {
                        hovered_rail
                    } else {
                        rail
                    },
                    horizontal_rail: if is_horizontal_scrollbar_hovered {
                        hovered_rail
                    } else {
                        rail
                    },
                    gap: None,
                    auto_scroll: scrollable::AutoScroll {
                        background: iced::Color::TRANSPARENT.into(),
                        border: Default::default(),
                        shadow: Default::default(),
                        icon: iced::Color::TRANSPARENT,
                    },
                },
                scrollable::Status::Dragged {
                    is_vertical_scrollbar_dragged,
                    is_horizontal_scrollbar_dragged,
                    ..
                } => scrollable::Style {
                    container: container::Style::default(),
                    vertical_rail: if is_vertical_scrollbar_dragged {
                        hovered_rail
                    } else {
                        rail
                    },
                    horizontal_rail: if is_horizontal_scrollbar_dragged {
                        hovered_rail
                    } else {
                        rail
                    },
                    gap: None,
                    auto_scroll: scrollable::AutoScroll {
                        background: iced::Color::TRANSPARENT.into(),
                        border: Default::default(),
                        shadow: Default::default(),
                        icon: iced::Color::TRANSPARENT,
                    },
                },
            }
        });

    // ── Dialog content ───────────────────────────────────────────
    let content = column![header, header_sep, scrollable_table]
        .spacing(10)
        .padding(20)
        .width(Length::Fixed(MODAL_WIDTH));

    // ── Dialog box with themed border ────────────────────────────
    let dialog_box = container(content)
        .style(|_theme| container::Style {
            background: Some(theme::bg1().into()),
            border: iced::Border {
                color: theme::accent_bright(),
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        })
        .max_height(MODAL_MAX_HEIGHT)
        .width(Length::Shrink);

    // ── Backdrop + opaque wrapper (prevents click-through) ───────
    let backdrop = mouse_area(
        container(opaque(dialog_box))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(|_theme| {
                let mut bg = theme::bg0_hard();
                bg.a = 0.6;
                container::Style {
                    background: Some(bg.into()),
                    ..Default::default()
                }
            }),
    )
    .on_press(InfoModalMessage::Close);

    Some(opaque(backdrop))
}

// =============================================================================
// Helpers
// =============================================================================

/// A subtle horizontal separator line between table rows.
fn row_separator<'a>() -> Element<'a, InfoModalMessage> {
    container(space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme| {
            let mut c = theme::fg4();
            c.a = 0.12;
            container::Style {
                background: Some(c.into()),
                ..Default::default()
            }
        })
        .into()
}

/// A more prominent separator (used under the header).
fn separator_line<'a>() -> Element<'a, InfoModalMessage> {
    container(space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme| {
            let mut c = theme::fg4();
            c.a = 0.2;
            container::Style {
                background: Some(c.into()),
                ..Default::default()
            }
        })
        .into()
}
