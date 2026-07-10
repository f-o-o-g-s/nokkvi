//! Trawl mix-builder modal — state, messages, and the row model.
//!
//! The modal is the *editor* for the persistent [`TrawlCrate`] on `Nokkvi`
//! (`trawl_crate`): a whole-library seed search on top (Harbour's search
//! machinery: immediate fire, [`SEARCH_MIN_CHARS`] gate, generation
//! stale-drop) and the crate tray below (chips, blend, min-length, CTAs).
//! Opened from the Harbour "Trawl" row; `Some` on `Nokkvi.trawl_modal` =
//! open. The view lives in this module too (mounted by
//! `wrap_with_global_overlays`, modeled on `default_playlist_picker`).
//!
//! Like Harbour, render and activation both derive rows from ONE builder —
//! [`build_trawl_rows`] — so a centered index always resolves to the row the
//! user sees.

use nokkvi_data::types::{
    library_search::LibrarySearchResults,
    trawl::{TrawlCrate, TrawlSeed},
};

use crate::{views::harbour::SEARCH_MIN_CHARS, widgets::slot_list_view::SlotListView};

/// `text_input` id for the modal's search field (focused on open).
pub(crate) const TRAWL_SEARCH_INPUT_ID: &str = "trawl_modal_search";

/// Scrollable id for the horizontal chip strip — the tray's wheel
/// `mouse_area` routes deltas here via the `scroll_by` widget operation.
pub(crate) fn chips_scrollable_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("trawl_chips_strip")
}

/// Modal editor state. The crate itself lives on `Nokkvi.trawl_crate` and
/// survives closing the modal; only search + viewport state lives here. The
/// search stale-drop generation lives on `Nokkvi.trawl_search_generation`
/// (root-owned so it survives close/reopen — a fresh modal must not re-mint
/// a generation an in-flight fan-out already captured).
#[derive(Debug, Default)]
pub struct TrawlModalState {
    pub search_query: String,
    /// Whether the search `text_input` holds keyboard focus. Set on open and
    /// on typing; cleared when Tab (SlotListDown) exits the field — the
    /// regular views' "Tab doubles as exit-search" rule.
    pub search_input_focused: bool,
    /// `None` = nothing fetched yet (idle or in flight); `Some` = last landed
    /// fan-out for the current generation.
    pub search_results: Option<LibrarySearchResults>,
    pub search_loading: bool,
    pub slot_list: SlotListView,
}

#[derive(Debug, Clone)]
pub enum TrawlModalMessage {
    Open,
    Close,
    SearchChanged(String),
    /// The whole-library search fan-out completed for a keystroke.
    SearchLoaded {
        generation: u64,
        result: Result<Box<LibrarySearchResults>, String>,
    },
    SlotListUp,
    SlotListDown,
    SlotListSetOffset(usize),
    /// Click on a result row (index into [`build_trawl_rows`] output).
    ClickRow(usize),
    /// Enter — toggle the centered result in/out of the crate.
    ActivateCenter,
    /// Remove the chip at this crate index.
    RemoveSeed(usize),
    IncWeight(usize),
    DecWeight(usize),
    SetBlend(nokkvi_data::types::trawl::TrawlBlend),
    SetMinLength(nokkvi_data::types::trawl::TrawlMinLength),
    SetMaxLength(nokkvi_data::types::trawl::TrawlMaxLength),
    SetMinRating(nokkvi_data::types::trawl::TrawlMinRating),
    SetMaxTracks(nokkvi_data::types::trawl::TrawlMaxTracks),
    ClearCrate,
    /// Ctrl+Enter / the Play Mix CTA.
    PlayMix,
    PlayMixCompleted(Result<(), String>),
    AddMixToQueue,
    AddMixCompleted(Result<usize, String>),
    /// Wheel over the chips tray — scrolls the horizontal chip strip.
    ChipsScrolled(f32),
}

/// One row of the modal's results list.
///
/// Headers are passive dividers (no caret, no count, no toggle — deliberately
/// NOT Harbour's interactive section-header vocabulary, so a lookalike header
/// that no-ops doesn't read as broken).
#[derive(Debug)]
pub(crate) enum TrawlRow {
    /// Passive group divider ("Artists", "Albums", …).
    Header(&'static str),
    /// A search hit, carrying the ready-made seed (labels captured here) and
    /// whether its identity is already in the crate.
    Result {
        seed: TrawlSeed,
        /// 80px-mini cache key: album id for albums/songs, artist id for
        /// artists (the artist-mini path), `None` for genres/playlists
        /// (type-glyph thumb in the modal — no quad fan-out here).
        art_album_id: Option<String>,
        in_crate: bool,
    },
    /// Centered helper text (keep typing / searching / no matches / idle).
    Hint(String),
}

/// Single source of the modal's row order — both `view()` and the update
/// handler derive rows through here, so a centered index resolves to the row
/// the user sees (the Harbour row-order lesson).
pub(crate) fn build_trawl_rows(state: &TrawlModalState, mix: &TrawlCrate) -> Vec<TrawlRow> {
    use nokkvi_data::types::batch::BatchItem;

    let mut rows = Vec::new();
    let query = state.search_query.trim();

    if query.is_empty() {
        rows.push(TrawlRow::Hint(
            "Search your library to fill the crate — artists, albums, songs, genres and \
             playlists all work as seeds."
                .to_string(),
        ));
        return rows;
    }
    if query.chars().count() < SEARCH_MIN_CHARS {
        rows.push(TrawlRow::Hint(
            "Keep typing to search your library…".to_string(),
        ));
        return rows;
    }
    let Some(results) = &state.search_results else {
        rows.push(TrawlRow::Hint(if state.search_loading {
            "Searching…".to_string()
        } else {
            "Search failed — edit the query to retry.".to_string()
        }));
        return rows;
    };
    if results.is_empty() {
        rows.push(TrawlRow::Hint("No matches.".to_string()));
        return rows;
    }

    let push_result = |rows: &mut Vec<TrawlRow>, seed: TrawlSeed, art_album_id: Option<String>| {
        let in_crate = mix.contains(&seed.item);
        rows.push(TrawlRow::Result {
            seed,
            art_album_id,
            in_crate,
        });
    };

    if !results.artists.is_empty() {
        rows.push(TrawlRow::Header("Artists"));
        for a in &results.artists {
            push_result(
                &mut rows,
                TrawlSeed::new(BatchItem::Artist(a.id.clone()), a.name.clone(), "Artist"),
                // Artist images live in `album_art` keyed by the artist id —
                // the same single-mini path Harbour's search rows use.
                Some(a.id.clone()),
            );
        }
    }
    if !results.albums.is_empty() {
        rows.push(TrawlRow::Header("Albums"));
        for a in &results.albums {
            let sublabel = a
                .artist
                .clone()
                .or_else(|| a.album_artist.clone())
                .unwrap_or_default();
            push_result(
                &mut rows,
                TrawlSeed::new(BatchItem::Album(a.id.clone()), a.name.clone(), sublabel),
                Some(a.id.clone()),
            );
        }
    }
    if !results.songs.is_empty() {
        rows.push(TrawlRow::Header("Songs"));
        for s in &results.songs {
            let art = s.album_id.clone();
            push_result(
                &mut rows,
                TrawlSeed::new(
                    BatchItem::Song(Box::new(s.clone())),
                    s.title.clone(),
                    s.artist.clone(),
                ),
                art,
            );
        }
    }
    if !results.genres.is_empty() {
        rows.push(TrawlRow::Header("Genres"));
        for g in &results.genres {
            push_result(
                &mut rows,
                {
                    let n = g.album_count;
                    let noun = if n == 1 { "album" } else { "albums" };
                    TrawlSeed::new(
                        BatchItem::Genre(g.name.clone()),
                        g.name.clone(),
                        format!("{n} {noun}"),
                    )
                },
                None,
            );
        }
    }
    if !results.playlists.is_empty() {
        rows.push(TrawlRow::Header("Playlists"));
        for p in &results.playlists {
            push_result(
                &mut rows,
                {
                    let n = p.song_count;
                    let noun = if n == 1 { "song" } else { "songs" };
                    TrawlSeed::new(
                        BatchItem::Playlist(p.id.clone()),
                        p.name.clone(),
                        format!("{n} {noun}"),
                    )
                },
                None,
            );
        }
    }

    rows
}

// ─────────────────────────────────────────────────────────────────────────────
// View
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashMap;

use iced::{
    Alignment, Border, Length, Padding,
    font::Weight,
    widget::{Space, button, column, container, image, mouse_area, pick_list, row, svg, text},
};
use nokkvi_data::types::trawl::{TRAWL_WEIGHT_MAX, TRAWL_WEIGHT_MIN, TrawlBlend, TrawlMinLength};

use crate::{
    embedded_svg, theme,
    widgets::{pill_segmented_button, slot_list},
};

/// Modal chrome heights (local, like the picker's — not sizes.rs constants).
const TITLE_BAR_HEIGHT: f32 = 38.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;
/// The crate tray is a FIXED-height band so `with_dynamic_slots` sees a
/// stable chrome height — the visible slot count must not jump when the
/// first seed lands (empty-crate hint swaps CONTENT, never height).
const TRAY_HEIGHT: f32 = 142.0;
/// Chip band inside the tray (chips scroll horizontally within it).
const CHIP_BAND_HEIGHT: f32 = 46.0;

/// Entity glyph stems — the nav bar's own vocabulary, reused per seed type.
fn seed_type_icon(item: &nokkvi_data::types::batch::BatchItem) -> &'static str {
    use nokkvi_data::types::batch::BatchItem;
    match item {
        BatchItem::Song(_) => "assets/icons/music-2.svg",
        BatchItem::Album(_) => "assets/icons/disc-3.svg",
        BatchItem::Artist(_) => "assets/icons/mic.svg",
        BatchItem::Genre(_) => "assets/icons/tags.svg",
        BatchItem::Playlist(_) => "assets/icons/list-music.svg",
    }
}

/// Per-type chip tint (theme-role colors, readable in every palette).
fn seed_type_tint(item: &nokkvi_data::types::batch::BatchItem) -> iced::Color {
    use nokkvi_data::types::batch::BatchItem;
    match item {
        BatchItem::Song(_) => theme::fg1(),
        BatchItem::Album(_) => theme::warning(),
        BatchItem::Artist(_) => theme::accent_bright(),
        BatchItem::Genre(_) => theme::danger(),
        BatchItem::Playlist(_) => theme::success(),
    }
}

/// The full Trawl modal overlay. Mounted by `wrap_with_global_overlays`;
/// chrome mirrors `default_playlist_picker_overlay` (shared
/// `modal_frame_style` + `modal_scaffold`, wheel → list nav) with the crate
/// tray as an extra fixed band above the panel's bottom edge.
pub(crate) fn trawl_modal_overlay<'a>(
    state: &'a TrawlModalState,
    mix: &'a TrawlCrate,
    window_height: f32,
    album_art: &'a HashMap<String, image::Handle>,
) -> iced::Element<'a, TrawlModalMessage> {
    let modal_height = (window_height * 0.70).max(320.0);
    // Chrome = fixed bands + the panel container's own 4px padding top and
    // bottom, so the slot budget matches the Fill area main_area really gets.
    let modal_chrome = TITLE_BAR_HEIGHT + SEARCH_BAR_HEIGHT + TRAY_HEIGHT + 8.0;

    // ── Title bar ──
    let dim_color = theme::fg4();
    let label_size = 13.0;

    let close_btn = button(
        embedded_svg::svg_widget("assets/icons/x.svg")
            .width(Length::Fixed(label_size))
            .height(Length::Fixed(label_size))
            .style(move |_theme, _status| svg::Style {
                color: Some(dim_color),
            }),
    )
    .on_press(TrawlModalMessage::Close)
    .style(theme::transparent_button_style)
    .padding(Padding::new(2.0));

    let seed_count: iced::Element<'a, TrawlModalMessage> = if mix.is_empty() {
        Space::new().width(Length::Shrink).into()
    } else {
        let n = mix.len();
        let noun = if n == 1 { "seed" } else { "seeds" };
        text(format!("{n} {noun}"))
            .size(11.0)
            .color(theme::fg3())
            .into()
    };

    let title_row = row![
        Space::new().width(Length::Fixed(12.0)),
        embedded_svg::svg_widget("assets/icons/anchor.svg")
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0))
            .style(|_theme, _status| svg::Style {
                color: Some(theme::accent()),
            }),
        Space::new().width(Length::Fixed(6.0)),
        text("Trawl")
            .size(label_size)
            .font(theme::weighted_ui_font(Weight::Bold))
            .color(theme::fg0()),
        Space::new().width(Length::Fixed(8.0)),
        seed_count,
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
        "Search the whole library...",
        TRAWL_SEARCH_INPUT_ID,
        TrawlModalMessage::SearchChanged,
        Some(theme::settings_search_input_style),
    );
    let search_bar = container(search_input)
        .width(Length::Fill)
        .height(Length::Fixed(SEARCH_BAR_HEIGHT))
        .padding(Padding::new(4.0).left(12.0).right(12.0));

    // ── Results (slot list) or centered hint ──
    let rows = build_trawl_rows(state, mix);
    let hint_only = matches!(rows.as_slice(), [TrawlRow::Hint(_)]);
    let main_area: iced::Element<'a, TrawlModalMessage> = if hint_only {
        let TrawlRow::Hint(msg) = &rows[0] else {
            unreachable!("hint_only guarantees a Hint row");
        };
        container(
            text(msg.clone())
                .size(14)
                .color(theme::fg4())
                .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(Padding::new(0.0).left(32.0).right(32.0))
        .into()
    } else {
        let config = slot_list::SlotListConfig::with_dynamic_slots(modal_height, modal_chrome);
        let total = rows.len();
        slot_list::slot_list_view_with_scroll(
            &state.slot_list,
            &rows,
            &config,
            TrawlModalMessage::SlotListUp,
            TrawlModalMessage::SlotListDown,
            move |f| TrawlModalMessage::SlotListSetOffset((f * total as f32) as usize),
            None,
            move |row, ctx| {
                render_trawl_slot(
                    row,
                    ctx.item_index,
                    ctx.is_center,
                    ctx.row_height,
                    album_art,
                )
            },
        )
    };

    // ── Crate tray ──
    let tray = render_tray(mix);

    let modal_panel = container(
        column![title_bar, search_bar, main_area, tray]
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
        TrawlModalMessage::Close,
        theme::MODAL_BACKDROP_ALPHA,
    );

    // Wheel over the backdrop / results = list nav. The chips band mounts its
    // own inner mouse_area which captures first, so wheel-over-tray scrolls
    // the chip strip instead (`opaque` captures presses, not scrolls).
    mouse_area(scaffold)
        .on_scroll(|delta| {
            let y = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            if y > 0.0 {
                TrawlModalMessage::SlotListUp
            } else {
                TrawlModalMessage::SlotListDown
            }
        })
        .into()
}

/// One results row: passive group divider, seed hit, or centered hint.
fn render_trawl_slot<'a>(
    row_model: &TrawlRow,
    item_index: usize,
    is_center: bool,
    row_height: f32,
    album_art: &HashMap<String, image::Handle>,
) -> iced::Element<'a, TrawlModalMessage> {
    match row_model {
        TrawlRow::Header(title) => {
            // Passive divider — dim uppercase label, no caret, no count, no
            // fill: deliberately NOT Harbour's interactive header vocabulary.
            container(
                text(title.to_uppercase())
                    .size(11.0)
                    .color(theme::fg3())
                    .font(theme::weighted_ui_font(Weight::Bold)),
            )
            .width(Length::Fill)
            .height(Length::Fixed(row_height))
            .align_y(Alignment::End)
            .padding(Padding::new(0.0).left(12.0).bottom(6.0))
            .into()
        }
        TrawlRow::Hint(msg) => container(text(msg.clone()).size(14).color(theme::fg4()))
            .width(Length::Fill)
            .height(Length::Fixed(row_height))
            .center(Length::Fill)
            .into(),
        TrawlRow::Result {
            seed,
            art_album_id,
            in_crate,
        } => {
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
            let art_size = (row_height - 16.0).max(32.0);

            let art_handle = art_album_id.as_ref().and_then(|id| album_art.get(id));
            let thumbnail: iced::Element<'a, TrawlModalMessage> = match art_handle {
                Some(handle) => container(
                    image(handle.clone())
                        .content_fit(iced::ContentFit::Cover)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .width(Length::Fixed(art_size))
                .height(Length::Fixed(art_size))
                .clip(true)
                .style(move |_theme: &iced::Theme| container::Style {
                    background: Some(theme::bg2().into()),
                    border: Border {
                        radius: theme::ui_border_radius(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
                None => {
                    let tint = seed_type_tint(&seed.item);
                    container(
                        embedded_svg::svg_widget(seed_type_icon(&seed.item))
                            .width(Length::Fixed(art_size * 0.5))
                            .height(Length::Fixed(art_size * 0.5))
                            .style(move |_theme, _status| svg::Style { color: Some(tint) }),
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
            };

            // Trailing affordance: plus (add) / check (already in the crate).
            let (affordance_icon, affordance_color) = if *in_crate {
                ("assets/icons/check.svg", theme::accent_bright())
            } else {
                ("assets/icons/plus.svg", theme::fg4())
            };
            let affordance = container(
                embedded_svg::svg_widget(affordance_icon)
                    .width(Length::Fixed(15.0))
                    .height(Length::Fixed(15.0))
                    .style(move |_theme, _status| svg::Style {
                        color: Some(affordance_color),
                    }),
            )
            .width(Length::Fixed(26.0))
            .center(Length::Fixed(26.0));

            let text_col = column![
                text(seed.label.clone())
                    .size(14.0)
                    .font(theme::weighted_ui_font(weight))
                    .color(label_color)
                    .wrapping(text::Wrapping::None),
                text(seed.sublabel.clone())
                    .size(11.0)
                    .font(theme::ui_font())
                    .color(subtitle_color)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(2.0);

            let content = row![
                Space::new().width(Length::Fixed(12.0)),
                thumbnail,
                Space::new().width(Length::Fixed(12.0)),
                container(text_col).width(Length::Fill).clip(true),
                affordance,
                Space::new().width(Length::Fixed(12.0)),
            ]
            .align_y(Alignment::Center)
            .height(Length::Fill);

            // Keyboard cursor = the app's border-only selection ring; the
            // mouse hover keeps the shared HoverOverlay wash. Categorically
            // distinct cues, like every main slot list.
            let body = container(content)
                .width(Length::Fill)
                .height(Length::Fixed(row_height))
                .style(move |_theme: &iced::Theme| container::Style {
                    background: None,
                    border: if is_center {
                        Border {
                            color: theme::selection_ring_on(theme::bg0_hard()),
                            width: 2.0,
                            radius: theme::ui_border_radius(),
                        }
                    } else {
                        Border::default()
                    },
                    ..Default::default()
                });

            mouse_area(body)
                .on_press(TrawlModalMessage::ClickRow(item_index))
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
        }
    }
}

/// Shared style for the tray's pick_lists (min length, max tracks) — the EQ
/// modal recipe: bg0_hard field, accent_bright border on hover/open.
fn tray_pick_list_style(status: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: theme::fg1(),
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: match status {
                pick_list::Status::Active | pick_list::Status::Disabled => theme::bg3(),
                pick_list::Status::Hovered | pick_list::Status::Opened { .. } => {
                    theme::accent_bright()
                }
            },
            width: 1.0,
            radius: theme::ui_border_radius(),
        },
        placeholder_color: theme::fg3(),
        handle_color: theme::fg3(),
    }
}

fn tray_pick_list_menu_style() -> iced::widget::overlay::menu::Style {
    iced::widget::overlay::menu::Style {
        text_color: theme::fg0(),
        background: theme::bg1().into(),
        border: iced::Border {
            color: theme::accent_bright(),
            width: 1.0,
            radius: theme::ui_border_radius(),
        },
        selected_text_color: theme::bg0_hard(),
        selected_background: theme::accent_bright().into(),
        shadow: iced::Shadow::default(),
    }
}

/// The fixed-height crate tray: chip band, controls row, hint line.
fn render_tray<'a>(mix: &'a TrawlCrate) -> iced::Element<'a, TrawlModalMessage> {
    let empty = mix.is_empty();

    // ── Chip band (fixed height; content swaps, height never does) ──
    let chip_band: iced::Element<'a, TrawlModalMessage> = if empty {
        container(
            text(
                "The crate is empty — add seeds from the search above. Enter adds a seed, \
                 Ctrl+Enter plays the mix.",
            )
            .size(12.0)
            .color(theme::fg4()),
        )
        .width(Length::Fill)
        .height(Length::Fixed(CHIP_BAND_HEIGHT))
        .align_y(Alignment::Center)
        .padding(Padding::new(0.0).left(4.0))
        .into()
    } else {
        let weighted = mix.blend == TrawlBlend::Weighted;
        let mut chips = row![].spacing(6.0).align_y(Alignment::Center);
        for (i, seed) in mix.seeds.iter().enumerate() {
            chips = chips.push(render_chip(i, seed, weighted));
        }
        // Bottom padding gives the 3px rail its own lane under the pills.
        let chips = container(chips).padding(Padding::new(0.0).bottom(5.0));
        let strip = iced::widget::scrollable(chips)
            .direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::new()
                    .width(3)
                    .scroller_width(3),
            ))
            .id(chips_scrollable_id())
            .width(Length::Fill)
            .style(theme::settings_scrollable_style);
        // Inner mouse_area: wheel over the tray drives the chip strip, not
        // the results list (inner widgets see the scroll first).
        mouse_area(
            container(strip)
                .width(Length::Fill)
                .height(Length::Fixed(CHIP_BAND_HEIGHT))
                .align_y(Alignment::Center),
        )
        .on_scroll(|delta| {
            let y = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            TrawlModalMessage::ChipsScrolled(-y)
        })
        .into()
    };

    // ── Controls row ──
    let blend_options: Vec<pill_segmented_button::PillOption> = TrawlBlend::ALL
        .iter()
        .map(|b| pill_segmented_button::PillOption {
            display: b.label().to_string(),
            key: b.label().to_string(),
            on: *b == mix.blend,
        })
        .collect();
    let blend_pills = pill_segmented_button::pill_segmented_button(
        &blend_options,
        pill_segmented_button::PillVariant::Single,
        pill_segmented_button::PillRowParams {
            font_size: 12.0,
            is_center: true,
            opacity: 1.0,
        },
        |key: String| {
            let blend = TrawlBlend::ALL
                .iter()
                .find(|b| b.label() == key)
                .copied()
                .unwrap_or_default();
            TrawlModalMessage::SetBlend(blend)
        },
    );

    let min_length_picker = pick_list(
        Some(mix.min_length),
        TrawlMinLength::ALL,
        |m: &TrawlMinLength| m.label().to_string(),
    )
    .on_select(TrawlModalMessage::SetMinLength)
    .font(theme::ui_font())
    .text_size(12.0)
    .padding([4, 8])
    .style(|_theme, status| tray_pick_list_style(status))
    .menu_style(|_theme| tray_pick_list_menu_style());

    let max_length_picker = pick_list(
        Some(mix.max_length),
        nokkvi_data::types::trawl::TrawlMaxLength::ALL,
        |m: &nokkvi_data::types::trawl::TrawlMaxLength| m.label().to_string(),
    )
    .on_select(TrawlModalMessage::SetMaxLength)
    .font(theme::ui_font())
    .text_size(12.0)
    .padding([4, 8])
    .style(|_theme, status| tray_pick_list_style(status))
    .menu_style(|_theme| tray_pick_list_menu_style());

    let min_rating_picker = pick_list(
        Some(mix.min_rating),
        nokkvi_data::types::trawl::TrawlMinRating::ALL,
        |m: &nokkvi_data::types::trawl::TrawlMinRating| m.label().to_string(),
    )
    .on_select(TrawlModalMessage::SetMinRating)
    .font(theme::ui_font())
    .text_size(12.0)
    .padding([4, 8])
    .style(|_theme, status| tray_pick_list_style(status))
    .menu_style(|_theme| tray_pick_list_menu_style());

    let max_tracks_picker = pick_list(
        Some(mix.max_tracks),
        nokkvi_data::types::trawl::TrawlMaxTracks::ALL,
        |m: &nokkvi_data::types::trawl::TrawlMaxTracks| m.label().to_string(),
    )
    .on_select(TrawlModalMessage::SetMaxTracks)
    .font(theme::ui_font())
    .text_size(12.0)
    .padding([4, 8])
    .style(|_theme, status| tray_pick_list_style(status))
    .menu_style(|_theme| tray_pick_list_menu_style());

    let clear_btn = {
        let label = text("Clear").size(12.0).color(theme::fg2());
        let mut btn = button(label)
            .style(theme::transparent_button_style)
            .padding([4, 8]);
        if !empty {
            btn = btn.on_press(TrawlModalMessage::ClearCrate);
        }
        btn
    };

    let queue_btn = {
        let mut btn = button(
            text("Add to Queue")
                .size(13.0)
                .wrapping(text::Wrapping::None)
                .color(if empty { theme::fg4() } else { theme::fg0() }),
        )
        .padding([5, 14])
        .style(move |_theme: &iced::Theme, _status| button::Style {
            background: Some(theme::bg3().into()),
            text_color: theme::fg0(),
            border: Border {
                color: if empty {
                    theme::bg3()
                } else {
                    theme::accent_bright()
                },
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        });
        if !empty {
            btn = btn.on_press(TrawlModalMessage::AddMixToQueue);
        }
        btn
    };

    let play_btn = {
        let mut btn = button(
            text("Play Mix")
                .size(13.0)
                .font(theme::weighted_ui_font(Weight::Bold))
                .wrapping(text::Wrapping::None)
                .color(theme::bg0_hard()),
        )
        .padding([5, 16])
        .style(move |_theme: &iced::Theme, _status| button::Style {
            background: Some(if empty {
                theme::bg3().into()
            } else {
                theme::accent().into()
            }),
            text_color: if empty {
                theme::fg4()
            } else {
                theme::bg0_hard()
            },
            border: Border {
                color: if empty {
                    theme::bg3()
                } else {
                    theme::accent_border_light()
                },
                width: 1.0,
                radius: theme::ui_radius_sm(),
            },
            ..Default::default()
        });
        if !empty {
            btn = btn.on_press(TrawlModalMessage::PlayMix);
        }
        btn
    };

    // ── Controls row: blend + the four filters (buttons live below) ──
    let controls = row![
        blend_pills,
        Space::new().width(Length::Fixed(10.0)),
        min_length_picker,
        Space::new().width(Length::Fixed(8.0)),
        max_length_picker,
        Space::new().width(Length::Fixed(8.0)),
        min_rating_picker,
        Space::new().width(Length::Fixed(8.0)),
        max_tracks_picker,
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    // ── Actions row: blend hint left, CTAs right (always rendered —
    // height stability; the keyboard facts live in the empty-crate hint) ──
    let hint_line = row![
        text(mix.blend.hint()).size(11.0).color(theme::fg4()),
        Space::new().width(Length::Fill),
        clear_btn,
        Space::new().width(Length::Fixed(8.0)),
        queue_btn,
        Space::new().width(Length::Fixed(8.0)),
        play_btn,
    ]
    .align_y(Alignment::Center);

    let rule = container(Space::new().width(Length::Fill).height(Length::Fixed(1.0)))
        .width(Length::Fill)
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        });

    container(
        column![
            rule,
            container(
                column![chip_band, controls, hint_line]
                    .spacing(6.0)
                    .width(Length::Fill),
            )
            .padding(Padding::new(8.0).left(12.0).right(12.0))
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(TRAY_HEIGHT))
    .into()
}

/// One crate chip: type glyph, 2-line label (sublabel disambiguates duplicate
/// names), weight steppers (Weighted blend only), remove button.
fn render_chip<'a>(
    index: usize,
    seed: &'a TrawlSeed,
    weighted: bool,
) -> iced::Element<'a, TrawlModalMessage> {
    let tint = seed_type_tint(&seed.item);
    let glyph = embedded_svg::svg_widget(seed_type_icon(&seed.item))
        .width(Length::Fixed(12.0))
        .height(Length::Fixed(12.0))
        .style(move |_theme, _status| svg::Style { color: Some(tint) });

    let labels = column![
        text(truncate_label(&seed.label, 28))
            .size(12.0)
            .font(theme::weighted_ui_font(Weight::Bold))
            .color(theme::fg0())
            .wrapping(text::Wrapping::None),
        text(truncate_label(&seed.sublabel, 32))
            .size(10.0)
            .font(theme::ui_font())
            .color(theme::fg3())
            .wrapping(text::Wrapping::None),
    ]
    .spacing(1.0);

    let mut chip = row![glyph, labels].spacing(7.0).align_y(Alignment::Center);

    if weighted {
        chip = chip.push(Space::new().width(Length::Fixed(2.0)));
        chip = chip.push(stepper_button(
            "assets/icons/chevron-left.svg",
            seed.weight > TRAWL_WEIGHT_MIN,
            TrawlModalMessage::DecWeight(index),
        ));
        chip = chip.push(
            text(seed.weight.to_string())
                .size(12.0)
                .font(theme::weighted_ui_font(Weight::Bold))
                .color(theme::fg0()),
        );
        chip = chip.push(stepper_button(
            "assets/icons/chevron-right.svg",
            seed.weight < TRAWL_WEIGHT_MAX,
            TrawlModalMessage::IncWeight(index),
        ));
    }

    let remove = button(
        embedded_svg::svg_widget("assets/icons/x.svg")
            .width(Length::Fixed(11.0))
            .height(Length::Fixed(11.0))
            .style(move |_theme, _status| svg::Style {
                color: Some(theme::fg4()),
            }),
    )
    .on_press(TrawlModalMessage::RemoveSeed(index))
    .style(theme::transparent_button_style)
    .padding(2.0);
    chip = chip.push(remove);

    container(chip)
        .padding(Padding::new(5.0).left(10.0).right(6.0))
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(theme::bg1().into()),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: theme::ui_radius_pill(),
            },
            ..Default::default()
        })
        .into()
}

/// `‹ n ›` stepper chevron — the settings numeric-row `arrow_button` recipe.
fn stepper_button<'a>(
    icon_path: &'static str,
    enabled: bool,
    on_press: TrawlModalMessage,
) -> iced::Element<'a, TrawlModalMessage> {
    let icon_color = if enabled { theme::fg2() } else { theme::fg4() };
    let icon = embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(11.0))
        .height(Length::Fixed(11.0))
        .style(move |_theme, _status| svg::Style {
            color: Some(icon_color),
        });

    let body = container(icon)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(theme::bg0().into()),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: theme::ui_radius_pill(),
            },
            ..Default::default()
        });

    let mut btn = button(body)
        .style(theme::transparent_button_style)
        .padding(0);
    if enabled {
        btn = btn.on_press(on_press);
    }
    btn.into()
}

/// Character-capped label with an ellipsis — chips have no width to spare.
fn truncate_label(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::{
        batch::BatchItem, genre::Genre, library_search::LibrarySearchResults, song::Song,
    };

    use super::*;

    fn data_song(id: &str, title: &str, artist: &str) -> Song {
        Song {
            id: id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            artist_id: None,
            album: "Album".to_string(),
            album_id: Some(format!("al_{id}")),
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

    fn genre(name: &str, albums: u32) -> Genre {
        Genre {
            id: name.to_lowercase(),
            name: name.to_string(),
            album_count: albums,
            song_count: 0,
        }
    }

    fn state_with(query: &str, results: Option<LibrarySearchResults>) -> TrawlModalState {
        TrawlModalState {
            search_query: query.to_string(),
            search_input_focused: false,
            search_results: results,
            search_loading: false,
            slot_list: SlotListView::new(),
        }
    }

    fn data_artist(id: &str, name: &str) -> nokkvi_data::types::artist::Artist {
        nokkvi_data::types::artist::Artist {
            id: id.to_string(),
            name: name.to_string(),
            album_count: Some(3),
            song_count: Some(30),
            starred: None,
            starred_at: None,
            large_image_url: None,
            medium_image_url: None,
            small_image_url: None,
            play_count: None,
            play_date: None,
            size: None,
            mbz_artist_id: None,
            biography: None,
            similar_artists: None,
            external_url: None,
            external_info_updated_at: None,
            rating: None,
        }
    }

    fn full_results() -> LibrarySearchResults {
        LibrarySearchResults {
            artists: vec![data_artist("ar1", "Burial")],
            songs: vec![data_song("s1", "Archangel", "Burial")],
            genres: vec![genre("Phonk", 12)],
            ..Default::default()
        }
    }

    fn header_titles(rows: &[TrawlRow]) -> Vec<&'static str> {
        rows.iter()
            .filter_map(|r| match r {
                TrawlRow::Header(t) => Some(*t),
                TrawlRow::Result { .. } | TrawlRow::Hint(_) => None,
            })
            .collect()
    }

    #[test]
    fn empty_query_shows_onboarding_hint() {
        let rows = build_trawl_rows(&state_with("", None), &TrawlCrate::default());
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], TrawlRow::Hint(t) if t.contains("Search your library")));
    }

    #[test]
    fn one_char_query_shows_keep_typing_hint() {
        let rows = build_trawl_rows(&state_with("a", None), &TrawlCrate::default());
        assert!(matches!(&rows[0], TrawlRow::Hint(t) if t.contains("Keep typing")));
    }

    #[test]
    fn loading_and_no_match_hints() {
        let mut state = state_with("bu", None);
        state.search_loading = true;
        let rows = build_trawl_rows(&state, &TrawlCrate::default());
        assert!(matches!(&rows[0], TrawlRow::Hint(t) if t == "Searching…"));

        let empty = state_with("bu", Some(LibrarySearchResults::default()));
        let rows = build_trawl_rows(&empty, &TrawlCrate::default());
        assert!(matches!(&rows[0], TrawlRow::Hint(t) if t == "No matches."));
    }

    #[test]
    fn groups_render_in_fixed_order_and_empty_groups_are_omitted() {
        let state = state_with("bu", Some(full_results()));
        let rows = build_trawl_rows(&state, &TrawlCrate::default());
        // Albums + Playlists empty → omitted entirely.
        assert_eq!(header_titles(&rows), vec!["Artists", "Songs", "Genres"]);
    }

    #[test]
    fn result_rows_carry_ready_made_seeds_with_labels() {
        let state = state_with("bu", Some(full_results()));
        let rows = build_trawl_rows(&state, &TrawlCrate::default());
        let seeds: Vec<&TrawlSeed> = rows
            .iter()
            .filter_map(|r| match r {
                TrawlRow::Result { seed, .. } => Some(seed),
                TrawlRow::Header(_) | TrawlRow::Hint(_) => None,
            })
            .collect();
        assert_eq!(seeds.len(), 3);
        assert_eq!(seeds[0].label, "Burial");
        assert_eq!(seeds[0].sublabel, "Artist");
        assert_eq!(seeds[1].label, "Archangel");
        assert_eq!(seeds[1].sublabel, "Burial");
        assert_eq!(seeds[2].label, "Phonk");
        assert_eq!(seeds[2].sublabel, "12 albums");
        assert!(matches!(seeds[2].item, BatchItem::Genre(ref n) if n == "Phonk"));
    }

    #[test]
    fn in_crate_flag_derives_live_from_the_crate() {
        let state = state_with("bu", Some(full_results()));
        let mut mix = TrawlCrate::default();

        let rows = build_trawl_rows(&state, &mix);
        let flags: Vec<bool> = rows
            .iter()
            .filter_map(|r| match r {
                TrawlRow::Result { in_crate, .. } => Some(*in_crate),
                TrawlRow::Header(_) | TrawlRow::Hint(_) => None,
            })
            .collect();
        assert_eq!(flags, vec![false, false, false]);

        // Toggle the genre in; the SAME state re-derives with the flag set —
        // this is the property the crate-as-param design exists for.
        mix.add(TrawlSeed::new(
            BatchItem::Genre("Phonk".into()),
            "Phonk",
            "12 albums",
        ));
        let rows = build_trawl_rows(&state, &mix);
        let flags: Vec<bool> = rows
            .iter()
            .filter_map(|r| match r {
                TrawlRow::Result { in_crate, .. } => Some(*in_crate),
                TrawlRow::Header(_) | TrawlRow::Hint(_) => None,
            })
            .collect();
        assert_eq!(flags, vec![false, false, true]);
    }

    #[test]
    fn artist_and_song_rows_carry_art_keys_genres_do_not() {
        let state = state_with("bu", Some(full_results()));
        let rows = build_trawl_rows(&state, &TrawlCrate::default());
        let arts: Vec<Option<String>> = rows
            .iter()
            .filter_map(|r| match r {
                TrawlRow::Result { art_album_id, .. } => Some(art_album_id.clone()),
                TrawlRow::Header(_) | TrawlRow::Hint(_) => None,
            })
            .collect();
        assert_eq!(
            arts[0].as_deref(),
            Some("ar1"),
            "artist mini keyed on artist id"
        );
        assert_eq!(arts[1].as_deref(), Some("al_s1"), "song thumb via album id");
        assert_eq!(arts[2], None, "genre gets the type glyph");
    }
}
