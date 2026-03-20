//! Browsing Panel — thin tab container for split-view playlist editing.
//!
//! When playlist edit mode is active, this panel appears alongside the queue as
//! the right pane. It provides a tab bar to switch between library views
//! (Albums, Songs, Artists, Genres) and delegates rendering to the existing
//! page structs — no duplicated slot list/search/sort logic.

use iced::{
    Element, Length,
    widget::{button, container, row, text},
};

use crate::theme;

/// Which library view is active in the browsing panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowsingView {
    Albums,
    Songs,
    Artists,
    Genres,
}

impl BrowsingView {
    /// All available browsing views in tab-bar order.
    pub const ALL: &[BrowsingView] = &[
        BrowsingView::Songs,
        BrowsingView::Albums,
        BrowsingView::Artists,
        BrowsingView::Genres,
    ];

    /// Display label for the tab bar.
    pub fn label(self) -> &'static str {
        match self {
            BrowsingView::Albums => "Albums",
            BrowsingView::Songs => "Songs",
            BrowsingView::Artists => "Artists",
            BrowsingView::Genres => "Genres",
        }
    }
}

/// Browsing panel state — just tracks which tab is active.
///
/// The actual slot list state, search, and sort live in the existing page structs
/// (`AlbumsPage`, `SongsPage`, etc.) on `Nokkvi`.
#[derive(Debug)]
pub struct BrowsingPanel {
    pub active_view: BrowsingView,
}

impl BrowsingPanel {
    pub fn new() -> Self {
        Self {
            active_view: BrowsingView::Songs,
        }
    }

    /// Render the tab bar at the top of the browsing panel.
    ///
    /// Follows the canonical view_header pattern: bg0_soft buttons,
    /// transparent border on idle, accent_bright on hover/active, radius 0.
    pub fn tab_bar(&self) -> Element<'_, BrowsingPanelMessage> {
        let tabs = BrowsingView::ALL.iter().map(|&view| {
            let label = text(view.label())
                .font(iced::font::Font {
                    weight: iced::font::Weight::Medium,
                    ..theme::ui_font()
                })
                .size(12);

            let styled: Element<'_, BrowsingPanelMessage> = if view == self.active_view {
                // Active tab: accent border, bg0_soft background
                button(label)
                    .padding([6, 12])
                    .on_press(BrowsingPanelMessage::SwitchView(view))
                    .style(|_theme, _status| button::Style {
                        background: Some(theme::bg0_soft().into()),
                        text_color: theme::fg0(),
                        border: iced::Border {
                            color: theme::accent_bright(),
                            width: 2.0,
                            radius: theme::ui_border_radius(),
                        },
                        ..Default::default()
                    })
                    .into()
            } else {
                // Inactive tab: transparent border, accent on hover
                button(label)
                    .padding([6, 12])
                    .on_press(BrowsingPanelMessage::SwitchView(view))
                    .style(|_theme, status| button::Style {
                        background: Some(theme::bg0_soft().into()),
                        text_color: theme::fg2(),
                        border: iced::Border {
                            color: if matches!(status, button::Status::Hovered) {
                                theme::accent_bright()
                            } else {
                                iced::Color::TRANSPARENT
                            },
                            width: 2.0,
                            radius: theme::ui_border_radius(),
                        },
                        ..Default::default()
                    })
                    .into()
            };
            styled
        });

        container(row(tabs).spacing(4).padding([6, 8]).width(Length::Fill))
            .style(|_theme| container::Style {
                background: Some(theme::bg0_hard().into()),
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    }
}

/// Messages for the browsing panel.
#[derive(Debug, Clone)]
pub enum BrowsingPanelMessage {
    /// Switch the active tab in the browsing panel.
    SwitchView(BrowsingView),
}
