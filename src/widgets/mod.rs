//! Reusable UI widgets — player bar, visualizer, slot list, nav bar, 3D-styled controls
//!
//! Includes the slot list slot system (9-slot visible window with dynamic scaling),
//! GPU-accelerated visualizer (wgpu + RustFFT), responsive player/nav bars with
//! breakpoint culling, and 3D Gruvbox-styled buttons/sliders.

// Components
pub(crate) mod artwork_split_handle;
pub(crate) mod badge_pip;
pub(crate) mod base_slot_list_layout;
pub(crate) mod boat;
pub(crate) mod drag_column;
pub(crate) mod eq_slider;
pub(crate) mod format_info;
pub(crate) mod hover_overlay;
pub(crate) mod library_filter_trigger;
pub(crate) mod link_text;
pub(crate) mod marquee_text;
pub(crate) mod menu_chrome;
pub(crate) mod menu_constants;
pub(crate) mod metadata_pill;
pub(crate) mod modal_button;
pub(crate) mod nav_bar;
pub(crate) mod pill_segmented_button;
pub(crate) mod player_bar;
pub(crate) mod player_modes_menu;
pub(crate) mod scroll_indicator;
pub(crate) mod search_bar;
pub(crate) mod side_nav_bar;
pub(crate) mod sizes;
pub(crate) mod slot_list;
pub(crate) mod slot_list_page;
pub(crate) mod track_info_strip;
pub(crate) mod visualizer;

// UI widgets (from old ui/)
pub(crate) mod about_modal;
pub(crate) mod checkbox_dropdown;
pub(crate) mod context_menu;
pub(crate) mod default_playlist_chip;
pub(crate) mod default_playlist_picker;
pub(crate) mod eq_modal;
pub(crate) mod hamburger_menu;
pub(crate) mod info_modal;
pub(crate) mod progress_bar;
pub(crate) mod slot_list_view;
pub(crate) mod text_input_dialog;
pub(crate) mod view_header;
pub(crate) mod volume_slider;

// Re-export commonly used items
pub(crate) use eq_modal::{EqModalMessage, PresetChoice, eq_modal_overlay};
pub(crate) use nav_bar::{NavBarMessage, NavBarViewData, NavView, nav_bar};
pub(crate) use player_bar::{PlayerBarMessage, PlayerBarViewData, player_bar};
pub(crate) use side_nav_bar::{SideNavBarData, side_nav_bar};
pub(crate) use slot_list_page::{SlotListPageAction, SlotListPageMessage, SlotListPageState};
pub(crate) use slot_list_view::{HoveredSlot, SlotListView};
pub(crate) use volume_slider::{SliderVariant, volume_slider};

/// Format an integer with comma thousands separators ("13627" → "13,627").
///
/// Used by the library selector popover for the dim right-column song
/// count. Manual implementation (vs `num-format` crate) keeps zero new
/// dependencies and locks the format to en-US grouping; locales aren't a
/// concern for nokkvi today.
pub(crate) fn format_count_with_commas(n: u32) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Empty state that routes through base_slot_list_layout to preserve widget tree
/// structure (and thus text_input focus) when transitioning between results/no-results.
/// Use this instead of empty_state_message for views that use base_slot_list_layout.
pub(crate) fn base_slot_list_empty_state<'a, M: 'a>(
    header: impl Into<iced::Element<'a, M>>,
    message: &'a str,
    layout_config: &crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig,
) -> iced::Element<'a, M> {
    use iced::{
        Alignment, Length,
        widget::{container, text},
    };

    use crate::{
        theme,
        widgets::base_slot_list_layout::{base_slot_list_empty_artwork, base_slot_list_layout},
    };

    // Build an empty-state content element that occupies the slot list slot
    let empty_content = container(
        text(message)
            .size(16)
            .font(theme::ui_font())
            .color(theme::fg2()),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(theme::container_bg0_hard)
    .into();

    // Use a placeholder artwork element to maintain the same root widget type
    // (Row when artwork is visible, Column when not) as the normal results path.
    // Passing None here would switch from Row→Column, destroying text_input focus.
    let artwork = base_slot_list_empty_artwork(layout_config);
    base_slot_list_layout(layout_config, header.into(), empty_content, artwork)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_count_with_commas_small_numbers() {
        assert_eq!(format_count_with_commas(0), "0");
        assert_eq!(format_count_with_commas(47), "47");
        assert_eq!(format_count_with_commas(999), "999");
    }

    #[test]
    fn format_count_with_commas_inserts_thousands_separator() {
        assert_eq!(format_count_with_commas(1_000), "1,000");
        assert_eq!(format_count_with_commas(13_627), "13,627");
        assert_eq!(format_count_with_commas(8_221), "8,221");
    }

    #[test]
    fn format_count_with_commas_handles_millions() {
        assert_eq!(format_count_with_commas(1_234_567), "1,234,567");
        assert_eq!(format_count_with_commas(u32::MAX), "4,294,967,295");
    }
}
