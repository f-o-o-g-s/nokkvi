//! Shared song-list pane renderer.
//!
//! `song_list_pane` is the single rendering implementation for a scrollable,
//! drag-reorderable list of `QueueSongUIViewData` rows with the queue's column
//! layout (index / thumbnail / title+artist / album+genre / rating / duration /
//! plays / love). It is extracted verbatim from the queue view so the queue and
//! the (future) playlist editor render through one source of truth — no visual
//! drift.
//!
//! The pane is generic over the caller's message type `M`. Callers map the
//! neutral [`SongListRowEvent`] vocabulary to their own `Message` via a single
//! `on_event` closure, and supply the per-row context-menu chrome via a
//! `build_context_menu` closure (the menu is caller-specific — the queue's
//! 11-entry menu is not shared).
//!
//! The per-mode column-visibility helpers (`rating_column_visible`,
//! `album_column_visible`, …) live here too, beside the renderer that consumes
//! them. The queue view re-exports them for its existing unit tests.

use iced::{
    Alignment, Element, Length,
    widget::{Row, column, container},
};
use nokkvi_data::backend::queue::QueueSongUIViewData;

use super::queue::{QueueColumnVisibility, QueueSortMode};
use crate::widgets::{
    SlotListPageMessage, SlotListView,
    drag_column::DragEvent,
    slot_list::{SlotListConfig, SlotListRowContext},
};

/// Hide the queue stars column when the queue panel is narrower than this.
/// Queue panel is measured (via `iced::widget::responsive`), so this fires
/// correctly in split-view where the queue is roughly half the window.
pub(crate) const BREAKPOINT_HIDE_QUEUE_STARS: f32 = 400.0;

/// Pure decision: should the queue's stars rating column be rendered?
///
/// Two independent gates: the user toggle (always wins when off) and the
/// responsive width gate (always wins when below the breakpoint).
pub(crate) fn rating_column_visible(
    _sort: Option<QueueSortMode>,
    panel_width: f32,
    user_visible: bool,
) -> bool {
    user_visible && panel_width >= BREAKPOINT_HIDE_QUEUE_STARS
}

/// Pure decision: should the album column be rendered? User toggle only —
/// no responsive gate yet (the album column carries inline genre when
/// sort = Genre, so hiding it on narrow widths is a separate question).
pub(crate) fn album_column_visible(user_visible: bool) -> bool {
    user_visible
}

/// Pure decision: should the duration column be rendered? User toggle only.
pub(crate) fn duration_column_visible(user_visible: bool) -> bool {
    user_visible
}

/// Pure decision: should the love (heart) column be rendered? User toggle only.
pub(crate) fn love_column_visible(user_visible: bool) -> bool {
    user_visible
}

/// Pure decision: should the plays column be rendered? Either the user toggle
/// is on, OR an applied Most Played sort is in effect (auto-show so the user
/// always sees the data they're sorting by). `sort` is the *applied* mode —
/// `None` when the queue is unsorted, so the remembered mode never auto-shows.
pub(crate) fn plays_column_visible(sort: Option<QueueSortMode>, user_visible: bool) -> bool {
    user_visible || sort == Some(QueueSortMode::MostPlayed)
}

/// Pure decision: should the genre be rendered (stacked under album, or in
/// place of the album when album is hidden)? Toggle on, OR an applied Genre
/// sort is in effect — mirrors the plays-on-MostPlayed auto-show. `sort` is the
/// *applied* mode (`None` when unsorted).
pub(crate) fn genre_column_visible(sort: Option<QueueSortMode>, user_visible: bool) -> bool {
    user_visible || sort == Some(QueueSortMode::Genre)
}

/// Neutral row-interaction vocabulary. Each caller maps these to its own
/// `Message` via the `on_event` closure.
pub(crate) enum SongListRowEvent {
    /// Slot-button click, selection toggle, nav up/down, scroll-seek, hover.
    Slot(SlotListPageMessage),
    /// Drag-reorder event from the underlying `DragColumn`.
    Drag(DragEvent),
    /// Title click — `item_index`. (Queue maps to its GetInfo context action.)
    TitleClick(usize),
    /// Navigate to + expand the row's artist.
    NavArtist(String),
    /// Navigate to + expand the row's album.
    NavAlbum(String),
    /// Navigate to + expand the row's genre.
    NavGenre(String),
    /// Set the row's rating — `(item_index, star)`.
    SetRating(usize, usize),
    /// Toggle the row's love (heart) — `item_index`.
    ToggleLove(usize),
}

/// Borrowed inputs + per-row config for [`song_list_pane`]. Mirrors the values
/// the queue view feeds into the slot-list renderer today.
///
/// `slot_list`, `songs`, and `list_config` are consumed during the build pass
/// only — rows are materialized eagerly into owned `Element`s, so these carry a
/// shorter `'b` lifetime (mirroring the elided lifetimes on
/// `slot_list_view_with_drag`'s borrowed args).
///
/// `album_art` carries the element lifetime `'a`: `slot_list_artwork_column`
/// borrows the `Handle` reference into its returned `Element<'a>`, so the map
/// must outlive the produced rows. The queue's `data.album_art` is already an
/// `'a` borrow from `QueueViewData<'a>`, so this is the existing contract.
pub(crate) struct SongListPaneParams<'a, 'b> {
    pub slot_list: &'b SlotListView,
    pub songs: &'b [QueueSongUIViewData],
    pub list_config: &'b SlotListConfig,
    pub drop_indicator_slot: Option<usize>,
    pub columns: QueueColumnVisibility,
    /// The *applied* queue sort — `None` when the queue is unsorted. Drives the
    /// plays/genre auto-show columns only when a real sort is in effect.
    pub sort_mode: Option<QueueSortMode>,
    pub album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    pub current_playing_song_id: Option<String>,
    pub current_playing_entry_id: Option<u64>,
    pub stable_viewport: bool,
}

/// Render the shared song-list pane: a drag-reorderable slot list of rows with
/// the queue column layout, wrapped in the standard slot-list background.
///
/// `on_event` maps each neutral [`SongListRowEvent`] to the caller's `Message`.
/// `build_context_menu` wraps a row's slot-button element in the caller's own
/// context-menu chrome (`(slot_button, item_index) -> Element`).
pub(crate) fn song_list_pane<'a, 'b, M, F, G>(
    params: SongListPaneParams<'a, 'b>,
    on_event: F,
    build_context_menu: G,
) -> Element<'a, M>
where
    // `'static`: the slot-list row widgets (`slot_list_text_column`,
    // `LinkText`, the favorite/star helpers) require the message type to
    // outlive any element lifetime. Every iced `Message` in this codebase is
    // already `'static`, so this is no real restriction on callers.
    M: Clone + 'static,
    F: Fn(SongListRowEvent) -> M + Clone + 'a,
    // `Clone` so each row's `responsive(move ...)` closure can own its own copy
    // — the menu builder is invoked inside that `move` closure (the menu wraps
    // the width-gated slot button), and a `move` closure captures by value.
    G: Fn(Element<'a, M>, usize) -> Element<'a, M> + Clone + 'a,
{
    let SongListPaneParams {
        slot_list,
        songs,
        list_config,
        drop_indicator_slot,
        columns,
        sort_mode,
        album_art,
        current_playing_song_id,
        current_playing_entry_id,
        stable_viewport,
    } = params;

    let current_sort_mode = sort_mode;
    let column_visibility = columns;
    let show_album_column = album_column_visible(column_visibility.album);
    let show_genre_column = genre_column_visible(current_sort_mode, column_visibility.genre);
    let show_duration_column = duration_column_visible(column_visibility.duration);
    let show_love_column = love_column_visible(column_visibility.love);
    let show_plays_column = plays_column_visible(current_sort_mode, column_visibility.plays);

    // Build the render_item closure (shared between drag and non-drag paths)
    let render_item = |song: &QueueSongUIViewData, ctx: SlotListRowContext| -> Element<'a, M> {
        // Clone all data from song at the start to avoid lifetime issues
        let title = song.title.clone();
        let artist = song.artist.clone();
        let album = song.album.clone();
        let album_id = song.album_id.clone();
        let duration = song.duration.clone();
        let genre = song.genre.clone();
        let starred = song.starred;
        let rating = song.rating.unwrap_or(0).min(5) as usize;
        let play_count = song.play_count.unwrap_or(0);
        let song_id = song.id.clone();
        let artist_id = song.artist_id.clone();

        // Match on per-row entry_id — drift-immune and duplicate-aware
        // by construction. The song_id check is kept as a defense-in-
        // depth filter while the projection settles after a queue swap
        // (e.g. PlayAlbum re-stamps fresh entry_ids on rows that briefly
        // collide with the previous queue's ids).
        //
        // Suppressed while ctrl/shift is held (active multi-selection)
        // so users can clearly see which items are selected.
        let entry_id = song.entry_id;
        let is_current = !(ctx.modifiers.shift() || ctx.modifiers.control())
            && current_playing_entry_id.is_some_and(|eid| eid == entry_id)
            && current_playing_song_id.as_ref() == Some(&song_id);

        // Per-row clones of the event mapper + menu builder so each captured
        // use owns its own copy — the nested `responsive(move ...)` closure
        // below captures by value, so it must not move the renderer's single
        // copy out (which would make `render_item` `FnOnce`).
        let on_event = on_event.clone();
        let build_context_menu = build_context_menu.clone();
        // Reserved for the select-column wrapper below, before `on_event` is
        // moved into the per-row `responsive(move ...)` closure.
        let on_event_select = on_event.clone();

        // Wrap the row in `responsive(...)` so the queue-stars column hide
        // is gated by the queue panel's measured width rather than the full
        // window width. This is correct in split-view (Ctrl+E), where the
        // queue panel is roughly half the window.
        let responsive_row = iced::widget::responsive(move |size| {
            let panel_width = size.width;

            // Re-clone owned values each layout pass: the responsive
            // closure is `Fn`, so it borrows captured strings; the row
            // builders below take owned values.
            let title = title.clone();
            let artist = artist.clone();
            let album = album.clone();
            let album_id = album_id.clone();
            let duration = duration.clone();
            let genre = genre.clone();
            let artist_id = artist_id.clone();
            let on_event = on_event.clone();

            // Get centralized slot list slot styling
            use crate::widgets::slot_list::{
                SLOT_LIST_SLOT_PADDING, slot_list_index_column, slot_list_text,
            };
            let style = ctx.slot_style(is_current, is_current, 0);

            let m = ctx.metrics;
            let artwork_size = m.artwork_size;
            let title_size = m.title_size_lg;
            let subtitle_size = m.subtitle_size;
            let index_size = m.metadata_size;
            let duration_size = m.metadata_size;
            let icon_size = m.star_size;

            // Dynamic column proportions: title gets more space when album/rating columns are hidden
            let show_rating_column =
                rating_column_visible(current_sort_mode, panel_width, column_visibility.stars);
            let title_portion: u16 = if show_rating_column { 35 } else { 40 };

            // Layout: [Index?] [Thumbnail?] [Title/Artist] [Album?] [Rating?] [Duration] [Heart]
            let mut content_row = Row::new().spacing(6.0).align_y(Alignment::Center);
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
                content_row = content_row.push(slot_list_artwork_column(
                    album_art.get(&album_id),
                    artwork_size,
                    ctx.is_center,
                    is_current,
                    ctx.opacity,
                ));
            }
            content_row = content_row.push({
                use crate::widgets::slot_list::slot_list_text_column;
                let title_click = Some(on_event(SongListRowEvent::TitleClick(ctx.item_index)));
                slot_list_text_column(
                    title,
                    title_click,
                    artist.clone(),
                    Some(on_event(SongListRowEvent::NavArtist(artist_id.clone()))),
                    title_size,
                    subtitle_size,
                    style,
                    ctx.is_center || is_current,
                    title_portion,
                )
            });

            // 3. Album / genre column — slot renders when either is visible.
            //    Both → column![album, small_genre]. Album only → album.
            //    Genre only → genre at album-size font, vertically centered.
            if show_album_column || show_genre_column {
                content_row = content_row.push(
                    container({
                        let links_enabled = crate::theme::is_slot_text_links();
                        let click_album = on_event(SongListRowEvent::NavAlbum(album_id.clone()));
                        let click_genre = on_event(SongListRowEvent::NavGenre(genre.clone()));
                        let genre_label = if genre.is_empty() {
                            "Unknown".to_string()
                        } else {
                            genre.clone()
                        };
                        let stacked_genre_size = nokkvi_data::utils::scale::calculate_font_size(
                            10.0,
                            ctx.row_height,
                            ctx.scale_factor,
                        ) * ctx.scale_factor;
                        let make_link =
                            |label: String, font_size: f32, click: M| -> Element<'_, M> {
                                crate::widgets::link_text::LinkText::new(label)
                                    .size(font_size)
                                    .color(style.subtext_color)
                                    .hover_color(style.hover_text_color)
                                    .font(crate::theme::ui_font())
                                    .on_press(if links_enabled { Some(click) } else { None })
                                    .into()
                            };
                        let content: Element<'_, M> = match (show_album_column, show_genre_column) {
                            (true, true) => {
                                let album_widget = make_link(album, subtitle_size, click_album);
                                let genre_widget =
                                    make_link(genre_label, stacked_genre_size, click_genre);
                                column![album_widget, genre_widget].spacing(2.0).into()
                            }
                            (true, false) => make_link(album, subtitle_size, click_album),
                            (false, true) => make_link(genre_label, subtitle_size, click_genre),
                            (false, false) => unreachable!(),
                        };
                        content
                    })
                    .width(Length::FillPortion(30))
                    .height(Length::Fill)
                    .clip(true)
                    .align_y(Alignment::Center),
                );
            }

            // 4. Rating column — only shown for Rating sort mode (dedicated column, not inline with title)
            if show_rating_column {
                let star_icon_size = m.title_size;
                let idx = ctx.item_index;
                use crate::widgets::slot_list::slot_list_star_rating;
                let on_event_rating = on_event.clone();
                content_row = content_row.push(slot_list_star_rating(
                    rating,
                    star_icon_size,
                    style,
                    Some(15),
                    Some(move |star: usize| {
                        on_event_rating(SongListRowEvent::SetRating(idx, star))
                    }),
                ));
            }

            // 5. Duration - right aligned (user-toggleable)
            if show_duration_column {
                content_row = content_row.push(
                    container(slot_list_text(duration, duration_size, style.subtext_color))
                        .width(Length::FillPortion(10))
                        .align_x(Alignment::End)
                        .align_y(Alignment::Center),
                );
            }

            // 6. Plays - right aligned. User-toggleable, also auto-shown
            // when sort = MostPlayed.
            if show_plays_column {
                content_row = content_row.push(
                    container(slot_list_text(
                        format!("{play_count} plays"),
                        duration_size,
                        style.subtext_color,
                    ))
                    .width(Length::FillPortion(10))
                    .align_x(Alignment::End)
                    .align_y(Alignment::Center),
                );
            }

            // 7. Heart Icon - use reusable component, with symmetric padding
            // for centering (user-toggleable via columns dropdown).
            if show_love_column {
                content_row = content_row.push(
                    container({
                        use crate::widgets::slot_list::{
                            FavoriteIconKind, slot_list_favorite_icon,
                        };
                        slot_list_favorite_icon(
                            starred,
                            style,
                            icon_size,
                            FavoriteIconKind::Heart,
                            Some(on_event(SongListRowEvent::ToggleLove(ctx.item_index))),
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

            // When the love column is hidden, the rightmost trailing
            // column (duration or plays) sits flush against the slot
            // edge — bump the row's right padding to restore the
            // breathing room the love column would have provided.
            let row_right_padding = if show_love_column { 4.0 } else { 12.0 };
            let content = content_row
                .padding(iced::Padding {
                    left: SLOT_LIST_SLOT_PADDING,
                    right: row_right_padding,
                    top: 4.0,
                    bottom: 4.0,
                })
                .height(Length::Fill);

            // Wrap in clickable container
            let clickable = container(content)
                .style(move |_theme| style.to_container_style())
                .width(Length::Fill);

            // Make it interactive
            let on_event_slot = on_event.clone();
            let slot_button = crate::widgets::slot_list::primary_slot_button(
                clickable,
                &ctx,
                stable_viewport,
                move |m| on_event_slot(SongListRowEvent::Slot(m)),
            );

            // Overlay the breathing glow (pulsing inner glow + travelling
            // shimmer) on the now-playing row; a no-op pass-through otherwise.
            let glowing = crate::widgets::slot_list::glow_overlay(slot_button, style);

            // Wrap in caller-provided context menu chrome (per-row owned clone).
            build_context_menu(glowing, ctx.item_index)
        });

        // Intentionally bypasses `wrap_with_select_column_for`: selection here
        // routes through the pane's `SongListRowEvent::Slot` event channel via a
        // captured `on_event_select` closure, not a plain fn-pointer, so the
        // context-driven convenience wrapper cannot express it. Stays on the
        // base helper directly, mirroring how `for_slot_list` itself stays
        // per-call for the same reason.
        crate::widgets::slot_list::wrap_with_select_column(
            column_visibility.select,
            ctx.is_selected,
            ctx.item_index,
            move |idx| {
                on_event_select(SongListRowEvent::Slot(
                    SlotListPageMessage::SelectionToggle(idx),
                ))
            },
            responsive_row.into(),
        )
    };

    // Build slot list content: always use DragColumn so we detect drag attempts
    // (toast shown if drag is disabled for current sort/search state)
    let slot_list_content = {
        use crate::widgets::slot_list::{SlotHoverCallback, slot_list_view_with_drag};
        let on_up = on_event.clone();
        let on_down = on_event.clone();
        let on_seek = on_event.clone();
        let on_drag = on_event.clone();
        let on_hover_enter = on_event.clone();
        let on_hover_exit = on_event.clone();
        slot_list_view_with_drag(
            slot_list,
            songs,
            list_config,
            on_up(SongListRowEvent::Slot(SlotListPageMessage::NavigateUp)),
            on_down(SongListRowEvent::Slot(SlotListPageMessage::NavigateDown)),
            crate::views::scroll_seek_msg(songs.len(), move |m| on_seek(SongListRowEvent::Slot(m))),
            move |d| on_drag(SongListRowEvent::Drag(d)),
            Some(SlotHoverCallback::new(
                move |h| {
                    on_hover_enter(SongListRowEvent::Slot(SlotListPageMessage::HoverEnterSlot(
                        h,
                    )))
                },
                move |h| {
                    on_hover_exit(SongListRowEvent::Slot(SlotListPageMessage::HoverExitSlot(
                        h,
                    )))
                },
            )),
            drop_indicator_slot,
            render_item,
        )
    };

    // Wrap slot list content with standard background (prevents color bleed-through)
    use crate::widgets::slot_list::slot_list_background_container;
    slot_list_background_container(slot_list_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE_PANEL: f32 = 1200.0;

    #[test]
    fn rating_column_visible_for_all_sort_modes() {
        for sort in QueueSortMode::all() {
            assert!(
                rating_column_visible(Some(sort), WIDE_PANEL, true),
                "stars column should render for sort mode {sort:?}"
            );
        }
    }

    #[test]
    fn rating_column_hidden_below_breakpoint() {
        for sort in QueueSortMode::all() {
            assert!(
                !rating_column_visible(Some(sort), BREAKPOINT_HIDE_QUEUE_STARS - 1.0, true),
                "stars column should hide below breakpoint for {sort:?}"
            );
        }
    }

    #[test]
    fn rating_column_visible_at_breakpoint() {
        // Boundary is `>=`: the exact breakpoint width keeps stars visible.
        for sort in QueueSortMode::all() {
            assert!(
                rating_column_visible(Some(sort), BREAKPOINT_HIDE_QUEUE_STARS, true),
                "stars column should remain visible at exact breakpoint for {sort:?}"
            );
        }
    }

    #[test]
    fn rating_column_responsive_overrides_sort_mode() {
        // Width wins over sort mode: even Rating sort hides when too narrow.
        assert!(!rating_column_visible(
            Some(QueueSortMode::Rating),
            BREAKPOINT_HIDE_QUEUE_STARS - 1.0,
            true,
        ));
    }

    #[test]
    fn rating_column_user_toggle_off_overrides_wide_panel() {
        // User toggle wins over width: a wide panel still hides stars when
        // the user has toggled them off.
        for sort in QueueSortMode::all() {
            assert!(
                !rating_column_visible(Some(sort), WIDE_PANEL, false),
                "user toggle off should hide stars even at wide panel ({sort:?})"
            );
        }
    }

    #[test]
    fn rating_column_responsive_still_hides_when_user_visible_true() {
        // The two gates AND together: user wants stars visible, but the
        // panel is too narrow → still hidden.
        assert!(!rating_column_visible(
            Some(QueueSortMode::Album),
            BREAKPOINT_HIDE_QUEUE_STARS - 1.0,
            true,
        ));
    }

    #[test]
    fn album_column_visible_follows_user_toggle() {
        assert!(album_column_visible(true));
        assert!(!album_column_visible(false));
    }

    #[test]
    fn duration_column_visible_follows_user_toggle() {
        assert!(duration_column_visible(true));
        assert!(!duration_column_visible(false));
    }

    #[test]
    fn plays_column_visible_auto_shows_on_most_played() {
        // Sort overrides the user toggle: MostPlayed always shows, regardless of toggle.
        assert!(plays_column_visible(Some(QueueSortMode::MostPlayed), false));
        assert!(plays_column_visible(Some(QueueSortMode::MostPlayed), true));
    }

    #[test]
    fn plays_column_visible_follows_user_toggle_for_other_sorts() {
        assert!(!plays_column_visible(Some(QueueSortMode::Title), false));
        assert!(plays_column_visible(Some(QueueSortMode::Title), true));
        assert!(!plays_column_visible(Some(QueueSortMode::Rating), false));
        assert!(plays_column_visible(Some(QueueSortMode::Rating), true));
    }

    #[test]
    fn plays_column_unsorted_does_not_auto_show() {
        // Unsorted (None): the remembered mode never auto-shows the plays
        // column — only the user toggle can.
        assert!(!plays_column_visible(None, false));
        assert!(plays_column_visible(None, true));
    }

    #[test]
    fn genre_column_visible_auto_shows_on_genre_sort() {
        assert!(genre_column_visible(Some(QueueSortMode::Genre), false));
        assert!(genre_column_visible(Some(QueueSortMode::Genre), true));
    }

    #[test]
    fn genre_column_visible_follows_user_toggle_for_other_sorts() {
        assert!(!genre_column_visible(Some(QueueSortMode::Title), false));
        assert!(genre_column_visible(Some(QueueSortMode::Title), true));
        assert!(!genre_column_visible(
            Some(QueueSortMode::MostPlayed),
            false
        ));
        assert!(genre_column_visible(Some(QueueSortMode::MostPlayed), true));
    }

    #[test]
    fn genre_column_unsorted_does_not_auto_show() {
        // Unsorted (None): no genre auto-show; user toggle still wins.
        assert!(!genre_column_visible(None, false));
        assert!(genre_column_visible(None, true));
    }
}
