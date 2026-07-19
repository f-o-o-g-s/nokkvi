//! Rules-session view — the smart-playlist editor's two-pane layout inside
//! `View::PlaylistEditor` (`EditorSessionKind::Rules`).
//!
//! Layout: edit-bar band (eyebrow + name + comment + public + actions) over
//! `[ results pane ≈55% | rules form ≈45% ]`. No browsing panel — the
//! server refuses track mutations on smart playlists, so it would be dead
//! weight. Root widget types stay stable across renders (always the same
//! Column/Row skeleton; state changes are content swaps inside it).
//!
//! Visual chassis (binding per the design ruling): rule rows are
//! fixed-height bands inside the form's own scrolling container; cell
//! triggers are pill-style chips on the canonical
//! `mouse_area(HoverOverlay(container(...)))` clickable-cell pattern with
//! `theme::selection_ring_on` marking the form cursor; the remove
//! affordance is a `modal_icon_button`, not a bare web-form `×`. No new
//! colors, no new radii.

use iced::{
    Alignment, Element, Length,
    widget::{Space, column, container, mouse_area, row, text, text_input},
};
use nokkvi_data::types::{
    rules_session::RulesTarget,
    smart_criteria::{
        Conjunction, CriteriaNode, DiagnosticLocation, SEED_PRESETS, Severity, ValueShape,
    },
};

use crate::{
    app_message::{EditorMessage, Message, RulesEditorMessage},
    state::{
        FormCell, FormMode, FormRow, PlaylistEditorState, PreviewColumnVisibility, PreviewPhase,
        RulesPane, RulesSessionUi,
    },
    theme,
    widgets::hover_overlay::HoverOverlay,
};

/// Fixed band height for form rows (the settings detail-pane vocabulary).
const ROW_H: f32 = 40.0;
/// Preview row height (compact — the pane is a preview, not a queue).
const PREVIEW_ROW_H: f32 = 44.0;

/// Everything the rules view borrows from app state.
pub(crate) struct RulesViewData<'a> {
    pub session: &'a RulesSessionUi,
    pub album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    pub window_height: f32,
    /// The playlist's uploaded custom cover, when set AND its handle is warm
    /// (edit sessions only) — takes precedence over the derived quad.
    pub custom_cover: Option<&'a iced::widget::image::Handle>,
    /// Album ids feeding the 2×2 quad fallback: the playlist's frozen
    /// artwork ids on an edit session, or the current preview's distinct
    /// album covers on a create session (a live preview).
    pub cover_album_ids: Vec<String>,
    /// The cover accepts Set/Reset (an edit session against a saved
    /// playlist); a create session shows the quad but can't upload yet.
    pub cover_editable: bool,
    /// Which optional columns the preview renders (persistent copy on
    /// `Nokkvi`, restored on `PlayerSettingsLoaded`, toggled via the cog).
    pub column_visibility: PreviewColumnVisibility,
    /// Whether the preview/results-pane columns cog dropdown is open.
    pub column_dropdown_open: bool,
    /// Anchor bounds captured when the columns cog was clicked (open only).
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
}

impl PlaylistEditorState {
    /// Render the rules session. Returns root `Message` elements — the
    /// session composes three message families (Editor for the edit-bar
    /// inputs, RulesEditor for the form, SplitView for exit).
    pub(crate) fn rules_view<'a>(&'a self, data: RulesViewData<'a>) -> Element<'a, Message> {
        let session = data.session;
        let cover = super::cover::EditorCover {
            custom: data.custom_cover,
            album_ids: data.cover_album_ids,
            album_art: data.album_art,
            editable: data.cover_editable,
        };

        let edit_bar = self.rules_edit_bar(session, &cover);
        let sep = theme::horizontal_separator(1.0);

        let results = results_pane(
            session,
            data.album_art,
            data.window_height,
            data.column_visibility,
            data.column_dropdown_open,
            data.column_dropdown_trigger_bounds,
        );
        let form = form_pane(session, data.window_height);

        // Left preview pane recedes to bg0_hard (a "results/output" surface,
        // like the settings sidebar and Trawl results); the form stays bg0 so
        // its inset children — bg0_hard value cells, the JSON editor, bg1
        // chips — keep their calibrated contrast. A full-height 1px border
        // rule divides them (the prior divider was a horizontal hairline
        // clamped to a 1px×1px nub — effectively invisible).
        let panes = row![
            container(results)
                .width(Length::FillPortion(11))
                .height(Length::Fill)
                .style(|_t| iced::widget::container::Style {
                    background: Some(theme::bg0_hard().into()),
                    ..Default::default()
                }),
            pane_divider(),
            container(form)
                .width(Length::FillPortion(9))
                .height(Length::Fill),
        ]
        .height(Length::Fill);

        let mut root = column![edit_bar, sep, panes];

        // Save-conflict / target-gone banners render as content ABOVE the
        // panes (stable root: the column is always the same type).
        if session.save_conflict {
            root = column![
                self.rules_edit_bar(session, &cover),
                theme::horizontal_separator(1.0),
                conflict_banner(),
                panes_placeholder(
                    session,
                    data.album_art,
                    data.window_height,
                    data.column_visibility,
                    data.column_dropdown_open,
                    data.column_dropdown_trigger_bounds,
                ),
            ];
        } else if session.save_target_gone {
            root = column![
                self.rules_edit_bar(session, &cover),
                theme::horizontal_separator(1.0),
                target_gone_banner(),
                panes_placeholder(
                    session,
                    data.album_art,
                    data.window_height,
                    data.column_visibility,
                    data.column_dropdown_open,
                    data.column_dropdown_trigger_bounds,
                ),
            ];
        }

        let base: Element<'a, Message> = container(root)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_t| iced::widget::container::Style {
                background: Some(theme::bg0().into()),
                ..Default::default()
            })
            .into();

        // Root type stays `Stack` in EVERY state (a single-child stack when
        // no overlay is up) — flipping Container↔Stack rebuilds the base
        // subtree and wipes pane scroll offsets + text_input focus (the
        // root-widget-stability rule). Overlays are single-active: the
        // handler never opens the sub-picker and the discard confirm at once.
        let mut layers = iced::widget::stack![base];
        if let Some(picker) = &session.sub_picker {
            layers = layers.push(sub_picker_overlay(session, picker));
        } else if session.confirm_discard {
            layers = layers.push(discard_confirm_overlay());
        }
        layers.into()
    }

    /// The edit-bar band — cursor row 0 of the form's keyboard ring:
    /// nothing on it is mouse-only.
    fn rules_edit_bar<'a>(
        &'a self,
        session: &'a RulesSessionUi,
        cover: &super::cover::EditorCover<'a>,
    ) -> Element<'a, Message> {
        let on_bar = session.pane == RulesPane::Form
            && matches!(session.rows.get(session.cursor), Some(FormRow::EditBar));
        let cell_ring = |cell: FormCell| on_bar && session.cell == cell;
        // A ringed text field shows the pencil affordance too (but not while
        // it's actively being edited — the caret takes over then).
        let cursor_mode = session.mode != FormMode::Editing;

        let eyebrow = text("SMART PLAYLIST — RULES")
            .font(theme::weighted_ui_font(iced::font::Weight::Semibold))
            .size(9.5)
            .color(theme::accent())
            .wrapping(iced::widget::text::Wrapping::None);

        let name_input = ring_wrap(
            // Ring only while NAVIGATING to the cell (Cursor mode) — not while
            // typing the name (Editing), where the caret already marks focus.
            // A fresh create opens in Editing on Name, so this leaves the name a
            // clean borderless title (matching the regular playlist editor)
            // rather than a boxed, full-width field.
            cell_ring(FormCell::Name) && cursor_mode,
            edit_bar_field(
                text_input("Playlist name", &self.edit.playlist_name)
                    .id(RULES_NAME_INPUT_ID)
                    .on_input(|s| Message::Editor(EditorMessage::NameChanged(s)))
                    .font(theme::weighted_ui_font(iced::font::Weight::Bold))
                    .size(14)
                    .width(Length::Fill)
                    .padding([2, 4])
                    .style(bar_input_style)
                    .into(),
                cell_ring(FormCell::Name) && cursor_mode,
            ),
        );
        let comment_input = ring_wrap(
            cell_ring(FormCell::Comment) && cursor_mode,
            edit_bar_field(
                text_input("Comment", &self.edit.playlist_comment)
                    .id(RULES_COMMENT_INPUT_ID)
                    .on_input(|s| Message::Editor(EditorMessage::CommentChanged(s)))
                    .font(theme::ui_font())
                    .size(11)
                    .width(Length::Fill)
                    .padding([2, 4])
                    .style(bar_input_style)
                    .into(),
                cell_ring(FormCell::Comment) && cursor_mode,
            ),
        );

        let public_pill = ring_wrap(
            cell_ring(FormCell::Public),
            chip(
                if self.edit.playlist_public {
                    "Public"
                } else {
                    "Private"
                },
                false,
                Message::Editor(EditorMessage::PublicToggled(!self.edit.playlist_public)),
            ),
        );

        // Name diagnostics (empty blocks Save; duplicate warns).
        let name_diags = diag_line(session, &DiagnosticLocation::Name);

        let mut actions = row![].spacing(6).align_y(Alignment::Center);
        // The true pre-save Preview (Ctrl+Enter) — a press with unchanged
        // rules IS the manual re-evaluate.
        actions = actions.push(chip(
            "Preview",
            false,
            Message::RulesEditor(RulesEditorMessage::Preview),
        ));
        let save_label = if session.saving { "Saving…" } else { "Save" };
        actions = actions.push(chip(
            save_label,
            true,
            Message::RulesEditor(RulesEditorMessage::Save),
        ));
        actions = actions.push(ring_wrap(
            cell_ring(FormCell::SaveAsNew),
            chip(
                "Save as new…",
                false,
                Message::RulesEditor(RulesEditorMessage::SaveAsNew),
            ),
        ));
        actions = actions.push(chip(
            "Discard",
            false,
            Message::RulesEditor(RulesEditorMessage::EscapePressed),
        ));

        let left = column![eyebrow, name_input, comment_input,]
            .spacing(2)
            .width(Length::FillPortion(3));
        let right = column![
            row![public_pill, actions]
                .spacing(8)
                .align_y(Alignment::Center),
            name_diags,
        ]
        .spacing(2)
        .align_x(Alignment::End)
        .width(Length::FillPortion(2));

        container(
            row![
                super::cover::cover_thumbnail(
                    cover,
                    Message::RulesEditor(RulesEditorMessage::SetCover),
                    Message::RulesEditor(RulesEditorMessage::ResetCover),
                ),
                left,
                right
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding([6, 10]),
        )
        .width(Length::Fill)
        .into()
    }
}

// =========================================================================
// Results pane (left)
// =========================================================================

fn results_pane<'a>(
    session: &'a RulesSessionUi,
    album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    _window_height: f32,
    cols: PreviewColumnVisibility,
    column_dropdown_open: bool,
    column_dropdown_trigger_bounds: Option<iced::Rectangle>,
) -> Element<'a, Message> {
    let is_create = matches!(session.target, RulesTarget::Create);

    // Header strip: match count + age-aware freshness stamp (or the
    // in-flight copy — style-only variance, same widget tree).
    let phase = session.preview.phase();
    let header_text = match (&phase, session.preview.total) {
        (PreviewPhase::Evaluating, _) => "evaluating…".to_owned(),
        (PreviewPhase::Loaded, Some(total)) => {
            let stamp = session
                .preview
                .evaluated_at
                .as_deref()
                .and_then(nokkvi_data::utils::formatters::format_evaluated_stamp_now)
                .unwrap_or_default();
            if stamp.is_empty() {
                format!("{total} matches")
            } else {
                format!("{total} matches · {stamp}")
            }
        }
        (PreviewPhase::Loaded, None) => format!("{} matches", session.preview.rows.len()),
        _ if is_create => String::new(),
        _ => "Showing the saved rules' matches".to_owned(),
    };
    // The columns cog — same checkbox dropdown every library view uses, but on
    // the preview's own `OpenMenu::CheckboxDropdownPreview` discriminator (this
    // surface has no `View` variant, like Similar).
    let cog: Element<'a, Message> = crate::widgets::checkbox_dropdown::preview_columns_dropdown(
        cols.dropdown_entries(),
        |c| Message::RulesEditor(RulesEditorMessage::ToggleColumnVisible(c)),
        Message::SetOpenMenu,
        column_dropdown_open,
        column_dropdown_trigger_bounds,
    )
    .into();

    // A persistent "PREVIEW" eyebrow (mirrors the rules pane's eyebrow) so a
    // populated pane can't be mistaken for the saved playlist's tracks — these
    // are the rules' current matches, not what's committed. The columns cog sits
    // at the far end of the strip.
    let header = container(
        row![
            text("PREVIEW")
                .size(9.5)
                .font(theme::weighted_ui_font(iced::font::Weight::Semibold))
                .color(theme::accent())
                .wrapping(iced::widget::text::Wrapping::None),
            text(header_text)
                .size(11)
                .font(theme::ui_font())
                .color(theme::fg3()),
            Space::new().width(Length::Fill),
            cog,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .width(Length::Fill);

    // Body: the six honest states.
    let body: Element<'a, Message> = match phase {
        // Blank creates have no draft yet (zero network at open) — this
        // copy is exactly truthful; rules-bearing sessions land here only
        // for the instant before the open-preview's Evaluating flips in.
        PreviewPhase::PreFirst => empty_copy("Preview to see matches"),
        PreviewPhase::Evaluating if session.preview.rows.is_empty() => empty_copy("evaluating…"),
        PreviewPhase::Unavailable => iced::widget::column![
            empty_copy("Server unreachable — preview unavailable"),
            iced::widget::container(chip(
                "Retry",
                true,
                Message::RulesEditor(RulesEditorMessage::Preview),
            ))
            .center_x(Length::Fill)
            .padding(8),
        ]
        .into(),
        PreviewPhase::Failed if session.preview.rows.is_empty() => {
            empty_copy("Couldn't read the matches — Re-evaluate to retry")
        }
        _ if session.has_blocking_errors() && session.preview.rows.is_empty() => {
            empty_copy("Fix the marked rule to preview")
        }
        _ if session.preview.rows.is_empty() => {
            empty_copy("No matches — the rules are valid but nothing fits")
        }
        _ => {
            let focused = session.pane == RulesPane::Results;
            let mut rows_col = column![].spacing(0);
            for (i, song) in session.preview.rows.iter().enumerate() {
                rows_col = rows_col.push(preview_row(
                    song,
                    i,
                    focused && session.preview.cursor == i,
                    album_art,
                    cols,
                ));
            }
            // Failure retains last-good rows + stamp with a retry line —
            // never blanks the pane.
            if matches!(phase, PreviewPhase::Failed) {
                rows_col = rows_col.push(
                    container(
                        text("Couldn't refresh — Re-evaluate to retry")
                            .size(11)
                            .font(theme::ui_font())
                            .color(theme::warning()),
                    )
                    .padding(8),
                );
            }
            iced::widget::scrollable(rows_col)
                .id(iced::widget::Id::new(RULES_PREVIEW_SCROLLABLE_ID))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::settings_scrollable_style)
                .into()
        }
    };

    column![header, theme::horizontal_separator(1.0), body].into()
}

fn preview_row<'a>(
    song: &'a nokkvi_data::backend::queue::QueueSongUIViewData,
    _index: usize,
    centered: bool,
    album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    cols: PreviewColumnVisibility,
) -> Element<'a, Message> {
    let art: Element<'a, Message> = match album_art.get(&song.album_id) {
        Some(handle) => iced::widget::image(handle.clone())
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .into(),
        None => container(Space::new())
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .style(|_t| iced::widget::container::Style {
                background: Some(theme::bg1().into()),
                ..Default::default()
            })
            .into(),
    };
    // Wrapping::None + Ellipsis::End lets iced's text layout truncate a long
    // title/artist with "…" at the (clipped, bounded) column edge instead of
    // overflowing into the trailing columns — the slot_list_text recipe.
    let title = text(&song.title)
        .size(12.5)
        .font(theme::ui_font())
        .color(theme::fg0())
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::widget::text::Ellipsis::End);
    let meta = text(format!("{} · {}", song.artist, song.album))
        .size(10.5)
        .font(theme::ui_font())
        .color(theme::fg3())
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::widget::text::Ellipsis::End);
    // The text column takes the flex space; a clipped, Fill-width container
    // bounds it so the ellipsis has an edge to truncate at.
    let text_col = container(column![title, meta].spacing(1))
        .width(Length::Fill)
        .clip(true);

    // Each visible column is a fixed-width trailing cell so they line up across
    // rows. All display-only — the preview shows the values, never mutates.
    let mut cells = row![art, text_col]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([4, 10]);
    if cols.stars {
        cells = cells.push(preview_stars_cell(song.rating));
    }
    if cols.love {
        cells = cells.push(preview_love_cell(song.starred));
    }
    if cols.plays {
        cells = cells.push(preview_plays_cell(song.play_count));
    }
    if cols.genre {
        cells = cells.push(preview_genre_cell(&song.genre));
    }
    if cols.duration {
        cells = cells.push(preview_duration_cell(&song.duration));
    }

    // The preview pane is now bg0_hard — the surface-aware ring must match.
    let ring = if centered {
        theme::selection_ring_on(theme::bg0_hard())
    } else {
        iced::Color::TRANSPARENT
    };
    let row_container = container(cells)
        .width(Length::Fill)
        .height(Length::Fixed(PREVIEW_ROW_H))
        .style(move |_t| iced::widget::container::Style {
            border: iced::Border {
                color: ring,
                width: 2.0,
                radius: theme::ui_radius_sm(),
            },
            ..Default::default()
        });
    // Tag the centered row so `center_in_scrollable` can measure it and keep
    // the keyboard cursor in view as it walks a long preview.
    if centered {
        row_container
            .id(iced::widget::Id::new(RULES_PREVIEW_CURSOR_ID))
            .into()
    } else {
        row_container.into()
    }
}

// -- Preview column cells (display-only) ----------------------------------
// Fixed widths so the columns line up across rows in the narrow pane. Every
// trailing cell is Fixed-width (including duration) so the Fill text column
// absorbs a constant amount of slack and the column group's start-x is
// identical row-to-row.
const PREVIEW_STARS_W: f32 = 62.0;
const PREVIEW_LOVE_W: f32 = 18.0;
const PREVIEW_PLAYS_W: f32 = 34.0;
const PREVIEW_GENRE_W: f32 = 88.0;
const PREVIEW_DURATION_W: f32 = 48.0;

/// A plain, unselected, full-opacity slot style so the preview can reuse the
/// canonical `slot_list_star_rating` / `slot_list_favorite_icon` renderers
/// instead of hand-copying their icon-path + color recipe (which would silently
/// drift from the six library views on a future glyph change).
/// `for_slot(is_center, is_highlighted, is_playing, is_selected,
/// has_multi_selection, opacity, depth)` — all off, full opacity, top level.
fn preview_glyph_style() -> crate::widgets::slot_list::SlotListSlotStyle {
    crate::widgets::slot_list::SlotListSlotStyle::for_slot(
        false, false, false, false, false, 1.0, 0,
    )
}

/// Five stars filled to `rating` (dim outlines for the remainder), so a rules
/// author can read a 2-vs-3-star match at a glance. Unrated → 5 dim outlines.
/// Display-only (`on_click: None`) — the preview shows, it doesn't rate.
fn preview_stars_cell<'a>(rating: Option<u32>) -> Element<'a, Message> {
    container(crate::widgets::slot_list::slot_list_star_rating(
        rating.unwrap_or(0) as usize,
        10.0,
        preview_glyph_style(),
        None,
        None::<fn(usize) -> Message>,
    ))
    .width(Length::Fixed(PREVIEW_STARS_W))
    .align_y(Alignment::Center)
    .into()
}

/// A filled heart when loved, else a dim outline heart. Display-only.
fn preview_love_cell<'a>(starred: bool) -> Element<'a, Message> {
    container(crate::widgets::slot_list::slot_list_favorite_icon(
        starred,
        preview_glyph_style(),
        12.0,
        crate::widgets::slot_list::FavoriteIconKind::Heart,
        None,
    ))
    .width(Length::Fixed(PREVIEW_LOVE_W))
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(Alignment::Center)
    .into()
}

/// Right-aligned play count (`0` when unknown).
fn preview_plays_cell<'a>(play_count: Option<u32>) -> Element<'a, Message> {
    container(
        text(play_count.unwrap_or(0).to_string())
            .size(11)
            .font(theme::ui_font())
            .color(theme::fg3())
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fixed(PREVIEW_PLAYS_W))
    .align_x(iced::alignment::Horizontal::Right)
    .align_y(Alignment::Center)
    .into()
}

/// Genre text, ellipsized to a fixed width (no wrap, no row-height jitter).
fn preview_genre_cell<'a>(genre: &'a str) -> Element<'a, Message> {
    container(
        text(genre)
            .size(10.5)
            .font(theme::ui_font())
            .color(theme::fg3())
            .wrapping(iced::widget::text::Wrapping::None)
            .ellipsis(iced::widget::text::Ellipsis::End),
    )
    .width(Length::Fixed(PREVIEW_GENRE_W))
    .align_y(Alignment::Center)
    .clip(true)
    .into()
}

/// Right-aligned duration in a fixed-width cell (fits H:MM:SS) so it doesn't
/// shift the other columns' start-x row-to-row.
fn preview_duration_cell<'a>(duration: &'a str) -> Element<'a, Message> {
    container(
        text(duration)
            .size(11)
            .font(theme::ui_font())
            .color(theme::fg3())
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fixed(PREVIEW_DURATION_W))
    .align_x(iced::alignment::Horizontal::Right)
    .align_y(Alignment::Center)
    .into()
}

// =========================================================================
// Rules form (right)
// =========================================================================

fn form_pane<'a>(session: &'a RulesSessionUi, _window_height: f32) -> Element<'a, Message> {
    // JSON mode swaps the form body for the editable text_editor — the
    // app's first: styled inside the existing form chrome, rendered in
    // theme::ui_font() (nokkvi ships no mono face — deliberate).
    if let Some(json) = &session.json {
        let editor = iced::widget::text_editor(&json.content)
            .id(RULES_JSON_EDITOR_ID)
            .on_action(|a| Message::RulesEditor(RulesEditorMessage::JsonEdited(a)))
            .font(theme::ui_font())
            .size(12)
            .height(Length::Fill)
            .style(json_editor_style);
        let mut col = column![
            form_hint("Raw rules JSON — Escape applies a clean parse; Ctrl+Enter validates"),
            container(editor)
                .padding(6)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_t| iced::widget::container::Style {
                    background: Some(theme::bg0_hard().into()),
                    border: iced::Border {
                        color: theme::bg3(),
                        width: 1.0,
                        radius: theme::ui_radius_sm(),
                    },
                    ..Default::default()
                }),
        ]
        .spacing(4)
        .padding(8);
        if let Some(err) = &json.parse_error {
            col = col.push(
                text(format!("Parse error: {err}"))
                    .size(11)
                    .font(theme::ui_font())
                    .color(theme::warning()),
            );
        }
        if json.revert_offer {
            col = col.push(
                row![
                    chip(
                        "Keep editing",
                        false,
                        Message::RulesEditor(RulesEditorMessage::JsonKeepEditing),
                    ),
                    chip(
                        "Revert to last valid rules",
                        true,
                        Message::RulesEditor(RulesEditorMessage::JsonRevertToLastGood),
                    ),
                ]
                .spacing(8),
            );
        }
        return col.into();
    }

    // Create empty state: the preset list + Start empty + Import from
    // .nsp — activatable rows, keyboard-reachable via `empty_state_cursor`.
    if session.is_blank_create() {
        let mut col = column![].spacing(2).padding(8);
        for (i, (title, subtitle, msg)) in empty_state_entries().into_iter().enumerate() {
            // The two build-it-yourself paths come first, then the seeded
            // presets under their own label (inserted before the first one).
            if i == EMPTY_STATE_LEAD_ACTIONS {
                col = col.push(form_hint("Or start from a preset"));
            }
            col = col.push(action_row(
                title,
                subtitle,
                msg,
                i == session.empty_state_cursor,
            ));
        }
        return iced::widget::scrollable(col)
            .height(Length::Fill)
            .style(theme::settings_scrollable_style)
            .into();
    }

    let locked = !session.form_editable();
    let mut col = column![].spacing(2).padding(8);
    if locked {
        col = col.push(form_hint(
            "Rules nested deeper than one level — shown read-only; edit as JSON below",
        ));
    }

    let focused_form = session.pane == RulesPane::Form;
    let mut prev_section: Option<u8> = None;
    for (row_idx, form_row) in session.rows.iter().enumerate() {
        // The edit-bar band renders above the panes, not in the form body.
        if matches!(form_row, FormRow::EditBar) {
            continue;
        }
        // A hairline + breathing room between the matching / sort / limit /
        // JSON sections so the form reads as grouped bands, not one float of
        // text. Visual-only — the keyboard cursor is indexed off
        // `session.rows`, never these inserted children.
        let section = form_section(form_row);
        if prev_section.is_some_and(|p| p != section) {
            col = col.push(Space::new().height(Length::Fixed(6.0)));
            col = col.push(theme::horizontal_separator(1.0));
            col = col.push(Space::new().height(Length::Fixed(6.0)));
        }
        prev_section = Some(section);
        let cursor_here =
            focused_form && session.cursor == row_idx && session.mode != FormMode::Json;
        col = col.push(render_form_row(session, form_row, row_idx, cursor_here));
        // Row-anchored diagnostics render under their row.
        if let FormRow::Rule(path) | FormRow::GroupHeader(path) = form_row {
            col = col.push(diag_line(session, &DiagnosticLocation::Rule(path.clone())));
        }
        if let FormRow::SortKey(i) = form_row {
            col = col.push(diag_line(session, &DiagnosticLocation::Sort(*i)));
        }
        if matches!(form_row, FormRow::Limit) {
            col = col.push(diag_line(session, &DiagnosticLocation::Limit));
            col = col.push(diag_line(session, &DiagnosticLocation::Offset));
            // Contextual, not a permanent cramped label: the offset only bites
            // when a limit is set (the server applies it after the cap).
            if session.rules.offset.is_some()
                && session.rules.limit.is_none()
                && session.rules.limit_percent.is_none()
            {
                col = col.push(form_hint("Offset applies only when a limit is set"));
            }
        }
    }
    col = col.push(diag_line(session, &DiagnosticLocation::Root));

    iced::widget::scrollable(col)
        .height(Length::Fill)
        .style(theme::settings_scrollable_style)
        .into()
}

fn render_form_row<'a>(
    session: &'a RulesSessionUi,
    form_row: &'a FormRow,
    row_idx: usize,
    cursor_here: bool,
) -> Element<'a, Message> {
    let locked = !session.form_editable();
    let cell_ring = |cell: FormCell| cursor_here && session.cell == cell;
    let click =
        |cell: FormCell| Message::RulesEditor(RulesEditorMessage::ClickCell { row: row_idx, cell });

    let content: Element<'a, Message> = match form_row {
        FormRow::EditBar => Space::new().into(), // rendered above the panes
        FormRow::Match => {
            let conj = session
                .rules
                .root
                .as_ref()
                .map_or(Conjunction::All, |r| r.conjunction);
            row![
                text("Match")
                    .size(12)
                    .font(theme::ui_font())
                    .color(theme::fg2()),
                ring_wrap(
                    cell_ring(FormCell::ConjunctionPill),
                    chip(
                        match conj {
                            Conjunction::All => "All",
                            Conjunction::Any => "Any",
                        },
                        true,
                        click(FormCell::ConjunctionPill),
                    ),
                ),
                text("of the following")
                    .size(12)
                    .font(theme::ui_font())
                    .color(theme::fg2()),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        FormRow::GroupHeader(path) => {
            let conj = match session.node_at(path) {
                Some(CriteriaNode::Group(g)) => g.conjunction,
                _ => Conjunction::All,
            };
            row![
                Space::new().width(Length::Fixed(16.0)),
                ring_wrap(
                    cell_ring(FormCell::ConjunctionPill),
                    chip(
                        match conj {
                            Conjunction::All => "All of…",
                            Conjunction::Any => "Any of…",
                        },
                        true,
                        click(FormCell::ConjunctionPill),
                    ),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        FormRow::Rule(path) => render_rule_row(session, path, cursor_here, locked, row_idx),
        FormRow::AddRule(path) => {
            let indent = if path.is_empty() { 0.0 } else { 16.0 };
            row![
                Space::new().width(Length::Fixed(indent)),
                ring_wrap(
                    cursor_here,
                    dim_action("+ Add rule", click(FormCell::RowAction)),
                ),
            ]
            .into()
        }
        FormRow::SortKey(i) => {
            let keys = session.rules.effective_sort_keys();
            let Some(key) = keys.get(*i).cloned() else {
                return Space::new().into();
            };
            row![
                text(if *i == 0 { "Sort by" } else { "then" })
                    .size(12)
                    .font(theme::ui_font())
                    .color(theme::fg2())
                    .width(Length::Fixed(52.0)),
                ring_wrap(
                    cell_ring(FormCell::SortField),
                    chip(&key.field, false, click(FormCell::SortField)),
                ),
                ring_wrap(
                    cell_ring(FormCell::SortDirection),
                    chip(
                        if key.descending { "desc" } else { "asc" },
                        false,
                        click(FormCell::SortDirection),
                    ),
                ),
                Space::new().width(Length::Fill),
                ring_wrap(
                    cell_ring(FormCell::Remove),
                    crate::widgets::modal_button::modal_icon_button(
                        "assets/icons/list-minus.svg",
                        14.0,
                        click(FormCell::Remove),
                    ),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        FormRow::AddSortKey => ring_wrap(
            cursor_here,
            dim_action("+ Add sort", click(FormCell::RowAction)),
        ),
        FormRow::Limit => {
            let (limit_text, is_pct) = match (session.rules.limit, session.rules.limit_percent) {
                (Some(n), _) => (n.to_string(), false),
                (None, Some(p)) => (p.to_string(), true),
                (None, None) => (String::new(), false),
            };
            let offset_text = session
                .rules
                .offset
                .map(|n| n.to_string())
                .unwrap_or_default();
            row![
                text("Limit")
                    .size(12)
                    .font(theme::ui_font())
                    .color(theme::fg2())
                    .width(Length::Fixed(52.0)),
                ring_wrap(
                    cell_ring(FormCell::LimitValue),
                    value_cell(
                        session,
                        FormCell::LimitValue,
                        row_idx,
                        &limit_text,
                        "none",
                        cursor_here,
                    ),
                ),
                ring_wrap(
                    cell_ring(FormCell::LimitMode),
                    chip(
                        if is_pct { "%" } else { "#" },
                        false,
                        click(FormCell::LimitMode)
                    ),
                ),
                text("offset")
                    .size(11)
                    .font(theme::ui_font())
                    .color(theme::fg3()),
                ring_wrap(
                    cell_ring(FormCell::OffsetValue),
                    value_cell(
                        session,
                        FormCell::OffsetValue,
                        row_idx,
                        &offset_text,
                        "0",
                        cursor_here,
                    ),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        FormRow::JsonToggle => ring_wrap(
            cursor_here,
            dim_action("Edit as JSON…", click(FormCell::RowAction)),
        ),
    };

    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H))
        .padding([2, 4])
        .into()
}

fn render_rule_row<'a>(
    session: &'a RulesSessionUi,
    path: &[usize],
    cursor_here: bool,
    locked: bool,
    row_idx: usize,
) -> Element<'a, Message> {
    let cell_ring = |cell: FormCell| cursor_here && session.cell == cell;
    let click =
        |cell: FormCell| Message::RulesEditor(RulesEditorMessage::ClickCell { row: row_idx, cell });
    let indent = if path.len() > 1 { 16.0 } else { 0.0 };

    match session.node_at(path) {
        Some(CriteriaNode::Leaf(leaf)) => {
            let field_label = leaf.field.clone();
            let op_label = leaf.operator.label();
            let shape = leaf
                .operator
                .value_shape(session.field_class_of(&leaf.field));

            let mut cells = row![Space::new().width(Length::Fixed(indent))]
                .spacing(6)
                .align_y(Alignment::Center);
            cells = cells.push(ring_wrap(
                cell_ring(FormCell::Field),
                chip(&field_label, false, click(FormCell::Field)),
            ));
            cells = cells.push(ring_wrap(
                cell_ring(FormCell::Operator),
                chip(op_label, false, click(FormCell::Operator)),
            ));
            match shape {
                ValueShape::FieldFlag => {}
                ValueShape::Toggle => {
                    let on = leaf.value.as_bool().unwrap_or(false);
                    cells = cells.push(ring_wrap(
                        cell_ring(FormCell::Value),
                        chip(if on { "On" } else { "Off" }, false, click(FormCell::Value)),
                    ));
                }
                ValueShape::PlaylistRef => {
                    // Dangling refs render the raw id — never a blank cell.
                    let id = leaf.value.as_str().unwrap_or_default();
                    let label = session
                        .session_playlists
                        .iter()
                        .find(|(pid, _)| pid == id)
                        .map_or_else(|| id.to_owned(), |(_, name)| name.clone());
                    cells = cells.push(ring_wrap(
                        cell_ring(FormCell::Value),
                        chip(&label, false, click(FormCell::Value)),
                    ));
                }
                ValueShape::Pair | ValueShape::DatePair => {
                    let placeholder = if shape == ValueShape::DatePair {
                        "YYYY-MM-DD"
                    } else {
                        "0"
                    };
                    let v1 = session.leaf_value_text(path, false);
                    let v2 = session.leaf_value_text(path, true);
                    cells = cells.push(ring_wrap(
                        cell_ring(FormCell::Value),
                        value_cell(
                            session,
                            FormCell::Value,
                            row_idx,
                            &v1,
                            placeholder,
                            cursor_here,
                        ),
                    ));
                    cells = cells.push(
                        text("to")
                            .size(11)
                            .font(theme::ui_font())
                            .color(theme::fg3()),
                    );
                    cells = cells.push(ring_wrap(
                        cell_ring(FormCell::Value2),
                        value_cell(
                            session,
                            FormCell::Value2,
                            row_idx,
                            &v2,
                            placeholder,
                            cursor_here,
                        ),
                    ));
                }
                ValueShape::Days => {
                    let v = session.leaf_value_text(path, false);
                    cells = cells.push(ring_wrap(
                        cell_ring(FormCell::Value),
                        value_cell(session, FormCell::Value, row_idx, &v, "30", cursor_here),
                    ));
                    cells = cells.push(
                        text("days")
                            .size(11)
                            .font(theme::ui_font())
                            .color(theme::fg3()),
                    );
                }
                ValueShape::Date => {
                    let v = session.leaf_value_text(path, false);
                    cells = cells.push(ring_wrap(
                        cell_ring(FormCell::Value),
                        value_cell(
                            session,
                            FormCell::Value,
                            row_idx,
                            &v,
                            "YYYY-MM-DD",
                            cursor_here,
                        ),
                    ));
                }
                ValueShape::Text | ValueShape::Number => {
                    let v = session.leaf_value_text(path, false);
                    cells = cells.push(ring_wrap(
                        cell_ring(FormCell::Value),
                        value_cell(session, FormCell::Value, row_idx, &v, "value", cursor_here),
                    ));
                }
            }
            cells = cells.push(Space::new().width(Length::Fill));
            if !locked {
                cells = cells.push(ring_wrap(
                    cell_ring(FormCell::Remove),
                    crate::widgets::modal_button::modal_icon_button(
                        "assets/icons/list-minus.svg",
                        14.0,
                        click(FormCell::Remove),
                    ),
                ));
            }
            cells.into()
        }
        Some(CriteriaNode::Unknown(value)) => {
            // Read-only raw pill — never dropped, edit-as-JSON reaches it.
            let raw = serde_json::to_string(value).unwrap_or_default();
            row![
                Space::new().width(Length::Fixed(indent)),
                container(
                    text(raw)
                        .size(11)
                        .font(theme::ui_font())
                        .color(theme::fg3())
                        .wrapping(iced::widget::text::Wrapping::None),
                )
                .padding([4, 8])
                .style(|_t| iced::widget::container::Style {
                    background: Some(theme::bg1().into()),
                    border: iced::Border {
                        color: theme::bg3(),
                        width: 1.0,
                        radius: theme::ui_radius_sm(),
                    },
                    ..Default::default()
                }),
                Space::new().width(Length::Fill),
                ring_wrap(
                    cell_ring(FormCell::Remove),
                    crate::widgets::modal_button::modal_icon_button(
                        "assets/icons/list-minus.svg",
                        14.0,
                        click(FormCell::Remove),
                    ),
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
        }
        _ => Space::new().into(),
    }
}

// =========================================================================
// Sub-picker + confirm overlays
// =========================================================================

fn sub_picker_overlay<'a>(
    session: &'a RulesSessionUi,
    picker: &'a crate::state::SubPicker,
) -> Element<'a, Message> {
    // The date picker is a calendar grid, not a filterable list — it renders
    // from its own displayed month + focused day, no entries.
    if let crate::state::SubPickerKind::DateValue { year, month, .. } = &picker.kind {
        return date_picker_overlay(*year, *month, picker.cursor);
    }
    // The Nokkvi-side entries builder lives on the root impl; rebuild here
    // from the same inputs (pure).
    let entries = crate::update::rules_editor::rules_picker_entries(session, picker);
    let search = text_input("Type to filter…", &picker.query)
        .on_input(|s| Message::RulesEditor(RulesEditorMessage::SubPickerQuery(s)))
        .on_submit(Message::RulesEditor(RulesEditorMessage::SubPickerCommit))
        .id(RULES_SUB_PICKER_INPUT_ID)
        .size(13)
        .font(theme::ui_font())
        .padding(8)
        .width(Length::Fill)
        .style(value_input_style);

    let mut rows_col = column![].spacing(0);
    for (i, (value, label)) in entries.iter().enumerate().take(RULES_PICKER_RENDER_CAP) {
        let selected = i == picker.cursor;
        let label_el = text(label.clone())
            .size(12.5)
            .font(theme::ui_font())
            .color(theme::fg0());
        // The dim right column only when it says something new — a tag-value
        // entry's value IS its label, so it would otherwise print twice.
        let content: Element<'a, Message> = if value == label {
            label_el.into()
        } else {
            row![
                label_el,
                Space::new().width(Length::Fill),
                text(value.clone())
                    .size(10.5)
                    .font(theme::ui_font())
                    .color(theme::fg4()),
            ]
            .align_y(Alignment::Center)
            .into()
        };
        let mut cell = container(content)
            .padding([6, 10])
            .width(Length::Fill)
            // The keyboard cursor is a subtle fill, not a boxed border — a
            // list highlight, not a stray outline.
            .style(move |_t| iced::widget::container::Style {
                background: selected.then(|| theme::bg2().into()),
                border: iced::Border {
                    radius: theme::ui_radius_sm(),
                    ..Default::default()
                },
                ..Default::default()
            });
        // Tag the cursor row so keyboard nav can scroll it into view.
        if selected {
            cell = cell.id(iced::widget::Id::new(RULES_SUB_PICKER_CURSOR_ID));
        }
        rows_col = rows_col.push(
            mouse_area(HoverOverlay::new(cell).border_radius(theme::ui_radius_sm()))
                .on_press(Message::RulesEditor(RulesEditorMessage::ClickPickerRow(i))),
        );
    }

    let panel = container(
        column![
            search,
            theme::horizontal_separator(1.0),
            iced::widget::scrollable(rows_col)
                .id(iced::widget::Id::new(RULES_SUB_PICKER_SCROLLABLE_ID))
                .height(Length::Fixed(320.0))
                .style(theme::settings_scrollable_style),
        ]
        .spacing(4),
    )
    .width(Length::Fixed(380.0))
    .padding(8)
    .style(theme::modal_frame_style);

    theme::modal_scaffold(
        panel.into(),
        Message::RulesEditor(RulesEditorMessage::SubPickerCancel),
        theme::MODAL_BACKDROP_ALPHA,
    )
}

/// A themed month calendar: month header with ‹ › nav, a Sunday-first weekday
/// row, and a 7-column day grid. The focused day (keyboard cursor / seeded
/// value) is filled with the accent; today carries a thin accent ring. Every
/// surface is theme-driven — no default iced palette.
fn date_picker_overlay<'a>(year: i32, month: u32, focused_day: usize) -> Element<'a, Message> {
    use nokkvi_data::utils::calendar;

    const CELL_W: f32 = 36.0;
    const CELL_H: f32 = 30.0;

    let grid = calendar::month_grid(year, month);
    let today = calendar::today_ymd();

    let header = row![
        date_nav_button(
            "‹",
            Message::RulesEditor(RulesEditorMessage::DatePickerShiftMonth { forward: false }),
        ),
        container(
            text(format!("{} {}", grid.month_name, grid.year))
                .size(13)
                .font(theme::weighted_ui_font(iced::font::Weight::Bold))
                .color(theme::fg0()),
        )
        .width(Length::Fill)
        .align_x(Alignment::Center),
        date_nav_button(
            "›",
            Message::RulesEditor(RulesEditorMessage::DatePickerShiftMonth { forward: true }),
        ),
    ]
    .align_y(Alignment::Center);

    // Sunday-first weekday header (leading letters intentionally repeat S/T).
    let mut weekdays = row![].spacing(2);
    for wd in ["S", "M", "T", "W", "T", "F", "S"] {
        weekdays = weekdays.push(
            container(
                text(wd)
                    .size(10.5)
                    .font(theme::ui_font())
                    .color(theme::fg4()),
            )
            .width(Length::Fixed(CELL_W))
            .height(Length::Fixed(18.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
        );
    }

    let blank = || {
        container(text(""))
            .width(Length::Fixed(CELL_W))
            .height(Length::Fixed(CELL_H))
    };
    let mut grid_col = column![].spacing(2);
    let mut week = row![].spacing(2);
    let mut col = 0u32;
    for _ in 0..grid.leading_blanks {
        week = week.push(blank());
        col += 1;
    }
    for day in 1..=grid.days_in_month {
        let focused = day as usize == focused_day;
        let is_today = (grid.year, grid.month, day) == today;
        week = week.push(day_cell(day, focused, is_today, CELL_W, CELL_H));
        col += 1;
        if col == 7 {
            grid_col = grid_col.push(week);
            week = row![].spacing(2);
            col = 0;
        }
    }
    if col > 0 {
        for _ in col..7 {
            week = week.push(blank());
        }
        grid_col = grid_col.push(week);
    }

    // A fixed width bounds the panel to the 7-column grid (7·CELL_W + 6·2px
    // spacing + 2·10px padding). Without it the header's Fill month-name and
    // the full-width separator propagate up and stretch the panel across the
    // whole window (the flex-width gotcha).
    const PANEL_W: f32 = 7.0 * CELL_W + 6.0 * 2.0 + 2.0 * 10.0;
    let panel =
        container(column![header, theme::horizontal_separator(1.0), weekdays, grid_col].spacing(6))
            .width(Length::Fixed(PANEL_W))
            .padding(10)
            .style(theme::modal_frame_style);

    theme::modal_scaffold(
        panel.into(),
        Message::RulesEditor(RulesEditorMessage::SubPickerCancel),
        theme::MODAL_BACKDROP_ALPHA,
    )
}

/// A ‹ / › month-nav button for the calendar header.
fn date_nav_button<'a>(glyph: &'a str, msg: Message) -> Element<'a, Message> {
    mouse_area(
        HoverOverlay::new(
            container(
                text(glyph)
                    .size(16)
                    .font(theme::ui_font())
                    .color(theme::fg1()),
            )
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(|_t| iced::widget::container::Style {
                border: iced::Border {
                    radius: theme::ui_radius_sm(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .border_radius(theme::ui_radius_sm()),
    )
    .on_press(msg)
    .into()
}

/// One day cell in the calendar grid. `focused` = the accent-filled day that
/// Enter commits; `today` = a thin accent ring when it isn't the focused day.
fn day_cell<'a>(day: u32, focused: bool, today: bool, w: f32, h: f32) -> Element<'a, Message> {
    let label_color = if focused {
        theme::bg0_hard()
    } else {
        theme::fg0()
    };
    let cell = container(
        text(day.to_string())
            .size(12.5)
            .font(theme::ui_font())
            .color(label_color),
    )
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_t| iced::widget::container::Style {
        background: focused.then(|| theme::accent_bright().into()),
        border: iced::Border {
            color: if today && !focused {
                theme::accent()
            } else {
                iced::Color::TRANSPARENT
            },
            width: if today && !focused { 1.5 } else { 0.0 },
            radius: theme::ui_radius_sm(),
        },
        ..Default::default()
    });
    // An accent-filled focused cell needs the neutral hover pigment so the wash
    // stays visible on top of the accent.
    let hover = HoverOverlay::new(cell)
        .border_radius(theme::ui_radius_sm())
        .on_accent_surface(focused);
    mouse_area(hover)
        .on_press(Message::RulesEditor(RulesEditorMessage::DatePickerPickDay(
            day,
        )))
        .into()
}

/// Scrollable id for the results/preview pane + the id tagged onto the
/// centered row — feed `center_in_scrollable` so the keyboard cursor
/// stays in view.
pub(crate) const RULES_PREVIEW_SCROLLABLE_ID: &str = "rules_preview_scrollable";
pub(crate) const RULES_PREVIEW_CURSOR_ID: &str = "rules_preview_cursor_row";

/// `text_input` id for the edit-bar name field — focused when a create
/// session opens and when the Name cell enters Editing mode.
pub(crate) const RULES_NAME_INPUT_ID: &str = "rules_edit_bar_name";
/// `text_input` id for the edit-bar comment field.
pub(crate) const RULES_COMMENT_INPUT_ID: &str = "rules_edit_bar_comment";
/// `text_editor` id for the raw-JSON mode editor — focused on entry so
/// keyboard-only JSON editing works without a mouse click.
pub(crate) const RULES_JSON_EDITOR_ID: &str = "rules_json_editor";

/// `text_input` id for the sub-picker search — focused on open.
pub(crate) const RULES_SUB_PICKER_INPUT_ID: &str = "rules_sub_picker_search";
/// Scrollable id for the sub-picker list + the id tagged onto the cursor
/// row, so keyboard nav can scroll the selected entry into view.
pub(crate) const RULES_SUB_PICKER_SCROLLABLE_ID: &str = "rules_sub_picker_scrollable";
pub(crate) const RULES_SUB_PICKER_CURSOR_ID: &str = "rules_sub_picker_cursor_row";
/// How many picker entries render at once. The keyboard cursor is clamped
/// to this so Enter never targets an unrendered row; entries past it are
/// reached by narrowing the search.
pub(crate) const RULES_PICKER_RENDER_CAP: usize = 200;

fn discard_confirm_overlay<'a>() -> Element<'a, Message> {
    let panel = container(
        column![
            text("Discard rule changes?")
                .size(14)
                .font(theme::weighted_ui_font(iced::font::Weight::Bold))
                .color(theme::fg0()),
            text("Unsaved edits to this smart playlist will be lost.")
                .size(12)
                .font(theme::ui_font())
                .color(theme::fg3()),
            row![
                chip(
                    "Keep editing",
                    false,
                    Message::RulesEditor(RulesEditorMessage::CancelDiscard),
                ),
                chip(
                    "Discard",
                    true,
                    Message::RulesEditor(RulesEditorMessage::ConfirmDiscard),
                ),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .padding(16)
    .style(theme::modal_frame_style);
    theme::modal_scaffold(
        panel.into(),
        Message::RulesEditor(RulesEditorMessage::CancelDiscard),
        theme::MODAL_BACKDROP_ALPHA,
    )
}

fn conflict_banner<'a>() -> Element<'a, Message> {
    container(
        row![
            text("This playlist changed on the server while you were editing.")
                .size(12)
                .font(theme::ui_font())
                .color(theme::fg0()),
            Space::new().width(Length::Fill),
            chip(
                "Reload server rules",
                false,
                Message::RulesEditor(RulesEditorMessage::ReloadServerRules),
            ),
            chip(
                "Overwrite anyway",
                true,
                Message::RulesEditor(RulesEditorMessage::ConfirmOverwrite),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .style(|_t| iced::widget::container::Style {
        background: Some(theme::bg1().into()),
        ..Default::default()
    })
    .into()
}

fn target_gone_banner<'a>() -> Element<'a, Message> {
    container(
        row![
            text("The playlist was deleted on the server.")
                .size(12)
                .font(theme::ui_font())
                .color(theme::fg0()),
            Space::new().width(Length::Fill),
            chip(
                "Save as new…",
                true,
                Message::RulesEditor(RulesEditorMessage::SaveAsNew),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .style(|_t| iced::widget::container::Style {
        background: Some(theme::bg1().into()),
        ..Default::default()
    })
    .into()
}

/// The pane duo re-rendered under a banner (kept as a helper so both banner
/// branches share the shape).
fn panes_placeholder<'a>(
    session: &'a RulesSessionUi,
    album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    window_height: f32,
    cols: PreviewColumnVisibility,
    column_dropdown_open: bool,
    column_dropdown_trigger_bounds: Option<iced::Rectangle>,
) -> Element<'a, Message> {
    // Same tonal step + full-height divider as the live `panes` row, so the
    // save-conflict / target-gone banner state doesn't lose the separation.
    row![
        container(results_pane(
            session,
            album_art,
            window_height,
            cols,
            column_dropdown_open,
            column_dropdown_trigger_bounds,
        ))
        .width(Length::FillPortion(11))
        .height(Length::Fill)
        .style(|_t| iced::widget::container::Style {
            background: Some(theme::bg0_hard().into()),
            ..Default::default()
        }),
        pane_divider(),
        container(form_pane(session, window_height))
            .width(Length::FillPortion(9))
            .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}

/// Which visual band a form row belongs to — matching (0) / sort (1) /
/// limit (2) / JSON (3). Drives the section separators in `form_pane`.
fn form_section(row: &FormRow) -> u8 {
    match row {
        FormRow::EditBar
        | FormRow::Match
        | FormRow::GroupHeader(_)
        | FormRow::Rule(_)
        | FormRow::AddRule(_) => 0,
        FormRow::SortKey(_) | FormRow::AddSortKey => 1,
        FormRow::Limit => 2,
        FormRow::JsonToggle => 3,
    }
}

/// The full-height 1px pane divider (the settings-view precedent). Kept as
/// one helper so the live panes and the banner-state placeholder can't drift.
fn pane_divider<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fill)
        .style(|_t| iced::widget::container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        })
        .into()
}

// =========================================================================
// Shared atoms
// =========================================================================

/// A Trawl-tray-style labeled chip on the canonical clickable-cell chassis.
fn chip<'a>(label: &str, accent: bool, on_press: Message) -> Element<'a, Message> {
    let color = if accent {
        theme::bg0_hard()
    } else {
        theme::fg1()
    };
    let bg = if accent {
        theme::accent_bright()
    } else {
        theme::bg1()
    };
    mouse_area(
        HoverOverlay::new(
            container(
                text(label.to_owned())
                    .size(11.5)
                    .font(theme::ui_font())
                    .color(color)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .padding([4, 10])
            .style(move |_t| iced::widget::container::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    color: theme::bg3(),
                    width: 1.0,
                    radius: theme::ui_radius_pill(),
                },
                ..Default::default()
            }),
        )
        .border_radius(theme::ui_radius_pill()),
    )
    .on_press(on_press)
    .into()
}

/// A dimmed full-width action row (add-rows, the JSON toggle).
fn dim_action<'a>(label: &str, on_press: Message) -> Element<'a, Message> {
    mouse_area(
        HoverOverlay::new(
            container(
                text(label.to_owned())
                    .size(11.5)
                    .font(theme::ui_font())
                    .color(theme::fg3()),
            )
            .padding([4, 8])
            .width(Length::Fill),
        )
        .border_radius(theme::ui_radius_sm()),
    )
    .on_press(on_press)
    .into()
}

/// An activatable empty-state row (preset list) — the Harbour-anchor-row
/// interaction precedent on the HoverOverlay chassis.
/// Non-preset lead actions (Start empty, Import) rendered before the preset
/// list. The "Or start from a preset" hint is inserted at this index, and it
/// marks where `PresetChosen` indices begin in `empty_state_entries()`.
pub(crate) const EMPTY_STATE_LEAD_ACTIONS: usize = 2;

/// The blank-create empty-state actions in render order — the single source
/// of truth shared by the view (rendering + cursor highlight) and the
/// keyboard handler (Enter activation). Index N here is row N on screen.
pub(crate) fn empty_state_entries() -> Vec<(&'static str, &'static str, Message)> {
    let mut v = vec![
        (
            "Start empty",
            "Build the rules from scratch",
            Message::RulesEditor(RulesEditorMessage::StartEmpty),
        ),
        (
            "Import from .nsp file…",
            "Load rules from a smart-playlist file on disk",
            Message::RulesEditor(RulesEditorMessage::ImportNsp),
        ),
    ];
    for (i, preset) in SEED_PRESETS.iter().enumerate() {
        v.push((
            preset.name,
            preset.description,
            Message::RulesEditor(RulesEditorMessage::PresetChosen(i)),
        ));
    }
    v
}

fn action_row<'a>(
    title: &str,
    subtitle: &str,
    on_press: Message,
    selected: bool,
) -> Element<'a, Message> {
    mouse_area(
        HoverOverlay::new(
            container(
                column![
                    text(title.to_owned())
                        .size(13)
                        .font(theme::weighted_ui_font(iced::font::Weight::Semibold))
                        .color(theme::fg0()),
                    text(subtitle.to_owned())
                        .size(10.5)
                        .font(theme::ui_font())
                        .color(theme::fg3()),
                ]
                .spacing(1),
            )
            .padding([8, 10])
            .width(Length::Fill)
            // The keyboard cursor is a subtle fill (same as the sub-picker
            // list) — a highlight, not a stray outline.
            .style(move |_t| iced::widget::container::Style {
                background: selected.then(|| theme::bg2().into()),
                border: iced::Border {
                    radius: theme::ui_radius_sm(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .border_radius(theme::ui_radius_sm()),
    )
    .on_press(on_press)
    .into()
}

/// A value cell: the live `text_input` when this exact cell is in Editing
/// mode, else a static read of the committed value (click enters Editing —
/// the ClickCell one-write-path).
fn value_cell<'a>(
    session: &'a RulesSessionUi,
    cell: FormCell,
    row_idx: usize,
    committed: &str,
    placeholder: &str,
    cursor_here: bool,
) -> Element<'a, Message> {
    // Match the EXACT editing cell (row AND cell): two Value cells on
    // different rules share `FormCell::Value`, so a cell check alone would
    // render another row's editing buffer here (the cross-row caret leak).
    let editing_here = cursor_here
        && session.mode == FormMode::Editing
        && session
            .editing
            .as_ref()
            .is_some_and(|e| e.cell == cell && session.rows.get(row_idx) == Some(&e.row));
    if editing_here {
        let buffer = session
            .editing
            .as_ref()
            .map(|e| e.buffer.clone())
            .unwrap_or_default();
        text_input(placeholder, &buffer)
            .on_input(|s| Message::RulesEditor(RulesEditorMessage::EditingInput(s)))
            .on_submit(Message::RulesEditor(RulesEditorMessage::CommitEditing))
            .id(RULES_VALUE_INPUT_ID)
            .size(12)
            .font(theme::ui_font())
            .padding([3, 6])
            .width(Length::Fixed(110.0))
            .style(value_input_style)
            .into()
    } else {
        let shown = if committed.is_empty() {
            placeholder.to_owned()
        } else {
            committed.to_owned()
        };
        let dim = committed.is_empty();
        let label = text(shown)
            .size(12)
            .font(theme::ui_font())
            .color(if dim { theme::fg4() } else { theme::fg0() })
            .wrapping(iced::widget::text::Wrapping::None);
        // When the cursor ring is on this text cell, show a pencil so the
        // ring reads as "editable — press Enter to type" rather than being
        // mistaken for a (two-mode-absent) caret.
        let inner: Element<'a, Message> = if cursor_here {
            row![
                label,
                Space::new().width(Length::Fill),
                super::cover::edit_affordance()
            ]
            .align_y(Alignment::Center)
            .into()
        } else {
            label.into()
        };
        mouse_area(
            HoverOverlay::new(
                container(inner)
                    .padding([3, 6])
                    .width(Length::Fixed(110.0))
                    .style(|_t| iced::widget::container::Style {
                        background: Some(theme::bg0_hard().into()),
                        border: iced::Border {
                            color: theme::bg3(),
                            width: 1.0,
                            radius: theme::ui_radius_sm(),
                        },
                        ..Default::default()
                    }),
            )
            .border_radius(theme::ui_radius_sm()),
        )
        .on_press(Message::RulesEditor(RulesEditorMessage::ClickCell {
            row: row_idx,
            cell,
        }))
        .into()
    }
}

/// `text_input` id for the focused value cell.
pub(crate) const RULES_VALUE_INPUT_ID: &str = "rules_value_input";

/// The cursor ring wrapper — marks the form cursor's focused cell.
fn ring_wrap<'a>(on: bool, inner: Element<'a, Message>) -> Element<'a, Message> {
    container(inner)
        .style(move |_t| iced::widget::container::Style {
            border: iced::Border {
                color: if on {
                    theme::selection_ring_on(theme::bg0())
                } else {
                    iced::Color::TRANSPARENT
                },
                width: 2.0,
                radius: theme::ui_radius_sm(),
            },
            ..Default::default()
        })
        .padding(1)
        .into()
}

/// Wrap an edit-bar input, appending the pencil affordance when its cell is
/// the ringed cursor target (and not being edited).
fn edit_bar_field<'a>(input: Element<'a, Message>, show_pencil: bool) -> Element<'a, Message> {
    if show_pencil {
        row![
            input,
            Space::new().width(Length::Fixed(4.0)),
            super::cover::edit_affordance()
        ]
        .align_y(Alignment::Center)
        .into()
    } else {
        input
    }
}

fn form_hint<'a>(copy: &str) -> Element<'a, Message> {
    container(
        text(copy.to_owned())
            .size(10.5)
            .font(theme::ui_font())
            .color(theme::fg4()),
    )
    .padding([2, 4])
    .into()
}

fn empty_copy<'a>(copy: &'static str) -> Element<'a, Message> {
    container(
        text(copy)
            .size(12.5)
            .font(theme::ui_font())
            .color(theme::fg3()),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center(Length::Fill)
    .into()
}

fn diag_line<'a>(
    session: &'a RulesSessionUi,
    location: &DiagnosticLocation,
) -> Element<'a, Message> {
    let mut col = column![].spacing(1);
    let mut any = false;
    for diag in session.diagnostics_at(location) {
        any = true;
        // Match the app's semantic status palette (login / about use
        // danger_bright for errors): a blocking error reads as danger, an
        // advisory warning as amber — not amber-for-error + dim-gray-for-warn.
        let color = match diag.severity {
            Severity::Error => theme::danger_bright(),
            Severity::Warning => theme::warning(),
        };
        col = col.push(
            container(
                text(diag.message.clone())
                    .size(10.5)
                    .font(theme::ui_font())
                    .color(color),
            )
            .padding(iced::Padding {
                left: 12.0,
                ..Default::default()
            }),
        );
    }
    if any { col.into() } else { Space::new().into() }
}

fn bar_input_style(
    _theme: &iced::Theme,
    _status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: iced::Background::Color(iced::Color::TRANSPARENT),
        border: iced::Border {
            color: theme::bg3(),
            width: 0.0,
            radius: theme::ui_border_radius(),
        },
        icon: theme::fg0(),
        placeholder: theme::fg2(),
        value: theme::fg0(),
        selection: theme::selection_color(),
    }
}

/// The value-cell `text_input` style, matching the non-editing cell
/// (bg0_hard fill, bg3 frame) so entering Editing doesn't flash iced's
/// default blue border; the frame brightens to accent while focused.
fn value_input_style(
    _theme: &iced::Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let focused = matches!(status, iced::widget::text_input::Status::Focused { .. });
    iced::widget::text_input::Style {
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: if focused {
                theme::accent_bright()
            } else {
                theme::bg3()
            },
            width: 1.0,
            radius: theme::ui_radius_sm(),
        },
        icon: theme::fg4(),
        placeholder: theme::fg4(),
        value: theme::fg0(),
        selection: theme::selection_color(),
    }
}

/// The JSON mode `text_editor` style. The wrapping container owns the
/// bg0_hard fill + bg3 frame, so the editor stays transparent (no default
/// iced background, no default blue focus border) and only themes its text
/// and selection — matching the form's other bg0_hard-inset fields.
fn json_editor_style(
    _theme: &iced::Theme,
    _status: iced::widget::text_editor::Status,
) -> iced::widget::text_editor::Style {
    iced::widget::text_editor::Style {
        background: iced::Background::Color(iced::Color::TRANSPARENT),
        border: iced::Border {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
            radius: theme::ui_radius_sm(),
        },
        placeholder: theme::fg4(),
        value: theme::fg0(),
        selection: theme::selection_color(),
    }
}
