//! Settings view — slot-list-based settings UI
//!
//! Displays configurable settings in a panel with drill-down navigation.
//! Level 1: Category picker (General, Hotkeys, Theme, Visualizer)
//! Level 2: All items within a category, grouped under auto-expanded section headers
//! Up/Down navigates items, Enter activates edit mode, Escape goes back.
//! ColorArray items open a sub-list showing individual gradient colors.

use std::time::Instant;

use nokkvi_data::types::{
    hotkey_config::{HotkeyAction, KeyCombo},
    theme_file::ThemeFile,
};

use crate::{visualizer_config::VisualizerConfig, widgets::SlotListView};

mod entries;
pub(crate) mod items;
mod items_general;
mod items_hotkeys;
mod items_interface;
mod items_playback;
mod items_theme;
mod items_visualizer;
pub(crate) mod presets;
mod rendering;
pub(crate) mod sentinel;
mod sub_lists;
#[cfg(test)]
pub(crate) mod test_support;
mod view;

use items::{ActivateKind, SettingValue, SettingsEntry};
// Only named by the theme-picker handler tests; the render path reads
// `ThemeRow::preview` fields without naming the type.
#[cfg(test)]
pub(crate) use sub_lists::ThemePreviewColors;
pub(crate) use sub_lists::{FontSubListState, SubListState, ThemeRow, ThemeSubListState};

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

/// Pick the right `SettingsAction` write variant for a single setting write.
///
/// Precedence: `general.` key prefix wins over the `is_theme_key` flag — a
/// `general.*` row is always routed to `WriteGeneralSetting` even when the
/// row also happens to be tagged as a theme-file key (the tag is dead in
/// that case; see `items_theme.rs`). Non-`general.` keys route to
/// `WriteConfig` with a `ConfigKey::theme_scalar` when `is_theme` is set, and
/// `ConfigKey::app_scalar` otherwise.
///
/// `description` is only carried on the `WriteConfig` variant — the
/// `WriteGeneralSetting` arm does not surface a TOML comment because the
/// general-settings backend stores values in redb, not config.toml.
pub(crate) fn route_write(
    key: String,
    is_theme: bool,
    value: items::SettingValue,
    description: Option<String>,
) -> SettingsAction {
    if key.starts_with("general.") {
        return SettingsAction::WriteGeneralSetting { key, value };
    }
    let config_key = if is_theme {
        crate::config_writer::ConfigKey::theme_scalar(key)
    } else {
        crate::config_writer::ConfigKey::app_scalar(key)
    };
    SettingsAction::WriteConfig {
        key: config_key,
        value,
        description,
    }
}

#[cfg(test)]
mod route_write_tests {
    use super::{SettingsAction, items::SettingValue, route_write};
    use crate::config_writer::ConfigKey;

    fn assert_write_general(action: &SettingsAction, expect_key: &str) {
        match action {
            SettingsAction::WriteGeneralSetting { key, .. } => {
                assert_eq!(key, expect_key, "WriteGeneralSetting key mismatch");
            }
            other => panic!("expected WriteGeneralSetting, got {other:?}"),
        }
    }

    fn assert_write_theme(action: &SettingsAction, expect_key: &str) {
        match action {
            SettingsAction::WriteConfig { key, .. } => {
                assert!(
                    matches!(key, ConfigKey::Theme(_)),
                    "expected ConfigKey::Theme, got {key:?}"
                );
                assert!(key.is_theme(), "ConfigKey::is_theme() should be true");
                assert_eq!(key.as_str(), expect_key, "theme key path mismatch");
            }
            other => panic!("expected WriteConfig, got {other:?}"),
        }
    }

    fn assert_write_app(action: &SettingsAction, expect_key: &str) {
        match action {
            SettingsAction::WriteConfig { key, .. } => {
                assert!(
                    matches!(key, ConfigKey::AppScalar(_)),
                    "expected ConfigKey::AppScalar, got {key:?}"
                );
                assert!(!key.is_theme(), "ConfigKey::is_theme() should be false");
                assert_eq!(key.as_str(), expect_key, "app key path mismatch");
            }
            other => panic!("expected WriteConfig, got {other:?}"),
        }
    }

    // ── general.* short-circuit ─────────────────────────────────────────

    #[test]
    fn general_prefix_with_theme_flag_routes_to_general() {
        // Row tagged is_theme_key=true but keyed general.* (light_mode) — the
        // `with_theme_key()` tag is dead because `general.` wins.
        let action = route_write(
            "general.light_mode".to_string(),
            true,
            SettingValue::Bool(true),
            None,
        );
        assert_write_general(&action, "general.light_mode");
    }

    #[test]
    fn general_prefix_no_theme_flag_routes_to_general() {
        let action = route_write(
            "general.rounded_mode".to_string(),
            false,
            SettingValue::Bool(true),
            None,
        );
        assert_write_general(&action, "general.rounded_mode");
    }

    #[test]
    fn general_prefix_opacity_gradient_routes_to_general() {
        let action = route_write(
            "general.opacity_gradient".to_string(),
            false,
            SettingValue::Bool(true),
            None,
        );
        assert_write_general(&action, "general.opacity_gradient");
    }

    #[test]
    fn general_prefix_toggleset_routes_to_general() {
        let action = route_write(
            "general.queue.show_album".to_string(),
            false,
            SettingValue::ToggleSet(vec![]),
            None,
        );
        assert_write_general(&action, "general.queue.show_album");
    }

    // ── theme scalar (is_theme=true, non-general) ───────────────────────

    #[test]
    fn dark_theme_hexcolor_routes_to_theme_scalar() {
        let action = route_write(
            "dark.background.hard".to_string(),
            true,
            SettingValue::HexColor("#000000".to_string()),
            None,
        );
        assert_write_theme(&action, "dark.background.hard");
    }

    #[test]
    fn light_theme_hexcolor_routes_to_theme_scalar() {
        let action = route_write(
            "light.background.hard".to_string(),
            true,
            SettingValue::HexColor("#ffffff".to_string()),
            None,
        );
        assert_write_theme(&action, "light.background.hard");
    }

    // ── app scalar (is_theme=false, non-general) ────────────────────────

    #[test]
    fn settings_visualizer_bool_routes_to_app_scalar() {
        let action = route_write(
            "settings.visualizer.bars".to_string(),
            false,
            SettingValue::Bool(true),
            None,
        );
        assert_write_app(&action, "settings.visualizer.bars");
    }

    #[test]
    fn settings_queue_enum_routes_to_app_scalar() {
        let action = route_write(
            "settings.queue.sort_mode".to_string(),
            false,
            SettingValue::Enum {
                val: "Title".to_string(),
                options: vec!["Title", "Artist"],
            },
            None,
        );
        assert_write_app(&action, "settings.queue.sort_mode");
    }

    // ── HexInputSubmit-style forward protection ─────────────────────────

    #[test]
    fn hex_input_submit_general_with_theme_flag_short_circuits() {
        // Forward-protective assertion: today no `general.*` key has a
        // HexColor row, but if one is ever added, HexInputSubmit must route
        // through WriteGeneralSetting (not WriteConfig).
        let action = route_write(
            "general.foo".to_string(),
            true,
            SettingValue::HexColor("#abcdef".to_string()),
            None,
        );
        assert_write_general(&action, "general.foo");
    }

    // ── description plumbing ────────────────────────────────────────────

    #[test]
    fn description_is_attached_to_write_config_only() {
        let action = route_write(
            "settings.visualizer.bars".to_string(),
            false,
            SettingValue::Bool(true),
            Some("desc".to_string()),
        );
        match action {
            SettingsAction::WriteConfig { description, .. } => {
                assert_eq!(description.as_deref(), Some("desc"));
            }
            other => panic!("expected WriteConfig, got {other:?}"),
        }
    }

    #[test]
    fn description_is_ignored_for_general_writes() {
        let action = route_write(
            "general.light_mode".to_string(),
            true,
            SettingValue::Bool(true),
            Some("desc".to_string()),
        );
        // WriteGeneralSetting carries no description field; confirm the
        // variant choice ignores it without erroring.
        assert_write_general(&action, "general.light_mode");
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
    /// Sub-list search query changed (typing in the font OR theme picker
    /// sub-list search box). Only one sub-list is open at a time, so the active
    /// sub-list handler owns the meaning.
    SubListSearchChanged(String),
    /// Settings search query changed (typing in main settings search bar)
    SearchChanged(String),
    /// Toggle search overlay on/off (triggered by `/` key)
    ToggleSearch,
    /// Directly set a value by string (from clickable badges: bool On/Off, enum options)
    EditSetValue(String),
    /// Drag the centered Float/Int value to a fraction in `[0.0, 1.0]`.
    /// Emitted by the draggable settings slider; the handler quantizes to
    /// the item's `step` and clamps to `[min, max]`.
    EditSetFraction(f32),
    /// Toggle a single badge in a ToggleSet (key = the setting_key of the toggled badge)
    ToggleSetToggle(String),
    /// Move sidebar focus to the next category (Shift+Tab default)
    SidebarDown,
    /// Move sidebar focus to the previous category (Shift+Backspace default)
    SidebarUp,
    /// Scrollbar-seek the sidebar to a specific category offset
    SidebarSetOffset(usize, iced::keyboard::Modifiers),
    /// Sidebar row clicked — focus + activate (loads the category into the detail pane)
    SidebarClickItem(usize),
    /// Mini-index pill clicked — scroll the detail pane so the header at
    /// the given entry index lands at the top of the viewport.
    JumpToSection(usize),
}

/// Build the scrollbar-seek closure shared by every settings slot list.
///
/// `slot_list::slot_list_view_with_scroll` calls the seek callback with a
/// normalized fraction `f ∈ [0.0, 1.0]`; mapping it to the absolute slot
/// offset requires the entry-list length at construction time. The three
/// settings surfaces (main entries list, font picker sub-list, color
/// gradient sub-list) all need the byte-identical closure; this helper
/// collapses them to a one-liner so the [`SlotListSetOffset`] message shape
/// can't drift across call sites.
///
/// [`SlotListSetOffset`]: SettingsMessage::SlotListSetOffset
pub(super) fn settings_seek_to(total: usize) -> impl Fn(f32) -> SettingsMessage {
    move |f| {
        SettingsMessage::SlotListSetOffset(
            (f * total as f32) as usize,
            iced::keyboard::Modifiers::default(),
        )
    }
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
    /// Apply a preset theme. Carries the theme's stem (apply key) and display
    /// name (for the confirmation toast) so the root handler applies by stem
    /// directly — there is no positional index into the theme list to drift.
    ApplyPreset {
        stem: String,
        display_name: String,
    },
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
    /// Open the default-playlist picker modal (root-level overlay).
    OpenDefaultPlaylistPicker,
    /// Open the masked dialog to set/clear the ListenBrainz scrobble token.
    OpenListenBrainzTokenDialog,
    /// Validate the currently-configured ListenBrainz token (toasts the result).
    VerifyListenBrainz,
    /// Open the dialog to enter the Last.fm app key + secret.
    OpenLastfmCredentialsDialog,
    /// Begin the Last.fm browser-auth flow.
    ConnectLastfm,
    /// Disconnect Last.fm (clear the stored session).
    DisconnectLastfm,
}

// ============================================================================
// Settings View Data
// ============================================================================

/// Read-only data passed in from the parent for rendering.
///
/// Composition: the tab-specific payloads live in
/// [`GeneralSettingsData`] / [`InterfaceSettingsData`] /
/// [`PlaybackSettingsData`] (from `nokkvi_data::types::settings_data`),
/// and the cross-cutting fields (theme/visualizer/window/hotkey) stay flat
/// at this level. `build_settings_view_data` constructs each sub-struct
/// directly; `entries.rs::build_tab_entries` hands the matching sub-struct
/// straight to `items::build_*_items`.
pub(crate) struct SettingsViewData {
    pub general: nokkvi_data::types::settings_data::GeneralSettingsData,
    pub interface: nokkvi_data::types::settings_data::InterfaceSettingsData,
    pub playback: nokkvi_data::types::settings_data::PlaybackSettingsData,
    pub visualizer_config: VisualizerConfig,
    pub theme_file: ThemeFile,
    pub active_theme_stem: String,
    pub hotkey_config: nokkvi_data::types::hotkey_config::HotkeyConfig,
    pub is_light_mode: bool,
    pub rounded_mode: nokkvi_data::types::player_settings::RoundedMode,
    pub opacity_gradient: bool,
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

/// Unique text_input ID for the theme picker search field (for hotkey focus)
pub(crate) const THEME_SEARCH_INPUT_ID: &str = "settings_theme_search";

/// Unique text_input ID for the main settings search field
pub(crate) const SETTINGS_SEARCH_INPUT_ID: &str = "settings_search";

/// Unique text_input ID for the hex color editor
pub(crate) const HEX_EDITOR_INPUT_ID: &str = "hex_editor_input";

/// Unique scrollable ID for the detail pane. Auto-scroll (Tab/Backspace nav
/// and mini-index jumps) targets this scrollable to keep the focused row in
/// view via measured geometry — see [`crate::widgets::scroll_into_view`].
pub(crate) const DETAIL_SCROLLABLE_ID: &str = "settings_detail_scrollable";

/// Widget ID attached to the currently-focused detail-pane row. The measured
/// auto-scroll reads this row's real laid-out bounds (rather than an estimated
/// pixel height, which drifts on variable-height rows) to center it within
/// `DETAIL_SCROLLABLE_ID`.
pub(crate) const DETAIL_FOCUSED_ROW_ID: &str = "settings_detail_focused_row";

/// Settings page state
pub struct SettingsPage {
    /// Index of the currently keyboard-cursored badge within a ToggleSet (None = no cursor)
    pub(crate) toggle_cursor: Option<usize>,
    /// Slot list navigation state
    pub(crate) slot_list: SlotListView,
    /// Index of the entry currently being edited (None = browse mode)
    pub(crate) editing_index: Option<usize>,
    /// Cached entries the view renders verbatim — rebuilt in the update path
    /// (`refresh_entries` / `Nokkvi::refresh_settings_entries_if_dirty`) on
    /// tab switch, search, config writes, hot-reloads, and view entry
    pub(crate) cached_entries: Vec<SettingsEntry>,
    /// Sub-list state for color array editing (None = main settings slot list)
    pub(crate) sub_list: Option<SubListState>,
    /// Sub-list state for font family selection (None = main settings slot list)
    pub(crate) font_sub_list: Option<FontSubListState>,
    /// Sub-list state for theme selection (None = main settings slot list).
    /// Mutually exclusive with `font_sub_list` — only one picker opens at a time.
    pub(crate) theme_sub_list: Option<ThemeSubListState>,
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
    /// Cached system font families — populated once on first font picker open
    pub(crate) cached_system_fonts: Option<Vec<String>>,
    /// Whether config has changed since last entry rebuild (set by file watcher
    /// and config writes, cleared after `refresh_entries`). Starts `true` so
    /// the first render always populates entries.
    pub(crate) config_dirty: bool,
    /// Active category whose entries populate the detail pane (right side of
    /// the persistent two-pane layout). Defaults to `General`.
    pub(crate) active_category: SettingsTab,
    /// Slot-list state for the categories sidebar (left pane). Distinct from
    /// `slot_list`, which tracks the detail pane focus.
    pub(crate) sidebar_slot_list: SlotListView,
}

impl SettingsPage {
    pub(crate) fn new() -> Self {
        Self {
            slot_list: SlotListView::new(),
            toggle_cursor: None,
            editing_index: None,
            cached_entries: Vec::new(),
            sub_list: None,
            font_sub_list: None,
            theme_sub_list: None,
            capturing_hotkey: None,
            conflict_label: None,
            hex_input: String::new(),
            search_query: String::new(),
            search_active: false,
            cached_system_fonts: None,
            config_dirty: true,
            active_category: SettingsTab::General,
            sidebar_slot_list: SlotListView::new(),
        }
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

        // ── Theme sub-list mode: delegate to theme sub-list handler ──
        if self.theme_sub_list.is_some() {
            tracing::debug!(
                " [SETTINGS] Theme sub-list active, delegating {:?}",
                message
            );
            return self.update_theme_sub_list(message);
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
                action
            }
            SettingsMessage::SlotListClickItem(offset) => {
                self.editing_index = None;
                self.toggle_cursor = None;
                self.slot_list.set_offset(offset, total);
                self.snap_to_non_header(true);
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
                        Some(SettingsEntry::Header { label: _, .. }) => {
                            // Detail-pane headers are section separators —
                            // non-interactive. Category drill-down moved to the
                            // sidebar slot list in the persistent two-pane
                            // shell, so Enter on a header is a no-op here.
                            return SettingsAction::None;
                        }
                        Some(SettingsEntry::Item(item)) => {
                            let key_ref = item.key.clone();
                            // Typed sentinel dispatch — see `sentinel::SentinelKind`.
                            // All `__*` sentinel keys round-trip through the enum;
                            // a `None` return means the key is a regular settings
                            // key and falls through to the value-edit dispatch
                            // below.
                            match sentinel::SentinelKind::from_key(key_ref.as_ref()) {
                                Some(sentinel::SentinelKind::Logout) => {
                                    return SettingsAction::Logout;
                                }
                                // Reset sentinels (visualizer settings + all
                                // hotkeys) — funnel through
                                // `handle_restore_defaults`, which opens the
                                // matching confirmation dialog.
                                Some(
                                    sentinel::SentinelKind::RestoreVisualizer
                                    | sentinel::SentinelKind::RestoreAllHotkeys,
                                ) => {
                                    return self.handle_restore_defaults(key_ref.as_ref());
                                }
                                Some(sentinel::SentinelKind::SetListenBrainzToken) => {
                                    return SettingsAction::OpenListenBrainzTokenDialog;
                                }
                                Some(sentinel::SentinelKind::VerifyListenBrainz) => {
                                    return SettingsAction::VerifyListenBrainz;
                                }
                                Some(sentinel::SentinelKind::SetLastfmCredentials) => {
                                    return SettingsAction::OpenLastfmCredentialsDialog;
                                }
                                Some(sentinel::SentinelKind::ConnectLastfm) => {
                                    return SettingsAction::ConnectLastfm;
                                }
                                Some(sentinel::SentinelKind::DisconnectLastfm) => {
                                    return SettingsAction::DisconnectLastfm;
                                }
                                None => {
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
                                        // Structural activation intent (font
                                        // picker / text dialog / playlist
                                        // picker). Set by the builder via
                                        // `with_activate`; matched here instead
                                        // of string-matching the key so a key
                                        // rename can't drift the action.
                                        SettingValue::Text(current) => match item.on_activate {
                                            Some(ActivateKind::FontPicker) => {
                                                let fonts = self.system_fonts().to_vec();
                                                self.font_sub_list = Some(FontSubListState::new(
                                                    fonts,
                                                    self.slot_list.viewport_offset,
                                                ));
                                            }
                                            Some(ActivateKind::ThemePicker) => {
                                                self.theme_sub_list = Some(ThemeSubListState::new(
                                                    self.slot_list.viewport_offset,
                                                ));
                                            }
                                            Some(ActivateKind::TextInputDialog) => {
                                                return SettingsAction::OpenTextInput {
                                                    key: item.key.to_string(),
                                                    current_value: current.clone(),
                                                    label: item.label.clone(),
                                                };
                                            }
                                            Some(ActivateKind::PlaylistPicker) => {
                                                return SettingsAction::OpenDefaultPlaylistPicker;
                                            }
                                            None => {}
                                        },
                                        _ => {}
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                SettingsAction::None
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
            SettingsMessage::EditSetFraction(fraction) => {
                self.auto_enter_edit_if_needed();
                self.apply_edit(|v| v.set_fraction(fraction))
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
                    let is_theme = item.is_theme_key;
                    // Route through `route_write` for forward protection: a
                    // future non-`general.*` toggle badge would land in
                    // config.toml instead of being silently written to redb.
                    return route_write(toggle_key, is_theme, SettingValue::Bool(new_val), None);
                }
                SettingsAction::None
            }
            SettingsMessage::ResetToDefault => {
                // Hotkey items: reset single binding
                if self.active_category == SettingsTab::Hotkeys
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
                        let is_theme = item.is_theme_key;
                        // Skip reset if no key or value is already the default
                        if !key.is_empty() && item.value.display() != default_value.display() {
                            // Update cached entry
                            if let Some(SettingsEntry::Item(item_mut)) =
                                self.cached_entries.get_mut(edit_idx)
                            {
                                item_mut.value = default_value.clone();
                            }
                            self.editing_index = None;
                            return route_write(key, is_theme, default_value, None);
                        }
                    }
                    // Exit edit mode even if no reset happened
                    self.editing_index = None;
                }
                SettingsAction::None
            }
            SettingsMessage::Escape => {
                tracing::debug!(
                    " [SETTINGS] Escape: search_active={}, editing={}, capturing={}",
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
                    // Clear visible search filter and restore entries for the
                    // current active category.
                    self.search_active = false;
                    self.search_query.clear();
                    self.slot_list = SlotListView::new();
                    self.refresh_entries(data);
                    SettingsAction::None
                } else {
                    // Top-level Escape — reset transient state for clean
                    // re-open. The zombie scenario (Tab cleared
                    // search_active but left search_query populated) still
                    // funnels through here, so both flags are reset.
                    self.search_query.clear();
                    self.search_active = false;
                    self.cached_entries.clear();
                    self.slot_list = SlotListView::new();
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
                    let is_theme = item.is_theme_key;
                    // Update cached entry
                    if let Some(SettingsEntry::Item(item_mut)) =
                        self.cached_entries.get_mut(edit_idx)
                    {
                        item_mut.value = SettingValue::HexColor(normalized.clone());
                    }
                    self.editing_index = None;
                    self.hex_input.clear();
                    // Defense-in-depth: skip any `__*` key (sentinel or future
                    // un-registered variant). Broader than `SentinelKind`
                    // on purpose — config writes should never name a sentinel.
                    if !key.is_empty() && !key.starts_with("__") {
                        // Route through `route_write` so a future
                        // `general.*` HexColor row short-circuits to
                        // `WriteGeneralSetting` instead of silently
                        // landing in config.toml.
                        return route_write(
                            key,
                            is_theme,
                            SettingValue::HexColor(normalized),
                            None,
                        );
                    }
                }
                SettingsAction::None
            }
            // These messages are only handled in sub-list mode or capture mode
            SettingsMessage::HotkeyCaptured(_, _) | SettingsMessage::SubListSearchChanged(_) => {
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
            SettingsMessage::SidebarDown => {
                self.sidebar_step(true, data);
                SettingsAction::None
            }
            SettingsMessage::SidebarUp => {
                self.sidebar_step(false, data);
                SettingsAction::None
            }
            SettingsMessage::SidebarSetOffset(offset, _) => {
                self.sidebar_set_index(offset, data);
                SettingsAction::None
            }
            SettingsMessage::SidebarClickItem(offset) => {
                self.sidebar_set_index(offset, data);
                SettingsAction::None
            }
            // Intercepted at the Nokkvi level in `handle_settings` so the
            // scroll task can read `cached_entries` directly; this arm
            // exists only for exhaustiveness.
            SettingsMessage::JumpToSection(_) => SettingsAction::None,
        }
    }

    /// Advance the sidebar cursor by one row (forward = next category) and
    /// update `active_category` in lockstep. Resets the detail-pane slot list
    /// when the category actually changes.
    fn sidebar_step(&mut self, forward: bool, data: &SettingsViewData) {
        let total = SettingsTab::ALL.len();
        if forward {
            self.sidebar_slot_list.move_down(total);
        } else {
            self.sidebar_slot_list.move_up(total);
        }
        self.apply_sidebar_index(data);
    }

    /// Set the sidebar cursor to a specific index and update `active_category`.
    fn sidebar_set_index(&mut self, index: usize, data: &SettingsViewData) {
        let total = SettingsTab::ALL.len();
        self.sidebar_slot_list.set_offset(index, total);
        self.apply_sidebar_index(data);
    }

    /// Read `sidebar_slot_list.viewport_offset` and synchronise
    /// `active_category` + detail pane state. No-op when the category is
    /// already current.
    fn apply_sidebar_index(&mut self, data: &SettingsViewData) {
        let idx = self
            .sidebar_slot_list
            .viewport_offset
            .min(SettingsTab::ALL.len() - 1);
        let new_tab = SettingsTab::ALL[idx];
        if new_tab == self.active_category {
            return;
        }
        self.active_category = new_tab;
        self.slot_list = SlotListView::new();
        self.editing_index = None;
        self.toggle_cursor = None;
        self.hex_input.clear();
        self.refresh_entries(data);
    }

    /// Route the two surviving reset sentinels to their confirmation dialogs.
    /// Per-color theme restores were removed — theme colors are edited directly
    /// in the theme TOML file, so there is no GUI "restore color group" path
    /// anymore. (The visualizer/hotkeys resets reset config values, not files.)
    pub(crate) fn handle_restore_defaults(&mut self, restore_key: &str) -> SettingsAction {
        match sentinel::SentinelKind::from_key(restore_key) {
            Some(sentinel::SentinelKind::RestoreAllHotkeys) => {
                SettingsAction::OpenResetHotkeysDialog
            }
            Some(sentinel::SentinelKind::RestoreVisualizer) => {
                SettingsAction::OpenResetVisualizerDialog
            }
            // Logout / unknown keys reach here only via the public API; ignore.
            _ => SettingsAction::None,
        }
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
            let is_theme = item.is_theme_key;

            // Monstercat snap: values in (0.0, MIN_EFFECTIVE) are a dead zone where the
            // filter amplifies instead of attenuating.  Snap based on direction:
            //   incrementing from 0.0 → jump to MIN_EFFECTIVE
            //   decrementing from MIN_EFFECTIVE → jump to 0.0
            if key == crate::visualizer_config::keys::MONSTERCAT {
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
                let description = self.cached_entries.get(edit_idx).and_then(|e| match e {
                    SettingsEntry::Item(item) => item.subtitle.as_deref().map(String::from),
                    SettingsEntry::Header { .. } => None,
                });
                return route_write(key, is_theme, new_value, description);
            }
        }
        SettingsAction::None
    }

    /// Populate cached entries from config data: either the cross-tab
    /// search results (when a query is active) or the active category's
    /// items. The persistent sidebar drives `active_category`; the detail
    /// pane renders what this populates.
    pub(crate) fn refresh_entries(&mut self, data: &SettingsViewData) {
        if !self.search_query.is_empty() {
            self.cached_entries = Self::search_all_entries(data, &self.search_query);
        } else {
            self.cached_entries = Self::build_category_sections(self.active_category, data);
        }
    }

    /// If the current viewport offset is on a header at Level 2, snap to the
    /// nearest non-header `Item` entry in the given direction.
    ///
    /// `forward == true`: prefer scanning forward, fall back to backward.
    /// `forward == false`: prefer scanning backward, fall back to forward.
    ///
    /// No-op when the current entry is already an `Item`.
    pub(crate) fn snap_to_non_header(&mut self, forward: bool) {
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
