//! Browsing Panel — thin tab container for split-view playlist editing.
//!
//! When playlist edit mode is active, this panel appears alongside the queue as
//! the right pane. It provides a tab bar to switch between library views
//! (Albums, Songs, Artists, Genres) and delegates rendering to the existing
//! page structs — no duplicated slot list/search/sort logic.

use iced::{
    Element, Length,
    widget::{container, mouse_area, row, text},
};

use crate::{theme, widgets::hover_overlay::HoverOverlay};

/// Which library view is active in the browsing panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowsingView {
    Albums,
    Songs,
    Artists,
    Genres,
    Similar,
}

impl BrowsingView {
    /// All available browsing views in tab-bar order.
    pub const ALL: &[BrowsingView] = &[
        BrowsingView::Songs,
        BrowsingView::Albums,
        BrowsingView::Artists,
        BrowsingView::Genres,
        BrowsingView::Similar,
    ];

    /// Display label for the tab bar.
    pub fn label(self) -> &'static str {
        match self {
            BrowsingView::Albums => "Albums",
            BrowsingView::Songs => "Songs",
            BrowsingView::Artists => "Artists",
            BrowsingView::Genres => "Genres",
            BrowsingView::Similar => "Similar",
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
    /// Uses mouse_area + HoverOverlay(container) so HoverOverlay sees mouse
    /// events directly — native button captures ButtonPressed which prevents
    /// HoverOverlay's passive press tracker from firing (no scale effect).
    pub fn tab_bar(&self, similar_label: Option<&str>) -> Element<'_, BrowsingPanelMessage> {
        let tabs = BrowsingView::ALL.iter().map(|&view| {
            let mut label_str = view.label();
            if view == BrowsingView::Similar {
                if let Some(lbl) = similar_label {
                    if lbl.starts_with("Top Songs") {
                        label_str = "Top Songs";
                    }
                }
            }

            let label = text(label_str)
                .font(iced::font::Font {
                    weight: iced::font::Weight::Medium,
                    ..theme::ui_font()
                })
                .size(12)
                .color(if view == self.active_view {
                    theme::fg0()
                } else {
                    theme::fg2()
                });

            // Active tab keeps the accent border as a selection indicator.
            // Both active and inactive use mouse_area + HoverOverlay(container)
            // so the press scale effect actually fires.
            let border_color = if view == self.active_view {
                theme::accent_bright()
            } else {
                iced::Color::TRANSPARENT
            };

            let tab: Element<'_, BrowsingPanelMessage> = mouse_area(
                HoverOverlay::new(container(label).padding([6, 12]).style(move |_theme| {
                    container::Style {
                        background: Some(theme::bg0_soft().into()),
                        border: iced::Border {
                            color: border_color,
                            width: 2.0,
                            radius: theme::ui_border_radius(),
                        },
                        ..Default::default()
                    }
                }))
                .border_radius(theme::ui_border_radius()),
            )
            .on_press(BrowsingPanelMessage::SwitchView(view))
            .interaction(iced::mouse::Interaction::Pointer)
            .into();

            tab
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
