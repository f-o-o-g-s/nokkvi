//! Shared edit-bar cover thumbnail for the playlist editor.
//!
//! Both the regular (Tracks) and smart (Rules) edit bars render the same
//! clickable cover: the uploaded custom art, else a 2×2 quad of the playlist's
//! album covers, else a placeholder glyph. Generic over the message type so
//! each bar wires its own Set/Reset variants (Tracks → `EditorMessage`, Rules →
//! `RulesEditorMessage`) — the actual upload/delete handlers are shared.

use std::collections::HashMap;

use iced::{
    Alignment, ContentFit, Element, Length,
    widget::{column, container, image, mouse_area, text},
};

use crate::{theme, widgets::hover_overlay::HoverOverlay};

/// Cover inputs for the edit-bar thumbnail.
pub(crate) struct EditorCover<'a> {
    /// The uploaded custom cover, when set AND its handle is warm — takes
    /// precedence over the derived quad.
    pub custom: Option<&'a image::Handle>,
    /// Album ids feeding the 2×2 quad fallback (the playlist's frozen artwork
    /// ids, or a live set derived from the working buffer/preview).
    pub album_ids: Vec<String>,
    /// The 80px album-id-keyed art snapshot.
    pub album_art: &'a HashMap<String, image::Handle>,
    /// Whether Set/Reset are live (an edit session against a saved playlist);
    /// an unsaved create session shows the quad but can't upload yet.
    pub editable: bool,
}

/// The clickable edit-bar cover. `on_set` fires on click (upload custom art);
/// `on_reset` fires from the small Reset affordance shown over a custom cover.
pub(crate) fn cover_thumbnail<'a, M: Clone + 'a>(
    cover: &EditorCover<'a>,
    on_set: M,
    on_reset: M,
) -> Element<'a, M> {
    // A hard, fixed SQUARE — the edit-bar's height budget. Everything inside
    // fills THIS box (never the row), and overflow clips, so a non-square
    // custom cover or an oversize quad can't stretch the layout.
    const SIZE: f32 = 44.0;
    let quad =
        crate::services::collage_artwork::resolve_quad_handles(&cover.album_ids, cover.album_art);
    let is_placeholder = cover.custom.is_none() && quad.is_none();

    let art: Element<'a, M> = if let Some(handle) = cover.custom {
        image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .into()
    } else if let Some(tiles) = quad {
        crate::widgets::slot_list::slot_list_artwork_quad_column(&tiles, SIZE, true, false, 1.0)
    } else {
        crate::embedded_svg::svg_widget("assets/icons/disc-3.svg")
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0))
            .style(|_t, _s| iced::widget::svg::Style {
                color: Some(theme::fg4()),
            })
            .into()
    };

    let framed = container(iced::widget::stack![
        container(art)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill),
        // The pencil marks the cover clickable-to-set — bottom-right, inside
        // the fixed frame (Fill fills the 44px box, not the edit-bar).
        container(edit_affordance())
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(2),
    ])
    .width(Length::Fixed(SIZE))
    .height(Length::Fixed(SIZE))
    .clip(true)
    .style(move |_t| iced::widget::container::Style {
        background: is_placeholder.then(|| theme::bg2().into()),
        border: iced::Border {
            color: theme::bg3(),
            width: 1.0,
            radius: theme::ui_radius_sm(),
        },
        ..Default::default()
    });

    let clickable =
        mouse_area(HoverOverlay::new(framed).border_radius(theme::ui_radius_sm())).on_press(on_set);

    if cover.editable && cover.custom.is_some() {
        // Reset affordance under the cover (only when a custom cover is set).
        column![
            clickable,
            mouse_area(
                text("Reset")
                    .size(9.5)
                    .font(theme::ui_font())
                    .color(theme::fg4())
            )
            .on_press(on_reset),
        ]
        .spacing(2)
        .align_x(Alignment::Center)
        .into()
    } else {
        clickable.into()
    }
}

/// The pencil glyph marking an editable affordance (cover, value cells). No
/// message of its own — generic so any edit bar can drop it in.
pub(crate) fn edit_affordance<'a, M: 'a>() -> Element<'a, M> {
    crate::embedded_svg::svg_widget("assets/icons/pencil-line.svg")
        .width(Length::Fixed(11.0))
        .height(Length::Fixed(11.0))
        .style(|_t, _s| iced::widget::svg::Style {
            color: Some(theme::fg4()),
        })
        .into()
}
