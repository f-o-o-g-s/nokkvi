//! MilkDrop preset picker — a searchable modal over the running visualizer.
//! Opened from the Choose Preset key (`m`) or the Queue / Theater panel menus.
//!
//! The list state and modal chrome are the shared picker shell
//! (`widgets::picker_modal`, also used by the default-playlist picker). The
//! difference is behavioral: the centered row plays live behind the modal as
//! the list moves, Enter keeps (and locks) it, and closing any other way
//! returns to the preset that was on screen when the picker opened.
//!
//! State lives on `Nokkvi.milkdrop.picker`; the handler is
//! `update/milkdrop_picker.rs`.

use iced::{
    Alignment, Border, Element, Length, Padding,
    font::Weight,
    widget::{Space, button, column, container, mouse_area, row, svg, text},
};
use nokkvi_data::services::milkdrop_presets::{NOKKVI_PRESET_PREFIX, PresetLibrary, PresetSource};

use crate::{
    embedded_svg, theme,
    widgets::picker_modal::{PickerChrome, PickerList, picker_modal},
};

/// `text_input` ID for the picker's search field, focused on open.
pub(crate) const MILKDROP_PICKER_SEARCH_INPUT_ID: &str = "milkdrop_picker_search";

/// State for the preset picker overlay.
#[derive(Debug, Clone)]
pub struct MilkdropPickerState {
    /// Every preset name in the library, in library (name) order; `filtered`
    /// holds the names matching the query and the favorites chip.
    pub(crate) list: PickerList<String>,
    /// Show only favorites (the title-bar chip).
    pub favorites_only: bool,
    /// The preset on screen (or loading) when the picker opened; closing
    /// without choosing returns to it.
    pub original: Option<String>,
    /// The lock state when the picker opened; closing without choosing
    /// restores it (the picker holds the lock while it is open).
    pub was_locked: bool,
}

impl MilkdropPickerState {
    /// A picker over every preset in `library`, centered on `original`.
    pub(crate) fn new(library: &PresetLibrary, original: Option<String>, was_locked: bool) -> Self {
        let all: Vec<String> = library.entries().iter().map(|e| e.name.clone()).collect();
        let mut list = PickerList::new(all);
        if let Some(name) = &original {
            list.center_on(name);
        }
        Self {
            list,
            favorites_only: false,
            original,
            was_locked,
        }
    }

    /// Recompute the filtered names from the query (case-insensitive
    /// substring) and the favorites chip; see [`PickerList::refilter`] for
    /// where the centered row lands.
    pub(crate) fn refilter(&mut self, library: &PresetLibrary) {
        let favorites_only = self.favorites_only;
        self.list.refilter(|name, query| {
            (!favorites_only || library.is_favorite(name))
                && (query.is_empty() || name.to_lowercase().contains(query))
        });
    }

    /// The preset on the centered row.
    pub(crate) fn centered(&self) -> Option<&str> {
        self.list.centered().map(String::as_str)
    }
}

/// Picker messages.
#[derive(Debug, Clone)]
pub enum MilkdropPickerMessage {
    /// Close without choosing (Escape, backdrop click, X): back to the
    /// preset that was on screen at open.
    Close,
    SearchChanged(String),
    SlotListUp,
    SlotListDown,
    SlotListSetOffset(usize),
    /// Click on a row of the filtered list: choose it.
    ClickItem(usize),
    /// Choose the centered row (Enter).
    ActivateCenter,
    /// Flip a preset's favorite mark (the row's heart).
    ToggleFavorite(String),
    /// Hide a preset from the rotation, or bring a hidden one back (the
    /// row's eye).
    ToggleHidden(String),
    /// The title-bar Favorites chip.
    ToggleFavoritesOnly,
}

/// What the overlay reads besides its own state.
pub(crate) struct MilkdropPickerViewData<'a> {
    pub library: &'a PresetLibrary,
    /// The preset whose frames are on screen now.
    pub on_screen: Option<&'a str>,
    pub window_height: f32,
}

/// Render the picker overlay, meant to be stacked on top of the app.
pub(crate) fn milkdrop_picker_overlay<'a>(
    state: &'a MilkdropPickerState,
    data: MilkdropPickerViewData<'a>,
) -> Element<'a, MilkdropPickerMessage> {
    let favorites_only = state.favorites_only;
    let chip_color = if favorites_only {
        theme::accent()
    } else {
        theme::fg4()
    };
    let chip_icon = if favorites_only {
        "assets/icons/heart-filled.svg"
    } else {
        "assets/icons/heart.svg"
    };
    let favorites_chip = button(
        row![
            embedded_svg::svg_widget(chip_icon)
                .width(Length::Fixed(12.0))
                .height(Length::Fixed(12.0))
                .style(move |_theme, _status| svg::Style {
                    color: Some(chip_color),
                }),
            text("Favorites")
                .size(12.0)
                .font(theme::ui_font())
                .color(chip_color),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press(MilkdropPickerMessage::ToggleFavoritesOnly)
    .style(theme::transparent_button_style)
    .padding(Padding::new(4.0).left(8.0).right(8.0));

    let count = text(format!(
        "{} / {}",
        state.list.filtered.len(),
        state.list.all.len()
    ))
    .size(11.0)
    .font(theme::ui_font())
    .color(theme::fg4());

    let empty_text = if state.favorites_only && state.list.search_query.is_empty() {
        "No favorite presets yet"
    } else {
        "No presets match the search query"
    };

    let library = data.library;
    let on_screen = data.on_screen;
    picker_modal(
        PickerChrome {
            title: "MilkDrop Presets",
            after_title: Some(count.into()),
            before_close: Some(favorites_chip.into()),
            search_placeholder: "Type to filter presets...",
            search_input_id: MILKDROP_PICKER_SEARCH_INPUT_ID,
            on_search: MilkdropPickerMessage::SearchChanged,
            on_close: MilkdropPickerMessage::Close,
            on_up: MilkdropPickerMessage::SlotListUp,
            on_down: MilkdropPickerMessage::SlotListDown,
            on_set_offset: MilkdropPickerMessage::SlotListSetOffset,
            empty_text,
            window_height: data.window_height,
        },
        &state.list,
        move |name, ctx| {
            render_preset_slot(
                name,
                PresetFlags::of(library, name, on_screen),
                ctx.item_index,
                ctx.is_center,
                ctx.row_height,
            )
        },
    )
}

/// A row's live curation and status flags, read from the library at render
/// time so a heart or eye click shows at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PresetFlags {
    favorite: bool,
    hidden: bool,
    broken: bool,
    on_screen: bool,
    ours: bool,
    user: bool,
}

impl PresetFlags {
    fn of(library: &PresetLibrary, name: &str, on_screen: Option<&str>) -> Self {
        Self {
            favorite: library.is_favorite(name),
            hidden: library.is_hidden(name),
            broken: library.is_broken(name),
            on_screen: on_screen == Some(name),
            ours: name.starts_with(NOKKVI_PRESET_PREFIX),
            user: matches!(library.source(name), Some(PresetSource::User(_))),
        }
    }

    /// The row's second line: where it comes from and anything unusual.
    fn subtitle(self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.on_screen {
            parts.push("On screen");
        }
        if self.ours {
            parts.push("nokkvi");
        }
        if self.user {
            parts.push("Your preset");
        }
        if self.hidden {
            parts.push("Hidden from rotation");
        }
        if self.broken {
            parts.push("Failed to load");
        }
        parts.join(" · ")
    }
}

fn render_preset_slot<'a>(
    name: &str,
    flags: PresetFlags,
    item_index: usize,
    is_center: bool,
    row_height: f32,
) -> Element<'a, MilkdropPickerMessage> {
    let muted = flags.hidden || flags.broken;
    let label_color = match (is_center, muted) {
        (true, false) => theme::fg0(),
        (true, true) | (false, false) => theme::fg2(),
        (false, true) => theme::fg4(),
    };
    let subtitle_color = if is_center {
        theme::fg2()
    } else {
        theme::fg3()
    };
    let weight = if is_center {
        Weight::Bold
    } else {
        Weight::Medium
    };

    let title = text(name.to_string())
        .size(14.0)
        .font(theme::weighted_ui_font(weight))
        .color(if flags.on_screen && !is_center {
            theme::accent()
        } else {
            label_color
        })
        .wrapping(iced::widget::text::Wrapping::None);
    let subtitle = flags.subtitle();
    let text_col: Element<'a, MilkdropPickerMessage> = if subtitle.is_empty() {
        title.into()
    } else {
        column![
            title,
            text(subtitle)
                .size(11.0)
                .font(theme::ui_font())
                .color(subtitle_color)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(2)
        .into()
    };

    let heart = row_toggle(
        if flags.favorite {
            "assets/icons/heart-filled.svg"
        } else {
            "assets/icons/heart.svg"
        },
        flags.favorite,
        MilkdropPickerMessage::ToggleFavorite(name.to_string()),
    );
    let eye = row_toggle(
        "assets/icons/eye-off.svg",
        flags.hidden,
        MilkdropPickerMessage::ToggleHidden(name.to_string()),
    );

    let body = container(
        row![
            Space::new().width(Length::Fixed(16.0)),
            container(text_col).width(Length::Fill).clip(true),
            Space::new().width(Length::Fixed(8.0)),
            heart,
            eye,
            Space::new().width(Length::Fixed(8.0)),
        ]
        .align_y(Alignment::Center)
        .height(Length::Fill),
    )
    .height(Length::Fixed(row_height))
    .width(Length::Fill)
    .style(move |_: &iced::Theme| container::Style {
        background: is_center.then(|| theme::bg1().into()),
        border: Border {
            radius: theme::ui_radius_sm(),
            ..Default::default()
        },
        ..Default::default()
    });

    // The heart and eye are buttons, so their presses are captured before
    // this row's press fires (the row's press chooses the preset).
    mouse_area(body)
        .on_press(MilkdropPickerMessage::ClickItem(item_index))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// A row's heart / eye toggle: accent while on, dim while off.
fn row_toggle<'a>(
    icon: &'static str,
    on: bool,
    message: MilkdropPickerMessage,
) -> Element<'a, MilkdropPickerMessage> {
    let color = if on { theme::accent() } else { theme::fg4() };
    button(
        embedded_svg::svg_widget(icon)
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0))
            .style(move |_theme, _status| svg::Style { color: Some(color) }),
    )
    .on_press(message)
    .style(theme::transparent_button_style)
    .padding(Padding::new(6.0))
    .into()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use nokkvi_data::services::milkdrop_presets::Curation;

    use super::*;

    static PACK: &[(&str, &str)] = &[
        ("Alpha - one", "{}"),
        ("Beta - two", "{}"),
        ("Gamma - three", "{}"),
        ("nokkvi - fjord", "{}"),
    ];

    fn library() -> PresetLibrary {
        PresetLibrary::new(
            PACK,
            Path::new("/nonexistent/nokkvi-picker-tests"),
            Curation::default(),
        )
    }

    #[test]
    fn opens_centered_on_the_original() {
        let lib = library();
        let state = MilkdropPickerState::new(&lib, Some("Gamma - three".into()), false);
        assert_eq!(state.centered(), Some("Gamma - three"));
        assert_eq!(state.list.filtered.len(), 4);
    }

    #[test]
    fn opens_at_the_top_without_an_original() {
        let lib = library();
        let state = MilkdropPickerState::new(&lib, None, false);
        assert_eq!(state.centered(), Some("Alpha - one"));
    }

    #[test]
    fn refilter_is_case_insensitive_and_keeps_a_matching_center() {
        let lib = library();
        let mut state = MilkdropPickerState::new(&lib, Some("Gamma - three".into()), false);
        state.list.search_query = "A - T".into();
        state.refilter(&lib);
        assert_eq!(state.list.filtered, ["Beta - two", "Gamma - three"]);
        assert_eq!(state.centered(), Some("Gamma - three"));

        state.list.search_query = "fjord".into();
        state.refilter(&lib);
        assert_eq!(state.centered(), Some("nokkvi - fjord"), "center moved off");
    }

    #[test]
    fn favorites_chip_narrows_to_favorites() {
        let mut lib = library();
        lib.toggle_favorite("Beta - two");
        let mut state = MilkdropPickerState::new(&lib, None, false);
        state.favorites_only = true;
        state.refilter(&lib);
        assert_eq!(state.list.filtered, ["Beta - two"]);
    }

    #[test]
    fn subtitle_names_origin_and_status() {
        let mut lib = library();
        lib.hide("nokkvi - fjord");
        let flags = PresetFlags::of(&lib, "nokkvi - fjord", Some("nokkvi - fjord"));
        assert_eq!(
            flags.subtitle(),
            "On screen · nokkvi · Hidden from rotation"
        );
        assert_eq!(
            PresetFlags::of(&lib, "Alpha - one", None).subtitle(),
            "",
            "a plain bundled preset has no second line"
        );
    }
}
