//! Similar Songs Page Component
//!
//! Displays algorithmic recommendations from Navidrome's getSimilarSongs2 / getTopSongs endpoints.
//! Stripped-down version of SongsPage — no sort, no search, no pagination.
//! Results are ephemeral (populated via context menu, not persisted).

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{container, image},
};
use nokkvi_data::{types::song::Song, utils::formatters};

use crate::widgets::{
    self, SlotListPageMessage, SlotListPageState,
    view_header::{HeaderButton, SortMode, ViewHeaderConfig},
};

/// Similar page local state — just a slot list, no sort/search/pagination.
#[derive(Debug)]
pub struct SimilarPage {
    pub common: SlotListPageState,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: SimilarColumnVisibility,
}

// Title/Artist is always shown; everything else is user-toggleable through the columns-cog dropdown.
// Select is opt-in like everywhere else in the app; all others default on to match the historical layout.
super::define_view_columns! {
    SimilarColumn => SimilarColumnVisibility {
        Select: select = false => set_similar_show_select,
        Index: index = true => set_similar_show_index,
        Thumbnail: thumbnail = true => set_similar_show_thumbnail,
        Album: album = true => set_similar_show_album,
        Duration: duration = true => set_similar_show_duration,
        Love: love = true => set_similar_show_love,
    }
}

/// View data passed from root (read-only borrows from app state).
pub struct SimilarViewData<'a> {
    pub songs: &'a [Song],
    pub album_art: &'a HashMap<String, image::Handle>,
    pub large_artwork: &'a HashMap<String, image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    /// Provenance label: "Similar to: Paranoid Android" or "Top Songs: Radiohead"
    pub label: &'a str,
    /// Whether an API call is in flight
    pub loading: bool,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus can resolve their own open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
    /// Whether the column-visibility checkbox dropdown is open (controlled
    /// by `Nokkvi.open_menu`).
    pub column_dropdown_open: bool,
    /// Trigger bounds captured when the dropdown was opened.
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
}

/// Messages for local similar page interactions
#[derive(Debug, Clone)]
pub enum SimilarMessage {
    NoOp,

    // Slot list navigation (unified carrier)
    SlotList(SlotListPageMessage),

    // Mouse click on star/heart (item_index, value)
    ClickToggleStar(usize),

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    /// Toggle a similar column's visibility from the columns-cog dropdown.
    ToggleColumnVisible(SimilarColumn),

    /// Context-menu open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_similar` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Always-Vertical artwork drag handle event — intercepted at root.
    ArtworkColumnVerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum SimilarAction {
    PlayBatch(nokkvi_data::types::batch::BatchPayload),
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    ToggleStar(String, bool),
    LoadLargeArtwork(String),
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>),
    ShowInFolder(String),
    /// Recursive discovery: FindSimilar from within the similar results
    FindSimilar(String, String),
    /// Top Songs for an artist, triggered from within similar results
    FindTopSongs(String, String),
    /// User toggled a similar column's visibility — persist to config.toml.
    ColumnVisibilityChanged(SimilarColumn, bool),
    None,
}

impl Default for SimilarPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                SortMode::Title, // unused — no sort controls
                true,
            ),
            column_visibility: SimilarColumnVisibility::default(),
        }
    }
}

impl SimilarPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: SimilarMessage,
        songs: &[Song],
    ) -> (Task<SimilarMessage>, SimilarAction) {
        let total_items = songs.len();

        match message {
            SimilarMessage::NoOp => (Task::none(), SimilarAction::None),
            // Routed up to root in `handle_similar` before this match runs;
            // arm exists only for exhaustiveness.
            SimilarMessage::SetOpenMenu(_) => (Task::none(), SimilarAction::None),
            SimilarMessage::ArtworkColumnDrag(_) | SimilarMessage::ArtworkColumnVerticalDrag(_) => {
                // Intercepted at root before reaching this update; never reached.
                (Task::none(), SimilarAction::None)
            }
            SimilarMessage::SlotList(msg) => {
                use crate::widgets::SlotListPageAction;

                // NavigateUp/Down need the post-navigation center for artwork loading.
                let needs_artwork_load = matches!(
                    msg,
                    SlotListPageMessage::NavigateUp
                        | SlotListPageMessage::NavigateDown
                        | SlotListPageMessage::SetOffset(_, _)
                );
                match self.common.handle(msg, total_items) {
                    SlotListPageAction::AddCenterToQueue => {
                        use nokkvi_data::types::batch::BatchItem;

                        let target_indices = self.common.get_queue_target_indices(total_items);

                        if target_indices.is_empty() {
                            return (Task::none(), SimilarAction::None);
                        }

                        let payload = super::expansion::build_batch_payload(target_indices, |i| {
                            songs.get(i).map(|s| BatchItem::Song(Box::new(s.clone())))
                        });

                        (Task::none(), SimilarAction::AddBatchToQueue(payload))
                    }
                    _ => {
                        if needs_artwork_load
                            && let Some(center_idx) = self.common.get_center_item_index(total_items)
                            && let Some(song) = songs.get(center_idx)
                            && let Some(album_id) = &song.album_id
                        {
                            return (
                                Task::none(),
                                SimilarAction::LoadLargeArtwork(album_id.clone()),
                            );
                        }
                        (Task::none(), SimilarAction::None)
                    }
                }
            }
            SimilarMessage::ToggleColumnVisible(col) => {
                let new_value = !self.column_visibility.get(col);
                self.column_visibility.set(col, new_value);
                (
                    Task::none(),
                    SimilarAction::ColumnVisibilityChanged(col, new_value),
                )
            }
            SimilarMessage::ClickToggleStar(item_index) => {
                if let Some(song) = songs.get(item_index) {
                    (
                        Task::none(),
                        SimilarAction::ToggleStar(song.id.clone(), !song.starred),
                    )
                } else {
                    (Task::none(), SimilarAction::None)
                }
            }
            SimilarMessage::ContextMenuAction(clicked_idx, entry) => {
                use nokkvi_data::types::batch::BatchItem;

                use crate::widgets::context_menu::LibraryContextEntry;

                let build_all_payload = || {
                    super::expansion::build_batch_payload(0..songs.len(), |i| {
                        songs.get(i).map(|s| BatchItem::Song(Box::new(s.clone())))
                    })
                };

                if let Some(song) = songs.get(clicked_idx) {
                    match entry {
                        LibraryContextEntry::ReplaceQueueWithAllFound => {
                            (Task::none(), SimilarAction::PlayBatch(build_all_payload()))
                        }
                        LibraryContextEntry::AddAllFoundToQueue => (
                            Task::none(),
                            SimilarAction::AddBatchToQueue(build_all_payload()),
                        ),
                        LibraryContextEntry::AddAllFoundToPlaylist => (
                            Task::none(),
                            SimilarAction::AddBatchToPlaylist(build_all_payload()),
                        ),
                        LibraryContextEntry::AddToQueue => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload =
                                super::expansion::build_batch_payload(target_indices, |i| {
                                    songs.get(i).map(|s| BatchItem::Song(Box::new(s.clone())))
                                });
                            (Task::none(), SimilarAction::AddBatchToQueue(payload))
                        }
                        LibraryContextEntry::AddToPlaylist => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload =
                                super::expansion::build_batch_payload(target_indices, |i| {
                                    songs.get(i).map(|s| BatchItem::Song(Box::new(s.clone())))
                                });
                            (Task::none(), SimilarAction::AddBatchToPlaylist(payload))
                        }
                        LibraryContextEntry::GetInfo => {
                            use nokkvi_data::types::info_modal::InfoModalItem;
                            let item = InfoModalItem::from_song(song);
                            (Task::none(), SimilarAction::ShowInfo(Box::new(item)))
                        }
                        LibraryContextEntry::ShowInFolder => {
                            (Task::none(), SimilarAction::ShowInFolder(song.path.clone()))
                        }
                        LibraryContextEntry::FindSimilar => (
                            Task::none(),
                            SimilarAction::FindSimilar(song.id.clone(), song.title.clone()),
                        ),
                        LibraryContextEntry::TopSongs => (
                            Task::none(),
                            SimilarAction::FindTopSongs(
                                song.artist.clone(),
                                format!("Top Songs: {}", song.artist),
                            ),
                        ),
                        LibraryContextEntry::Separator => (Task::none(), SimilarAction::None),
                    }
                } else {
                    (Task::none(), SimilarAction::None)
                }
            }
        }
    }

    /// Build the view
    pub fn view<'a>(&'a self, data: SimilarViewData<'a>) -> Element<'a, SimilarMessage> {
        // Replace custom header with standard view_header
        let header_prefix = if data.label.is_empty() {
            "similar to selected".to_string()
        } else {
            // "Similar to: Song Name"
            data.label.to_lowercase()
        };

        // We don't have sort mode options for Similar, we can pass an empty slice
        // to hide the sort dropdown entirely, and pass the header_prefix as the view title.
        let empty_options: &[String] = &[];

        // Columns-cog dropdown for the view header. Emits its own
        // `OpenMenu::CheckboxDropdownSimilar` variant since `View` has no
        // `Similar` member to disambiguate against.
        let column_dropdown: Element<'a, SimilarMessage> =
            crate::widgets::checkbox_dropdown::similar_columns_dropdown(
                vec![
                    (
                        SimilarColumn::Select,
                        "Select",
                        self.column_visibility.select,
                    ),
                    (SimilarColumn::Index, "Index", self.column_visibility.index),
                    (
                        SimilarColumn::Thumbnail,
                        "Thumbnail",
                        self.column_visibility.thumbnail,
                    ),
                    (SimilarColumn::Album, "Album", self.column_visibility.album),
                    (
                        SimilarColumn::Duration,
                        "Duration",
                        self.column_visibility.duration,
                    ),
                    (SimilarColumn::Love, "Love", self.column_visibility.love),
                ],
                SimilarMessage::ToggleColumnVisible,
                SimilarMessage::SetOpenMenu,
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into();

        let header = widgets::view_header::view_header(ViewHeaderConfig {
            current_view: header_prefix,
            view_options: empty_options,
            sort_ascending: true, // ascending dummy
            search_query: "",     // no search query
            filtered_count: data.songs.len(),
            total_count: data.songs.len(),
            item_type: "songs",
            search_input_id: crate::views::SIMILAR_SEARCH_ID,
            on_view_selected: Box::new(|_| SimilarMessage::NoOp),
            show_search: false, // Hide search
            on_search_change: Box::new(|_| SimilarMessage::NoOp),
            // No sort/refresh/center/add buttons; only the columns-cog dropdown.
            buttons: vec![HeaderButton::Trailing(column_dropdown)],
            on_roulette: None, // Similar lives only in the browsing panel — no roulette
        });

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the visible row count.
        let header = crate::widgets::slot_list::compose_header_with_select(
            self.column_visibility.select,
            self.common.select_all_state(data.songs.len()),
            SimilarMessage::SlotList(SlotListPageMessage::SelectAllToggle),
            header,
        );

        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let slot_list_chrome = chrome_height_with_select_header(select_header_visible);

        // Layout config
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
            slot_list_chrome,
        };

        // Loading state
        if data.loading && data.songs.is_empty() {
            let loading_text = if data.label.starts_with("Top Songs") {
                "Loading top songs…"
            } else {
                "Loading similar songs…"
            };

            return widgets::base_slot_list_empty_state(header, loading_text, &layout_config);
        }

        // Empty state (no results loaded yet)
        if data.songs.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "Right-click a track, album, or artist\nand select 'Find Similar' to discover new music.",
                &layout_config,
            );
        }

        let vertical_artwork_chrome =
            crate::widgets::base_slot_list_layout::vertical_artwork_chrome(&layout_config);
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            slot_list_chrome + vertical_artwork_chrome,
        )
        .with_modifiers(data.modifiers);

        let songs = data.songs;
        let song_artwork = data.album_art;
        let center_index = self.common.slot_list.get_center_item_index(songs.len());
        let open_menu_for_rows = data.open_menu;
        let column_visibility = self.column_visibility;

        // Render slot list
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            songs,
            &config,
            SimilarMessage::SlotList(SlotListPageMessage::NavigateUp),
            SimilarMessage::SlotList(SlotListPageMessage::NavigateDown),
            {
                let total = songs.len();
                move |f| {
                    SimilarMessage::SlotList(SlotListPageMessage::ScrollSeek(
                        (f * total as f32) as usize,
                    ))
                }
            },
            Some(crate::widgets::slot_list::SlotHoverCallback::for_slot_list(
                SimilarMessage::SlotList,
            )),
            |song, ctx| {
                let song_title = song.title.clone();
                let song_artist = song.artist.clone();
                let song_album = song.album.clone();
                let album_id = song.album_id.clone();
                let duration = song.duration;
                let is_starred = song.starred;

                use crate::widgets::slot_list::{
                    SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
                    slot_list_text,
                };
                let style = SlotListSlotStyle::for_slot(
                    ctx.is_center,
                    false,
                    ctx.is_selected,
                    ctx.has_multi_selection,
                    ctx.opacity,
                    0,
                );

                let m = ctx.metrics;
                let artwork_size = m.artwork_size;
                let title_size = m.title_size_lg;
                let subtitle_size = m.subtitle_size;
                let metadata_size = m.metadata_size;
                let star_size = m.star_size;
                let index_size = m.metadata_size;

                // Title slot grows when other columns hide so the row never
                // collapses into empty filler.
                let title_portion: u16 = {
                    let mut p: u16 = 45;
                    if !column_visibility.album {
                        p += 30;
                    }
                    if !column_visibility.duration {
                        p += 15;
                    }
                    if !column_visibility.love {
                        p += 5;
                    }
                    p
                };

                // Layout: [Index?] [Art?] [Title/Artist] [Album?] [Duration?] [Heart?]
                let mut content_row = iced::widget::Row::new()
                    .spacing(6.0)
                    .align_y(Alignment::Center);
                if column_visibility.index {
                    content_row = content_row.push(slot_list_index_column(
                        ctx.item_index,
                        index_size,
                        style,
                        ctx.opacity,
                    ));
                }
                if column_visibility.thumbnail {
                    use crate::widgets::slot_list::slot_list_artwork_column;
                    let artwork_handle = album_id.as_ref().and_then(|id| song_artwork.get(id));
                    content_row = content_row.push(slot_list_artwork_column(
                        artwork_handle,
                        artwork_size,
                        ctx.is_center,
                        false,
                        ctx.opacity,
                    ));
                }
                content_row = content_row.push({
                    use crate::widgets::slot_list::slot_list_text_column;
                    slot_list_text_column(
                        song_title,
                        None,
                        song_artist,
                        None,
                        title_size,
                        subtitle_size,
                        style,
                        ctx.is_center,
                        title_portion,
                    )
                });
                if column_visibility.album {
                    use crate::widgets::slot_list::slot_list_metadata_column;
                    content_row = content_row.push(slot_list_metadata_column(
                        song_album,
                        None,
                        metadata_size,
                        style,
                        30,
                    ));
                }
                if column_visibility.duration {
                    let duration_str = formatters::format_time(duration);
                    content_row = content_row.push(
                        container(slot_list_text(
                            duration_str,
                            metadata_size,
                            style.subtext_color,
                        ))
                        .width(Length::FillPortion(15))
                        .height(Length::Fill)
                        .align_x(Alignment::End)
                        .align_y(Alignment::Center),
                    );
                }
                if column_visibility.love {
                    content_row = content_row.push(
                        container({
                            use crate::widgets::slot_list::slot_list_favorite_icon;
                            slot_list_favorite_icon(
                                is_starred,
                                ctx.is_center,
                                false,
                                ctx.opacity,
                                star_size,
                                "heart",
                                Some(SimilarMessage::ClickToggleStar(ctx.item_index)),
                            )
                        })
                        .width(Length::FillPortion(5))
                        .padding(iced::Padding {
                            left: 4.0,
                            right: 4.0,
                            ..Default::default()
                        })
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center),
                    );
                }
                let content = content_row
                    .padding(iced::Padding {
                        left: SLOT_LIST_SLOT_PADDING,
                        right: 4.0,
                        top: 4.0,
                        bottom: 4.0,
                    })
                    .height(Length::Fill);

                let clickable = container(content)
                    .style(move |_theme| style.to_container_style())
                    .width(Length::Fill);

                let slot_button = crate::widgets::slot_list::highlight_only_slot_button(
                    clickable,
                    &ctx,
                    SimilarMessage::SlotList,
                );

                use crate::widgets::context_menu::{similar_entries, wrap_similar_row};
                let cm_row = wrap_similar_row(
                    ctx.item_index,
                    slot_button,
                    similar_entries(),
                    open_menu_for_rows,
                    SimilarMessage::ContextMenuAction,
                    SimilarMessage::SetOpenMenu,
                );
                crate::widgets::slot_list::wrap_with_select_column(
                    select_header_visible,
                    ctx.is_selected,
                    ctx.item_index,
                    |idx| SimilarMessage::SlotList(SlotListPageMessage::SelectionToggle(idx)),
                    cm_row,
                )
            },
        );

        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_menu;

        let centered_song = center_index.and_then(|idx| songs.get(idx));
        let artwork_handle = centered_song
            .and_then(|song| song.album_id.as_ref())
            .and_then(|album_id| data.large_artwork.get(album_id));
        // No refresh action wired up for Similar artwork — pass inert
        // controlled-component arguments. The helper short-circuits because
        // `on_refresh` is None.
        let artwork_content = Some(single_artwork_panel_with_menu::<SimilarMessage>(
            artwork_handle,
            None,
            false,
            None,
            |_| SimilarMessage::SetOpenMenu(None),
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(SimilarMessage::ArtworkColumnDrag),
            Some(SimilarMessage::ArtworkColumnVerticalDrag),
        )
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

use crate::app_message::Message;

impl super::ViewPage for SimilarPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn search_input_id(&self) -> &'static str {
        super::SIMILAR_SEARCH_ID
    }

    // No sort mode cycling for Similar view
    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        None
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::NoOp
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Similar(SimilarMessage::SlotList(
            SlotListPageMessage::AddCenterToQueue,
        )))
    }

    fn slot_list_message(&self, msg: SlotListPageMessage) -> Message {
        Message::Similar(SimilarMessage::SlotList(msg))
    }
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::song::Song;

    use super::*;
    use crate::widgets::context_menu::LibraryContextEntry;

    fn test_song(id: &str, title: &str, artist: &str) -> Song {
        Song {
            id: id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            artist_id: None,
            album: "Album".to_string(),
            album_id: None,
            cover_art: None,
            duration: 180,
            track: None,
            disc: None,
            year: None,
            genre: None,
            path: String::new(),
            size: 0,
            bitrate: None,
            starred: false,
            play_count: None,
            bpm: None,
            channels: None,
            comment: None,
            rating: None,
            album_artist: None,
            suffix: None,
            sample_rate: None,
            created_at: None,
            play_date: None,
            compilation: None,
            bit_depth: None,
            updated_at: None,
            replay_gain: None,
            tags: None,
            participants: None,
            original_position: None,
        }
    }

    #[test]
    fn star_toggle_emits_action() {
        let mut page = SimilarPage::new();
        let songs = vec![test_song("s1", "Song", "Artist")];
        let (_, action) = page.update(SimilarMessage::ClickToggleStar(0), &songs);
        assert!(
            matches!(action, SimilarAction::ToggleStar(ref id, true) if id == "s1"),
            "expected ToggleStar(s1, true), got {action:?}",
        );
    }

    #[test]
    fn star_toggle_unstar_when_starred() {
        let mut page = SimilarPage::new();
        let mut song = test_song("s1", "Song", "Artist");
        song.starred = true;
        let songs = vec![song];
        let (_, action) = page.update(SimilarMessage::ClickToggleStar(0), &songs);
        assert!(
            matches!(action, SimilarAction::ToggleStar(ref id, false) if id == "s1"),
            "expected ToggleStar(s1, false), got {action:?}",
        );
    }

    #[test]
    fn context_add_to_queue_emits_batch() {
        let mut page = SimilarPage::new();
        let songs = vec![test_song("s1", "Song", "Artist")];
        page.common
            .handle_slot_click(0, songs.len(), Default::default());
        let (_, action) = page.update(
            SimilarMessage::ContextMenuAction(0, LibraryContextEntry::AddToQueue),
            &songs,
        );
        assert!(
            matches!(action, SimilarAction::AddBatchToQueue(ref p) if !p.items.is_empty()),
            "expected non-empty batch, got {action:?}",
        );
    }

    #[test]
    fn context_find_similar_emits_recursive_action() {
        let mut page = SimilarPage::new();
        let songs = vec![test_song("s1", "Test Song", "Test Artist")];
        page.common
            .handle_slot_click(0, songs.len(), Default::default());
        let (_, action) = page.update(
            SimilarMessage::ContextMenuAction(0, LibraryContextEntry::FindSimilar),
            &songs,
        );
        match action {
            SimilarAction::FindSimilar(id, title) => {
                assert_eq!(id, "s1");
                assert_eq!(title, "Test Song");
            }
            other => panic!("expected FindSimilar, got {other:?}"),
        }
    }

    #[test]
    fn context_top_songs_emits_find_top_songs_action() {
        let mut page = SimilarPage::new();
        let songs = vec![test_song("s1", "Test Song", "Radiohead")];
        page.common
            .handle_slot_click(0, songs.len(), Default::default());
        let (_, action) = page.update(
            SimilarMessage::ContextMenuAction(0, LibraryContextEntry::TopSongs),
            &songs,
        );
        match action {
            SimilarAction::FindTopSongs(artist, label) => {
                assert_eq!(artist, "Radiohead");
                assert!(label.contains("Radiohead"));
            }
            other => panic!("expected FindTopSongs, got {other:?}"),
        }
    }

    #[test]
    fn context_replace_queue_with_all_emits_full_batch() {
        let mut page = SimilarPage::new();
        let songs = vec![
            test_song("s1", "Song 1", "Artist"),
            test_song("s2", "Song 2", "Artist"),
        ];
        let (_, action) = page.update(
            SimilarMessage::ContextMenuAction(0, LibraryContextEntry::ReplaceQueueWithAllFound),
            &songs,
        );
        match action {
            SimilarAction::PlayBatch(batch) => {
                assert_eq!(batch.items.len(), 2, "Batch should contain all found songs");
            }
            other => panic!("expected PlayBatch, got {other:?}"),
        }
    }

    #[test]
    fn context_add_all_to_playlist_emits_full_batch() {
        let mut page = SimilarPage::new();
        let songs = vec![
            test_song("s1", "Song 1", "Artist"),
            test_song("s2", "Song 2", "Artist"),
        ];
        let (_, action) = page.update(
            SimilarMessage::ContextMenuAction(0, LibraryContextEntry::AddAllFoundToPlaylist),
            &songs,
        );
        match action {
            SimilarAction::AddBatchToPlaylist(batch) => {
                assert_eq!(batch.items.len(), 2, "Batch should contain all found songs");
            }
            other => panic!("expected AddBatchToPlaylist, got {other:?}"),
        }
    }

    #[test]
    fn context_add_all_to_queue_emits_full_batch() {
        let mut page = SimilarPage::new();
        let songs = vec![
            test_song("s1", "Song 1", "Artist"),
            test_song("s2", "Song 2", "Artist"),
        ];
        let (_, action) = page.update(
            SimilarMessage::ContextMenuAction(0, LibraryContextEntry::AddAllFoundToQueue),
            &songs,
        );
        match action {
            SimilarAction::AddBatchToQueue(batch) => {
                assert_eq!(batch.items.len(), 2, "Batch should contain all found songs");
            }
            other => panic!("expected AddBatchToQueue, got {other:?}"),
        }
    }

    #[test]
    fn out_of_bounds_star_toggle_is_noop() {
        let mut page = SimilarPage::new();
        let songs = vec![test_song("s1", "Song", "Artist")];
        let (_, action) = page.update(SimilarMessage::ClickToggleStar(99), &songs);
        assert!(
            matches!(action, SimilarAction::None),
            "expected None for out-of-bounds, got {action:?}",
        );
    }
}
