//! Default-playlist picker — modal overlay that lets the user choose a new
//! default playlist. Triggered from the chip in the Playlists/Queue headers.
//!
//! The list state and modal chrome are the shared picker shell
//! (`widgets::picker_modal`, also used by the MilkDrop preset picker); this
//! module owns the entries and how a row looks.
//!
//! State lives on `Nokkvi` root (cross-cutting between Playlists and Queue
//! views), opened via `Message::DefaultPlaylistPicker(Open(...))`.

use std::collections::HashMap;

use iced::{
    Alignment, Border, Element, Length,
    font::Weight,
    widget::{Space, column, container, image, mouse_area, row, svg, text},
};
use nokkvi_data::{
    backend::playlists::PlaylistUIViewData, utils::formatters::format_duration_short,
};

use crate::{
    embedded_svg, theme,
    widgets::picker_modal::{PickerChrome, PickerList, picker_modal},
};

/// `text_input` ID for the picker's search field. Used by the open handler
/// to focus the input as soon as the modal opens.
pub(crate) const PICKER_SEARCH_INPUT_ID: &str = "default_playlist_picker_search";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PickerEntry {
    /// Virtual top entry — selecting it clears the default (id = None).
    Clear,
    /// A real playlist option, projected from `PlaylistUIViewData` so the
    /// picker can render artwork + stats without re-borrowing the library
    /// at slot-render time.
    Playlist {
        id: String,
        name: String,
        song_count: u32,
        duration_seconds: u32,
    },
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
    pub(crate) list: PickerList<PickerEntry>,
}

impl DefaultPlaylistPickerState {
    /// Build a new picker state from the current playlists list.
    /// Prepends the "Clear default" virtual entry. Smart playlists are
    /// excluded — the default playlist is an APPEND target, and the server
    /// rejects track mutations on smart playlists (error 50/403).
    pub(crate) fn new(playlists: &[PlaylistUIViewData]) -> Self {
        let mut all_entries = vec![PickerEntry::Clear];
        for p in playlists.iter().filter(|p| !p.is_smart) {
            all_entries.push(PickerEntry::Playlist {
                id: p.id.clone(),
                name: p.name.clone(),
                song_count: p.song_count,
                duration_seconds: p.duration as u32,
            });
        }
        Self {
            list: PickerList::new(all_entries),
        }
    }

    /// Recompute the filtered entries against the search query (see
    /// [`PickerList::refilter`]). "Clear default" remains visible at the top
    /// regardless of the query.
    pub(crate) fn refilter(&mut self) {
        self.list.refilter(|entry, query| match entry {
            PickerEntry::Clear => true,
            PickerEntry::Playlist { name, .. } => {
                query.is_empty() || name.to_lowercase().contains(query)
            }
        });
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
    SlotListSetOffset(usize),
    /// Click on a specific entry index in the filtered list.
    ClickItem(usize),
    /// Activate the centered entry (Enter / click center).
    ActivateCenter,
}

/// Render the picker overlay. Returns an Element that fills the available
/// area; intended to be stacked on top of the main app via `iced::widget::stack`.
///
/// `playlist_art` is the same `HashMap<playlist_id, Handle>` snapshot the
/// Playlists view renders from — populated by the collage-prefetch pipeline
/// once the playlists list has been loaded.
pub(crate) fn default_playlist_picker_overlay<'a>(
    state: &'a DefaultPlaylistPickerState,
    window_height: f32,
    playlist_art: &'a HashMap<String, image::Handle>,
) -> Element<'a, DefaultPlaylistPickerMessage> {
    picker_modal(
        PickerChrome {
            title: "Default Playlist",
            after_title: None,
            before_close: None,
            search_placeholder: "Type to filter playlists...",
            search_input_id: PICKER_SEARCH_INPUT_ID,
            on_search: DefaultPlaylistPickerMessage::SearchChanged,
            on_close: DefaultPlaylistPickerMessage::Close,
            on_up: DefaultPlaylistPickerMessage::SlotListUp,
            on_down: DefaultPlaylistPickerMessage::SlotListDown,
            on_set_offset: DefaultPlaylistPickerMessage::SlotListSetOffset,
            empty_text: "No playlists match the search query",
            window_height,
        },
        &state.list,
        move |entry, ctx| {
            render_picker_slot(
                entry,
                ctx.item_index,
                ctx.is_center,
                ctx.row_height,
                playlist_art,
            )
        },
    )
}

fn render_picker_slot<'a>(
    entry: &PickerEntry,
    item_index: usize,
    is_center: bool,
    row_height: f32,
    playlist_art: &HashMap<String, image::Handle>,
) -> Element<'a, DefaultPlaylistPickerMessage> {
    let label_color = if is_center {
        theme::fg0()
    } else {
        theme::fg2()
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

    // Square thumbnail box sized off the row height (with 8px vertical padding).
    let art_size = (row_height - 16.0).max(32.0);

    let thumbnail: Element<'a, DefaultPlaylistPickerMessage> = match entry {
        PickerEntry::Clear => container(
            embedded_svg::svg_widget("assets/icons/x.svg")
                .width(Length::Fixed(art_size * 0.5))
                .height(Length::Fixed(art_size * 0.5))
                .style(move |_theme, _status| svg::Style {
                    color: Some(subtitle_color),
                }),
        )
        .width(Length::Fixed(art_size))
        .height(Length::Fixed(art_size))
        .center(Length::Fixed(art_size))
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(theme::bg2().into()),
            border: Border {
                radius: theme::ui_border_radius(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into(),
        PickerEntry::Playlist { id, .. } => {
            let handle = playlist_art.get(id).cloned();
            if let Some(h) = handle {
                container(
                    image(h)
                        .width(Length::Fixed(art_size))
                        .height(Length::Fixed(art_size))
                        .content_fit(iced::ContentFit::Cover),
                )
                .width(Length::Fixed(art_size))
                .height(Length::Fixed(art_size))
                .style(|_theme| container::Style {
                    border: Border {
                        radius: theme::ui_border_radius(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .clip(true)
                .into()
            } else {
                container(
                    embedded_svg::svg_widget("assets/icons/list-music.svg")
                        .width(Length::Fixed(art_size * 0.5))
                        .height(Length::Fixed(art_size * 0.5))
                        .style(move |_theme, _status| svg::Style {
                            color: Some(subtitle_color),
                        }),
                )
                .width(Length::Fixed(art_size))
                .height(Length::Fixed(art_size))
                .center(Length::Fixed(art_size))
                .style(move |_theme: &iced::Theme| container::Style {
                    background: Some(theme::bg2().into()),
                    border: Border {
                        radius: theme::ui_border_radius(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into()
            }
        }
    };

    let title = text(entry.label().to_string())
        .size(14.0)
        .font(theme::weighted_ui_font(weight))
        .color(label_color)
        .wrapping(iced::widget::text::Wrapping::None);

    let subtitle_str = match entry {
        PickerEntry::Clear => "Remove the current default".to_string(),
        PickerEntry::Playlist {
            song_count,
            duration_seconds,
            ..
        } => {
            let song_word = if *song_count == 1 { "song" } else { "songs" };
            let duration = format_duration_short(f64::from(*duration_seconds));
            format!("{song_count} {song_word} · {duration}")
        }
    };
    let subtitle = text(subtitle_str)
        .size(11.0)
        .font(theme::ui_font())
        .color(subtitle_color)
        .wrapping(iced::widget::text::Wrapping::None);

    let text_col = column![title, subtitle].spacing(2);

    let body = container(
        row![
            Space::new().width(Length::Fixed(12.0)),
            thumbnail,
            Space::new().width(Length::Fixed(12.0)),
            text_col,
            Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .height(Length::Fill),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_playlist(id: &str, name: &str) -> PlaylistUIViewData {
        PlaylistUIViewData {
            id: id.to_string(),
            name: name.to_string(),
            comment: String::new(),
            duration: 0.0,
            song_count: 0,
            owner_name: String::new(),
            public: false,
            updated_at: String::new(),
            artwork_album_ids: vec![],
            uploaded_image: None,
            is_smart: false,
            rules: None,
            evaluated_at: None,
            is_file_backed: false,
            sync: false,
            owner_id: String::new(),
            searchable_lower: name.to_lowercase(),
            image: Default::default(),
        }
    }

    fn sample_playlists() -> Vec<PlaylistUIViewData> {
        vec![
            make_playlist("p1", "Workout"),
            make_playlist("p2", "Chill"),
            make_playlist("p3", "Focus"),
        ]
    }

    #[test]
    fn new_prepends_clear_entry() {
        let state = DefaultPlaylistPickerState::new(&sample_playlists());
        assert!(matches!(state.list.all[0], PickerEntry::Clear));
        assert_eq!(state.list.all.len(), 4);
    }

    #[test]
    fn new_projects_song_count_and_duration() {
        let mut p = make_playlist("p1", "Workout");
        p.song_count = 32;
        p.duration = 6862.0;

        let state = DefaultPlaylistPickerState::new(&[p]);
        match &state.list.all[1] {
            PickerEntry::Playlist {
                id,
                song_count,
                duration_seconds,
                ..
            } => {
                assert_eq!(id, "p1");
                assert_eq!(*song_count, 32);
                assert_eq!(*duration_seconds, 6862);
            }
            PickerEntry::Clear => panic!("expected Playlist entry at index 1"),
        }
    }

    #[test]
    fn refilter_keeps_clear_entry_visible() {
        let mut state = DefaultPlaylistPickerState::new(&sample_playlists());
        state.list.search_query = "zzz_no_match".to_string();
        state.refilter();
        assert_eq!(state.list.filtered.len(), 1);
        assert!(matches!(state.list.filtered[0], PickerEntry::Clear));
    }

    #[test]
    fn refilter_matches_substring_case_insensitive() {
        let mut state = DefaultPlaylistPickerState::new(&sample_playlists());
        state.list.search_query = "WORK".to_string();
        state.refilter();
        // Clear + Workout
        assert_eq!(state.list.filtered.len(), 2);
        if let PickerEntry::Playlist { name, .. } = &state.list.filtered[1] {
            assert_eq!(name, "Workout");
        } else {
            panic!("expected Playlist entry");
        }
    }

    #[test]
    fn empty_query_returns_all_entries() {
        let mut state = DefaultPlaylistPickerState::new(&sample_playlists());
        state.list.search_query = String::new();
        state.refilter();
        assert_eq!(state.list.filtered.len(), 4);
    }
}
