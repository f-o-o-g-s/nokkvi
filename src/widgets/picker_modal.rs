//! Shared shell for the searchable picker modals (default playlist, MilkDrop
//! preset): [`PickerList`], the list state that filters and keeps its
//! centered row, and [`picker_modal`], the modal chrome around it.
//!
//! Both pickers are a searchable slot list with immediate (non-debounced)
//! filtering, Up/Down/Enter navigation, centered over the shared modal
//! backdrop, and dismissed by Escape / X / a click outside. What differs
//! (the entries, what a row shows, extra title-bar controls, what choosing
//! does) stays in each picker's own module.

use iced::{
    Alignment, Element, Length, Padding, Widget as _,
    font::Weight,
    widget::{Space, button, column, container, mouse_area, row, svg, text},
};

use crate::{
    embedded_svg, theme,
    widgets::{
        SlotListView,
        slot_list::{self, SlotListRowContext},
    },
};

const TITLE_BAR_HEIGHT: f32 = 38.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;

/// A picker's entries, search query, the entries that pass it, and the slot
/// list over them.
#[derive(Debug, Clone)]
pub(crate) struct PickerList<T> {
    /// Every entry, in display order.
    pub(crate) all: Vec<T>,
    pub(crate) search_query: String,
    /// The entries passing the last [`Self::refilter`], in `all` order.
    pub(crate) filtered: Vec<T>,
    pub(crate) slot_list: SlotListView,
}

impl<T: Clone + PartialEq> PickerList<T> {
    /// Every entry visible, the first one centered.
    pub(crate) fn new(all: Vec<T>) -> Self {
        Self {
            filtered: all.clone(),
            all,
            search_query: String::new(),
            slot_list: SlotListView::new(),
        }
    }

    /// The entry on the centered row.
    pub(crate) fn centered(&self) -> Option<&T> {
        self.slot_list
            .get_center_item_index(self.filtered.len())
            .and_then(|i| self.filtered.get(i))
    }

    /// Scroll so `entry` sits on the centered row (no-op when filtered out).
    pub(crate) fn center_on(&mut self, entry: &T) {
        if let Some(index) = self.filtered.iter().position(|e| e == entry) {
            self.slot_list.set_offset(index, self.filtered.len());
        }
    }

    /// Recompute `filtered` from the entries `keep` accepts. `keep` gets the
    /// search query lowercased, for case-insensitive substring matching. The
    /// centered entry stays centered when it still passes; otherwise the
    /// list starts at its first row.
    pub(crate) fn refilter(&mut self, keep: impl Fn(&T, &str) -> bool) {
        let centered = self.centered().cloned();
        let query = self.search_query.to_lowercase();
        self.filtered = self
            .all
            .iter()
            .filter(|entry| keep(entry, &query))
            .cloned()
            .collect();
        self.slot_list = SlotListView::new();
        if let Some(entry) = centered {
            self.center_on(&entry);
        }
    }

    pub(crate) fn move_up(&mut self) {
        self.slot_list.move_up(self.filtered.len());
    }

    pub(crate) fn move_down(&mut self) {
        self.slot_list.move_down(self.filtered.len());
    }

    pub(crate) fn set_offset(&mut self, offset: usize) {
        self.slot_list.set_offset(offset, self.filtered.len());
    }
}

/// The parts of a picker modal around its list: title bar, search field,
/// and the messages its chrome emits.
pub(crate) struct PickerChrome<'a, M> {
    pub(crate) title: &'static str,
    /// Shown right after the title (e.g. a `shown / total` count).
    pub(crate) after_title: Option<Element<'a, M>>,
    /// Shown just left of the X (e.g. a filter chip).
    pub(crate) before_close: Option<Element<'a, M>>,
    pub(crate) search_placeholder: &'static str,
    /// The search field's `text_input` id, focused on open.
    pub(crate) search_input_id: &'static str,
    pub(crate) on_search: fn(String) -> M,
    /// Escape, the X, or a click on the backdrop.
    pub(crate) on_close: M,
    pub(crate) on_up: M,
    pub(crate) on_down: M,
    /// The scrollbar's seek: jump to this row of the filtered list.
    pub(crate) on_set_offset: fn(usize) -> M,
    /// Shown instead of the list when nothing passes the filter.
    pub(crate) empty_text: &'static str,
    pub(crate) window_height: f32,
}

/// Render a picker modal over `list`, meant to be stacked on top of the
/// app. `render_row` draws one entry; a press on a row is the row's own
/// business.
pub(crate) fn picker_modal<'a, T, M: Clone + 'a>(
    chrome: PickerChrome<'a, M>,
    list: &'a PickerList<T>,
    render_row: impl Fn(&T, SlotListRowContext) -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    let PickerChrome {
        title,
        after_title,
        before_close,
        search_placeholder,
        search_input_id,
        on_search,
        on_close,
        on_up,
        on_down,
        on_set_offset,
        empty_text,
        window_height,
    } = chrome;
    let modal_height = (window_height * 0.70).max(320.0);
    // Chrome = fixed bands + the panel's own padding top and bottom, so the
    // slot budget matches the Fill area main_area really gets.
    let modal_chrome = TITLE_BAR_HEIGHT + SEARCH_BAR_HEIGHT + 2.0 * theme::MODAL_PANEL_PADDING;
    let label_size = 13.0;

    // ── Title bar: title, optional extras, X ──
    let dim_color = theme::fg4();
    let close_btn = button(
        embedded_svg::svg_widget("assets/icons/x.svg")
            .width(Length::Fixed(label_size))
            .height(Length::Fixed(label_size))
            .style(move |_theme, _status| svg::Style {
                color: Some(dim_color),
            }),
    )
    .on_press(on_close.clone())
    .style(theme::transparent_button_style)
    .padding(Padding::new(2.0));

    let mut title_row = row![
        Space::new().width(Length::Fixed(12.0)),
        text(title)
            .size(label_size)
            .font(theme::weighted_ui_font(Weight::Bold))
            .color(theme::fg0()),
    ];
    if let Some(extra) = after_title {
        title_row = title_row
            .push(Space::new().width(Length::Fixed(10.0)).boxed())
            .push(extra);
    }
    title_row = title_row.push(Space::new().width(Length::Fill).boxed());
    if let Some(extra) = before_close {
        title_row = title_row
            .push(extra)
            .push(Space::new().width(Length::Fixed(8.0)).boxed());
    }
    let title_row = title_row
        .push(close_btn.boxed())
        .push(Space::new().width(Length::Fixed(12.0)).boxed())
        .align_y(Alignment::Center)
        .height(Length::Fixed(TITLE_BAR_HEIGHT));
    let title_bar = container(title_row).width(Length::Fill);

    // ── Search bar ──
    let search_input = crate::widgets::search_bar::search_bar(
        &list.search_query,
        search_placeholder,
        search_input_id,
        on_search,
        Some(theme::settings_search_input_style),
    );
    let search_bar = container(search_input)
        .width(Length::Fill)
        .height(Length::Fixed(SEARCH_BAR_HEIGHT))
        .padding(Padding::new(4.0).left(12.0).right(12.0));

    // ── Slot list or empty state ──
    let main_area: Element<'a, M> = if list.filtered.is_empty() {
        container(
            text(empty_text)
                .size(14)
                .font(theme::ui_font())
                .color(theme::fg4()),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .boxed()
    } else {
        let config = slot_list::SlotListConfig::with_dynamic_slots(modal_height, modal_chrome);
        let total = list.filtered.len();
        slot_list::slot_list_view_with_scroll(
            &list.slot_list,
            &list.filtered,
            &config,
            on_up.clone(),
            on_down.clone(),
            move |f| on_set_offset((f * total as f32) as usize),
            None,
            render_row,
        )
    };

    // Shared modal frame: bg0_hard fill + 1 px accent_bright outline +
    // ui_radius_lg corners (`theme::modal_frame_style`).
    let modal_panel = container(
        column![title_bar, search_bar, main_area]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::FillPortion(5))
    .height(Length::Fixed(modal_height))
    .clip(true)
    .padding(Padding::new(theme::MODAL_PANEL_PADDING))
    .style(theme::modal_frame_style);

    // The panel takes the middle 5/7 of the backdrop's width.
    // `theme::modal_scaffold` adds the `opaque(...)` press-capture shield (a
    // click on padding inside the panel doesn't bubble to the backdrop and
    // dismiss the modal) and the `MODAL_BACKDROP_ALPHA` dim.
    let modal_row = row![
        Space::new().width(Length::FillPortion(1)),
        modal_panel,
        Space::new().width(Length::FillPortion(1)),
    ]
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let scaffold = theme::modal_scaffold(modal_row.boxed(), on_close, theme::MODAL_BACKDROP_ALPHA);

    // `opaque` (inside the scaffold) captures presses, not scrolls, so the
    // wheel steps the list anywhere over the modal.
    mouse_area(scaffold)
        .on_scroll(move |delta| {
            let y = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            if y > 0.0 {
                on_up.clone()
            } else {
                on_down.clone()
            }
        })
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> PickerList<&'static str> {
        PickerList::new(vec!["Alpha", "Beta", "Gamma", "Delta"])
    }

    fn contains(entry: &&str, query: &str) -> bool {
        entry.to_lowercase().contains(query)
    }

    #[test]
    fn opens_on_the_first_entry() {
        assert_eq!(list().centered(), Some(&"Alpha"));
    }

    #[test]
    fn refilter_matches_case_insensitively() {
        let mut list = list();
        list.search_query = "ETA".into();
        list.refilter(contains);
        assert_eq!(list.filtered, ["Beta"]);
    }

    /// The two pickers used to disagree here: one kept its centered row
    /// through a search, the other jumped back to the top.
    #[test]
    fn refilter_keeps_a_matching_center_and_otherwise_starts_at_the_top() {
        let mut list = list();
        list.center_on(&"Gamma");
        list.search_query = "a".into();
        list.refilter(contains);
        assert_eq!(list.centered(), Some(&"Gamma"));

        list.search_query = "del".into();
        list.refilter(contains);
        assert_eq!(list.centered(), Some(&"Delta"), "Gamma filtered out");

        list.search_query = "zzz".into();
        list.refilter(contains);
        assert_eq!(list.centered(), None);
    }

    #[test]
    fn navigation_stays_within_the_filtered_entries() {
        let mut list = list();
        list.search_query = "ta".into(); // Beta, Delta
        list.refilter(contains);
        list.move_down();
        assert_eq!(list.centered(), Some(&"Delta"));
        list.move_down();
        assert_eq!(list.centered(), Some(&"Delta"), "stops at the last match");
        list.set_offset(0);
        assert_eq!(list.centered(), Some(&"Beta"));
    }
}
