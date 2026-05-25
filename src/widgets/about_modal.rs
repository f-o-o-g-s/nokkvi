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

/// Build the `(label, value)` rows shown in the About modal's info table.
///
/// Shared by the visual renderer (`about_modal_overlay`) and the Copy All
/// clipboard handler so the two paths can't drift — every row visible
/// on-screen lands in the clipboard, and vice versa. Previously the copy
/// handler open-coded a 6-line subset that dropped Captain + Shipwrights
/// and ordered User/Navidrome opposite to the on-screen layout.
pub(crate) fn build_about_rows(data: &AboutViewData<'_>) -> Vec<(&'static str, String)> {
    let version = env!("CARGO_PKG_VERSION");
    let git_hash = option_env!("GIT_HASH").unwrap_or_default();

    let mut rows: Vec<(&'static str, String)> = vec![
        ("Captain", "foogs".to_string()),
        ("Shipwrights", "Claude Opus 4.7".to_string()),
        ("Version", version.to_string()),
    ];
    if !git_hash.is_empty() {
        rows.push(("Commit", git_hash.to_string()));
    }
    if !data.server_url.is_empty() {
        rows.push(("Server", data.server_url.to_string()));
    }
    if !data.username.is_empty() {
        rows.push(("User", data.username.to_string()));
    }
    if let Some(sv) = data.server_version {
        rows.push(("Navidrome", sv.to_string()));
    }
    rows.push(("Toolkit", "Iced (wgpu)".to_string()));
    rows
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
    // Built from `build_about_rows` so the on-screen rows and the
    // clipboard-copy lines stay in lockstep.
    let entries = build_about_rows(&data);
    let mut rows: Vec<Element<'_, AboutModalMessage>> = Vec::with_capacity(entries.len() * 2);
    for (i, (label, value)) in entries.iter().enumerate() {
        rows.push(info_row(label, value));
        if i + 1 < entries.len() {
            rows.push(theme::modal_row_separator());
        }
    }
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

    // Shared modal frame: bg0_hard fill + 1 px accent_bright outline +
    // ui_radius_lg corners. Five overlay modals route through this helper.
    let dialog_box = container(content)
        .style(theme::modal_frame_style)
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
fn info_row<'a>(label: &str, value: &str) -> Element<'a, AboutModalMessage> {
    // Take owned `String`s into the `text` widgets so the returned
    // `Element` doesn't borrow back into a temporary `Vec` built by
    // `build_about_rows` — the helper's rows live only as long as the
    // surrounding `view()` call, so borrowing would dangle.
    row![
        container(
            text(label.to_string())
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
        text(value.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn anon_data() -> AboutViewData<'static> {
        AboutViewData {
            server_url: "",
            username: "",
            server_version: None,
        }
    }

    #[test]
    fn build_rows_preserves_attribution_when_disconnected() {
        // Captain + Shipwrights must remain in the rows even when the
        // user isn't logged in. They're the regression sentinel for the
        // Copy All attribution bug.
        let rows = build_about_rows(&anon_data());
        let labels: Vec<&str> = rows.iter().map(|(l, _)| *l).collect();
        assert!(
            labels.contains(&"Captain"),
            "Captain row missing: {labels:?}",
        );
        assert!(
            labels.contains(&"Shipwrights"),
            "Shipwrights row missing: {labels:?}",
        );
        // Toolkit is the final row regardless of connection state.
        assert_eq!(labels.last(), Some(&"Toolkit"));
    }

    #[test]
    fn build_rows_includes_connection_metadata_when_present() {
        let data = AboutViewData {
            server_url: "https://example.test",
            username: "alice",
            server_version: Some("0.55.2"),
        };
        let labels: Vec<&str> = build_about_rows(&data).iter().map(|(l, _)| *l).collect();
        // Ordering on-screen: Captain, Shipwrights, Version, [Commit,]
        // Server, User, Navidrome, Toolkit — User must come before
        // Navidrome (previously the copy handler swapped them).
        let user_pos = labels.iter().position(|l| *l == "User").expect("User row");
        let nav_pos = labels
            .iter()
            .position(|l| *l == "Navidrome")
            .expect("Navidrome row");
        assert!(
            user_pos < nav_pos,
            "User row must appear before Navidrome in the on-screen order; got {labels:?}",
        );
    }
}
