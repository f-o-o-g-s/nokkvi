//! Settings view — slot-list-based settings UI
//!
//! Displays configurable settings in a panel with drill-down navigation.
//! Level 1: Category picker (General, Hotkeys, Theme, Visualizer)
//! Level 2: All items within a category, grouped under auto-expanded section headers
//! Up/Down navigates items, Enter activates edit mode, Escape goes back.
//! ColorArray items open a sub-list showing individual gradient colors.

use std::{collections::HashMap, time::Instant};

use nokkvi_data::types::{
    hotkey_config::{HotkeyAction, KeyCombo},
    theme_file::ThemeFile,
};

use crate::{visualizer_config::VisualizerConfig, widgets::SlotListView};

#[macro_use]
pub(crate) mod items;
mod entries;
mod items_general;
mod items_hotkeys;
mod items_interface;
mod items_playback;
mod items_theme;
mod items_visualizer;
pub(crate) mod presets;
mod rendering;
mod sub_lists;
mod view;

use items::{SettingValue, SettingsEntry};
pub(crate) use sub_lists::{FontSubListState, SubListState};

/// Normalize and validate a hex color string.
/// Ensures the value starts with `#` and contains exactly 6 hex digits.
/// Returns `Some(normalized)` if valid, `None` if invalid.
pub(super) fn normalize_hex(hex: &str) -> Option<String> {
    let normalized = if hex.starts_with('#') {
        hex.to_string()
    } else {
        format!("#{hex}")
    };
    if normalized.len() == 7 && normalized[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        Some(normalized)
    } else {
        None
    }
}

// ============================================================================
// Settings Tab Enum
// ============================================================================

/// Settings category tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SettingsTab {
    Visualizer,
    Theme,
    General,
    Interface,
    Playback,
    Hotkeys,
}

impl SettingsTab {
    /// All tabs in display order
    pub(crate) const ALL: &'static [SettingsTab] = &[
        SettingsTab::General,
        SettingsTab::Interface,
        SettingsTab::Playback,
        SettingsTab::Hotkeys,
        SettingsTab::Theme,
        SettingsTab::Visualizer,
    ];

    /// Display label for the tab
    pub(crate) fn label(&self) -> &'static str {
        match self {
            SettingsTab::Visualizer => "Visualizer",
            SettingsTab::Theme => "Theme",
            SettingsTab::General => "General",
            SettingsTab::Interface => "Interface",
            SettingsTab::Playback => "Playback",
            SettingsTab::Hotkeys => "Hotkeys",
        }
    }

    /// SVG icon path for the tab
    pub(crate) fn icon_path(&self) -> &'static str {
        match self {
            SettingsTab::Visualizer => "assets/icons/audio-lines.svg",
            SettingsTab::Theme => "assets/icons/palette.svg",
            SettingsTab::General => "assets/icons/cog.svg",
            SettingsTab::Interface => "assets/icons/panels-top-left.svg",
            SettingsTab::Playback => "assets/icons/circle-play.svg",
            SettingsTab::Hotkeys => "assets/icons/keyboard.svg",
        }
    }

    /// Description for this category (shown in the description area at Level 1)
    pub(crate) fn description(&self) -> &'static str {
        match self {
            SettingsTab::General => "Application behavior, account, and cache settings",
            SettingsTab::Interface => "Layout, display, and metadata strip settings",
            SettingsTab::Playback => "Playback, scrobbling, and playlist behavior",
            SettingsTab::Hotkeys => "Keyboard shortcut bindings and customization",
            SettingsTab::Theme => "Visual theme, colors, fonts, and presets",
            SettingsTab::Visualizer => "Audio visualizer appearance and behavior",
        }
    }
}

// ============================================================================
// Navigation Level
// ============================================================================

/// A level in the drill-down navigation hierarchy.
/// The nav_stack stores these to track where the user is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavLevel {
    /// Level 1: category picker (General, Hotkeys, Theme, Visualizer)
    CategoryPicker,
    /// Level 2: all items within a category, under auto-expanded section headers
    Category(SettingsTab),
}

impl NavLevel {
    /// Unique key for cursor memory storage
    fn cursor_key(&self) -> String {
        match self {
            NavLevel::CategoryPicker => "L1".to_string(),
            NavLevel::Category(tab) => format!("L2:{}", tab.label()),
        }
    }
}

// ============================================================================
// Settings Messages
// ============================================================================

/// Messages produced by the settings view
#[derive(Debug, Clone)]
pub enum SettingsMessage {
    /// Navigate slot list up
    SlotListUp,
    /// Navigate slot list down
    SlotListDown,
    /// Set slot list to specific offset (scrollbar seek — does NOT activate)
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    /// Item clicked — focus + activate (like clicking then pressing Enter)
    SlotListClickItem(usize),
    /// Enter pressed on center slot — drill down, toggle edit mode, or enter sub-list
    EditActivate,
    /// Left arrow in edit mode — decrement value
    EditLeft,
    /// Right arrow in edit mode — increment value
    EditRight,
    /// Escape pressed — go back one level, exit edit mode, etc.
    Escape,
    /// Hex color text input changed (typing in the inline text input)
    HexInputChanged(String),
    /// Hex color text input submitted (Enter in text input)
    HexInputSubmit,
    /// Reset current value to default (R key in edit mode)
    ResetToDefault,
    /// Raw key event forwarded during hotkey capture mode
    HotkeyCaptured(iced::keyboard::Key, iced::keyboard::Modifiers),
    /// Font search query changed (typing in font picker sub-list)
    FontSearchChanged(String),
    /// Settings search query changed (typing in main settings search bar)
    SearchChanged(String),
    /// Toggle search overlay on/off (triggered by `/` key)
    ToggleSearch,
    /// Directly set a value by string (from clickable badges: bool On/Off, enum options)
    EditSetValue(String),
    /// Toggle a single badge in a ToggleSet (key = the setting_key of the toggled badge)
    ToggleSetToggle(String),
}

/// Actions that the settings view requests from the parent
#[derive(Debug, Clone)]
pub(crate) enum SettingsAction {
    None,
    /// Request to exit settings view (Escape pressed)
    ExitSettings,
    /// Write a changed value to config.toml or the active theme file.
    /// The `ConfigKey` variant chooses the writer at compile time.
    WriteConfig {
        key: crate::config_writer::ConfigKey,
        value: items::SettingValue,
        /// Setting description added as a TOML comment above new keys
        description: Option<String>,
    },
    /// Write a single color in a color array to config.toml or the active theme.
    WriteColorEntry {
        key: crate::config_writer::ConfigKey,
        index: usize,
        hex_color: String,
    },
    /// Apply a preset theme by index
    ApplyPreset(usize),
    /// Restore all colors in a group to their defaults
    RestoreColorGroup {
        /// Vec of (toml_key, default_hex) pairs to reset
        entries: Vec<(String, String)>,
    },
    /// Play collapse/expand sound effect

    /// Play enter sound effect (items only — headers use ExpandCollapse)
    PlayEnter,
    /// Focus the hex editor text input
    FocusHexInput,
    /// Focus the settings search input
    FocusSearch,
    /// Write a hotkey binding to redb
    WriteHotkeyBinding {
        action: HotkeyAction,
        combo: KeyCombo,
    },
    /// Steal a binding: swap the conflicting action's combo with the old one
    StealHotkeyBinding {
        action: HotkeyAction,
        combo: KeyCombo,
        conflicting_action: HotkeyAction,
        old_combo: KeyCombo,
    },
    /// Reset a single hotkey to default
    ResetHotkeyBinding(HotkeyAction),
    /// Write a new font family to config.toml
    WriteFontFamily(String),
    /// Write a general setting to redb (key like "general.start_view")
    WriteGeneralSetting {
        key: String,
        value: items::SettingValue,
    },
    /// Logout: clear session and return to login screen
    Logout,
    /// Open the TextInputDialog to edit a free-text general setting
    OpenTextInput {
        key: String,
        current_value: String,
        label: String,
    },
    /// Open the confirmation dialog for resetting visualizer settings
    OpenResetVisualizerDialog,
    /// Open the confirmation dialog for resetting all hotkey bindings
    OpenResetHotkeysDialog,
}

// ============================================================================
// Settings View Data
// ============================================================================

/// Read-only data passed in from the parent for rendering
pub(crate) struct SettingsViewData {
    pub visualizer_config: VisualizerConfig,
    pub theme_file: ThemeFile,
    pub active_theme_stem: String,
    pub window_height: f32,
    pub hotkey_config: nokkvi_data::types::hotkey_config::HotkeyConfig,
    // --- General tab data ---
    pub server_url: String,
    pub username: String,
    pub is_light_mode: bool,
    pub scrobbling_enabled: bool,
    pub scrobble_threshold: f32,
    pub start_view: String,
    pub stable_viewport: bool,
    pub auto_follow_playing: bool,
    pub enter_behavior: &'static str,
    pub local_music_path: String,
    pub library_page_size: &'static str,
    pub show_album_artists_only: bool,
    pub suppress_library_refresh_toasts: bool,
    pub rounded_mode: bool,
    pub slot_text_links: bool,
    pub nav_layout: &'static str,
    pub nav_display_mode: &'static str,
    pub track_info_display: &'static str,
    pub slot_row_height: &'static str,
    pub opacity_gradient: bool,
    pub crossfade_enabled: bool,
    pub crossfade_duration_secs: u32,
    /// Volume-normalization mode label ("Off" / "AGC" / "ReplayGain (Track)" / "ReplayGain (Album)").
    pub volume_normalization: &'static str,
    pub normalization_level: &'static str,
    pub replay_gain_preamp_db: i32,
    pub replay_gain_fallback_db: i32,
    pub replay_gain_fallback_to_agc: bool,
    pub replay_gain_prevent_clipping: bool,
    pub default_playlist_name: String,
    pub quick_add_to_playlist: bool,
    pub horizontal_volume: bool,
    pub font_family: String,
    pub strip_show_title: bool,
    pub strip_show_artist: bool,
    pub strip_show_album: bool,
    pub strip_show_format_info: bool,
    pub strip_merged_mode: bool,
    pub strip_click_action: &'static str,
    pub albums_artwork_overlay: bool,
    pub artists_artwork_overlay: bool,
    pub songs_artwork_overlay: bool,
    pub playlists_artwork_overlay: bool,
    pub verbose_config: bool,
    pub artwork_resolution: &'static str,
}

// ============================================================================
// Settings Page
// ============================================================================

/// Base chrome height for settings: nav_bar(30) + player_bar(56) + content_top_padding(10) = 96.
/// Settings has no view header — the sidebar replaces it.
/// Sub-lists add BREADCRUMB_HEIGHT and/or FONT_SEARCH_BAR_HEIGHT on top.
pub(super) const SETTINGS_CHROME_HEIGHT: f32 = 96.0;

/// Height of the breadcrumb bar (Fixed(28.0) in breadcrumb_header)
pub(super) const BREADCRUMB_HEIGHT: f32 = 38.0;

/// Height of the font search bar container (Fixed(40.0) — padding is inside the fixed height)
pub(super) const FONT_SEARCH_BAR_HEIGHT: f32 = 40.0;

/// Unique text_input ID for the font picker search field (for hotkey focus)
pub(crate) const FONT_SEARCH_INPUT_ID: &str = "settings_font_search";

/// Unique text_input ID for the main settings search field
pub(crate) const SETTINGS_SEARCH_INPUT_ID: &str = "settings_search";

/// Unique text_input ID for the hex color editor
pub(crate) const HEX_EDITOR_INPUT_ID: &str = "hex_editor_input";

/// Settings page state
pub struct SettingsPage {
    /// Navigation stack — tracks current drill-down position.
    /// Empty = Level 1 (category picker). 1 entry = Level 2 (sections). 2 = Level 3 (items).
    pub(crate) nav_stack: Vec<NavLevel>,
    /// Cursor position memory per nav level (keyed by NavLevel::cursor_key())
    pub(crate) level_cursors: HashMap<String, usize>,
    /// Index of the currently keyboard-cursored badge within a ToggleSet (None = no cursor)
    pub(crate) toggle_cursor: Option<usize>,
    /// Slot list navigation state
    pub(crate) slot_list: SlotListView,
    /// Index of the entry currently being edited (None = browse mode)
    pub(crate) editing_index: Option<usize>,
    /// Cached entries rebuilt each frame — used by update() for value editing
    pub(crate) cached_entries: Vec<SettingsEntry>,
    /// Sub-list state for color array editing (None = main settings slot list)
    pub(crate) sub_list: Option<SubListState>,
    /// Sub-list state for font family selection (None = main settings slot list)
    pub(crate) font_sub_list: Option<FontSubListState>,
    /// Hotkey capture state — which action is waiting for a key press
    pub(crate) capturing_hotkey: Option<HotkeyAction>,
    /// Conflict label with timestamp for auto-dismiss (displayed when capture hits a conflict)
    pub(crate) conflict_label: Option<(String, Instant)>,
    /// Hex input buffer for inline color editing in the main slot list
    pub(crate) hex_input: String,
    /// Search/filter query for the main settings slot list
    pub(crate) search_query: String,
    /// Whether the search overlay is active (replaces breadcrumb with search input)
    pub(crate) search_active: bool,
    /// Description text for the currently focused item
    pub(crate) description_text: String,
    /// Cached system font families — populated once on first font picker open
    pub(crate) cached_system_fonts: Option<Vec<String>>,
    /// Whether config has changed since last entry rebuild (set by file watcher
    /// and config writes, cleared after `refresh_entries`). Starts `true` so
    /// the first render always populates entries.
    pub(crate) config_dirty: bool,
}

impl SettingsPage {
    pub(crate) fn new() -> Self {
        Self {
            nav_stack: vec![NavLevel::CategoryPicker],
            level_cursors: HashMap::new(),
            slot_list: SlotListView::new(),
            toggle_cursor: None,
            editing_index: None,
            cached_entries: Vec::new(),
            sub_list: None,
            font_sub_list: None,
            capturing_hotkey: None,
            conflict_label: None,
            hex_input: String::new(),
            search_query: String::new(),
            search_active: false,
            description_text: String::new(),
            cached_system_fonts: None,
            config_dirty: true,
        }
    }

    /// Get the current navigation level (top of the stack)
    pub(crate) fn current_level(&self) -> &NavLevel {
        self.nav_stack.last().unwrap_or(&NavLevel::CategoryPicker)
    }

    /// Get the currently active tab based on navigation stack
    pub(crate) fn active_tab(&self) -> Option<SettingsTab> {
        for level in self.nav_stack.iter().rev() {
            match level {
                NavLevel::Category(tab) => return Some(*tab),
                NavLevel::CategoryPicker => {}
            }
        }
        None
    }

    /// Save cursor position for the current navigation level.
    fn save_cursor(&mut self) {
        let key = self.current_level().cursor_key();
        self.level_cursors
            .insert(key, self.slot_list.viewport_offset);
    }

    /// Reset slot list and restore saved cursor for the current level.
    fn reset_and_restore_cursor(&mut self) {
        self.slot_list = SlotListView::new();
        self.editing_index = None;
        self.toggle_cursor = None;
        self.hex_input.clear();
        let key = self.current_level().cursor_key();
        if let Some(&saved_offset) = self.level_cursors.get(&key) {
            self.slot_list.viewport_offset = saved_offset;
        }
    }

    /// Push a new level onto the nav stack, saving cursor position
    pub(crate) fn push_level(&mut self, level: NavLevel) {
        self.save_cursor();
        self.nav_stack.push(level);
        self.reset_and_restore_cursor();
    }

    /// Pop the nav stack (go back one level), restoring cursor position
    pub(crate) fn pop_level(&mut self) {
        if self.nav_stack.len() <= 1 {
            return; // Already at root
        }
        self.save_cursor();
        self.nav_stack.pop();
        self.reset_and_restore_cursor();
    }

    /// Restore parent slot list position after exiting a sub-list.
    pub(super) fn restore_parent_offset(&mut self, parent_offset: usize) {
        let total = self.cached_entries.len().max(1);
        self.slot_list.set_offset(parent_offset, total);
    }

    /// Get system fonts, populating the cache on first access.
    pub(crate) fn system_fonts(&mut self) -> &[String] {
        if self.cached_system_fonts.is_none() {
            self.cached_system_fonts =
                Some(nokkvi_data::services::font_discovery::discover_system_fonts());
        }
        self.cached_system_fonts.as_deref().unwrap_or_default()
    }

    /// Handle a settings message, return any action for the parent
    pub(crate) fn update(
        &mut self,
        message: SettingsMessage,
        data: &SettingsViewData,
    ) -> SettingsAction {
        // ── Auto-clear stale conflict labels (2s timeout) ──
        if let Some((_, t)) = &self.conflict_label
            && t.elapsed().as_secs() >= 2
        {
            self.conflict_label = None;
        }

        // ── Hotkey capture mode: intercept key events ──
        if let SettingsMessage::HotkeyCaptured(ref key, modifiers) = message {
            if let Some(action) = self.capturing_hotkey.take() {
                // Escape cancels capture mode
                if matches!(
                    key,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                ) {
                    tracing::info!(" [HOTKEY CAPTURE] Cancelled by Escape");
                    self.conflict_label = None;
                    return SettingsAction::None;
                }
                // Delete resets binding to default
                if matches!(
                    key,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete)
                ) {
                    tracing::info!(" [HOTKEY CAPTURE] Reset to default by Delete");
                    self.conflict_label = None;
                    return SettingsAction::ResetHotkeyBinding(action);
                }
                tracing::info!(
                    " [HOTKEY CAPTURE] Got key {:?} mods {:?} for action {:?}",
                    key,
                    modifiers,
                    action
                );
                // Convert iced key event to our KeyCombo
                if let Some(keycode) = crate::hotkeys::iced_key_to_keycode(key) {
                    let combo = nokkvi_data::types::hotkey_config::KeyCombo {
                        key: keycode,
                        shift: modifiers.shift(),
                        ctrl: modifiers.control(),
                        alt: modifiers.alt(),
                    };
                    // Check for conflicts
                    if let Some(conflicting) = data.hotkey_config.find_conflict(&combo, &action) {
                        let old_combo = data.hotkey_config.get_binding(&action);
                        tracing::info!(
                            " [HOTKEY CAPTURE] Swapping {:?} <-> {:?}",
                            action,
                            conflicting
                        );
                        self.conflict_label = Some((
                            format!("Swapped with {}", conflicting.display_name()),
                            Instant::now(),
                        ));
                        return SettingsAction::StealHotkeyBinding {
                            action,
                            combo,
                            conflicting_action: conflicting,
                            old_combo,
                        };
                    }
                    tracing::info!(
                        " [HOTKEY CAPTURE] No conflict, writing binding: {:?} -> {:?}",
                        action,
                        combo
                    );
                    self.conflict_label = None;
                    return SettingsAction::WriteHotkeyBinding { action, combo };
                }
                tracing::info!(" [HOTKEY CAPTURE] Unsupported key, re-entering capture");
                // Unsupported key — re-enter capture mode
                self.capturing_hotkey = Some(action);
            } else {
                tracing::info!(" [HOTKEY CAPTURE] HotkeyCaptured but no capturing_hotkey set!");
            }
            return SettingsAction::None;
        }

        // ── Font sub-list mode: delegate to font sub-list handler ──
        if self.font_sub_list.is_some() {
            tracing::debug!(" [SETTINGS] Font sub-list active, delegating {:?}", message);
            return self.update_font_sub_list(message);
        }

        // ── Sub-list mode: delegate to sub-list handler ──
        if self.sub_list.is_some() {
            tracing::debug!(" [SETTINGS] Sub-list active, delegating {:?}", message);
            return self.update_sub_list(message);
        }

        let total = self.cached_entries.len().max(1);
        match message {
            SettingsMessage::SlotListUp => {
                let mut action = SettingsAction::None;
                if self.editing_index.is_some() {
                    action = self.update(SettingsMessage::HexInputSubmit, data);
                }
                // When toggle cursor is active, Up enables the cursored badge
                if let Some(cursor_idx) = self.toggle_cursor {
                    return self.toggle_set_cursor_set(cursor_idx, true);
                }
                self.editing_index = None;
                self.toggle_cursor = None;
                self.slot_list.move_up(total);
                self.snap_to_non_header(false);
                self.update_description();
                action
            }
            SettingsMessage::SlotListDown => {
                let mut action = SettingsAction::None;
                if self.editing_index.is_some() {
                    action = self.update(SettingsMessage::HexInputSubmit, data);
                }
                // When toggle cursor is active, Down disables the cursored badge
                if let Some(cursor_idx) = self.toggle_cursor {
                    return self.toggle_set_cursor_set(cursor_idx, false);
                }
                self.editing_index = None;
                self.toggle_cursor = None;
                self.slot_list.move_down(total);
                self.snap_to_non_header(true);
                self.update_description();
                action
            }
            SettingsMessage::SlotListSetOffset(offset, _) => {
                let mut action = SettingsAction::None;
                if self.editing_index.is_some() {
                    action = self.update(SettingsMessage::HexInputSubmit, data);
                }
                self.editing_index = None;
                self.toggle_cursor = None;
                self.slot_list.set_offset(offset, total);
                self.snap_to_non_header(true);
                self.update_description();
                action
            }
            SettingsMessage::SlotListClickItem(offset) => {
                self.editing_index = None;
                self.toggle_cursor = None;
                self.slot_list.set_offset(offset, total);
                self.snap_to_non_header(true);
                self.update_description();
                // Activate the newly focused item (click = focus + activate, like Enter)
                self.update(SettingsMessage::EditActivate, data)
            }
            SettingsMessage::EditActivate => {
                // Toggle edit mode off if already editing
                if self.editing_index.is_some() {
                    self.editing_index = None;
                    return SettingsAction::None;
                }

                if let Some(center_idx) = self.slot_list.get_center_item_index(total) {
                    match self.cached_entries.get(center_idx) {
                        Some(SettingsEntry::Header { label, .. }) => {
                            // Headers drill down based on current nav level
                            match self.current_level().clone() {
                                NavLevel::CategoryPicker => {
                                    // Level 1: headers are category labels — drill into category
                                    if let Some(tab) =
                                        SettingsTab::ALL.iter().find(|t| t.label() == *label)
                                    {
                                        self.push_level(NavLevel::Category(*tab));
                                        // Rebuild entries for the new level so snap
                                        // can see the actual Level 2 items.
                                        self.refresh_entries(data);
                                        self.snap_to_non_header(true);
                                        self.update_description();
                                        return SettingsAction::PlayEnter;
                                    }
                                }
                                NavLevel::Category(_) => {
                                    // Level 2: headers are section separators (non-interactive)
                                }
                            }
                            return SettingsAction::None;
                        }
                        Some(SettingsEntry::Item(item)) => {
                            let key_ref = item.key.clone();
                            match key_ref.as_ref() {
                                // Restore-defaults sentinels
                                k if items::is_restore_key(k) => {
                                    return self.handle_restore_defaults(k);
                                }
                                // Inline preset sentinels
                                k if items::is_preset_key(k) => {
                                    if let Some(idx) = items::preset_key_index(k) {
                                        return SettingsAction::ApplyPreset(idx);
                                    }
                                }
                                // Action button sentinels
                                k if items::is_action_key(k) => {
                                    return match k {
                                        "__action_logout" => SettingsAction::Logout,
                                        _ => SettingsAction::None,
                                    };
                                }
                                // Special: reset all hotkeys (opens confirmation dialog)
                                "__restore_all_hotkeys" => {
                                    return SettingsAction::OpenResetHotkeysDialog;
                                }
                                _ => {
                                    match &item.value {
                                        // ToggleSet with active cursor: Enter toggles the
                                        // cursored badge on/off
                                        SettingValue::ToggleSet(items)
                                            if self.toggle_cursor.is_some() =>
                                        {
                                            let cursor_idx = self.toggle_cursor.unwrap_or(0);
                                            if let Some((_, _, enabled)) = items.get(cursor_idx) {
                                                return self
                                                    .toggle_set_cursor_set(cursor_idx, !enabled);
                                            }
                                        }
                                        SettingValue::ColorArray(colors) => {
                                            // Enter sub-list for gradient editing
                                            self.sub_list = Some(SubListState {
                                                key: item.key.to_string(),
                                                label: item.label.clone(),
                                                colors: colors.clone(),
                                                slot_list: SlotListView::new(),
                                                editing_color_index: None,
                                                hex_input: String::new(),
                                                parent_offset: self.slot_list.viewport_offset,
                                            });
                                        }
                                        SettingValue::HexColor(hex) => {
                                            // Enter hex input mode for single color editing
                                            self.editing_index = Some(center_idx);
                                            self.hex_input = hex.clone();
                                            return SettingsAction::FocusHexInput;
                                        }
                                        SettingValue::Hotkey(_) => {
                                            // Enter hotkey capture mode
                                            if let Some(action) =
                                                items::key_to_hotkey_action(&item.key)
                                            {
                                                self.capturing_hotkey = Some(action);
                                                self.conflict_label = None;
                                            }
                                        }
                                        v if v.is_editable() => {
                                            self.editing_index = Some(center_idx);
                                        }
                                        // Font family: open font picker sub-list
                                        SettingValue::Text(_)
                                            if item.key.as_ref() == "font_family" =>
                                        {
                                            let fonts = self.system_fonts().to_vec();
                                            self.font_sub_list = Some(FontSubListState::new(
                                                fonts,
                                                self.slot_list.viewport_offset,
                                            ));
                                        }
                                        // Local music path: open text input dialog for free-text edit
                                        SettingValue::Text(current)
                                            if item.key.as_ref() == "general.local_music_path" =>
                                        {
                                            return SettingsAction::OpenTextInput {
                                                key: item.key.to_string(),
                                                current_value: current.clone(),
                                                label: item.label.clone(),
                                            };
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                SettingsAction::PlayEnter
            }
            SettingsMessage::EditRight => {
                // ToggleSet: move cursor right instead of increment
                if self.center_item_is_toggle_set() {
                    return self.toggle_set_cursor_move(true);
                }
                self.auto_enter_edit_if_needed();
                self.apply_edit(|v| v.increment())
            }
            SettingsMessage::EditLeft => {
                // ToggleSet: move cursor left instead of decrement
                if self.center_item_is_toggle_set() {
                    return self.toggle_set_cursor_move(false);
                }
                self.auto_enter_edit_if_needed();
                self.apply_edit(|v| v.decrement())
            }
            SettingsMessage::EditSetValue(val_str) => {
                self.auto_enter_edit_if_needed();
                self.apply_edit(|v| v.parse_from_str(&val_str))
            }
            SettingsMessage::ToggleSetToggle(toggle_key) => {
                // Find the center item and flip the matching toggle badge
                if let Some(center_idx) = self.slot_list.get_center_item_index(total)
                    && let Some(SettingsEntry::Item(item)) = self.cached_entries.get_mut(center_idx)
                    && let SettingValue::ToggleSet(ref mut items) = item.value
                    && let Some(entry) = items.iter_mut().find(|(_, k, _)| k == &toggle_key)
                {
                    entry.2 = !entry.2;
                    let new_val = entry.2;
                    return SettingsAction::WriteGeneralSetting {
                        key: toggle_key,
                        value: SettingValue::Bool(new_val),
                    };
                }
                SettingsAction::None
            }
            SettingsMessage::ResetToDefault => {
                // Hotkey items: reset single binding
                if self.active_tab() == Some(SettingsTab::Hotkeys)
                    && let Some(center_idx) = self.slot_list.get_center_item_index(total)
                    && let Some(SettingsEntry::Item(item)) = self.cached_entries.get(center_idx)
                    && matches!(item.value, SettingValue::Hotkey(_))
                    && let Some(action) = items::key_to_hotkey_action(&item.key)
                {
                    self.capturing_hotkey = None;
                    self.conflict_label = None;
                    return SettingsAction::ResetHotkeyBinding(action);
                }
                // Regular items: reset currently editing value to its default
                if let Some(edit_idx) = self.editing_index {
                    if let Some(SettingsEntry::Item(item)) = self.cached_entries.get(edit_idx) {
                        let default_value = item.default.clone();
                        let key = item.key.to_string();
                        // Skip reset if no key or value is already the default
                        if !key.is_empty() && item.value.display() != default_value.display() {
                            // Update cached entry
                            if let Some(SettingsEntry::Item(item_mut)) =
                                self.cached_entries.get_mut(edit_idx)
                            {
                                item_mut.value = default_value.clone();
                            }
                            self.editing_index = None;
                            if key.starts_with("general.") {
                                return SettingsAction::WriteGeneralSetting {
                                    key,
                                    value: default_value,
                                };
                            }
                            return SettingsAction::WriteConfig {
                                key: crate::config_writer::ConfigKey::for_value(key),
                                value: default_value,
                                description: None,
                            };
                        }
                    }
                    // Exit edit mode even if no reset happened
                    self.editing_index = None;
                }
                SettingsAction::None
            }
            SettingsMessage::Escape => {
                tracing::debug!(
                    " [SETTINGS] Escape: nav_depth={}, search={}, editing={}, capturing={}",
                    self.nav_stack.len(),
                    self.search_active,
                    self.editing_index.is_some(),
                    self.capturing_hotkey.is_some(),
                );
                if self.capturing_hotkey.is_some() {
                    tracing::info!(" [HOTKEY CAPTURE] Escape pressed, cancelling capture");
                    self.capturing_hotkey = None;
                    self.conflict_label = None;
                    SettingsAction::None
                } else if self.toggle_cursor.is_some() {
                    self.toggle_cursor = None;
                    SettingsAction::None
                } else if self.editing_index.is_some() {
                    self.editing_index = None;
                    self.hex_input.clear();
                    SettingsAction::None
                } else if self.search_active && !self.search_query.is_empty() {
                    // Clear visible search filter and restore entries for current nav level
                    self.search_active = false;
                    self.search_query.clear();
                    self.slot_list = SlotListView::new();
                    self.refresh_entries(data);
                    SettingsAction::None
                } else if self.nav_stack.len() > 1 {
                    // Pop navigation stack — go back one level
                    // Clear any stale search query from a dismissed search bar
                    self.search_query.clear();
                    self.pop_level();
                    self.refresh_entries(data);
                    SettingsAction::None
                } else {
                    // Reset all transient state for clean re-open.
                    // This handles the zombie scenario where Tab sets
                    // search_active=false while search_query remains populated,
                    // causing Escape to skip the search-clearing branch.
                    self.search_query.clear();
                    self.search_active = false;
                    self.cached_entries.clear();
                    self.description_text.clear();
                    self.slot_list = SlotListView::new();
                    self.nav_stack.truncate(1); // Reset to CategoryPicker
                    tracing::debug!(" [SETTINGS] ExitSettings triggered!");
                    SettingsAction::ExitSettings
                }
            }
            SettingsMessage::HexInputChanged(new_hex) => {
                // Main slot list hex editing (for HexColor items)
                if self.editing_index.is_some() {
                    self.hex_input = new_hex;
                }
                SettingsAction::None
            }
            SettingsMessage::HexInputSubmit => {
                // Submit hex input for a HexColor item in the main slot list
                if let Some(edit_idx) = self.editing_index
                    && let Some(SettingsEntry::Item(item)) = self.cached_entries.get(edit_idx)
                    && matches!(item.value, SettingValue::HexColor(_))
                    && let Some(normalized) = normalize_hex(&self.hex_input)
                {
                    let key = item.key.to_string();
                    // Update cached entry
                    if let Some(SettingsEntry::Item(item_mut)) =
                        self.cached_entries.get_mut(edit_idx)
                    {
                        item_mut.value = SettingValue::HexColor(normalized.clone());
                    }
                    self.editing_index = None;
                    self.hex_input.clear();
                    if !key.is_empty() && !key.starts_with("__") {
                        return SettingsAction::WriteConfig {
                            key: crate::config_writer::ConfigKey::for_value(key),
                            value: SettingValue::HexColor(normalized),
                            description: None,
                        };
                    }
                }
                SettingsAction::None
            }
            // These messages are only handled in sub-list mode or capture mode
            SettingsMessage::HotkeyCaptured(_, _) | SettingsMessage::FontSearchChanged(_) => {
                SettingsAction::None
            }
            SettingsMessage::SearchChanged(query) => {
                self.search_active = !query.is_empty();
                self.search_query = query;
                self.editing_index = None;
                self.slot_list = SlotListView::new();
                self.refresh_entries(data);
                SettingsAction::None
            }
            SettingsMessage::ToggleSearch => {
                // Clear any existing search query and focus the input
                if !self.search_query.is_empty() {
                    self.search_active = false;
                    self.search_query.clear();
                    self.slot_list = SlotListView::new();
                    self.refresh_entries(data);
                }
                SettingsAction::FocusSearch
            }
        }
    }

    /// Collect (key, default_hex) pairs for a __restore_* group key.
    /// Walks the cached entries and collects all HexColor items in the same category.
    fn handle_restore_defaults(&mut self, restore_key: &str) -> SettingsAction {
        // Special: __restore_theme restores the active built-in theme to its original
        if restore_key == "__restore_theme" {
            let stem = presets::active_theme_stem();
            if let Err(e) = presets::restore_theme(&stem) {
                tracing::warn!(" [SETTINGS] Failed to restore theme '{stem}': {e}");
            }
            return SettingsAction::RestoreColorGroup { entries: vec![] };
        }

        // Special: __restore_visualizer opens a confirmation dialog
        if restore_key == "__restore_visualizer" {
            return SettingsAction::OpenResetVisualizerDialog;
        }

        // Find the category that this __restore key belongs to
        let category = self.cached_entries.iter().find_map(|e| {
            if let SettingsEntry::Item(item) = e
                && item.key.as_ref() == restore_key
            {
                return Some(item.category);
            }
            None
        });

        if let Some(category) = category {
            let entries: Vec<(String, String)> = self
                .cached_entries
                .iter()
                .filter_map(|e| {
                    if let SettingsEntry::Item(item) = e
                        && item.category == category
                        && !item.key.starts_with("__")
                        && !item.key.is_empty()
                        && let SettingValue::HexColor(_) = &item.value
                        && let SettingValue::HexColor(def) = &item.default
                    {
                        return Some((item.key.to_string(), def.clone()));
                    }
                    None
                })
                .collect();

            if !entries.is_empty() {
                return SettingsAction::RestoreColorGroup { entries };
            }
        }
        SettingsAction::None
    }

    /// Auto-enter edit mode on the center item if not already editing.
    /// Called by EditLeft/EditRight so the user doesn't need to press Enter first.
    fn auto_enter_edit_if_needed(&mut self) {
        if self.editing_index.is_some() {
            return; // Already editing
        }
        let total = self.cached_entries.len();
        if let Some(center_idx) = self.slot_list.get_center_item_index(total)
            && let Some(SettingsEntry::Item(item)) = self.cached_entries.get(center_idx)
            && item.value.is_editable()
        {
            self.editing_index = Some(center_idx);
        }
    }

    /// Check whether the center item is a ToggleSet.
    fn center_item_is_toggle_set(&self) -> bool {
        let total = self.cached_entries.len();
        if let Some(center_idx) = self.slot_list.get_center_item_index(total)
            && let Some(SettingsEntry::Item(item)) = self.cached_entries.get(center_idx)
        {
            matches!(item.value, SettingValue::ToggleSet(_))
        } else {
            false
        }
    }

    /// Move the toggle cursor left (forward=false) or right (forward=true).
    /// Initializes cursor on first press; wraps around at boundaries.
    fn toggle_set_cursor_move(&mut self, forward: bool) -> SettingsAction {
        let total = self.cached_entries.len();
        let Some(center_idx) = self.slot_list.get_center_item_index(total) else {
            return SettingsAction::None;
        };
        let Some(SettingsEntry::Item(item)) = self.cached_entries.get(center_idx) else {
            return SettingsAction::None;
        };
        let SettingValue::ToggleSet(items) = &item.value else {
            return SettingsAction::None;
        };
        let count = items.len();
        if count == 0 {
            return SettingsAction::None;
        }

        let new_idx = match self.toggle_cursor {
            Some(idx) => {
                if forward {
                    (idx + 1) % count
                } else {
                    (idx + count - 1) % count
                }
            }
            None => {
                // First press: Right starts at 0, Left starts at last
                if forward { 0 } else { count - 1 }
            }
        };
        self.toggle_cursor = Some(new_idx);
        SettingsAction::None
    }

    /// Set the cursored toggle badge to a specific on/off state.
    /// Used by Up (enable) and Down (disable).
    fn toggle_set_cursor_set(&mut self, cursor_idx: usize, enabled: bool) -> SettingsAction {
        let total = self.cached_entries.len();
        let Some(center_idx) = self.slot_list.get_center_item_index(total) else {
            return SettingsAction::None;
        };
        let Some(SettingsEntry::Item(item)) = self.cached_entries.get_mut(center_idx) else {
            return SettingsAction::None;
        };
        let SettingValue::ToggleSet(ref mut items) = item.value else {
            return SettingsAction::None;
        };
        if let Some(entry) = items.get_mut(cursor_idx)
            && entry.2 != enabled
        {
            entry.2 = enabled;
            let toggle_key = entry.1.clone();
            return SettingsAction::WriteGeneralSetting {
                key: toggle_key,
                value: SettingValue::Bool(enabled),
            };
        }
        SettingsAction::None
    }

    /// Apply an edit operation (increment/decrement) to the currently editing item.
    /// Returns WriteConfig action if the value changed.
    fn apply_edit(
        &mut self,
        change_fn: impl FnOnce(&items::SettingValue) -> Option<items::SettingValue>,
    ) -> SettingsAction {
        if let Some(edit_idx) = self.editing_index
            && let Some(SettingsEntry::Item(item)) = self.cached_entries.get(edit_idx)
            && let Some(mut new_value) = change_fn(&item.value)
        {
            let key = item.key.to_string();

            // Monstercat snap: values in (0.0, MIN_EFFECTIVE) are a dead zone where the
            // filter amplifies instead of attenuating.  Snap based on direction:
            //   incrementing from 0.0 → jump to MIN_EFFECTIVE
            //   decrementing from MIN_EFFECTIVE → jump to 0.0
            if key == "visualizer.monstercat" {
                let min = crate::visualizer_config::MONSTERCAT_MIN_EFFECTIVE;
                if let (
                    items::SettingValue::Float { val: old_val, .. },
                    items::SettingValue::Float {
                        val: new_val,
                        min: fmin,
                        max,
                        step,
                        unit,
                    },
                ) = (&item.value, &mut new_value)
                {
                    if *new_val > 0.0 && *new_val < min {
                        // Determine direction from old → new
                        if *new_val > *old_val {
                            // Incrementing into dead zone → snap up
                            *new_val = min;
                        } else {
                            // Decrementing into dead zone → snap to off
                            *new_val = *fmin;
                        }
                    }
                    let _ = (max, step, unit); // suppress unused warnings
                }
            }

            // Update the cached entry so the UI reflects the change immediately
            if let Some(SettingsEntry::Item(item_mut)) = self.cached_entries.get_mut(edit_idx) {
                item_mut.value = new_value.clone();
            }
            if !key.is_empty() {
                if key.starts_with("general.") {
                    return SettingsAction::WriteGeneralSetting {
                        key,
                        value: new_value,
                    };
                }
                return SettingsAction::WriteConfig {
                    key: crate::config_writer::ConfigKey::for_value(key),
                    value: new_value,
                    description: self.cached_entries.get(edit_idx).and_then(|e| match e {
                        SettingsEntry::Item(item) => item.subtitle.map(String::from),
                        _ => None,
                    }),
                };
            }
        }
        SettingsAction::None
    }

    /// Populate cached entries from config data based on current nav level.
    pub(crate) fn refresh_entries(&mut self, data: &SettingsViewData) {
        if !self.search_query.is_empty() {
            self.cached_entries = Self::search_all_entries(data, &self.search_query);
            self.update_description();
            return;
        }
        self.cached_entries = match self.current_level() {
            NavLevel::CategoryPicker => Self::build_category_picker_entries(),
            NavLevel::Category(tab) => Self::build_category_sections(*tab, data),
        };
        self.update_description();
    }

    /// Update the description_text from the center item's subtitle.
    /// Called after any slot list navigation or entry refresh.
    pub(crate) fn update_description(&mut self) {
        let total = self.cached_entries.len();
        self.description_text = self
            .slot_list
            .get_center_item_index(total)
            .and_then(|idx| self.cached_entries.get(idx))
            .map(|entry| match entry {
                SettingsEntry::Item(item) => item.subtitle.unwrap_or(item.category).to_string(),
                SettingsEntry::Header { label, .. } => {
                    // At Level 1 (without active search), look up the tab's
                    // description for a meaningful footer. During search, headers
                    // are section separators from within tabs and may collide with
                    // tab names (e.g. Visualizer's "General" section vs the General
                    // tab) — show the raw label instead.
                    if matches!(self.current_level(), NavLevel::CategoryPicker)
                        && self.search_query.is_empty()
                    {
                        SettingsTab::ALL
                            .iter()
                            .find(|t| t.label() == *label)
                            .map_or_else(|| label.to_string(), |t| t.description().to_string())
                    } else {
                        label.to_string()
                    }
                }
            })
            .unwrap_or_default();
    }

    /// If the current viewport offset is on a header at Level 2, snap to the
    /// nearest non-header `Item` entry in the given direction.
    ///
    /// `forward == true`: prefer scanning forward, fall back to backward.
    /// `forward == false`: prefer scanning backward, fall back to forward.
    ///
    /// No-op at Level 1 (all entries are selectable headers) or if the current
    /// entry is already an `Item`.
    pub(crate) fn snap_to_non_header(&mut self, forward: bool) {
        if matches!(self.current_level(), NavLevel::CategoryPicker) {
            return;
        }
        let offset = self.slot_list.viewport_offset;
        if self
            .cached_entries
            .get(offset)
            .is_some_and(|e| !e.is_header())
        {
            return;
        }
        let fwd = self.cached_entries[offset..]
            .iter()
            .position(|e| !e.is_header())
            .map(|i| offset + i);
        let bwd = self.cached_entries[..offset]
            .iter()
            .rposition(|e| !e.is_header());
        self.slot_list.viewport_offset =
            if forward { fwd.or(bwd) } else { bwd.or(fwd) }.unwrap_or(offset);
    }
}
