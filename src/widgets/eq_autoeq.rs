//! The equalizer modal's AutoEq panel: search the AutoEq project's list of
//! measured headphones and load one's correction profile, in place of the
//! Parametric editor below the modal header.
//!
//! The list comes from GitHub only when the user opens the panel (or asks
//! for a refresh), a profile only when they pick a row; `services::autoeq`
//! in the data crate does the fetching and the caching. The handlers live in
//! `update/eq_autoeq.rs`. The panel's state is [`AutoEqPanel`] on
//! `EqModalState::autoeq` (`None` while the editor shows); the stale-drop
//! counter its requests carry sits beside it on `EqModalState`, so it
//! outlives the panel.

use std::sync::Arc;

use iced::{
    Alignment, Element, Length, Padding, Widget as _,
    widget::{Column, column, container, mouse_area, row, scrollable, space, text, text_input},
};
use nokkvi_data::{
    audio::eq::EqBand,
    services::autoeq::{AutoEqEntry, AutoEqMatches, search},
    utils::noun::Noun,
};

use crate::{
    theme,
    widgets::{
        eq_modal::EqModalMessage, eq_parametric::action_button, format_count_with_commas,
        hover_overlay::HoverOverlay,
    },
};

// =============================================================================
// State & Messages
// =============================================================================

/// Most rows the results show; the count line says how many matched in all.
pub(crate) const SEARCH_LIMIT: usize = 100;

/// The search field's id, so opening the panel can focus it.
pub(crate) const SEARCH_INPUT_ID: &str = "eq-autoeq-search";
/// The results' scrollable.
pub(crate) const RESULTS_SCROLL_ID: &str = "eq-autoeq-results";

/// The stable widget id of the result row at `position`.
pub(crate) fn row_id(position: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("eq-autoeq-row-{position}"))
}

const PROFILE: Noun = Noun::new("profile", "profiles");

/// Where the list stands.
#[derive(Debug, Clone)]
pub(crate) enum AutoEqList {
    /// Asked for; `refresh` when it was a Refresh list (so Retry asks the
    /// same way).
    Loading {
        refresh: bool,
    },
    /// The request failed: why, and how it was asked.
    Failed {
        why: String,
        refresh: bool,
    },
    Ready(Arc<Vec<AutoEqEntry>>),
}

impl Default for AutoEqList {
    fn default() -> Self {
        Self::Loading { refresh: false }
    }
}

/// The open panel: what is typed, the list, what matches, the highlighted
/// row and the profile being downloaded.
#[derive(Debug, Clone, Default)]
pub(crate) struct AutoEqPanel {
    pub query: String,
    pub list: AutoEqList,
    /// What `query` matches in the list (empty until the list is ready).
    pub matches: AutoEqMatches,
    /// The highlighted row: a position in `matches.shown`.
    pub highlight: Option<usize>,
    /// The entry (an index into the list) whose profile is downloading.
    pub downloading: Option<usize>,
}

impl AutoEqPanel {
    /// Show `list`, searched with what is typed.
    pub(crate) fn show(&mut self, list: Arc<Vec<AutoEqEntry>>) {
        self.list = AutoEqList::Ready(list);
        self.refilter();
    }

    /// Search the list again for `query`; the best match is highlighted.
    pub(crate) fn refilter(&mut self) {
        self.matches = match &self.list {
            AutoEqList::Ready(list) => search(list, &self.query, SEARCH_LIMIT),
            AutoEqList::Loading { .. } | AutoEqList::Failed { .. } => AutoEqMatches::default(),
        };
        self.highlight = (!self.matches.shown.is_empty()).then_some(0);
    }

    /// The ready list's entry at `index`.
    pub(crate) fn entry(&self, index: usize) -> Option<&AutoEqEntry> {
        match &self.list {
            AutoEqList::Ready(list) => list.get(index),
            AutoEqList::Loading { .. } | AutoEqList::Failed { .. } => None,
        }
    }

    /// The list index of the highlighted row's entry.
    pub(crate) fn highlighted_entry(&self) -> Option<usize> {
        self.highlight
            .and_then(|position| self.matches.shown.get(position).copied())
    }

    /// Move the highlight one row up or down, held at the ends. `None` when
    /// nothing shows.
    pub(crate) fn step(&mut self, step: AutoEqStep) -> Option<usize> {
        let last = self.matches.shown.len().checked_sub(1)?;
        let next = match (self.highlight, step) {
            (None, _) => 0,
            (Some(at), AutoEqStep::Up) => at.saturating_sub(1),
            (Some(at), AutoEqStep::Down) => (at + 1).min(last),
        };
        self.highlight = Some(next.min(last));
        self.highlight
    }
}

/// Up / Down in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoEqStep {
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub enum AutoEqMessage {
    /// AutoEq… in the Parametric view: open the panel (asks for the list
    /// unless this session already has it).
    Open,
    /// The back control, or Escape: back to the editor.
    Back,
    SearchChanged(String),
    /// Enter: load the highlighted row's profile.
    Submit,
    /// Up / Down: move the highlight.
    Step(AutoEqStep),
    /// A row was pressed: load that entry's profile (an index into the list).
    Choose(usize),
    /// Ask for the list again the way the failed request asked.
    Retry,
    /// Refresh list: download the list again.
    Refresh,
    /// The list, or why not, for the request stamped `generation`.
    IndexLoaded {
        generation: u64,
        result: Result<Arc<Vec<AutoEqEntry>>, String>,
    },
    /// `entry`'s bands, or why not, for the request stamped `generation`.
    ProfileLoaded {
        generation: u64,
        entry: AutoEqEntry,
        result: Result<Vec<EqBand>, String>,
    },
}

// =============================================================================
// View
// =============================================================================

/// The panel's height: the Parametric body's (curve 200, status line about
/// 25, table 226, two 10 px gaps), so the modal keeps its size when the
/// panel opens.
const PANEL_HEIGHT: f32 = 471.0;
/// One result row, like a band table row.
const ROW_HEIGHT: f32 = 26.0;
const ROW_TEXT: f32 = 13.0;
const LABEL_TEXT: f32 = 12.0;
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_GAP: f32 = 6.0;

/// The panel: the back control and Refresh list, what this is and where it
/// comes from, the search field, the count, then the results (or the
/// list's state). The same widget shape in every state, so the search field
/// keeps its focus.
pub(crate) fn autoeq_panel(panel: &AutoEqPanel) -> Element<'_, EqModalMessage> {
    let controls = row![
        action_button("Back", EqModalMessage::AutoEq(AutoEqMessage::Back)),
        space::horizontal(),
        crate::widgets::eq_parametric::with_tooltip(
            action_button(
                "Refresh list",
                EqModalMessage::AutoEq(AutoEqMessage::Refresh)
            ),
            "Download the list from GitHub again",
        ),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let about = label(
        "Headphone profiles from the AutoEq project, downloaded from GitHub when you open this list.",
        theme::fg3(),
    );

    let search = text_input("Search headphones…", &panel.query)
        .id(SEARCH_INPUT_ID)
        .on_input(|typed| EqModalMessage::AutoEq(AutoEqMessage::SearchChanged(typed)))
        .on_submit(EqModalMessage::AutoEq(AutoEqMessage::Submit))
        .size(14.0)
        .padding([6, 10])
        .width(Length::Fill)
        .font(theme::ui_font())
        .style(search_style);

    let count = label(count_text(panel), theme::fg3());

    let results = container(results_body(panel))
        .width(Length::Fill)
        .height(Length::Fill);

    column![controls, about, search, count, results]
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fixed(PANEL_HEIGHT))
        .boxed()
}

/// `12 of 8,850 profiles`, `8,850 profiles` with nothing typed, and a note
/// when only the first rows show. Empty until the list is ready.
fn count_text(panel: &AutoEqPanel) -> String {
    let AutoEqList::Ready(list) = &panel.list else {
        return String::new();
    };
    let total = list.len();
    let commas = |n: usize| format_count_with_commas(u32::try_from(n).unwrap_or(u32::MAX));
    let matched = panel.matches.matched;
    let mut line = if panel.query.trim().is_empty() {
        format!("{} {}", commas(total), PROFILE.word(total))
    } else {
        format!(
            "{} of {} {}",
            commas(matched),
            commas(total),
            PROFILE.word(total)
        )
    };
    let shown = panel.matches.shown.len();
    if shown < matched {
        line.push_str(&format!(", the first {} shown", commas(shown)));
    }
    line
}

/// The results, or the list's state: `Loading…`, the failure with Retry,
/// `No match`.
fn results_body(panel: &AutoEqPanel) -> Element<'_, EqModalMessage> {
    match &panel.list {
        AutoEqList::Loading { .. } => centered(label("Loading…", theme::fg3()).boxed()),
        AutoEqList::Failed { why, .. } => centered(
            column![
                text("Couldn't load the list")
                    .size(14.0)
                    .font(theme::ui_font())
                    .color(theme::fg2()),
                label(why.as_str(), theme::fg4()),
                action_button("Retry", EqModalMessage::AutoEq(AutoEqMessage::Retry)),
            ]
            .spacing(10)
            .align_x(Alignment::Center)
            .boxed(),
        ),
        AutoEqList::Ready(_) if panel.matches.shown.is_empty() => {
            centered(label("No match", theme::fg3()).boxed())
        }
        AutoEqList::Ready(list) => {
            let rows = Column::with_children(panel.matches.shown.iter().enumerate().filter_map(
                |(position, &index)| {
                    list.get(index).map(|entry| {
                        result_row(
                            position,
                            index,
                            entry,
                            panel.highlight == Some(position),
                            panel.downloading == Some(index),
                        )
                    })
                },
            ))
            .width(Length::Fill);
            scrollable(rows)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(SCROLLBAR_WIDTH)
                        .scroller_width(SCROLLBAR_WIDTH)
                        .spacing(SCROLLBAR_GAP),
                ))
                .id(RESULTS_SCROLL_ID)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::settings_scrollable_style)
                .boxed()
        }
    }
}

fn centered(content: Element<'_, EqModalMessage>) -> Element<'_, EqModalMessage> {
    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .boxed()
}

/// One result: the headphone's name, then who measured it and on what (or
/// `Loading…` while its profile downloads). A band-table row: `bg1` and an
/// accent name when highlighted, a hover wash, a press loads it.
fn result_row(
    position: usize,
    index: usize,
    entry: &AutoEqEntry,
    highlighted: bool,
    downloading: bool,
) -> Element<'_, EqModalMessage> {
    let name = text(entry.name.as_str())
        .size(ROW_TEXT)
        .font(theme::ui_font())
        .color(if highlighted {
            theme::accent_bright()
        } else {
            theme::fg1()
        })
        .wrapping(text::Wrapping::None);
    let by = if downloading {
        label("Loading…", theme::fg2())
    } else {
        label(entry.by_line(), theme::fg3())
    }
    .wrapping(text::Wrapping::None);
    let cells = row![container(name).width(Length::Fill).clip(true), by,]
        .spacing(12)
        .width(Length::Fill)
        .align_y(Alignment::Center);

    mouse_area(
        HoverOverlay::new(
            container(cells)
                .id(row_id(position))
                .width(Length::Fill)
                .height(Length::Fixed(ROW_HEIGHT))
                .padding(Padding::new(0.0).left(8.0).right(8.0))
                .align_y(Alignment::Center)
                .style(move |_theme| container::Style {
                    background: highlighted.then(|| theme::bg1().into()),
                    border: iced::Border {
                        radius: theme::ui_radius_sm(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        )
        .border_radius(theme::ui_radius_sm()),
    )
    .on_press(EqModalMessage::AutoEq(AutoEqMessage::Choose(index)))
    .interaction(iced::mouse::Interaction::Pointer)
    .boxed()
}

fn label<'a>(content: impl text::IntoFragment<'a>, color: iced::Color) -> text::Text<'a> {
    text(content)
        .size(LABEL_TEXT)
        .font(theme::ui_font())
        .color(color)
}

/// The search field: `bg0_hard`, a 1 px border that turns `accent_bright`
/// when hovered or focused.
fn search_style(_theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: match status {
                text_input::Status::Hovered | text_input::Status::Focused { .. } => {
                    theme::accent_bright()
                }
                text_input::Status::Active | text_input::Status::Disabled => theme::border(),
            },
            width: 1.0,
            radius: theme::ui_radius_sm(),
        },
        placeholder: theme::fg4(),
        value: theme::fg0(),
        selection: theme::selection_color(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, source: &str) -> AutoEqEntry {
        AutoEqEntry {
            name: name.to_owned(),
            source: source.to_owned(),
            rig: None,
            path: format!("{source}/over-ear/{}", name.replace(' ', "%20")),
        }
    }

    fn ready(names: &[&str]) -> AutoEqPanel {
        let mut panel = AutoEqPanel::default();
        panel.show(Arc::new(
            names.iter().map(|n| entry(n, "oratory1990")).collect(),
        ));
        panel
    }

    #[test]
    fn the_highlight_starts_on_the_best_match_and_stops_at_the_ends() {
        let mut panel = ready(&["AKG K371", "Sennheiser HD 600", "Sennheiser HD 650"]);
        assert_eq!(panel.highlight, Some(0));
        assert_eq!(panel.step(AutoEqStep::Up), Some(0), "held at the top");
        assert_eq!(panel.step(AutoEqStep::Down), Some(1));
        assert_eq!(panel.step(AutoEqStep::Down), Some(2));
        assert_eq!(panel.step(AutoEqStep::Down), Some(2), "held at the bottom");
        assert_eq!(panel.highlighted_entry(), Some(2));

        panel.query = "zzz".into();
        panel.refilter();
        assert_eq!(panel.highlight, None);
        assert_eq!(panel.step(AutoEqStep::Down), None, "nothing to move to");
    }

    #[test]
    fn the_count_reads_like_other_nokkvi_counts() {
        let mut panel = ready(&["AKG K371", "Sennheiser HD 600", "Sennheiser HD 650"]);
        assert_eq!(count_text(&panel), "3 profiles");
        panel.query = "sennheiser".into();
        panel.refilter();
        assert_eq!(count_text(&panel), "2 of 3 profiles");

        let many: Vec<String> = (0..1_234).map(|i| format!("Model {i}")).collect();
        let names: Vec<&str> = many.iter().map(String::as_str).collect();
        let panel = ready(&names);
        assert_eq!(count_text(&panel), "1,234 profiles, the first 100 shown");

        let mut one = ready(&["AKG K371"]);
        assert_eq!(count_text(&one), "1 profile");
        one.list = AutoEqList::Loading { refresh: false };
        assert_eq!(count_text(&one), "");
    }
}
