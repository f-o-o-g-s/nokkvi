//! MilkDrop preset picker — a searchable modal over the running visualizer.
//! Opened from the Choose Preset key (`m`) or the Queue / Theater panel menus.
//!
//! Mirrors the default-playlist picker (`default_playlist_picker.rs`): a
//! searchable slot list with immediate filtering, Up/Down/Enter navigation,
//! centered over the shared modal backdrop, dismissed by Escape / X / a click
//! outside. The difference is behavioral: the centered row plays live behind
//! the modal as the list moves, Enter keeps (and locks) it, and closing any
//! other way returns to the preset that was on screen when the picker opened.
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
    widgets::{SlotListView, slot_list},
};

/// `text_input` ID for the picker's search field, focused on open.
pub(crate) const MILKDROP_PICKER_SEARCH_INPUT_ID: &str = "milkdrop_picker_search";

const TITLE_BAR_HEIGHT: f32 = 38.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;

/// State for the preset picker overlay.
#[derive(Debug, Clone)]
pub struct MilkdropPickerState {
    /// Every preset name in the library, in library (name) order.
    pub all: Vec<String>,
    pub search_query: String,
    /// Show only favorites (the title-bar chip).
    pub favorites_only: bool,
    /// The names matching the query and the favorites chip.
    pub filtered: Vec<String>,
    pub slot_list: SlotListView,
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
        let mut state = Self {
            filtered: all.clone(),
            all,
            search_query: String::new(),
            favorites_only: false,
            slot_list: SlotListView::new(),
            original,
            was_locked,
        };
        if let Some(name) = state.original.clone() {
            state.center_on(&name);
        }
        state
    }

    /// Recompute `filtered` from the query (case-insensitive substring) and
    /// the favorites chip. Keeps the centered preset centered when it still
    /// matches; otherwise the list starts at its first row.
    pub(crate) fn refilter(&mut self, library: &PresetLibrary) {
        let centered = self.centered().map(str::to_string);
        let query = self.search_query.to_lowercase();
        self.filtered = self
            .all
            .iter()
            .filter(|name| !self.favorites_only || library.is_favorite(name))
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
            .cloned()
            .collect();
        self.slot_list = SlotListView::new();
        if let Some(name) = centered {
            self.center_on(&name);
        }
    }

    /// The preset on the centered row.
    pub(crate) fn centered(&self) -> Option<&str> {
        self.slot_list
            .get_center_item_index(self.filtered.len())
            .and_then(|i| self.filtered.get(i))
            .map(String::as_str)
    }

    /// Scroll so `name` sits on the centered row (no-op when filtered out).
    pub(crate) fn center_on(&mut self, name: &str) {
        if let Some(index) = self.filtered.iter().position(|n| n == name) {
            self.slot_list.set_offset(index, self.filtered.len());
        }
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
    let modal_height = (data.window_height * 0.70).max(320.0);
    let modal_chrome = TITLE_BAR_HEIGHT + SEARCH_BAR_HEIGHT;
    let label_size = 13.0;

    // ── Title bar: title, Favorites chip, X ──
    let dim_color = theme::fg4();
    let close_btn = button(
        embedded_svg::svg_widget("assets/icons/x.svg")
            .width(Length::Fixed(label_size))
            .height(Length::Fixed(label_size))
            .style(move |_theme, _status| svg::Style {
                color: Some(dim_color),
            }),
    )
    .on_press(MilkdropPickerMessage::Close)
    .style(theme::transparent_button_style)
    .padding(Padding::new(2.0));

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

    let count = format!("{} / {}", state.filtered.len(), state.all.len());
    let title_row = row![
        Space::new().width(Length::Fixed(12.0)),
        text("MilkDrop Presets")
            .size(label_size)
            .font(theme::weighted_ui_font(Weight::Bold))
            .color(theme::fg0()),
        Space::new().width(Length::Fixed(10.0)),
        text(count)
            .size(11.0)
            .font(theme::ui_font())
            .color(theme::fg4()),
        Space::new().width(Length::Fill),
        favorites_chip,
        Space::new().width(Length::Fixed(8.0)),
        close_btn,
        Space::new().width(Length::Fixed(12.0)),
    ]
    .align_y(Alignment::Center)
    .height(Length::Fixed(TITLE_BAR_HEIGHT));
    let title_bar = container(title_row).width(Length::Fill);

    // ── Search bar ──
    let search_input = crate::widgets::search_bar::search_bar(
        &state.search_query,
        "Type to filter presets...",
        MILKDROP_PICKER_SEARCH_INPUT_ID,
        MilkdropPickerMessage::SearchChanged,
        Some(theme::settings_search_input_style),
    );
    let search_bar = container(search_input)
        .width(Length::Fill)
        .height(Length::Fixed(SEARCH_BAR_HEIGHT))
        .padding(Padding::new(4.0).left(12.0).right(12.0));

    // ── Slot list or empty state ──
    let main_area: Element<'a, MilkdropPickerMessage> = if state.filtered.is_empty() {
        let msg = if state.favorites_only && state.search_query.is_empty() {
            "No favorite presets yet"
        } else {
            "No presets match the search query"
        };
        container(
            text(msg)
                .size(14)
                .font(theme::ui_font())
                .color(theme::fg4()),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .into()
    } else {
        let config = slot_list::SlotListConfig::with_dynamic_slots(modal_height, modal_chrome);
        let total = state.filtered.len();
        let library = data.library;
        let on_screen = data.on_screen;
        slot_list::slot_list_view_with_scroll(
            &state.slot_list,
            &state.filtered,
            &config,
            MilkdropPickerMessage::SlotListUp,
            MilkdropPickerMessage::SlotListDown,
            move |f| MilkdropPickerMessage::SlotListSetOffset((f * total as f32) as usize),
            None,
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
    };

    let modal_panel = container(
        column![title_bar, search_bar, main_area]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::FillPortion(5))
    .height(Length::Fixed(modal_height))
    .clip(true)
    .padding(Padding::new(4.0))
    .style(theme::modal_frame_style);

    let modal_row = row![
        Space::new().width(Length::FillPortion(1)),
        modal_panel,
        Space::new().width(Length::FillPortion(1)),
    ]
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let scaffold = theme::modal_scaffold(
        modal_row.into(),
        MilkdropPickerMessage::Close,
        theme::MODAL_BACKDROP_ALPHA,
    );

    // `opaque` (inside the scaffold) captures presses, not scrolls, so the
    // wheel steps the list anywhere over the modal.
    mouse_area(scaffold)
        .on_scroll(|delta| {
            let y = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            if y > 0.0 {
                MilkdropPickerMessage::SlotListUp
            } else {
                MilkdropPickerMessage::SlotListDown
            }
        })
        .into()
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
        assert_eq!(state.filtered.len(), 4);
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
        state.search_query = "A - T".into();
        state.refilter(&lib);
        assert_eq!(state.filtered, ["Beta - two", "Gamma - three"]);
        assert_eq!(state.centered(), Some("Gamma - three"));

        state.search_query = "fjord".into();
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
        assert_eq!(state.filtered, ["Beta - two"]);
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
