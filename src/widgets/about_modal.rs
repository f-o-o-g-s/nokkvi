//! About Modal Widget
//!
//! A modal overlay displaying application version, commit, and connection info.
//! Triggered from the hamburger menu and rendered as a Stack overlay in app_view.
//!
//! Layout matches the existing `info_modal.rs` backdrop + dialog pattern:
//!   - Semi-transparent backdrop (click to close)
//!   - Centered dialog box with accent border
//!   - App name, version, commit, server info
//!   - "Copy All" and "Close" buttons

use iced::{
    Alignment, Element, Length,
    widget::{column, container, mouse_area, row, space, svg, text},
};

use crate::{
    theme,
    widgets::{
        hover_overlay::HoverOverlay,
        sizes::{MODAL_ICON_BUTTON_SIZE, MODAL_ICON_SIZE_LARGE, MODAL_ICON_SIZE_SMALL},
    },
};

// =============================================================================
// State & Messages
// =============================================================================

/// State for the about modal overlay.
#[derive(Debug, Default)]
pub struct AboutModalState {
    pub visible: bool,
}

impl AboutModalState {
    /// Open the about modal.
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// Close the about modal.
    pub fn close(&mut self) {
        self.visible = false;
    }
}

/// Messages emitted by the about modal.
#[derive(Debug, Clone)]
pub enum AboutModalMessage {
    /// Open the modal
    Open,
    /// User closed the modal (Escape, X button, or backdrop click)
    Close,
    /// Copy all info to clipboard
    CopyAll,
    /// Open the Ko-fi tip page in the user's browser
    OpenKofi,
}

// =============================================================================
// View Data
// =============================================================================

/// Data passed from the app to the about modal view (borrowed, not cloned).
pub(crate) struct AboutViewData<'a> {
    /// Connected Navidrome server URL (empty if not connected)
    pub server_url: &'a str,
    /// Connected username (empty if not connected)
    pub username: &'a str,
    /// Connected Navidrome server version (None if not fetched or failed)
    pub server_version: Option<&'a str>,
}

// =============================================================================
// View
// =============================================================================

/// Modal dialog width (narrower than the info modal since content is simpler)
const MODAL_WIDTH: f32 = 400.0;

/// Logo icon size in the modal
const LOGO_ICON_SIZE: f32 = 96.0;

/// Render the about modal overlay. Returns `None` if not visible.
pub(crate) fn about_modal_overlay<'a>(
    state: &'a AboutModalState,
    data: AboutViewData<'a>,
) -> Option<Element<'a, AboutModalMessage>> {
    if !state.visible {
        return None;
    }

    let version = env!("CARGO_PKG_VERSION");
    let git_hash = option_env!("GIT_HASH").unwrap_or_default();

    // ── Header: [Nokkvi  ·····  📋  X] ──────────────────────────
    let title_text = text("Nokkvi")
        .size(20.0)
        .font(iced::font::Font {
            weight: iced::font::Weight::Bold,
            ..theme::ui_font()
        })
        .color(theme::accent_bright());

    let close_button: Element<'_, AboutModalMessage> = modal_icon_button(
        "assets/icons/x.svg",
        MODAL_ICON_SIZE_LARGE,
        AboutModalMessage::Close,
    );

    let copy_button: Element<'_, AboutModalMessage> = modal_icon_button(
        "assets/icons/copy.svg",
        MODAL_ICON_SIZE_SMALL,
        AboutModalMessage::CopyAll,
    );

    let etymology = text("Old Norse nökkvi: a small, humble boat")
        .size(12.0)
        .font(iced::font::Font {
            style: iced::font::Style::Italic,
            ..theme::ui_font()
        })
        .color(theme::fg4());

    let title_col = column![title_text, etymology].spacing(2);

    let header = row![title_col, space::horizontal(), copy_button, close_button]
        .spacing(8)
        .align_y(Alignment::Center);

    let header_sep = theme::modal_header_separator();

    // ── Boat icon (theme-adaptive multi-color SVG) ──────────────
    let logo_svg = crate::embedded_svg::themed_logo_svg();
    let logo_handle = svg::Handle::from_memory(logo_svg.into_bytes());
    let boat_icon = container(
        svg(logo_handle)
            .width(LOGO_ICON_SIZE)
            .height(LOGO_ICON_SIZE),
    )
    .width(Length::Fill)
    .align_x(Alignment::Center);

    let tagline = text("A sturdy hull for the endless stream.")
        .size(14.0)
        .font(iced::font::Font {
            weight: iced::font::Weight::Bold,
            ..theme::ui_font()
        })
        .color(theme::fg1());

    let description = text("A Rust/Iced desktop client for Navidrome music servers")
        .size(12.0)
        .font(theme::ui_font())
        .color(theme::fg3());

    // ── Info rows ────────────────────────────────────────────────
    let mut rows: Vec<Element<'_, AboutModalMessage>> = vec![
        info_row("Captain", "foogs"),
        theme::modal_row_separator(),
        info_row("Shipwrights", "Claude Opus 4.7"),
        theme::modal_row_separator(),
        info_row("Version", version),
        theme::modal_row_separator(),
    ];

    if !git_hash.is_empty() {
        rows.push(info_row("Commit", git_hash));
        rows.push(theme::modal_row_separator());
    }

    if !data.server_url.is_empty() {
        rows.push(info_row("Server", data.server_url));
        rows.push(theme::modal_row_separator());
    }

    if !data.username.is_empty() {
        rows.push(info_row("User", data.username));
        rows.push(theme::modal_row_separator());
    }

    if let Some(sv) = data.server_version {
        rows.push(info_row("Navidrome", sv));
        rows.push(theme::modal_row_separator());
    }

    // GPU backend info (via iced's renderer)
    rows.push(info_row("Toolkit", "Iced (wgpu)"));

    let info_table = column(rows).width(Length::Fill);

    // ── Ko-fi support link ──────────────────────────────────────
    let kofi_row: Element<'_, AboutModalMessage> = mouse_area(
        HoverOverlay::new(
            container(
                row![
                    crate::embedded_svg::svg_widget("assets/icons/heart-filled.svg")
                        .width(Length::Fixed(14.0))
                        .height(Length::Fixed(14.0))
                        .style(|_theme, _status| svg::Style {
                            color: Some(theme::accent_bright()),
                        }),
                    text("Buy foogs a coffee on Ko-fi")
                        .size(13.0)
                        .font(theme::ui_font())
                        .color(theme::fg2()),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding(iced::Padding {
                top: 6.0,
                bottom: 6.0,
                left: 10.0,
                right: 10.0,
            })
            .style(|_theme| container::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            }),
        )
        .border_radius(theme::ui_border_radius()),
    )
    .on_press(AboutModalMessage::OpenKofi)
    .interaction(iced::mouse::Interaction::Pointer)
    .into();

    let kofi_container = container(kofi_row)
        .width(Length::Fill)
        .align_x(Alignment::Center);

    // ── Dialog content ───────────────────────────────────────────
    let content = column![
        header,
        header_sep,
        boat_icon,
        tagline,
        description,
        info_table,
        kofi_container
    ]
    .spacing(10)
    .padding(20)
    .width(Length::Fixed(MODAL_WIDTH));

    // ── Dialog box with themed border ────────────────────────────
    // Flat redesign: bg0_hard() (matches design's --bg-dim), 1px accent
    // outline, larger `lg` radius in rounded mode so modal frames read as
    // major surfaces rather than chips.
    let dialog_box = container(content)
        .style(|_theme| container::Style {
            background: Some(theme::bg0_hard().into()),
            border: iced::Border {
                color: theme::accent_bright(),
                width: 1.0,
                radius: theme::ui_radius_lg(),
            },
            ..Default::default()
        })
        .width(Length::Shrink);

    // ── Backdrop + opaque wrapper (prevents click-through) ───────
    Some(theme::modal_scaffold(
        dialog_box.into(),
        AboutModalMessage::Close,
        theme::MODAL_BACKDROP_ALPHA,
    ))
}

// =============================================================================
// Helpers
// =============================================================================

/// Fixed width for the label column in pixels
const LABEL_COL_WIDTH: f32 = 96.0;
const LABEL_SIZE: f32 = 12.0;
const VALUE_SIZE: f32 = 13.0;

/// A single label: value row.
fn info_row<'a>(label: &'a str, value: &'a str) -> Element<'a, AboutModalMessage> {
    row![
        container(
            text(label)
                .size(LABEL_SIZE)
                .font(theme::ui_font())
                .color(theme::fg4()),
        )
        .width(Length::Fixed(LABEL_COL_WIDTH))
        .padding(iced::Padding {
            top: 6.0,
            bottom: 6.0,
            left: 0.0,
            right: 8.0,
        }),
        text(value)
            .size(VALUE_SIZE)
            .font(theme::ui_font())
            .color(theme::fg1())
            .wrapping(iced::widget::text::Wrapping::None)
            .ellipsis(iced::widget::text::Ellipsis::End),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// Borderless icon-only button that uses the canonical
/// `mouse_area(HoverOverlay(container(svg(...))))` chrome.
///
/// Wraps the `MODAL_ICON_BUTTON_SIZE` × `MODAL_ICON_BUTTON_SIZE` chassis
/// previously open-coded for the close/copy buttons. `icon_size` lets
/// callers distinguish the visually-dominant close (X) glyph from the
/// secondary copy glyph by inner SVG size.
fn modal_icon_button<'a>(
    icon_path: &'static str,
    icon_size: f32,
    on_press: AboutModalMessage,
) -> Element<'a, AboutModalMessage> {
    mouse_area(
        HoverOverlay::new(
            container(
                crate::embedded_svg::svg_widget(icon_path)
                    .width(Length::Fixed(icon_size))
                    .height(Length::Fixed(icon_size))
                    .style(|_theme, _status| svg::Style {
                        color: Some(theme::fg3()),
                    }),
            )
            .width(Length::Fixed(MODAL_ICON_BUTTON_SIZE))
            .height(Length::Fixed(MODAL_ICON_BUTTON_SIZE))
            .style(|_theme| container::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .center(Length::Fixed(MODAL_ICON_BUTTON_SIZE)),
        )
        .border_radius(theme::ui_border_radius()),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}
