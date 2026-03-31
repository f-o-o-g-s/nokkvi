---
trigger: glob
globs: src/views/settings/**,src/update/settings.rs
---

# Settings View

## Module Structure

```
views/settings/
├── mod.rs             — State, Message, Action, update, view
├── entries.rs         — Entry building/filtering, cross-tab search
├── items.rs           — SettingValue types, SettingMeta + meta! macro
├── items_general.rs   — Application, Mouse, Account, Cache
├── items_interface.rs — Layout, Metadata Strip
├── items_playback.rs  — Playback, Scrobbling, Playlists
├── items_hotkeys.rs   — Per-category hotkey entries
├── items_theme.rs     — Font, colors, presets, opacity gradient
├── items_visualizer.rs — Bars, peaks, LED, 3D, gradient, lines
├── sub_lists.rs       — Font picker, color gradient editor sub-slot-lists
├── presets.rs         — Theme discovery/application
├── rendering.rs       — Slot rendering: headers, items, colors, hotkey badges, toggle sets
└── view.rs            — Layout: breadcrumb/search bar, footer, exit button
```

## Architecture

- 6 tabs: **General, Interface, Playback, Hotkeys, Theme, Visualizer**
- Two-level drill-down: Level 1 (CategoryPicker) → Level 2 (Category items with auto-expanded section headers)
- `NavLevel` enum, `nav_stack` with cursor memory, `snap_to_non_header` skips non-selectable headers
- Cross-tab search: active query combines entries from all tabs; tab-name matching includes all of a tab's entries

## SettingValue Types

| Type | Interaction |
|------|-------------|
| `Float` / `Int` | ←/→ increment/decrement with clickable arrow buttons |
| `Bool` | Toggle with clickable On/Off badges |
| `Enum` | Cycle with clickable option badges (`EditSetValue`) |
| `ToggleSet` | Multi-select badges. ←/→ cursor, Enter toggles, ↑/↓ sets on/off. `toggle_set_cursor_index` tracks cursor. |
| `HexColor` | Direct hex input |
| `ColorArray` | Opens sub-slot-list for gradient editing |
| `Hotkey` | Badge display + key capture mode (Escape cancels, Delete resets, steal-on-conflict) |

## Key Patterns

- **Config write routing**: Application/Playback/Interface settings → `WriteGeneralSetting` action → `config.toml`. Theme settings → `update_theme_value()` → active theme file. Don't misroute.
- **`verbose_config` toggle**: combined persist + TOML write in single async task to avoid races.
- **Strip visibility toggles**: affect both metadata strip AND progress track overlay. `ToggleSetToggle` flips cached entry + emits `WriteGeneralSetting`.
- **Font picker**: modal overlay sub-slot-list (not drill-down). System fonts via `font-kit`, `LazyLock`-cached.
- **Theme picker**: modal sub-slot-list. Switching rewrites `theme = "name"` in config.toml, triggers hot-reload.
- **Search pitfall**: `SlotListDown` must not rebuild entries — navigate within `cached_entries`; only `SearchChanged` rebuilds.
- **Icons**: `SettingItem` + `SettingsEntry::Header` carry `icon` path. Must be registered in `src/embedded_svg.rs` or silently falls back to play icon.
