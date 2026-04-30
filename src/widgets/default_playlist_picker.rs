//! Default-playlist picker — modal overlay that lets the user choose a new
//! default playlist. Triggered from the chip in the Playlists/Queue headers.
//!
//! Mirrors the font picker pattern (`src/views/settings/sub_lists.rs`):
//! - Searchable list with immediate (non-debounced) filtering
//! - Slot-list keyboard navigation (Up/Down/Enter)
//! - Modal panel centered over a dimmed backdrop
//! - Click-outside / Escape / X dismiss
//!
//! State lives on `Nokkvi` root (cross-cutting between Playlists and Queue
//! views), opened via `Message::DefaultPlaylistPicker(Open(...))`.

use iced::{
    Alignment, Border, Color, Element, Length, Padding,
    font::{Font, Weight},
    widget::{Space, button, column, container, mouse_area, row, svg, text},
};

use crate::{
    embedded_svg, theme,
    widgets::{SlotListView, slot_list},
};

/// Width threshold below which the picker label collapses on the chip.
/// (Kept here even though the chip widget owns its threshold, so a search
/// looking for "default playlist" finds both spots.)
pub(crate) const PICKER_SEARCH_INPUT_ID: &str = "default_playlist_picker_search";

const TITLE_BAR_HEIGHT: f32 = 38.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;

#[derive(Debug, Clone)]
pub enum PickerEntry {
    /// Virtual top entry — selecting it clears the default (id = None).
    Clear,
    /// A real playlist option.
    Playlist { id: String, name: String },
}

impl PickerEntry {
    fn label(&self) -> &str {
        match self {
            PickerEntry::Clear => "Clear default",
            PickerEntry::Playlist { name, .. } => name.as_str(),
        }
    }
}

/// State for the default-playlist picker overlay.
#[derive(Debug, Clone)]
pub struct DefaultPlaylistPickerState {
    pub all_entries: Vec<PickerEntry>,
    pub search_query: String,
    pub filtered: Vec<PickerEntry>,
    pub slot_list: SlotListView,
}

impl DefaultPlaylistPickerState {
    /// Build a new picker state from the current playlists list.
    /// Prepends the "Clear default" virtual entry.
    pub(crate) fn new(playlists: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut all_entries = vec![PickerEntry::Clear];
        for (id, name) in playlists {
            all_entries.push(PickerEntry::Playlist { id, name });
        }
        Self {
            filtered: all_entries.clone(),
            all_entries,
            search_query: String::new(),
            slot_list: SlotListView::new(),
        }
    }

    /// Recompute `filtered` from `all_entries` against `search_query`.
    /// "Clear default" remains visible at the top regardless of the query.
    pub(crate) fn refilter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered = self.all_entries.clone();
        } else {
            let query = self.search_query.to_lowercase();
            let mut filtered = vec![PickerEntry::Clear];
            for entry in &self.all_entries {
                if let PickerEntry::Playlist { name, .. } = entry
                    && name.to_lowercase().contains(&query)
                {
                    filtered.push(entry.clone());
                }
            }
            self.filtered = filtered;
        }
        self.slot_list = SlotListView::new();
    }
}

/// Picker messages — opened from a chip click, closed on Escape/select.
#[derive(Debug, Clone)]
pub enum DefaultPlaylistPickerMessage {
    /// Open the picker. The dispatcher reads the playlists list from app state.
    Open,
    /// Close the picker without selecting (Escape, backdrop click, X).
    Close,
    /// Search input changed.
    SearchChanged(String),
    /// Slot navigation.
    SlotListUp,
    SlotListDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    /// Click on a specific entry index in the filtered list.
    ClickItem(usize),
    /// Activate the centered entry (Enter / click center).
    ActivateCenter,
}

/// Render the picker overlay. Returns an Element that fills the available
/// area; intended to be stacked on top of the main app via `iced::widget::stack`.
pub(crate) fn default_playlist_picker_overlay<'a>(
    state: &'a DefaultPlaylistPickerState,
    window_height: f32,
) -> Element<'a, DefaultPlaylistPickerMessage> {
    // ── Modal dimensions ──
    let modal_height = (window_height * 0.70).max(320.0);
    let modal_chrome = TITLE_BAR_HEIGHT + SEARCH_BAR_HEIGHT;

    // ── Title bar ──
    let dim_color = theme::fg4();
    let active_color = theme::fg0();
    let label_size = 13.0;

    let close_btn = button(
        embedded_svg::svg_widget("assets/icons/x.svg")
            .width(Length::Fixed(label_size))
            .height(Length::Fixed(label_size))
            .style(move |_theme, _status| svg::Style {
                color: Some(dim_color),
            }),
    )
    .on_press(DefaultPlaylistPickerMessage::Close)
    .style(transparent_button_style)
    .padding(Padding::new(2.0));

    let title_row = row![
        Space::new().width(Length::Fixed(12.0)),
        text("Default Playlist")
            .size(label_size)
            .font(Font {
                weight: Weight::Bold,
                ..theme::ui_font()
            })
            .color(active_color),
        Space::new().width(Length::Fill),
        close_btn,
        Space::new().width(Length::Fixed(12.0)),
    ]
    .align_y(Alignment::Center)
    .height(Length::Fixed(TITLE_BAR_HEIGHT));
    let title_bar = container(title_row).width(Length::Fill);

    // ── Search bar ──
    let search_input = crate::widgets::search_bar::search_bar(
        &state.search_query,
        "Type to filter playlists...",
        PICKER_SEARCH_INPUT_ID,
        DefaultPlaylistPickerMessage::SearchChanged,
        Some(theme::settings_search_input_style),
    );
    let search_bar = container(search_input)
        .width(Length::Fill)
        .height(Length::Fixed(SEARCH_BAR_HEIGHT))
        .padding(Padding::new(4.0).left(12.0).right(12.0));

    // ── Slot list or empty state ──
    let main_area: Element<'a, DefaultPlaylistPickerMessage> = if state.filtered.is_empty() {
        container(
            text("No playlists match the search query")
                .size(14)
                .color(theme::fg4()),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .into()
    } else {
        let config = slot_list::SlotListConfig::with_dynamic_slots(modal_height, modal_chrome);
        let entries_owned = state.filtered.clone();

        slot_list::slot_list_view_with_scroll(
            &state.slot_list,
            &entries_owned,
            &config,
            DefaultPlaylistPickerMessage::SlotListUp,
            DefaultPlaylistPickerMessage::SlotListDown,
            {
                let total = entries_owned.len();
                move |f| {
                    DefaultPlaylistPickerMessage::SlotListSetOffset(
                        (f * total as f32) as usize,
                        iced::keyboard::Modifiers::default(),
                    )
                }
            },
            move |entry, ctx| {
                render_picker_slot(entry, ctx.item_index, ctx.is_center, ctx.row_height)
            },
        )
    };

    // ── Modal panel ──
    let modal_bg = theme::bg0_hard();
    let modal_border = theme::accent();
    let modal_radius = theme::ui_border_radius();

    let modal_panel = container(
        column![title_bar, search_bar, main_area]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::FillPortion(5))
    .height(Length::Fixed(modal_height))
    .clip(true)
    .padding(Padding::new(4.0))
    .style(move |_: &iced::Theme| container::Style {
        background: Some(modal_bg.into()),
        border: Border {
            color: modal_border,
            width: 1.5,
            radius: modal_radius,
        },
        ..Default::default()
    });

    let modal_row = row![
        Space::new().width(Length::FillPortion(1)),
        modal_panel,
        Space::new().width(Length::FillPortion(1)),
    ]
    .width(Length::Fill)
    .align_y(Alignment::Center);

    // ── Backdrop ──
    let backdrop_color = Color {
        a: 0.55,
        ..Color::BLACK
    };

    mouse_area(
        container(modal_row)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(backdrop_color.into()),
                ..Default::default()
            }),
    )
    .on_press(DefaultPlaylistPickerMessage::Close)
    .on_scroll(|delta| {
        let y = match delta {
            iced::mouse::ScrollDelta::Lines { y, .. } => y,
            iced::mouse::ScrollDelta::Pixels { y, .. } => y,
        };
        if y > 0.0 {
            DefaultPlaylistPickerMessage::SlotListUp
        } else {
            DefaultPlaylistPickerMessage::SlotListDown
        }
    })
    .into()
}

fn render_picker_slot<'a>(
    entry: &PickerEntry,
    item_index: usize,
    is_center: bool,
    row_height: f32,
) -> Element<'a, DefaultPlaylistPickerMessage> {
    let label_color = if is_center {
        theme::fg0()
    } else {
        theme::fg2()
    };
    let weight = if is_center {
        Weight::Bold
    } else {
        Weight::Medium
    };

    let icon_path = match entry {
        PickerEntry::Clear => "assets/icons/x.svg",
        PickerEntry::Playlist { .. } => "assets/icons/list-music.svg",
    };
    let icon = embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .style(move |_theme, _status| svg::Style {
            color: Some(label_color),
        });

    let label = text(entry.label().to_string())
        .size(14.0)
        .font(Font {
            weight,
            ..theme::ui_font()
        })
        .color(label_color)
        .wrapping(iced::widget::text::Wrapping::None);

    let body = container(
        row![
            Space::new().width(Length::Fixed(12.0)),
            icon,
            Space::new().width(Length::Fixed(10.0)),
            label,
            Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(row_height))
    .width(Length::Fill)
    .style(move |_: &iced::Theme| container::Style {
        background: if is_center {
            Some(theme::bg1().into())
        } else {
            None
        },
        border: Border {
            radius: theme::ui_border_radius(),
            ..Default::default()
        },
        ..Default::default()
    });

    mouse_area(body)
        .on_press(DefaultPlaylistPickerMessage::ClickItem(item_index))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn transparent_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered => Some(theme::bg1().into()),
            _ => None,
        },
        text_color: theme::fg0(),
        border: Border {
            radius: theme::ui_border_radius(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_playlists() -> Vec<(String, String)> {
        vec![
            ("p1".to_string(), "Workout".to_string()),
            ("p2".to_string(), "Chill".to_string()),
            ("p3".to_string(), "Focus".to_string()),
        ]
    }

    #[test]
    fn new_prepends_clear_entry() {
        let state = DefaultPlaylistPickerState::new(sample_playlists());
        assert!(matches!(state.all_entries[0], PickerEntry::Clear));
        assert_eq!(state.all_entries.len(), 4);
    }

    #[test]
    fn refilter_keeps_clear_entry_visible() {
        let mut state = DefaultPlaylistPickerState::new(sample_playlists());
        state.search_query = "zzz_no_match".to_string();
        state.refilter();
        assert_eq!(state.filtered.len(), 1);
        assert!(matches!(state.filtered[0], PickerEntry::Clear));
    }

    #[test]
    fn refilter_matches_substring_case_insensitive() {
        let mut state = DefaultPlaylistPickerState::new(sample_playlists());
        state.search_query = "WORK".to_string();
        state.refilter();
        // Clear + Workout
        assert_eq!(state.filtered.len(), 2);
        if let PickerEntry::Playlist { name, .. } = &state.filtered[1] {
            assert_eq!(name, "Workout");
        } else {
            panic!("expected Playlist entry");
        }
    }

    #[test]
    fn empty_query_returns_all_entries() {
        let mut state = DefaultPlaylistPickerState::new(sample_playlists());
        state.search_query = String::new();
        state.refilter();
        assert_eq!(state.filtered.len(), 4);
    }
}
