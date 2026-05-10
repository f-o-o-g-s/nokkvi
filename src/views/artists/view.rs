//! Artists view — `impl ArtistsPage { fn view, fn render_artist_row, fn render_album_child_row }`.
//!
//! Rendering for the artists page, plus the per-mode column-visibility
//! helpers (`artists_stars_visible`, `artists_plays_visible`).
//!
//! Update/state logic lives in `update.rs`; types live in `mod.rs`.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length,
    widget::{Row, button, container, image},
};
use nokkvi_data::backend::{albums::AlbumUIViewData, artists::ArtistUIViewData};

use super::{
    super::expansion::SlotListEntry, ArtistsColumn, ArtistsMessage, ArtistsPage, ArtistsViewData,
};
use crate::widgets::{
    self,
    view_header::{HeaderButton, SortMode, ViewHeaderConfig},
};

/// Stars auto-show when sort = Rating regardless of toggle.
pub(crate) fn artists_stars_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::Rating)
}

/// Plays auto-show when sort = MostPlayed regardless of toggle.
pub(crate) fn artists_plays_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::MostPlayed)
}

impl ArtistsPage {
    /// Build the view
    pub fn view<'a>(&'a self, data: ArtistsViewData<'a>) -> Element<'a, ArtistsMessage> {
        // Build the columns-visibility dropdown for the artists view header.
        let column_dropdown: Element<'a, ArtistsMessage> =
            crate::widgets::checkbox_dropdown::view_columns_dropdown(
                crate::View::Artists,
                vec![
                    (
                        ArtistsColumn::Select,
                        "Select",
                        self.column_visibility.select,
                    ),
                    (ArtistsColumn::Index, "Index", self.column_visibility.index),
                    (
                        ArtistsColumn::Thumbnail,
                        "Thumbnail",
                        self.column_visibility.thumbnail,
                    ),
                    (ArtistsColumn::Stars, "Stars", self.column_visibility.stars),
                    (
                        ArtistsColumn::AlbumCount,
                        "Album Count",
                        self.column_visibility.albumcount,
                    ),
                    (
                        ArtistsColumn::SongCount,
                        "Song Count",
                        self.column_visibility.songcount,
                    ),
                    (ArtistsColumn::Plays, "Plays", self.column_visibility.plays),
                    (ArtistsColumn::Love, "Love", self.column_visibility.love),
                ],
                ArtistsMessage::ToggleColumnVisible,
                ArtistsMessage::SetOpenMenu,
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into();

        let header = widgets::view_header::view_header(ViewHeaderConfig {
            current_view: self.common.current_sort_mode,
            view_options: crate::views::sort_api::sort_modes_for_view(crate::View::Artists),
            sort_ascending: self.common.sort_ascending,
            search_query: &self.common.search_query,
            filtered_count: data.artists.len(),
            total_count: data.total_artist_count,
            item_type: "artists",
            search_input_id: crate::views::ARTISTS_SEARCH_ID,
            on_view_selected: Box::new(ArtistsMessage::SortModeSelected),
            show_search: true,
            on_search_change: Box::new(ArtistsMessage::SearchQueryChanged),
            buttons: {
                let mut btns = vec![
                    HeaderButton::SortToggle(ArtistsMessage::ToggleSortOrder),
                    HeaderButton::Refresh(ArtistsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::RefreshViewData,
                    )),
                ];
                // Hidden in the browsing panel — the narrower pane needs the
                // header space for sort/refresh/columns/search, and the user
                // already has the main-pane button when they want to center.
                if !data.in_browsing_panel {
                    btns.push(HeaderButton::CenterOnPlaying(ArtistsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::CenterOnPlaying,
                    )));
                }
                btns.push(HeaderButton::Trailing(column_dropdown));
                btns
            },
            // Roulette is main-pane only — see Albums view for rationale.
            on_roulette: if data.in_browsing_panel {
                None
            } else {
                Some(ArtistsMessage::Roulette)
            },
        });

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *flattened* (visible) row count.
        let header = {
            let flattened_len = self.expansion.flattened_len(data.artists);
            crate::widgets::slot_list::compose_header_with_select(
                self.column_visibility.select,
                self.common.select_all_state(flattened_len),
                ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
                header,
            )
        };

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
        };

        // If loading, show header with loading message
        if data.loading {
            return widgets::base_slot_list_empty_state(header, "Loading...", &layout_config);
        }

        // If no artists match search, show message but keep the header
        if data.artists.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No artists match your search.",
                &layout_config,
            );
        }

        // Configure slot list with artists-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            chrome_height_with_select_header(select_header_visible),
        )
        .with_modifiers(data.modifiers);
        let artists = data.artists; // Borrow slice to extend lifetime
        let artist_art = data.artist_art;
        let open_menu_for_rows = data.open_menu;

        // Build flattened list (artists + injected albums when expanded)
        let flattened = self.expansion.build_flattened_list(artists, |a| &a.id);
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateUp),
            ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::NavigateDown),
            {
                let total = flattened.len();
                move |f| {
                    ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::ScrollSeek(
                        (f * total as f32) as usize,
                    ))
                }
            },
            |entry, ctx| match entry {
                SlotListEntry::Parent(artist) => {
                    let row = self.render_artist_row(
                        artist,
                        &ctx,
                        artist_art,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        |i| {
                            ArtistsMessage::SlotList(
                                crate::widgets::SlotListPageMessage::SelectionToggle(i),
                            )
                        },
                        row,
                    )
                }
                SlotListEntry::Child(album, _parent_artist_id) => {
                    let sub_index_label =
                        self.expansion
                            .child_sub_index_label(ctx.item_index, artists, |a| &a.id);
                    let row = self.render_album_child_row(
                        album,
                        &ctx,
                        &sub_index_label,
                        data.album_art,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        |i| {
                            ArtistsMessage::SlotList(
                                crate::widgets::SlotListPageMessage::SelectionToggle(i),
                            )
                        },
                        row,
                    )
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_pill;

        // Build artwork column — show parent artist art even when on a child album row
        let centered_artist = center_index.and_then(|idx| match flattened.get(idx) {
            Some(SlotListEntry::Parent(artist)) => {
                Some(artists.iter().find(|a| a.id == artist.id)?)
            }
            Some(SlotListEntry::Child(_, parent_id)) => artists.iter().find(|a| &a.id == parent_id),
            None => None,
        });

        let artwork_handle = centered_artist.and_then(|artist| data.large_artwork.get(&artist.id));
        let active_dominant_color =
            centered_artist.and_then(|artist| data.dominant_colors.get(&artist.id).copied());

        let pill_content = centered_artist
            .filter(|_| crate::theme::artists_artwork_overlay())
            .map(|artist| {
                use iced::widget::{button, column, text};

                use crate::theme;

                let mut col = column![
                    text(artist.name.clone())
                        .size(24)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..theme::ui_font()
                        })
                        .color(theme::fg0()),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center);

                use crate::widgets::metadata_pill::{auth_status_row, dot_row, play_stats_row};

                let mut lib_stats = vec![
                    format!("{} albums", artist.album_count),
                    format!("{} songs", artist.song_count),
                ];
                if let Some(plays) = artist.play_count {
                    lib_stats.push(format!("{plays} plays"));
                }

                if let Some(row) = dot_row::<ArtistsMessage>(lib_stats, 14.0, theme::fg2()) {
                    col = col.push(
                        iced::widget::container(row)
                            .width(iced::Length::Shrink)
                            .center_x(iced::Length::Fill),
                    );
                }

                if let Some(row) =
                    play_stats_row::<ArtistsMessage>(None, artist.play_date.as_deref())
                {
                    col = col.push(
                        iced::widget::container(row)
                            .width(iced::Length::Shrink)
                            .center_x(iced::Length::Fill),
                    );
                }

                if let Some(row) =
                    auth_status_row::<ArtistsMessage>(artist.is_starred, artist.rating)
                {
                    col = col.push(row);
                }

                // Biography section (artists-specific)
                if let Some(bio) = &artist.biography
                    && !bio.is_empty()
                {
                    let bio_preview: String = bio.chars().take(350).collect();
                    let bio_preview = if bio.chars().count() > 350 {
                        format!("{}...", bio_preview.trim_end())
                    } else {
                        bio_preview
                    };

                    let mut bio_col = column![
                        text(bio_preview)
                            .size(13)
                            .color(theme::fg1())
                            .font(theme::ui_font())
                            .center()
                    ]
                    .spacing(4)
                    .align_x(iced::Alignment::Center);

                    if let Some(url) = &artist.external_url {
                        let read_more_btn = button(
                            text("Read more on Last.fm")
                                .size(11)
                                .color(theme::accent_bright())
                                .font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..theme::ui_font()
                                }),
                        )
                        .on_press(ArtistsMessage::OpenExternalUrl(url.clone()))
                        .padding(iced::Padding {
                            top: 2.0,
                            bottom: 2.0,
                            left: 6.0,
                            right: 6.0,
                        })
                        .style(|_theme, status| {
                            let opacity = match status {
                                iced::widget::button::Status::Hovered
                                | iced::widget::button::Status::Pressed => 0.75,
                                _ => 1.0,
                            };
                            let mut color = theme::accent_bright();
                            color.a = opacity;
                            iced::widget::button::Style {
                                background: None,
                                border: iced::Border {
                                    width: 0.0,
                                    ..Default::default()
                                },
                                text_color: color,
                                ..Default::default()
                            }
                        });
                        bio_col = bio_col.push(read_more_btn);
                    }

                    col = col.push(bio_col);
                }

                col.into()
            });

        // Artists artwork panel has no refresh action wired up, but still
        // needs the controlled-component plumbing arguments. They're inert
        // when `on_refresh` is None — the helper short-circuits.
        let artwork_content = Some(single_artwork_panel_with_pill::<ArtistsMessage>(
            artwork_handle,
            pill_content,
            active_dominant_color,
            None,
            false,
            None,
            |_| ArtistsMessage::SetOpenMenu(None),
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(ArtistsMessage::ArtworkColumnDrag),
        )
    }

    /// Render an artist row in the slot list (standard layout)
    fn render_artist_row<'a>(
        &self,
        artist: &ArtistUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        artist_art: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, ArtistsMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
        };

        let artist_id = artist.id.clone();
        let artist_name = artist.name.clone();
        let album_count = artist.album_count;
        let song_count = artist.song_count;
        let is_starred = artist.is_starred;
        let rating = artist.rating.unwrap_or(0).min(5) as usize;
        let play_count = artist.play_count.unwrap_or(0);

        // Check if this artist is the expanded one (gives it the group highlight)
        let is_expanded = self.expansion.is_expanded_parent(&artist.id);
        let style = SlotListSlotStyle::for_slot(
            ctx.is_center,
            is_expanded,
            ctx.is_selected,
            ctx.has_multi_selection,
            ctx.opacity,
            0,
        );

        let m = ctx.metrics;
        let artwork_size = m.artwork_size;
        let title_size = m.title_size;
        let metadata_size = m.metadata_size;
        let star_size = m.star_size;
        let index_size = m.metadata_size;

        // Per-column visibility (sort overrides Stars/Plays toggles).
        let sort = self.common.current_sort_mode;
        let vis = self.column_visibility;
        let show_stars = artists_stars_visible(sort, vis.stars);
        let show_albumcount = vis.albumcount;
        let show_songcount = vis.songcount;
        let show_plays = artists_plays_visible(sort, vis.plays);
        let show_love = vis.love;

        // Fixed portions for each toggleable column. Name column expands to
        // fill whatever's left.
        const STARS_PORTION: u16 = 12;
        const ALBUMCOUNT_PORTION: u16 = 16;
        const SONGCOUNT_PORTION: u16 = 16;
        const PLAYS_PORTION: u16 = 16;
        const LOVE_PORTION: u16 = 5;
        let mut consumed: u16 = 0;
        if show_stars {
            consumed += STARS_PORTION;
        }
        if show_albumcount {
            consumed += ALBUMCOUNT_PORTION;
        }
        if show_songcount {
            consumed += SONGCOUNT_PORTION;
        }
        if show_plays {
            consumed += PLAYS_PORTION;
        }
        if show_love {
            consumed += LOVE_PORTION;
        }
        let name_portion = 100u16.saturating_sub(consumed).max(20);

        // Leading columns (Index, Thumbnail) are now user-toggleable; Name is always-on.
        let mut content_row = Row::new().spacing(6.0).align_y(Alignment::Center);
        if vis.index {
            content_row = content_row.push(slot_list_index_column(
                ctx.item_index,
                index_size,
                style,
                ctx.opacity,
            ));
        }
        if vis.thumbnail {
            use crate::widgets::slot_list::slot_list_artwork_column;
            content_row = content_row.push(slot_list_artwork_column(
                artist_art.get(&artist_id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ));
        }
        content_row = content_row.push({
            let title_click = Some(ArtistsMessage::ContextMenuAction(
                ctx.item_index,
                crate::widgets::context_menu::LibraryContextEntry::GetInfo,
            ));
            let link_color = if ctx.is_center {
                style.text_color
            } else {
                crate::theme::accent_bright()
            };
            container(
                crate::widgets::link_text::LinkText::new(artist_name)
                    .size(title_size)
                    .color(style.text_color)
                    .hover_color(link_color)
                    .on_press(title_click),
            )
            .width(Length::FillPortion(name_portion))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center)
        });

        // Stars column (auto-show on sort=Rating). Replaces the old inline
        // star widget under the artist name.
        if show_stars {
            use crate::widgets::slot_list::slot_list_star_rating;
            let star_icon_size = m.title_size;
            let idx = ctx.item_index;
            content_row = content_row.push(slot_list_star_rating(
                rating,
                star_icon_size,
                ctx.is_center,
                ctx.opacity,
                Some(STARS_PORTION),
                Some(move |star: usize| ArtistsMessage::ClickSetRating(idx, star)),
            ));
        }

        // Album Count column.
        if show_albumcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            let album_text = if album_count == 1 {
                "1 album".to_string()
            } else {
                format!("{album_count} albums")
            };
            let idx = ctx.item_index;
            content_row = content_row.push(slot_list_metadata_column(
                album_text,
                Some(ArtistsMessage::FocusAndExpand(idx)),
                metadata_size,
                style,
                ALBUMCOUNT_PORTION,
            ));
        }

        // Song Count column.
        if show_songcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                format!("{song_count} songs"),
                None,
                metadata_size,
                style,
                SONGCOUNT_PORTION,
            ));
        }

        // Plays column (auto-show on sort=MostPlayed).
        if show_plays {
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                format!("{play_count} plays"),
                None,
                metadata_size,
                style,
                PLAYS_PORTION,
            ));
        }

        // Heart (Love) column.
        if show_love {
            use crate::widgets::slot_list::slot_list_favorite_icon;
            content_row = content_row.push(
                container(slot_list_favorite_icon(
                    is_starred,
                    ctx.is_center,
                    false,
                    ctx.opacity,
                    star_size,
                    "heart",
                    Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
                ))
                .width(Length::FillPortion(LOVE_PORTION))
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

        // Wrap in clickable container
        let clickable = container(content)
            .style(move |_theme| style.to_container_style())
            .width(Length::Fill);

        let slot_button = button(clickable)
            .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::SetOffset(
                    ctx.item_index,
                    ctx.modifiers,
                ))
            } else if ctx.is_center {
                ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::ActivateCenter)
            } else if stable_viewport {
                ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::SetOffset(
                    ctx.item_index,
                    ctx.modifiers,
                ))
            } else {
                ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::ClickPlay(
                    ctx.item_index,
                ))
            })
            .style(|_theme, _status| button::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .padding(0)
            .width(Length::Fill);

        use crate::widgets::context_menu::{artist_entries_with_folder, wrap_library_row};
        wrap_library_row(
            crate::View::Artists,
            ctx.item_index,
            slot_button,
            artist_entries_with_folder(),
            open_menu,
            ArtistsMessage::ContextMenuAction,
            ArtistsMessage::SetOpenMenu,
        )
    }

    /// Render a child album row in the slot list (indented, simpler layout)
    fn render_album_child_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        sub_index_label: &str,
        album_art: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, ArtistsMessage> {
        let navigate_msg = ArtistsMessage::NavigateAndExpandAlbum(album.id.clone());
        let album_el = super::super::expansion::render_child_album_row(
            album,
            ctx,
            sub_index_label,
            album_art.get(&album.id),
            self.column_visibility.thumbnail,
            ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::ActivateCenter),
            if stable_viewport {
                ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::SetOffset(
                    ctx.item_index,
                    ctx.modifiers,
                ))
            } else {
                ArtistsMessage::SlotList(crate::widgets::SlotListPageMessage::ClickPlay(
                    ctx.item_index,
                ))
            },
            false, // artist is already the parent row
            Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
            Some(navigate_msg.clone()),
            Some(navigate_msg),
            None, // artist click - artist is already the parent
            1,    // depth 1: child albums under artist
        );

        use crate::widgets::context_menu::{library_entries_with_folder, wrap_library_row};
        wrap_library_row(
            crate::View::Artists,
            ctx.item_index,
            album_el,
            library_entries_with_folder(),
            open_menu,
            ArtistsMessage::ContextMenuAction,
            ArtistsMessage::SetOpenMenu,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artists_stars_visible_auto_shows_on_rating_sort() {
        assert!(artists_stars_visible(SortMode::Rating, false));
        assert!(artists_stars_visible(SortMode::Rating, true));
    }

    #[test]
    fn artists_stars_visible_follows_toggle_for_other_sorts() {
        assert!(!artists_stars_visible(SortMode::Name, false));
        assert!(artists_stars_visible(SortMode::Name, true));
    }

    #[test]
    fn artists_plays_visible_auto_shows_on_most_played() {
        assert!(artists_plays_visible(SortMode::MostPlayed, false));
        assert!(artists_plays_visible(SortMode::MostPlayed, true));
    }

    #[test]
    fn artists_plays_visible_follows_toggle_for_other_sorts() {
        assert!(!artists_plays_visible(SortMode::Name, false));
        assert!(artists_plays_visible(SortMode::Name, true));
    }
}
