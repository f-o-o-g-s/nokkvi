//! Queue view — `impl QueuePage { fn view }`.
//!
//! Rendering for the queue page. The per-row song-list composition (columns,
//! drag, context menu) is delegated to the shared `views::song_list_pane`
//! renderer; the per-mode column-visibility helpers and the
//! `BREAKPOINT_HIDE_QUEUE_STARS` constant live there too. Update/state logic
//! lives in `update.rs`; types live in `mod.rs`.

use iced::{
    Alignment, Element, Length,
    widget::{Row, Space, column, container, mouse_area, row},
};

use super::{
    PlaylistStripZone, QueueContextEntry, QueueMessage, QueuePage, QueueSortMode, QueueViewData,
};
use crate::{
    app_message::OpenMenu,
    views::song_list_pane::{SongListPaneParams, SongListRowEvent, song_list_pane},
    widgets::{
        self,
        view_header::{HeaderButton, ViewHeaderConfig},
    },
};

/// The two directions of the Queue view's server-sync action menu. `Copy` so it
/// satisfies [`crate::widgets::checkbox_dropdown::action_dropdown`]'s
/// `Key: Copy` bound; mapped to `QueueMessage::PushQueue` / `PullQueue` at the
/// call site.
#[derive(Debug, Clone, Copy)]
enum QueueSyncAction {
    Push,
    Pull,
}

/// Edge of the merged toolbar's playlist cover (a 2×2 quad or single cover)
/// in the 24 px Count strip.
const PLAYLIST_COVER_AT_REST: f32 = 16.0;
/// Edge of the cover in the revealed 50 px toolbar: the search field's height.
const PLAYLIST_COVER_REVEALED: f32 = 32.0;
/// Width of the playlist name column. A fixed width keeps the sort dropdown at
/// one x for every playlist; a narrow pane shrinks it (see [`identity_fit`]).
const PLAYLIST_NAME_W: f32 = 200.0;
/// Narrowest the name column gets beside the revealed count cell; below it the
/// count cell goes.
const PLAYLIST_NAME_MIN_W: f32 = 120.0;
/// Narrowest the name column gets at all; below it the revealed toolbar shows
/// the cover alone, with the name in its tooltip.
const PLAYLIST_NAME_FLOOR_W: f32 = 96.0;
/// Width the revealed toolbar keeps for the search field when fitting the name.
const PLAYLIST_SEARCH_MIN_W: f32 = 180.0;
/// Right padding of the identity cell: the toolbar's cell padding.
const PLAYLIST_IDENTITY_PAD_R: f32 = 14.0;
/// Width of the smart-playlist mark and its gap after the name.
const PLAYLIST_SMART_MARK_W: f32 = 16.0;
/// Right padding of the hover detail block.
const PLAYLIST_STRIP_PAD_X: f32 = 16.0;
/// Bottom padding of the hover-expanded detail block. Shared by its render and
/// its height in [`playlist_strip_detail`].
const PLAYLIST_STRIP_DETAIL_BOTTOM_PAD: f32 = 12.0;

/// Where the merged toolbar's cover and name sit so they line up with the
/// queue rows below: the cover centered over the rows' thumbnail column, the
/// name over their titles. See [`playlist_strip_geometry`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlaylistStripGeometry {
    /// Left edge of the rows' thumbnail column.
    art_x: f32,
    /// Width of the column the cover centers in: the rows' art, or the
    /// revealed cover's edge when the rows show no thumbnails.
    art_w: f32,
    /// Left edge of the name column: where the rows' titles start. The detail
    /// block is indented to it too.
    name_x: f32,
}

impl PlaylistStripGeometry {
    /// The cover's edge in the toolbar: its size for the state, no wider than
    /// the column it centers in.
    fn cover_size(&self, toolbar_collapsed: bool) -> f32 {
        let size = if toolbar_collapsed {
            PLAYLIST_COVER_AT_REST
        } else {
            PLAYLIST_COVER_REVEALED
        };
        size.min(self.art_w)
    }

    /// Left edge of a `size` cover centered in the thumbnail column, floored
    /// to a whole pixel so the image stays sharp.
    fn cover_x(&self, size: f32) -> f32 {
        self.art_x + ((self.art_w - size) / 2.0).floor()
    }
}

/// Line the identity up with the queue rows, which lay out as
/// `[select?] pad [index?] [thumbnail?] [title …]` (`song_list_pane`).
///
/// The row size comes from the chrome with the detail block closed even while
/// it is open: opening it shrinks the rows, and following them would move the
/// cover and the name every time it opens. Reading the closed chrome also
/// keeps this out of a loop, since the open chrome's height depends on the
/// detail indent computed here.
fn playlist_strip_geometry(inputs: &QueueChromeInputs<'_>) -> PlaylistStripGeometry {
    use crate::{
        views::song_list_pane::SONG_ROW_COLUMN_SPACING,
        widgets::slot_list::{
            SLOT_LIST_INDEX_WIDTH, SLOT_LIST_SELECT_WIDTH, SLOT_LIST_SLOT_PADDING, SlotListConfig,
            SlotListRowMetrics,
        },
    };

    let closed = QueueChromeInputs {
        strip_expanded: false,
        ..*inputs
    };
    let row_height =
        SlotListConfig::with_dynamic_slots(inputs.window_height, queue_effective_chrome(&closed))
            .row_height();
    let row_art = SlotListRowMetrics::from_row(row_height, 1.0).artwork_size;

    let mut art_x = SLOT_LIST_SLOT_PADDING;
    if inputs.select_visible {
        art_x += SLOT_LIST_SELECT_WIDTH;
    }
    if inputs.index_visible {
        art_x += SLOT_LIST_INDEX_WIDTH + SONG_ROW_COLUMN_SPACING;
    }
    // With the thumbnail column hidden the titles start where the art would,
    // so the name clears the revealed cover instead, in both states, so it
    // holds still on reveal.
    let art_w = if inputs.thumbnail_visible {
        row_art
    } else {
        PLAYLIST_COVER_REVEALED
    };
    PlaylistStripGeometry {
        art_x,
        art_w,
        name_x: art_x + art_w + SONG_ROW_COLUMN_SPACING,
    }
}

/// How the playlist identity fits the revealed toolbar.
#[derive(Debug, Clone, Copy, PartialEq)]
struct IdentityFit {
    /// The name column's width; `None` shows the cover alone.
    name_w: Option<f32>,
    /// Whether the revealed toolbar keeps its count cell.
    show_count: bool,
}

/// Fit the identity into a revealed toolbar `band` px wide while keeping
/// [`PLAYLIST_SEARCH_MIN_W`] for the search field: the name column shrinks
/// first, then the count cell goes, then the name. `toolbar_cells` and
/// `action_cells` count the 44 px icon cells (each with its divider) beside
/// the sort dropdown and after the count; `count_w` is the count cell's width.
fn identity_fit(
    band: f32,
    name_x: f32,
    toolbar_cells: usize,
    action_cells: usize,
    count_w: f32,
) -> IdentityFit {
    use crate::widgets::view_header::{ICON_CELL_WIDTH, SORT_CELL_MIN_WIDTH};
    let cells = (toolbar_cells + action_cells) as f32 * (ICON_CELL_WIDTH + 1.0);
    let room = band
        - (name_x + PLAYLIST_IDENTITY_PAD_R + 1.0)
        - (SORT_CELL_MIN_WIDTH + 1.0)
        - cells
        - PLAYLIST_SEARCH_MIN_W;
    if room - count_w >= PLAYLIST_NAME_MIN_W {
        IdentityFit {
            name_w: Some((room - count_w).min(PLAYLIST_NAME_W)),
            show_count: true,
        }
    } else if room >= PLAYLIST_NAME_FLOOR_W {
        IdentityFit {
            name_w: Some(room.min(PLAYLIST_NAME_W)),
            show_count: false,
        }
    } else {
        IdentityFit {
            name_w: None,
            show_count: false,
        }
    }
}

/// The name column's width in the Count strip beside `strip_w` of the strip's
/// own cells and action hints, capped at [`PLAYLIST_NAME_W`]; `None` shows the
/// cover alone.
fn rest_name_w(band: f32, name_x: f32, strip_w: f32) -> Option<f32> {
    let room = band - (name_x + PLAYLIST_IDENTITY_PAD_R + 1.0) - strip_w;
    (room >= PLAYLIST_NAME_FLOOR_W).then(|| room.min(PLAYLIST_NAME_W))
}

/// Width of the revealed toolbar's count cell showing `label`: the 12 px
/// label (`ui_font()` is monospace, so a char-count estimate holds), the
/// cell's 14 px side padding, and its divider.
fn count_cell_width(label: &str) -> f32 {
    const CHAR_W: f32 = 7.3;
    label.chars().count() as f32 * CHAR_W + 28.0 + 1.0
}

/// Resolve the artwork layout the queue's slot list will render for the given
/// window — used by the playlist identity's width math.
/// `resolve_artwork_layout` reads only the window dimensions, the
/// show-artwork flag, and the display-mode atomics, so `slot_list_chrome` /
/// `elevated` are irrelevant here and neutral values are fine.
fn playlist_strip_artwork_layout(
    window_width: f32,
    window_height: f32,
) -> Option<crate::widgets::base_slot_list_layout::ArtworkLayout> {
    use crate::widgets::base_slot_list_layout::{BaseSlotListLayoutConfig, resolve_artwork_layout};
    resolve_artwork_layout(&BaseSlotListLayoutConfig {
        window_width,
        window_height,
        show_artwork_column: true,
        slot_list_chrome: 0.0,
        elevated: false,
    })
}

/// Width available to the expanded playlist strip's content — the song-list
/// column, i.e. the content pane minus the horizontal artwork column when one
/// is shown. Vertical / hidden artwork leave the strip full-width. The comment
/// wraps within this width, so sizing the detail block to it (rather than to
/// the full pane) is what keeps the meta row from being clipped.
fn playlist_strip_band_width(window_width: f32, window_height: f32) -> f32 {
    use crate::widgets::base_slot_list_layout::ArtworkOrientation;
    match playlist_strip_artwork_layout(window_width, window_height) {
        Some(layout) if matches!(layout.orientation, ArtworkOrientation::Horizontal) => {
            (window_width - layout.extent).max(120.0)
        }
        _ => window_width,
    }
}

/// Lay out the hover-expanded detail block: clamp the comment to at most
/// `MAX_LINES` rendered lines (appending an ellipsis when it would overflow)
/// and return the display string together with the block height, sized to the
/// (clamped) comment plus the meta row.
///
/// `content_width` is the comment's real rendered width (band width minus the
/// strip padding). Sizing the block to it — and truncating the comment to fit
/// — keeps a long description from pushing the meta row past the container's
/// `clip(true)` and vanishing. Short comments still reserve no dead space, and
/// a description-less playlist (empty or whitespace-only comment) collapses the
/// block to the meta row alone — no reserved comment line at all.
/// `ui_font()` is monospace, so the char-count/width estimate is reliable.
fn playlist_strip_detail(comment: &str, content_width: f32) -> (String, f32) {
    const LINE_H: f32 = 16.0;
    const CHAR_W: f32 = 7.3;
    const META_ROW_H: f32 = 20.0;
    const ROW_GAP: f32 = 8.0;
    const BOTTOM_PAD: f32 = PLAYLIST_STRIP_DETAIL_BOTTOM_PAD;
    const MAX_LINES: f32 = 5.0;
    // No description (empty or whitespace-only): collapse the phantom comment
    // line + its 8px gap so the meta row rides up flush under the identity row.
    // Fulfils this fn's own "reserve no dead space" contract for the one case —
    // an absent comment — that previously broke it by reserving a blank line.
    // Only the emptiness *decision* is trimmed; a real comment still renders
    // verbatim below.
    if comment.trim().is_empty() {
        return (String::new(), META_ROW_H + BOTTOM_PAD);
    }
    let cols = (content_width / CHAR_W).floor().max(1.0);
    let char_count = comment.chars().count();
    let raw_lines = (char_count as f32 / cols).ceil().max(1.0);
    let (display, lines) = if raw_lines <= MAX_LINES {
        (comment.to_string(), raw_lines)
    } else {
        // Keep MAX_LINES worth of characters (less one for the ellipsis glyph)
        // so the rendered comment occupies at most MAX_LINES lines.
        let keep = ((MAX_LINES * cols) as usize).saturating_sub(1);
        let truncated: String = comment.chars().take(keep).collect();
        (format!("{}…", truncated.trim_end()), MAX_LINES)
    };
    (display, lines * LINE_H + ROW_GAP + META_ROW_H + BOTTOM_PAD)
}

/// Width the expanded strip's comment wraps within: the band width minus the
/// detail block's indent (the name column) and right padding. Shared by the
/// detail block's render and its height in [`queue_chrome_height`].
fn playlist_strip_comment_width(inputs: &QueueChromeInputs<'_>) -> f32 {
    (playlist_strip_band_width(inputs.pane_width, inputs.window_height)
        - playlist_strip_geometry(inputs).name_x
        - PLAYLIST_STRIP_PAD_X)
        .max(120.0)
}

/// Every input the queue's slot-list chrome depends on, derived once per read
/// by `Nokkvi::queue_chrome_inputs` and handed to BOTH `QueuePage::view` (via
/// [`QueueViewData::chrome`](super::QueueViewData::chrome)) and
/// `resync_slot_counts`. Deriving any of these a second time is how the stored
/// `slot_count` drifted from the render before.
#[derive(Debug, Clone, Copy)]
pub struct QueueChromeInputs<'a> {
    /// Width the queue renders at: the full content pane, or the split view's
    /// queue pane (`Nokkvi::queue_pane_width`).
    pub pane_width: f32,
    pub window_height: f32,
    /// Auto-hide collapse state, with an open header menu (columns cog or
    /// server sync) holding the toolbar expanded.
    pub toolbar_collapsed: bool,
    /// The active playlist's comment; `None` = no playlist identity in the
    /// toolbar.
    pub playlist_comment: Option<&'a str>,
    /// Whether the identity's detail block shows: hovered past the dwell AND
    /// the toolbar revealed, since the block renders only under the revealed
    /// toolbar.
    pub strip_expanded: bool,
    /// Whether the multi-select column's select-all bar is showing.
    pub select_visible: bool,
    /// Whether the rows show their index column. With `select_visible` and
    /// `thumbnail_visible`, places the identity's cover and name over the
    /// rows' art and titles ([`playlist_strip_geometry`]).
    pub index_visible: bool,
    /// Whether the rows show their thumbnail column.
    pub thumbnail_visible: bool,
}

/// Total slot-list chrome for the queue view, before the vertical artwork:
/// the view header (collapsed or expanded; a playing playlist's identity rides
/// inside it at no extra height), the identity's detail block and its 1 px
/// hairline while the block shows, then the select-all bar when the
/// multi-select column is on. A new bar stacked above the queue's slot list is
/// counted here.
///
/// `QueuePage::view` feeds this to `BaseSlotListLayoutConfig.slot_list_chrome`.
pub(crate) fn queue_chrome_height(inputs: &QueueChromeInputs<'_>) -> f32 {
    use crate::widgets::{
        slot_list::{SELECT_HEADER_HEIGHT, chrome_height_with_header},
        view_header::HEADER_BOTTOM_SEPARATOR,
    };

    let mut chrome = chrome_height_with_header(inputs.toolbar_collapsed);
    if let Some(comment) = inputs.playlist_comment
        && inputs.strip_expanded
    {
        let width = playlist_strip_comment_width(inputs);
        chrome += HEADER_BOTTOM_SEPARATOR + playlist_strip_detail(comment, width).1;
    }
    if inputs.select_visible {
        chrome += SELECT_HEADER_HEIGHT;
    }
    chrome
}

/// The effective slot-list chrome (queue chrome + vertical artwork) the queue's
/// [`view`](QueuePage::view) budgets for these inputs: the single input to
/// `with_dynamic_slots`. Shared with `resync_slot_counts` so the stored
/// `slot_count` (which the within-list drag maps slots to items against, and
/// the scrollbar thumb and centered-row reads use) comes from the same formula
/// and inputs as the rendered count. A drift here silently mis-lands picks and
/// drops, so both paths MUST read this one helper.
pub(crate) fn queue_effective_chrome(inputs: &QueueChromeInputs<'_>) -> f32 {
    use crate::widgets::base_slot_list_layout::{
        BaseSlotListLayoutConfig, vertical_artwork_chrome,
    };

    let chrome = queue_chrome_height(inputs);
    // `elevated` and `slot_list_chrome` don't reach the vertical term (see
    // `playlist_strip_artwork_layout`), so the render's elevation is moot here.
    let layout = BaseSlotListLayoutConfig {
        window_width: inputs.pane_width,
        window_height: inputs.window_height,
        show_artwork_column: true,
        slot_list_chrome: chrome,
        elevated: false,
    };
    chrome + vertical_artwork_chrome(&layout)
}

/// Whether the artwork panel is displaying the now-playing track's cover — the
/// gate for overlaying its lyrics, keyed to what actually RENDERS, not intent.
///
/// Playing: the locked branch shows the playing cover only when it's in the
/// filtered list AND its handle resolved (`playing_art_shown`); during the
/// cold-art window the panel falls back to the centered row, which only reads
/// as the now-playing cover when that row shares the playing ALBUM (identical
/// picture; `None` = no centered row → blank panel, lyrics harmless).
/// Paused/stopped: the cover follows the centered row, so lyrics show only
/// when the centered row IS the playing track. Pure and regression-pinned.
fn cover_shows_now_playing(
    is_playing: bool,
    playing_in_list: bool,
    playing_art_shown: bool,
    fallback_same_album: Option<bool>,
    centered_is_playing: bool,
) -> bool {
    if is_playing {
        playing_in_list && (playing_art_shown || fallback_same_album.unwrap_or(true))
    } else {
        centered_is_playing
    }
}

/// Format a playlist's total duration for the strip, e.g. `4h 53m` / `47m`.
fn format_strip_duration(secs: f32) -> String {
    let total_mins = (secs / 60.0).round() as u32;
    let (h, m) = (total_mins / 60, total_mins % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

impl QueuePage {
    /// Build the view
    pub fn view<'a>(&'a self, data: QueueViewData<'a>) -> Element<'a, QueueMessage> {
        use crate::widgets::slot_list::SlotListConfig;

        // Build ViewHeader using generic component
        const QUEUE_VIEW_OPTIONS: &[QueueSortMode] = &[
            QueueSortMode::Album,
            QueueSortMode::Artist,
            QueueSortMode::Title,
            QueueSortMode::Duration,
            QueueSortMode::Genre,
            QueueSortMode::Rating,
            QueueSortMode::MostPlayed,
            QueueSortMode::Random,
        ];

        // Build the columns-visibility dropdown for the queue's view header.
        let column_dropdown: Element<'a, QueueMessage> =
            crate::widgets::checkbox_dropdown::view_columns_dropdown(
                crate::View::Queue,
                self.column_visibility.dropdown_entries(),
                QueueMessage::ToggleColumnVisible,
                QueueMessage::SetOpenMenu,
                data.overlay.column_dropdown_open,
                data.overlay.column_dropdown_trigger_bounds,
            )
            .into();

        // Auto-hide toolbar: collapse to a hairline when enabled and not
        // currently revealed (hover / active search / hotkey window). The
        // collapse state rides in the shared chrome inputs so the render and
        // `resync_slot_counts` read one derivation.
        let autohide = crate::theme::is_autohide_toolbar();
        let toolbar_collapsed = data.chrome.toolbar_collapsed;

        let header_buttons = {
            let mut btns = vec![HeaderButton::SortToggle(QueueMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToggleSortOrder,
            ))];
            if let Some(entry_id) = data.current_playing_entry_id {
                btns.push(HeaderButton::CenterOnPlaying(
                    QueueMessage::FocusCurrentPlaying(entry_id, true),
                ));
            }
            // Trawl door for mouse users — the anchor is the feature's mark.
            btns.push(HeaderButton::Trawl(QueueMessage::OpenTrawl));
            // Server queue sync (OpenSubsonic indexBasedQueue): one neutral
            // "sync" trigger opening a close-on-click action menu with the
            // push / pull directions. Hidden unless the server advertises
            // the extension, and during radio (the queue position would be a
            // stream offset). Push / Pull are consequential full-replaces, so
            // the menu's labeled rows + consequence subtitles make the
            // direction explicit at the moment of choice — vs two near-mirror
            // arrow buttons one misclick apart. Reuses the columns-cog's
            // header-anchored dropdown chassis (mutual exclusion, dismissal,
            // trigger-bounds anchoring) via `action_dropdown`.
            if data.queue_sync_available && !data.is_radio {
                let sync_menu = widgets::checkbox_dropdown::action_dropdown(
                    "assets/icons/arrow-down-up.svg",
                    "Server queue (push / pull)",
                    vec![
                        (
                            QueueSyncAction::Push,
                            "assets/icons/arrow-up-to-line.svg",
                            "Push to server",
                            "Replaces the queue saved on the server",
                        ),
                        (
                            QueueSyncAction::Pull,
                            "assets/icons/arrow-down-to-line.svg",
                            "Pull from server",
                            "Replaces your local queue",
                        ),
                    ],
                    |action| match action {
                        QueueSyncAction::Push => QueueMessage::PushQueue,
                        QueueSyncAction::Pull => QueueMessage::PullQueue,
                    },
                    |rect| match rect {
                        Some(b) => QueueMessage::SetOpenMenu(Some(OpenMenu::QueueSync {
                            trigger_bounds: b,
                        })),
                        None => QueueMessage::SetOpenMenu(None),
                    },
                    data.sync_menu_open,
                    data.sync_menu_trigger_bounds,
                );
                btns.push(HeaderButton::Trailing(sync_menu.into()));
            }
            // Default-playlist chip is gated by a user setting; when on,
            // it sits left of the columns dropdown in the trailing region.
            if data.show_default_playlist_chip {
                let chip = crate::widgets::default_playlist_chip::default_playlist_chip(
                    data.default_playlist_name,
                    QueueMessage::OpenDefaultPlaylistPicker,
                );
                btns.push(HeaderButton::Trailing(chip));
            }
            btns.push(HeaderButton::Trailing(column_dropdown));
            btns
        };
        // Icon cells beside the sort dropdown: the identity's name fits around them.
        let header_cells = header_buttons.len();
        let header_config = ViewHeaderConfig {
            current_view: self.queue_sort_mode,
            view_options: QUEUE_VIEW_OPTIONS,
            sort_ascending: self.common.sort_ascending,
            search_query: &self.common.search_query,
            filtered_count: data.queue_songs.len(),
            total_count: data.total_queue_count,
            item_type: "songs",
            search_input_id: crate::views::QUEUE_SEARCH_ID,
            on_view_selected: Box::new(QueueMessage::SortModeSelected),
            show_search: true,
            on_search_change: Box::new(|q| {
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::SearchQueryChanged(q))
            }),
            // Queue has no refresh button; CenterOnPlaying only when there's a
            // currently-playing track in the queue.
            buttons: header_buttons,
            on_roulette: Some(QueueMessage::Roulette),
            collapsed: toolbar_collapsed,
            on_hover_enter: autohide.then_some({
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverEnter)
            }),
            on_hover_exit: autohide.then_some({
                QueueMessage::SlotList(crate::widgets::SlotListPageMessage::ToolbarHoverExit)
            }),
            on_dropdown_open: autohide.then_some(QueueMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(true),
            )),
            on_dropdown_close: autohide.then_some(QueueMessage::SlotList(
                crate::widgets::SlotListPageMessage::ToolbarDropdownToggled(false),
            )),
            total_duration_secs: Some(
                data.queue_songs
                    .iter()
                    .map(|s| u64::from(s.duration_seconds))
                    .sum(),
            ),
            // Show "Unsorted" until the user applies a queue sort — the queue
            // takes its order from whatever populated it, so the remembered
            // `queue_sort_mode` would otherwise misrepresent the actual order.
            sort_placeholder: (!self.queue_sorted).then_some("Unsorted"),
        };

        // A playing playlist's identity rides in the toolbar: cover + name
        // first, its actions after the count, and its detail block under the
        // revealed toolbar. The detail's comment is clamped to the band it
        // wraps in (the song-list column, excluding the horizontal artwork
        // column), so a long description can't push the meta row out of the
        // clipped block; `queue_chrome_height` sizes the block through the same
        // helper and width, so the render and the slot-list chrome agree.
        let (playlist_comment_display, playlist_detail_h) =
            data.playlist_context_info.as_ref().map_or_else(
                || (String::new(), 0.0),
                |ctx| {
                    playlist_strip_detail(&ctx.comment, playlist_strip_comment_width(&data.chrome))
                },
            );

        let identity: Option<widgets::view_header::HeaderIdentity<'a, QueueMessage>> =
            if let Some(ref ctx) = data.playlist_context_info {
                use iced::widget::svg;

                use crate::widgets::view_header::{
                    HeaderAction, HeaderIdentity, ICON_CELL_WIDTH, count_label, count_strip_width,
                };

                let accent = crate::theme::accent();
                let collapsed = data.chrome.toolbar_collapsed;
                let smart = data.playlist_context_is_smart;
                let geometry = playlist_strip_geometry(&data.chrome);

                // Smart playlists derive their tracks from rules, so saving the
                // queue over one is meaningless; their name carries the smart
                // mark instead of a save action.
                let mut actions = Vec::with_capacity(2);
                if !smart {
                    actions.push(HeaderAction {
                        icon: "assets/icons/save.svg",
                        tooltip: "Save Queue as Playlist",
                        on_press: QueueMessage::QuickSavePlaylist,
                    });
                }
                actions.push(HeaderAction {
                    icon: "assets/icons/pencil-line.svg",
                    tooltip: "Edit Playlist",
                    on_press: QueueMessage::EditPlaylist,
                });

                // The Count strip always has room for the whole name column; the
                // revealed toolbar fits it beside its controls.
                let band = playlist_strip_band_width(data.chrome.pane_width, data.window_height);
                let fit = if collapsed {
                    // The Count strip keeps its count; the name fits beside the
                    // strip's own cells and the action hints.
                    IdentityFit {
                        name_w: rest_name_w(
                            band,
                            geometry.name_x,
                            count_strip_width(&header_config)
                                + actions.len() as f32 * (ICON_CELL_WIDTH + 1.0),
                        ),
                        show_count: true,
                    }
                } else {
                    identity_fit(
                        band,
                        geometry.name_x,
                        header_cells,
                        actions.len(),
                        count_cell_width(&count_label(
                            data.queue_songs.len(),
                            data.total_queue_count,
                            "songs",
                        )),
                    )
                };

                // Cover, centered over the rows' art (see `playlist_strip_geometry`).
                // The playlist's uploaded cover wins, as on every other playlist
                // surface. Otherwise a 2×2 quad of the queue's first distinct album
                // covers — it reads as "a playlist", not "the song playing now" —
                // with the single first-album cover as the warm-up fallback, and a
                // blank square (as a row shows before its art arrives) until any of
                // them is cached, so the name never shifts when the art lands.
                use crate::widgets::base_slot_list_layout::quad_artwork_grid;
                let cover_size = geometry.cover_size(collapsed);
                let cover_x = geometry.cover_x(cover_size);
                let cover_edge = Length::Fixed(cover_size);
                let single = |handle: &iced::widget::image::Handle| -> Element<'a, QueueMessage> {
                    iced::widget::image(handle.clone())
                        .width(cover_edge)
                        .height(cover_edge)
                        .content_fit(iced::ContentFit::Cover)
                        .into()
                };
                let cover: Element<'a, QueueMessage> =
                    if let Some(handle) = data.playlist_custom_cover {
                        single(handle)
                    } else if let Some(tiles) = &data.playlist_quad {
                        quad_artwork_grid(tiles, cover_size, 1.0)
                    } else if let Some(handle) = data.playlist_cover {
                        single(handle)
                    } else {
                        quad_artwork_grid(&[], cover_size, 1.0)
                    };
                let tooltip_text = |label: String| {
                    container(
                        iced::widget::text(label)
                            .size(11.0)
                            .font(crate::theme::ui_font()),
                    )
                    .padding(4)
                };
                // The cover's tooltip says where the queue came from, and names the
                // playlist when a narrow toolbar shows the cover alone.
                let cover_tip = match fit.name_w {
                    Some(_) => "Playing from playlist".to_string(),
                    None => format!("Playing from {}", ctx.name),
                };
                let cover = iced::widget::tooltip(
                    container(cover)
                        .width(cover_edge)
                        .height(cover_edge)
                        .clip(true),
                    tooltip_text(cover_tip),
                    iced::widget::tooltip::Position::Top,
                )
                .gap(4)
                .style(crate::theme::container_tooltip);

                let mut identity_row = Row::new()
                    .align_y(Alignment::Center)
                    .push(Space::new().width(Length::Fixed(cover_x)))
                    .push(cover);
                if let Some(name_w) = fit.name_w {
                    let name = iced::widget::text(ctx.name.clone())
                        .font(crate::theme::weighted_ui_font(iced::font::Weight::Medium))
                        .size(12)
                        .color(crate::theme::fg0())
                        .wrapping(iced::widget::text::Wrapping::None)
                        .ellipsis(iced::widget::text::Ellipsis::End);
                    let name: Element<'a, QueueMessage> = if smart {
                        // The Playlists view's smart mark, kept quiet so the
                        // playing row stays the list's only accent.
                        let mark = iced::widget::tooltip(
                            crate::embedded_svg::svg_widget("assets/icons/sparkles.svg")
                                .width(Length::Fixed(12.0))
                                .height(Length::Fixed(12.0))
                                .style(|_theme, _status| svg::Style {
                                    color: Some(crate::theme::fg3()),
                                }),
                            tooltip_text("Smart playlist — updates itself from rules".to_string()),
                            iced::widget::tooltip::Position::Top,
                        )
                        .gap(4)
                        .style(crate::theme::container_tooltip);
                        row![
                            container(name).max_width(name_w - PLAYLIST_SMART_MARK_W),
                            mark
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center)
                        .into()
                    } else {
                        name.into()
                    };
                    identity_row = identity_row
                        .push(
                            Space::new()
                                .width(Length::Fixed(geometry.name_x - cover_x - cover_size)),
                        )
                        .push(container(name).width(Length::Fixed(name_w)).clip(true));
                }
                // Hovering the identity (not the controls beside it) opens the
                // detail block after a dwell; see `PlaylistStripHoverSettled`.
                let leading = mouse_area(
                    container(identity_row)
                        .padding(iced::Padding {
                            right: PLAYLIST_IDENTITY_PAD_R,
                            ..iced::Padding::ZERO
                        })
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                )
                .on_enter(QueueMessage::PlaylistStripHoverEnter(
                    PlaylistStripZone::Identity,
                ))
                .on_exit(QueueMessage::PlaylistStripHoverExit(
                    PlaylistStripZone::Identity,
                ));

                // Detail block (comment + meta row, or the meta row alone when
                // there is no description — see `playlist_strip_detail`).
                let below: Option<Element<'a, QueueMessage>> = if data.chrome.strip_expanded {
                    let meta_item =
                        |icon_path: &'static str, label: String| -> Element<'a, QueueMessage> {
                            row![
                                crate::embedded_svg::svg_widget(icon_path)
                                    .width(Length::Fixed(12.0))
                                    .height(Length::Fixed(12.0))
                                    .style(|_theme, _status| svg::Style {
                                        color: Some(crate::theme::fg3()),
                                    }),
                                iced::widget::text(label)
                                    .font(crate::theme::ui_font())
                                    .size(11)
                                    .color(crate::theme::fg3()),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center)
                            .into()
                        };

                    let count = if ctx.song_count > 0 {
                        ctx.song_count as usize
                    } else {
                        data.total_queue_count
                    };
                    let count_label = if count == 1 {
                        "1 song".to_string()
                    } else {
                        format!("{count} songs")
                    };

                    let mut meta_row = Row::new().spacing(14).align_y(Alignment::Center);
                    meta_row = meta_row.push(meta_item("assets/icons/music.svg", count_label));
                    if ctx.duration_secs > 0.0 {
                        meta_row = meta_row.push(meta_item(
                            "assets/icons/clock.svg",
                            format_strip_duration(ctx.duration_secs),
                        ));
                    }
                    if !ctx.updated.is_empty() {
                        let date =
                            nokkvi_data::utils::formatters::format_date_concise(&ctx.updated);
                        meta_row = meta_row.push(meta_item(
                            "assets/icons/calendar.svg",
                            format!("Updated {date}"),
                        ));
                    }

                    // Public/private chip — pill outline at 30% accent alpha.
                    let (chip_icon, chip_text) = if ctx.public {
                        ("assets/icons/lock-open.svg", "Public")
                    } else {
                        ("assets/icons/lock.svg", "Private")
                    };
                    let chip_border = iced::Color { a: 0.30, ..accent };
                    let chip = container(
                        row![
                            crate::embedded_svg::svg_widget(chip_icon)
                                .width(Length::Fixed(11.0))
                                .height(Length::Fixed(11.0))
                                .style(|_theme, _status| svg::Style {
                                    color: Some(crate::theme::fg2()),
                                }),
                            iced::widget::text(chip_text)
                                .font(crate::theme::ui_font())
                                .size(10.5)
                                .color(crate::theme::fg2()),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([2, 8])
                    .style(move |_theme| container::Style {
                        border: iced::Border {
                            color: chip_border,
                            width: 1.0,
                            radius: crate::theme::ui_radius_pill(),
                        },
                        ..Default::default()
                    });
                    meta_row = meta_row.push(chip);

                    // A description-less playlist collapses the block to the meta row
                    // alone (the helper returned the meta-only height + an empty
                    // display), so the stats ride up flush under the name with no
                    // reserved comment line. The display string is empty iff the
                    // height is meta-only, so keying the branch off `is_empty()` keeps
                    // the rendered structure and `playlist_detail_h` in lockstep.
                    let detail_body: Element<'a, QueueMessage> =
                        if playlist_comment_display.is_empty() {
                            meta_row.into()
                        } else {
                            let comment_text = iced::widget::text(playlist_comment_display)
                                .font(crate::theme::ui_font())
                                .size(12)
                                .color(crate::theme::fg2());
                            column![comment_text, meta_row].spacing(8).into()
                        };

                    let detail = container(detail_body)
                        .width(Length::Fill)
                        .height(Length::Fixed(playlist_detail_h))
                        .padding(iced::Padding {
                            top: 0.0,
                            right: PLAYLIST_STRIP_PAD_X,
                            bottom: PLAYLIST_STRIP_DETAIL_BOTTOM_PAD,
                            left: geometry.name_x,
                        })
                        .clip(true);

                    // The block keeps itself open while the cursor moves down into
                    // it from the identity.
                    Some(
                        mouse_area(detail)
                            .on_enter(QueueMessage::PlaylistStripHoverEnter(
                                PlaylistStripZone::Detail,
                            ))
                            .on_exit(QueueMessage::PlaylistStripHoverExit(
                                PlaylistStripZone::Detail,
                            ))
                            .into(),
                    )
                } else {
                    None
                };

                Some(HeaderIdentity {
                    leading: leading.into(),
                    actions,
                    hide_count: !fit.show_count,
                    below,
                })
            } else {
                None
            };
        let header = widgets::view_header::view_header_with_identity(header_config, identity);

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. The bar's tri-state derives from the
        // current selection set against the *filtered* (visible) row count.
        let header = crate::widgets::slot_list::compose_header_with_select(
            data.chrome.select_visible,
            self.common.select_all_state(data.queue_songs.len()),
            QueueMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
            header,
        );

        // Slot-list chrome: every bar stacked above the list (view header,
        // the playlist detail block and its hairline, select-all bar), from the helper
        // `resync_slot_counts` reads too, so the stored `slot_count` the drag
        // mapper uses equals the count rendered here.
        let chrome_height = queue_chrome_height(&data.chrome);

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
            slot_list_chrome: chrome_height,
            elevated: data.elevated,
        };

        // If no songs in filtered results, show appropriate message (like albums view)
        if data.queue_songs.is_empty() {
            let message = if data.total_queue_count == 0 {
                "Queue is empty."
            } else {
                "No songs match your search."
            };
            return widgets::base_slot_list_empty_state(header, message, &layout_config);
        }

        // Both paths MUST read `queue_effective_chrome` (see its doc).
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            queue_effective_chrome(&data.chrome),
        )
        .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let _scale_factor = data.scale_factor;
        let current_playing_song_id = data.current_playing_song_id;
        let current_playing_entry_id = data.current_playing_entry_id;
        // Effective applied sort: `None` when the queue is unsorted, so the
        // plays/genre columns auto-show only when a genuine Most Played / Genre
        // sort is in effect (not merely the remembered mode).
        let applied_sort_mode = self.queue_sorted.then_some(self.queue_sort_mode);
        let album_art = data.album_art; // Move artwork maps
        let large_artwork = data.large_artwork;
        let queue_songs = data.queue_songs; // Move ownership to extend lifetime
        let column_visibility = self.column_visibility;

        // Render the queue's song rows through the shared `song_list_pane`.
        // The queue maps the neutral row-event vocabulary back to the exact
        // `QueueMessage` each interaction emitted before extraction, and
        // supplies the queue-specific row context menu via the
        // `build_context_menu` closure — so behavior is byte-identical.
        let overlay_open_menu = data.overlay.open_menu;
        let slot_list_content = song_list_pane(
            SongListPaneParams {
                slot_list: &self.common.slot_list,
                songs: queue_songs.as_ref(),
                list_config: &config,
                drop_indicator_slot: data.drop_indicator_slot,
                columns: column_visibility,
                sort_mode: applied_sort_mode,
                album_art,
                current_playing_song_id: current_playing_song_id.clone(),
                current_playing_entry_id,
                stable_viewport: data.stable_viewport,
            },
            |e| match e {
                SongListRowEvent::Slot(m) => QueueMessage::SlotList(m),
                SongListRowEvent::Drag(d) => QueueMessage::DragReorder(d),
                SongListRowEvent::TitleClick(i) => {
                    QueueMessage::ContextMenuAction(i, QueueContextEntry::GetInfo)
                }
                SongListRowEvent::NavArtist(a) => QueueMessage::NavigateAndExpandArtist(a),
                SongListRowEvent::NavAlbum(a) => QueueMessage::NavigateAndExpandAlbum(a),
                SongListRowEvent::NavGenre(g) => QueueMessage::NavigateAndExpandGenre(g),
                SongListRowEvent::SetRating(i, s) => QueueMessage::ClickSetRating(i, s),
                SongListRowEvent::ToggleLove(i) => QueueMessage::ClickToggleStar(i),
            },
            move |slot_button, item_idx| {
                // Wrap in the queue's row context menu. Remove Duplicates and
                // Save Queue as Playlist act on the whole queue, not the row.
                use crate::widgets::context_menu::{context_menu, menu_button, menu_separator};
                let entries = vec![
                    QueueContextEntry::Play,
                    QueueContextEntry::PlayNext,
                    QueueContextEntry::Separator,
                    QueueContextEntry::RemoveFromQueue,
                    QueueContextEntry::RemoveDuplicates,
                    QueueContextEntry::Separator,
                    QueueContextEntry::AddToPlaylist,
                    QueueContextEntry::AddToMix,
                    QueueContextEntry::SaveAsPlaylist,
                    QueueContextEntry::Separator,
                    QueueContextEntry::OpenBrowsingPanel,
                    QueueContextEntry::Separator,
                    QueueContextEntry::GetInfo,
                    QueueContextEntry::ShowInFolder,
                    QueueContextEntry::FindSimilar,
                    QueueContextEntry::TopSongs,
                ];

                let cm_id = crate::app_message::ContextMenuId::QueueRow(item_idx);
                let (cm_open, cm_position) =
                    crate::widgets::context_menu::open_state_for(overlay_open_menu, &cm_id);
                let cm_id_for_msg = cm_id.clone();
                context_menu(
                    slot_button,
                    entries,
                    move |entry, _length| match entry {
                        QueueContextEntry::Play => menu_button(
                            Some("assets/icons/circle-play.svg"),
                            "Play",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::Play),
                        ),
                        QueueContextEntry::PlayNext => menu_button(
                            Some("assets/icons/list-end.svg"),
                            "Play Next",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::PlayNext),
                        ),
                        QueueContextEntry::RemoveFromQueue => menu_button(
                            Some("assets/icons/trash-2.svg"),
                            "Remove from Queue",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::RemoveFromQueue,
                            ),
                        ),
                        QueueContextEntry::RemoveDuplicates => menu_button(
                            Some("assets/icons/squares-unite.svg"),
                            "Remove Duplicates",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::RemoveDuplicates,
                            ),
                        ),
                        QueueContextEntry::Separator => menu_separator(),
                        QueueContextEntry::AddToPlaylist => menu_button(
                            Some("assets/icons/list-music.svg"),
                            "Add to Playlist",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::AddToPlaylist,
                            ),
                        ),
                        QueueContextEntry::AddToMix => menu_button(
                            Some("assets/icons/anchor.svg"),
                            "Add to Mix",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::AddToMix),
                        ),
                        QueueContextEntry::SaveAsPlaylist => menu_button(
                            Some("assets/icons/list-music.svg"),
                            "Save Queue as Playlist",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::SaveAsPlaylist,
                            ),
                        ),
                        QueueContextEntry::OpenBrowsingPanel => menu_button(
                            Some("assets/icons/panel-right-open.svg"),
                            "Library Browser",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::OpenBrowsingPanel,
                            ),
                        ),
                        QueueContextEntry::GetInfo => menu_button(
                            Some("assets/icons/info.svg"),
                            "Get Info",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::GetInfo),
                        ),
                        QueueContextEntry::ShowInFolder => menu_button(
                            Some("assets/icons/folder-open.svg"),
                            "Show in File Manager",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::ShowInFolder,
                            ),
                        ),
                        QueueContextEntry::FindSimilar => menu_button(
                            Some("assets/icons/radar.svg"),
                            "Find Similar",
                            QueueMessage::ContextMenuAction(
                                item_idx,
                                QueueContextEntry::FindSimilar,
                            ),
                        ),
                        QueueContextEntry::TopSongs => menu_button(
                            Some("assets/icons/star.svg"),
                            "Top Songs",
                            QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::TopSongs),
                        ),
                    },
                    cm_open,
                    cm_position,
                    move |position| match position {
                        Some(p) => {
                            QueueMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                                id: cm_id_for_msg.clone(),
                                position: p,
                            }))
                        }
                        None => QueueMessage::SetOpenMenu(None),
                    },
                )
                .into()
            },
        );

        // Get large artwork: prioritize currently playing song, fall back
        // to centered song's large, then to either song's mini. Mini is
        // upscaled by Iced — blurry, but lets the panel track the centered
        // slot during a roulette spin's fast cruise where LoadLarge can't
        // keep up with offset changes (see Albums view for the same
        // pattern).
        // The playing track's own cover, resolvable only while actively playing
        // AND present in the FILTERED list (a search that hides it must not
        // leak its cover while the panel menu targets the centered album). The
        // frosted lyrics-backdrop variant wins when ready; sharp fallback.
        let playing_song = if data.is_playing {
            current_playing_song_id
                .as_ref()
                .and_then(|song_id| queue_songs.iter().find(|s| &s.id == song_id))
        } else {
            None
        };
        let playing_handle = playing_song.and_then(|song| {
            data.lyrics_blurred_cover.or_else(|| {
                large_artwork
                    .get(&song.album_id)
                    .or_else(|| album_art.get(&song.album_id))
            })
        });
        let centered_song = self
            .common
            .slot_list
            .get_center_item_index(queue_songs.len())
            .and_then(|center_idx| queue_songs.get(center_idx));
        let center_artwork_handle: Option<&iced::widget::image::Handle> =
            playing_handle.or_else(|| {
                centered_song.and_then(|song| {
                    large_artwork
                        .get(&song.album_id)
                        .or_else(|| album_art.get(&song.album_id))
                })
            });

        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_visualizer_and_menu;

        // Build artwork column component — determine album_id for refresh action
        let center_album_id: Option<String> = if data.is_playing {
            current_playing_song_id
                .as_ref()
                .and_then(|song_id| queue_songs.iter().find(|s| &s.id == song_id))
                .map(|song| song.album_id.clone())
        } else {
            None
        }
        .or_else(|| {
            self.common
                .slot_list
                .get_center_item_index(queue_songs.len())
                .and_then(|center_idx| queue_songs.get(center_idx))
                .map(|song| song.album_id.clone())
        });
        let mut panel_menu_entries: Vec<_> = center_album_id
            .map(|id| {
                crate::widgets::context_menu::PanelMenuEntry::refresh_artwork(
                    QueueMessage::RefreshArtwork(id),
                )
            })
            .into_iter()
            .collect();
        panel_menu_entries.push(crate::widgets::context_menu::PanelMenuEntry::enter_theater(
            QueueMessage::EnterTheater,
        ));
        if data.milkdrop_on {
            panel_menu_entries.extend(crate::widgets::context_menu::milkdrop_panel_entries(
                data.milkdrop_locked,
                data.milkdrop_favorite,
                QueueMessage::Milkdrop,
            ));
        }
        let (artwork_menu_open, artwork_menu_position, on_artwork_menu_change) =
            crate::widgets::context_menu::artwork_panel_open_state(
                crate::View::Queue,
                data.overlay.open_menu,
                QueueMessage::SetOpenMenu,
            );
        // Over-cover visualizer overlay: render whenever the active mode is set
        // to draw over the cover (carried by `over_art_visualizer` — Scope
        // always, Bars/Lines when their placement is OverCover), independent of
        // play state. This mirrors the bottom-band path (`app_view`), which is
        // also ungated: when audio pauses, no fresh chunk reaches the FFT worker,
        // so `display.bars` / the ring waveform hold their last values and the
        // overlay freezes in place rather than vanishing. Otherwise (bottom-band
        // placement or `Off`) `over_art_visualizer` is `None` → plain cover.
        let over_art_overlay = data.over_art_visualizer;
        // Surfing boat over the cover — ungated to match the ring above; the boat
        // tick holds `visible` and the frozen position/handle while paused.
        let over_art_boat = data.over_art_boat;
        // Only overlay lyrics when the panel is actually DISPLAYING the
        // now-playing cover — not merely intending to. While playing, the
        // locked branch displays it only when its handle resolved; a cold-art
        // fallback to the centered row is fine only when that row shares the
        // playing album (same picture) or shows nothing. Paused/stopped, the
        // cover follows the centered row, so lyrics show only centered on the
        // playing track itself.
        let playing_id = current_playing_song_id.as_deref();
        let fallback_same_album =
            centered_song.and_then(|cs| playing_song.map(|ps| cs.album_id == ps.album_id));
        let centered_is_playing = playing_id.is_some()
            && centered_song.is_some_and(|cs| Some(cs.id.as_str()) == playing_id);
        let cover_is_now_playing = cover_shows_now_playing(
            data.is_playing,
            playing_song.is_some(),
            playing_handle.is_some(),
            fallback_same_album,
            centered_is_playing,
        );
        let lyrics_layer = data.lyrics.filter(|_| cover_is_now_playing);

        // The Cover Art setting swaps the picture for a black backdrop; the
        // lyrics and visualizer gates above are unaffected.
        let cover_hidden = crate::theme::artwork_cover().hides_cover(false);
        let panel = single_artwork_panel_with_visualizer_and_menu(
            center_artwork_handle.filter(|_| !cover_hidden),
            over_art_overlay,
            over_art_boat,
            // Lyrics layer: haloed text topmost, its scrim slotted BELOW the
            // over-cover visualizer (both hero surfaces co-render).
            lyrics_layer,
            // The wheel scrolls a PLAIN sheet by hand; the viewport ignores it
            // over a synced sheet and over the empty state, so right-click and
            // left-click keep reaching the panel beneath either way.
            Some(QueueMessage::LyricsWheel),
            if cover_hidden {
                crate::widgets::base_slot_list_layout::ArtworkPlaceholder::Backdrop
            } else {
                crate::widgets::base_slot_list_layout::ArtworkPlaceholder::Blank
            },
            crate::widgets::base_slot_list_layout::PanelShape::FollowColumnMode,
            panel_menu_entries,
            artwork_menu_open,
            artwork_menu_position,
            on_artwork_menu_change,
        );
        // Theater Mode's way in: an expand icon revealed while the cursor is
        // over the cover. `hover` is the OUTERMOST wrapper so the panel's
        // right-click menu (inside) keeps working; the icon's own
        // `mouse_area` captures only its left press.
        let artwork_content = Some(iced::widget::hover(
            panel,
            crate::widgets::theater_corner::bottom_right(
                crate::widgets::theater_corner::corner_button(
                    "assets/icons/maximize-2.svg",
                    QueueMessage::EnterTheater,
                ),
            ),
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(QueueMessage::ArtworkColumnDrag),
            Some(QueueMessage::ArtworkColumnVerticalDrag),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IdentityFit, PLAYLIST_COVER_REVEALED, PLAYLIST_IDENTITY_PAD_R, PLAYLIST_NAME_FLOOR_W,
        PLAYLIST_NAME_MIN_W, PLAYLIST_NAME_W, QueueChromeInputs, cover_shows_now_playing,
        identity_fit, playlist_strip_band_width, playlist_strip_detail, playlist_strip_geometry,
        rest_name_w,
    };

    #[test]
    fn cover_now_playing_gate_covers_every_case() {
        // Playing, in the filtered list, art resolved → cover locks to it.
        assert!(cover_shows_now_playing(
            true,
            true,
            true,
            Some(false),
            false
        ));
        // Playing but its art is cold and the centered fallback shows a
        // DIFFERENT album → the panel displays the wrong cover; no lyrics.
        assert!(!cover_shows_now_playing(
            true,
            true,
            false,
            Some(false),
            false
        ));
        // Cold art but the centered fallback is the SAME album (identical
        // picture) → lyrics stay up through the cold-art window.
        assert!(cover_shows_now_playing(
            true,
            true,
            false,
            Some(true),
            false
        ));
        // Cold art, no centered row at all (blank panel) → lyrics harmless.
        assert!(cover_shows_now_playing(true, true, false, None, false));
        // Playing but search filtered it out → centered cover shown; no lyrics.
        assert!(!cover_shows_now_playing(
            true,
            false,
            false,
            Some(false),
            false
        ));
        // Paused, centered ON the playing track → cover is now-playing.
        assert!(cover_shows_now_playing(
            false,
            false,
            false,
            Some(true),
            true
        ));
        // Paused, scrolled away → different cover; no lyrics.
        assert!(!cover_shows_now_playing(
            false,
            false,
            false,
            Some(false),
            false
        ));
        // Nothing loaded → never (both branch inputs are false/None-derived).
        assert!(!cover_shows_now_playing(false, false, false, None, false));
    }

    /// The band-width and identity-geometry tests mutate the artwork-column-mode
    /// atomics that `resolve_artwork_layout` reads. Take the crate-wide theme
    /// lock so they serialize against every other atomic-mutating test family.
    fn with_auto_artwork_mode() -> parking_lot::MutexGuard<'static, ()> {
        use nokkvi_data::types::player_settings::ArtworkColumnMode;
        let guard = crate::theme::THEME_MODE_LOCK.lock();
        crate::theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
        // Match the resolver tests' default so the portrait/landscape dims below
        // resolve deterministically regardless of any earlier test's writes.
        crate::theme::set_artwork_auto_max_pct(0.40);
        guard
    }

    // 1 line: 1*16 + 8 (gap) + 20 (meta) + 12 (bottom) = 56.
    const ONE_LINE_H: f32 = 56.0;
    // 5 lines (MAX_LINES): 5*16 + 8 + 20 + 12 = 120.
    const MAX_LINES_H: f32 = 120.0;
    // No description: the phantom comment line + its 8px gap collapse away, so
    // the block is just the meta row + bottom pad = 20 + 12 = 32.
    const META_ONLY_H: f32 = 32.0;

    #[test]
    fn short_comment_renders_verbatim_at_one_line() {
        let (display, height) = playlist_strip_detail("Short note", 555.0);
        assert_eq!(display, "Short note", "a one-line comment is not altered");
        assert!(
            (height - ONE_LINE_H).abs() < f32::EPSILON,
            "short comment reserves exactly one line + meta row, got {height}"
        );
    }

    #[test]
    fn empty_comment_collapses_to_meta_only() {
        // A description-less playlist drops the reserved comment line entirely:
        // no display string, and the block shrinks to the meta row alone so the
        // hover detail block shows no dead space above the stats.
        let (display, height) = playlist_strip_detail("", 555.0);
        assert_eq!(display, "");
        assert!(
            (height - META_ONLY_H).abs() < f32::EPSILON,
            "empty comment collapses to the meta row alone, got {height}"
        );
    }

    #[test]
    fn whitespace_only_comment_collapses_to_meta_only() {
        // Whitespace-only comments are indistinguishable from empty to a reader,
        // so they collapse identically — the emptiness decision trims first.
        let (display, height) = playlist_strip_detail("   \n\t ", 555.0);
        assert_eq!(display, "");
        assert!(
            (height - META_ONLY_H).abs() < f32::EPSILON,
            "whitespace-only comment collapses to the meta row alone, got {height}"
        );
    }

    #[test]
    fn overflowing_comment_is_ellipsized_and_height_caps() {
        // ~600 chars at a narrow width vastly exceeds the 5-line cap.
        let comment = "word ".repeat(120);
        let (display, height) = playlist_strip_detail(&comment, 200.0);
        assert!(
            display.ends_with('…'),
            "an overflowing comment must end with an ellipsis"
        );
        assert!(
            display.chars().count() < comment.chars().count(),
            "the display string must be shorter than the source"
        );
        assert!(
            (height - MAX_LINES_H).abs() < f32::EPSILON,
            "height must cap at MAX_LINES so the meta row stays in view, got {height}"
        );
    }

    #[test]
    fn comment_at_the_cap_is_not_truncated() {
        // cols = floor(555/7.3) = 76; 5 lines worth ≈ 380 chars. 300 fits.
        let comment = "x".repeat(300);
        let (display, _height) = playlist_strip_detail(&comment, 555.0);
        assert!(
            !display.ends_with('…'),
            "a comment within the 5-line budget keeps its full text"
        );
        assert_eq!(display.chars().count(), 300);
    }

    #[test]
    fn band_width_excludes_only_the_horizontal_artwork_column() {
        let _g = with_auto_artwork_mode();
        // Horizontal: the strip's content band is the pane minus the artwork
        // column, so it's narrower than the full window.
        let horizontal = playlist_strip_band_width(1920.0, 1080.0);
        assert!(
            horizontal > 0.0 && horizontal < 1920.0,
            "horizontal artwork narrows the strip band, got {horizontal}"
        );
        // Vertical: artwork stacks above, leaving the strip full-width.
        let vertical = playlist_strip_band_width(530.0, 1430.0);
        assert!(
            (vertical - 530.0).abs() < 1e-3,
            "vertical artwork leaves the strip full-width, got {vertical}"
        );
    }

    /// A playing playlist with a long comment, the toolbar revealed, every
    /// leading row column off but the thumbnail (the default queue layout).
    fn identity_inputs(window_height: f32) -> QueueChromeInputs<'static> {
        QueueChromeInputs {
            pane_width: 1400.0,
            window_height,
            toolbar_collapsed: false,
            playlist_comment: Some(
                "A long description that wraps onto several lines of the detail block. ",
            ),
            strip_expanded: false,
            select_visible: false,
            index_visible: false,
            thumbnail_visible: true,
        }
    }

    /// The rows' art size for these inputs with the detail block closed, as
    /// the slot list renders it.
    fn row_art(inputs: &QueueChromeInputs<'_>) -> f32 {
        use crate::widgets::slot_list::{SlotListConfig, SlotListRowMetrics};
        let closed = QueueChromeInputs {
            strip_expanded: false,
            ..*inputs
        };
        let row_height = SlotListConfig::with_dynamic_slots(
            inputs.window_height,
            super::queue_effective_chrome(&closed),
        )
        .row_height();
        SlotListRowMetrics::from_row(row_height, 1.0).artwork_size
    }

    #[test]
    fn identity_cover_centers_over_the_row_art_and_name_over_the_titles() {
        let _g = with_auto_artwork_mode();
        for height in (500..=1600).step_by(37) {
            for collapsed in [false, true] {
                let inputs = QueueChromeInputs {
                    toolbar_collapsed: collapsed,
                    ..identity_inputs(height as f32)
                };
                let g = playlist_strip_geometry(&inputs);
                let art = row_art(&inputs);
                // Row: 8 px padding, then the art, a 6 px gap, then the title.
                assert!((g.art_x - 8.0).abs() < 1e-3, "art column at 8, got {g:?}");
                assert!(
                    (g.name_x - (8.0 + art + 6.0)).abs() < 1e-3,
                    "name over the titles at height {height}, art {art}, got {g:?}"
                );
                let size = g.cover_size(collapsed);
                let x = g.cover_x(size);
                assert_eq!(x, x.floor(), "the cover sits on a whole pixel");
                let centre_offset = (x + size / 2.0) - (8.0 + art / 2.0);
                assert!(
                    (-1.0..=0.0).contains(&centre_offset),
                    "cover centered over the art at height {height}, got {centre_offset}"
                );
                assert!(size <= art + 1e-3, "the cover fits the art column");
            }
        }
    }

    #[test]
    fn identity_geometry_follows_the_leading_row_columns() {
        let _g = with_auto_artwork_mode();
        let base = playlist_strip_geometry(&identity_inputs(900.0));

        // The checkbox column (40 px) sits outside the row padding, the index
        // column (60 px + a 6 px gap) inside it, both ahead of the art.
        let with_columns = playlist_strip_geometry(&QueueChromeInputs {
            select_visible: true,
            index_visible: true,
            ..identity_inputs(900.0)
        });
        assert!((with_columns.art_x - (base.art_x + 40.0 + 66.0)).abs() < 1e-3);

        // No thumbnails: the titles start where the art would, so the name
        // clears the revealed cover, and holds still on reveal.
        for collapsed in [false, true] {
            let no_thumbs = playlist_strip_geometry(&QueueChromeInputs {
                thumbnail_visible: false,
                toolbar_collapsed: collapsed,
                ..identity_inputs(900.0)
            });
            assert!(
                (no_thumbs.name_x - (no_thumbs.art_x + PLAYLIST_COVER_REVEALED + 6.0)).abs() < 1e-3,
                "got {no_thumbs:?}"
            );
        }
    }

    #[test]
    fn identity_geometry_holds_still_while_the_detail_block_is_open() {
        // Opening the detail block shrinks the rows; the identity must not
        // chase them, or the cover and name jump whenever the block opens.
        let _g = with_auto_artwork_mode();
        for height in (500..=1600).step_by(37) {
            let closed = identity_inputs(height as f32);
            let open = QueueChromeInputs {
                strip_expanded: true,
                ..closed
            };
            assert_eq!(
                playlist_strip_geometry(&closed),
                playlist_strip_geometry(&open),
                "geometry moved on hover at height {height}"
            );
        }
    }

    #[test]
    fn identity_fit_shrinks_the_name_then_drops_the_count_then_the_name() {
        // 6 toolbar cells, save + edit, a 94 px count, the name at x 67.
        let fit = |band: f32| identity_fit(band, 67.0, 6, 2, 94.0);
        let fixed = (67.0 + PLAYLIST_IDENTITY_PAD_R + 1.0) + 131.0 + 8.0 * 45.0 + 180.0;

        // Wide: the full name column beside the count.
        assert_eq!(
            fit(1443.0),
            IdentityFit {
                name_w: Some(PLAYLIST_NAME_W),
                show_count: true
            }
        );
        // The name column gives way first.
        let squeezed = fit(fixed + 94.0 + 150.0);
        assert_eq!(squeezed.name_w, Some(150.0));
        assert!(squeezed.show_count);
        // Then the count cell, which hands its width back to the name.
        let no_count = fit(fixed + 94.0 + PLAYLIST_NAME_MIN_W - 1.0);
        assert!(!no_count.show_count);
        assert_eq!(
            no_count.name_w,
            Some((94.0 + PLAYLIST_NAME_MIN_W - 1.0).min(PLAYLIST_NAME_W))
        );
        // Then the name: the cover alone.
        assert_eq!(
            fit(fixed + PLAYLIST_NAME_FLOOR_W - 1.0),
            IdentityFit {
                name_w: None,
                show_count: false
            }
        );
    }

    #[test]
    fn rest_name_fits_beside_the_count_strip_or_leaves_the_cover_alone() {
        // The name at x 67, 400 px of strip cells and action hints.
        let fixed = 67.0 + PLAYLIST_IDENTITY_PAD_R + 1.0 + 400.0;
        assert_eq!(rest_name_w(1443.0, 67.0, 400.0), Some(PLAYLIST_NAME_W));
        assert_eq!(rest_name_w(fixed + 150.0, 67.0, 400.0), Some(150.0));
        assert_eq!(
            rest_name_w(fixed + PLAYLIST_NAME_FLOOR_W - 1.0, 67.0, 400.0),
            None
        );
    }
}
