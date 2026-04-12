//! Settings entry building and filtering — pure functions for constructing and filtering
//! the `SettingsEntry` lists from config data.
//!
//! Supports two navigation levels:
//! - Level 1 (CategoryPicker): one Header per tab
//! - Level 2 (Category): all items within a tab, under auto-expanded section headers

use super::{SettingsPage, SettingsTab, SettingsViewData, items, items::SettingsEntry};

impl SettingsPage {
    // ========================================================================
    // Level 1: Category Picker
    // ========================================================================

    /// Build entries for the category picker (Level 1) — one Header per tab.
    pub(super) fn build_category_picker_entries() -> Vec<SettingsEntry> {
        SettingsTab::ALL
            .iter()
            .map(|tab| SettingsEntry::Header {
                label: tab.label(),
                icon: tab.icon_path(),
            })
            .collect()
    }

    // ========================================================================
    // Level 2: Category Sections (all sections always inline)
    // ========================================================================

    /// Build entries for a category (Level 2).
    /// All sections are rendered inline with a header + all child items.
    pub(super) fn build_category_sections(
        tab: SettingsTab,
        data: &SettingsViewData,
    ) -> Vec<SettingsEntry> {
        // build_tab_entries already returns Header + Item sequences in section order
        Self::build_tab_entries(tab, data)
    }

    // ========================================================================
    // Tab Entry Builders (unchanged — delegates to items_*.rs)
    // ========================================================================

    /// Build entries for a single tab
    pub(super) fn build_tab_entries(
        tab: SettingsTab,
        data: &SettingsViewData,
    ) -> Vec<SettingsEntry> {
        match tab {
            SettingsTab::Visualizer => items::build_visualizer_items(
                &data.visualizer_config,
                &data.theme_file,
                &data.active_theme_stem,
            ),
            SettingsTab::Theme => items::build_theme_items(
                &data.theme_file,
                &data.active_theme_stem,
                data.rounded_mode,
                data.opacity_gradient,
                data.is_light_mode,
            ),
            SettingsTab::General => {
                let gdata = items::GeneralSettingsData {
                    server_url: &data.server_url,
                    username: &data.username,
                    start_view: &data.start_view,
                    stable_viewport: data.stable_viewport,
                    auto_follow_playing: data.auto_follow_playing,
                    enter_behavior: data.enter_behavior,
                    local_music_path: &data.local_music_path,
                    verbose_config: data.verbose_config,
                    library_page_size: data.library_page_size,
                    artwork_resolution: data.artwork_resolution,
                    show_album_artists_only: data.show_album_artists_only,
                };
                items::build_general_items(&gdata)
            }
            SettingsTab::Interface => {
                let idata = items::InterfaceSettingsData {
                    nav_layout: data.nav_layout,
                    nav_display_mode: data.nav_display_mode,
                    track_info_display: data.track_info_display,
                    slot_row_height: data.slot_row_height,
                    horizontal_volume: data.horizontal_volume,
                    font_family: &data.font_family,
                    strip_show_title: data.strip_show_title,
                    strip_show_artist: data.strip_show_artist,
                    strip_show_album: data.strip_show_album,
                    strip_show_format_info: data.strip_show_format_info,
                    strip_click_action: data.strip_click_action,
                };
                items::build_interface_items(&idata)
            }
            SettingsTab::Playback => {
                let pdata = items::PlaybackSettingsData {
                    crossfade_enabled: data.crossfade_enabled,
                    crossfade_duration_secs: data.crossfade_duration_secs as i64,
                    volume_normalization: data.volume_normalization,
                    normalization_level: data.normalization_level,
                    scrobbling_enabled: data.scrobbling_enabled,
                    scrobble_threshold: f64::from(data.scrobble_threshold),
                    quick_add_to_playlist: data.quick_add_to_playlist,
                    default_playlist_name: &data.default_playlist_name,
                };
                items::build_playback_items(&pdata)
            }
            SettingsTab::Hotkeys => items::build_hotkeys_items(&data.hotkey_config),
        }
    }

    // ========================================================================
    // Search (cross-tab)
    // ========================================================================

    /// Build entries from ALL tabs (for cross-tab search)
    fn build_all_entries(data: &SettingsViewData) -> Vec<SettingsEntry> {
        let mut all = Vec::new();
        for tab in SettingsTab::ALL {
            all.extend(Self::build_tab_entries(*tab, data));
        }
        all
    }

    /// Search across all tabs with tab-name, header, and item-level matching.
    ///
    /// If a tab name matches the query, all its entries are included.
    /// Otherwise, entries are filtered within each tab by header/item matching.
    pub(super) fn search_all_entries(data: &SettingsViewData, query: &str) -> Vec<SettingsEntry> {
        if query.is_empty() {
            return Self::build_all_entries(data);
        }
        let query_lower = query.to_lowercase();
        let mut result = Vec::new();

        for tab in SettingsTab::ALL {
            let tab_entries = Self::build_tab_entries(*tab, data);

            if tab.label().to_lowercase().contains(&query_lower) {
                // Tab name matches — include all entries from this tab
                result.extend(tab_entries);
            } else {
                // Filter within this tab by header/item matching
                result.extend(Self::filter_by_search(&tab_entries, query));
            }
        }
        result
    }

    /// Filter entries by search query. Matches against:
    /// - Item labels and categories (subtitle text)
    /// - Section header labels (e.g. "Playback", "Item Actions", tab names)
    ///
    /// When a header matches, all its child items are included.
    /// When query is empty, returns the input unchanged.
    pub(super) fn filter_by_search(entries: &[SettingsEntry], query: &str) -> Vec<SettingsEntry> {
        if query.is_empty() {
            return entries.to_vec();
        }
        let query_lower = query.to_lowercase();

        let mut result = Vec::new();
        let mut header_matches = false;
        let mut pending_header: Option<&SettingsEntry> = None;

        for entry in entries {
            match entry {
                SettingsEntry::Header { label, .. } => {
                    // Check if this header label matches the query
                    header_matches = label.to_lowercase().contains(&query_lower);
                    pending_header = Some(entry);
                }
                SettingsEntry::Item(item) => {
                    let item_matches = header_matches
                        || item.label.to_lowercase().contains(&query_lower)
                        || item.category.to_lowercase().contains(&query_lower);

                    if item_matches {
                        // Emit the pending header if we haven't yet
                        if let Some(h) = pending_header.take() {
                            result.push(h.clone());
                        }
                        result.push(entry.clone());
                    }
                }
            }
        }
        result
    }
}
