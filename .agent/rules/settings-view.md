---
trigger: glob
globs: src/views/settings/**,src/update/settings.rs
---

# Settings View

## Module Structure

```
views/settings/
├── mod.rs               — SettingsPage state, SettingsMessage, SettingsAction, update, view
├── entries.rs           — Entry building and filtering: level builders, cross-tab search
├── items.rs             — SettingValue types, SettingMeta + meta! macro, shared helpers
├── items_general.rs     — General tab item builders (Application, Mouse Behavior, Account, Cache)
├── items_interface.rs   — Interface tab item builders (Layout, Metadata Strip)
├── items_playback.rs    — Playback tab item builders (Playback, Scrobbling, Playlists)
├── items_hotkeys.rs     — Hotkeys tab item builders (per-category hotkey entries)
├── items_theme.rs       — Theme tab item builders (font, colors, presets, opacity gradient)
├── items_visualizer.rs  — Visualizer tab item builders (bars, peaks, LED, 3D, gradient, lines)
├── sub_lists.rs         — Sub-slot-list handling: font picker, color gradient editor
├── presets.rs           — 10 embedded preset themes applied inline (no separate sub-slot-list)
├── rendering.rs         — Slot rendering: headers, items, color sub-slot-list, presets, hotkey badges, toggle sets, row separators
└── view.rs              — Layout: breadcrumb/search bar, footer, font modal overlay, exit button
```

## Settings Architecture

- 6 tabs: **General, Interface, Playback, Hotkeys, Theme, Visualizer**
- **Two-level drill-down navigation** (not accordion):
  - Level 1 (`CategoryPicker`): one header per tab — Enter drills into the selected category
  - Level 2 (`Category`): all items within a tab, grouped under auto-expanded section headers (non-interactive separators)
- `NavLevel` enum tracks position; `nav_stack` maintains drill-down history with cursor memory
- Navigation skips non-selectable headers automatically (`snap_to_non_header`)
- **Breadcrumb navigation** shows location path: Tab › Section › Sub-item
- **Row separators**: bottom border lines separate rows visually in Level 2

## Search / Filter

- Cross-tab search: when a query is active, entries from all 6 tabs are combined and filtered
- Tab-name matching: if a tab name matches the query, all its entries are included
- `SETTINGS_SEARCH_INPUT_ID` is separate from per-view search IDs
- **Search navigation pitfall**: `SlotListDown` must **not** rebuild entries — navigate within `cached_entries`; entries are only rebuilt on `SearchChanged`
- **Exit button**: footer has a clickable X exit button (StepMania-style)

## SettingValue Types

| Type | Interaction |
|------|-------------|
| `Float` / `Int` | ←/→ increment/decrement with step + clamp. Arrow buttons are clickable. |
| `Bool` | Toggle. Clickable "On"/"Off" badges. |
| `Enum` | Cycle. Center slot shows all options as clickable badges (`EditSetValue`). |
| `ToggleSet` | Multi-select badges. Each badge independently toggleable via `ToggleSetToggle(key)`. `Vec<(label, key, enabled)>`. Keyboard navigation: ←/→ moves cursor between badges, Enter toggles cursored badge, EditUp/EditDown (↑/↓) sets badge on/off. `toggle_set_cursor_index` tracks active cursor (center row only). |
| `HexColor` | Direct hex input |
| `ColorArray` | Opens sub-slot-list for gradient editing |
| `Text` | Read-only (or editable via TextInputDialog for paths) |
| `Hotkey` | Badge display + key capture mode |

`SettingMeta` struct + `meta!` macro for concise item definitions. Key is `Cow<'static, str>`.

## General Tab

4 sections persisted to `config.toml` (hot-reloadable) via `SettingsManager`: **Application** (start view, enter behavior, local music path, verbose configuration mode), **Mouse Behavior** (stable viewport, auto-follow playing), **Account** (read-only server URL + username, logout), **Cache** (rebuild artwork/artist cache action buttons). 11 items total.

## Interface Tab

2 sections persisted to `config.toml` via `SettingsManager`: **Layout** (nav layout, nav display, track info display, row density, horizontal volume controls), **Metadata Strip** (visible fields as `ToggleSet`, click action enum).

`TrackInfoDisplay` enum: `Off | Player Bar | Top Bar | Progress Track`. The `ProgressTrack` variant shows scrolling metadata overlay + format info container on the progress bar track instead of a separate strip. These four modes are **mutually exclusive**.

`slot_row_height` is a `SlotRowHeight` enum (Compact/Default/Comfortable/Spacious) — not a numeric slider. Each variant maps to a fixed pixel height (50/70/90/110px).

Strip visibility toggles (`strip_show_title`, `strip_show_artist`, `strip_show_album`, `strip_show_format_info`) and click action use theme atomics for immediate UI response. **These toggles affect both the metadata strip AND the progress track overlay** — toggling off a field hides it from whichever display mode is active. `ToggleSetToggle` message flips the cached entry and emits `WriteGeneralSetting` with the individual field's key.

## Playback Tab

3 sections persisted to `config.toml` via `SettingsManager`: **Playback** (crossfade enabled, crossfade duration, volume normalization toggle, normalization level), **Scrobbling** (enabled toggle, threshold — percentage-based, no 4-minute floor), **Playlists** (quick add to playlist toggle, default playlist name — set via right-click context menu on playlists).

Volume normalization uses rodio's Automatic Gain Control (AGC). `NormalizationLevel` enum (Quiet/Normal/Loud) maps to AGC target levels (0.6/1.0/1.4).

## Font Picker

- Sub-slot-list rendered as a **modal overlay** (not a drill-down level)
- System font discovery via `font-kit` (`data/src/services/font_discovery.rs`, `LazyLock`-cached)
- Themed search bar (`FontSearchChanged` message)

## Hotkey Capture Mode

1. User activates hotkey item (Enter) → enters capture mode
2. Next key press is recorded as new binding (saved to `config.toml` using ASCII names e.g. `"Shift + DownArrow"`, `"Ctrl + R"`)
3. Escape cancels capture, Delete resets to default
4. **Steal-on-conflict**: if the new combo is already bound, it steals the binding — "Swapped with {action}" label shows (auto-dismissed after 2s)

### Reserved Actions

`HotkeyAction::Escape` and `ResetToDefault` are in `RESERVED` (not `ALL`) — hardcoded bindings, don't appear in the hotkey editor.

### Notable Hotkeys

| Hotkey | Action |
|--------|--------|
| `Ctrl+S` | `SaveQueueAsPlaylist` — opens save dialog |
| `Ctrl+E` | Toggle browsing panel from Queue view |
| `Shift+↑/↓` | Reorder queue tracks |
| `Tab` | Switch pane focus (queue ↔ browser) during split view |
| `↑` | `EditUp` — toggle setting on / enable ToggleSet field |
| `↓` | `EditDown` — toggle setting off / disable ToggleSet field |

## Confirmation Dialogs

- **Visualizer reset** and **hotkey reset** use `TextInputDialog` in confirmation mode (no text input — just confirm/cancel)
- Visualizer restore preserves user-modified colors when restoring non-color defaults

## Preset Themes

- 10 compile-time-embedded themes in `presets.rs`
- Displayed as inline entries in the main settings slot list
- Atomic apply: replaces all theme + visualizer settings at once

## Config Write-Back

- **ALL settings** (theme, visualizer, general, playback, hotkeys): write to `config.toml` via `SettingsManager`.
  - `verbose_config` toggle (Settings → General → Application): when enabled, `write_full_theme_and_visualizer()` writes all settings including defaults; when disabled, `strip_to_sparse()` removes default-value entries.
  - Toggling verbose_config triggers a combined persist + TOML write in a single async task to avoid race conditions.
  - Hot-reload picks up changes automatically.
- **`theme.light_mode`**: written to TOML, triggers theme reload
- Passwords remain in `redb`.

## Icons

Each `SettingItem` and `SettingsEntry::Header` carries an `icon: &'static str` path. Resolved at runtime by `embedded_svg::get_svg()`. **New icon paths MUST be registered in `src/embedded_svg.rs`** — unregistered paths silently render as the play icon fallback.
