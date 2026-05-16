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
            SettingsTab::General => items::build_general_items(&data.general),
            SettingsTab::Interface => items::build_interface_items(&data.interface),
            SettingsTab::Playback => items::build_playback_items(&data.playback),
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
    /// - Item labels (e.g. "Stable Viewport").
    /// - Item categories — section names like "Mouse Behavior".
    /// - Item subtitles — description text shown in the footer.
    /// - Section header labels (e.g. "Playback", "Item Actions", tab names).
    ///
    /// `subtitle` matching keeps description-text search working after the
    /// `define_settings!`-driven items split moved description text from
    /// the (formerly mis-named) `category` slot into `subtitle:`.
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
                    let subtitle_matches = item
                        .subtitle
                        .as_deref()
                        .is_some_and(|s| s.to_lowercase().contains(&query_lower));
                    let item_matches = header_matches
                        || item.label.to_lowercase().contains(&query_lower)
                        || item.category.to_lowercase().contains(&query_lower)
                        || subtitle_matches;

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
