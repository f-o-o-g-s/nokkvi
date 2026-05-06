---
trigger: glob
globs: src/widgets/**
---

# Widgets

## Global Theme State (`src/theme.rs`)

- **`DUAL_THEME` (`ArcSwap<ResolvedDualTheme>`)**: lock-free color reads (~12 ns/call). Color accessors do an atomic Arc clone — safe from any thread, including the visualizer.
- **`ROUNDED_MODE` (AtomicBool)**: `is_rounded_mode()` / `set_rounded_mode()`. `ui_border_radius()` → 6.0 or 0.0. ALWAYS use `ui_border_radius()` instead of hardcoded radii.
- **`opacity_gradient` (AtomicBool)**: non-center slot opacity fade.
- **`slot_row_height` (AtomicU8)** → `SlotRowHeight` enum: Compact 50, Default 70, Comfortable 90, Spacious 110.
- **Transparent-border clipping**: Iced clips background to border radius even with a 0px transparent border. Leave radius unset on flush-to-edge bars.

## Single-Active Overlay Menu (`Nokkvi.open_menu`)

Hamburger, player-bar kebab, view-header `checkbox_dropdown`, and right-click context menus are all **controlled** widgets — no local `is_open` state. Each widget bubbles `Message::SetOpenMenu(Option<OpenMenu>)` to root, which atomically replaces the current menu (so opening one closes any other). `OpenMenu` variants: `Hamburger`, `PlayerModes`, `CheckboxDropdown { view, trigger_bounds }`, `CheckboxDropdownSimilar { trigger_bounds }` (Similar lives in the browsing panel only and lacks a `View::Similar`), `Context { id: ContextMenuId, position }`. Auto-closes on `SwitchView` and `WindowResized`.

## Player Bar (`player_bar.rs`)

Adaptive layout via `PlayerBarLayout { kebab_mode_count, transports_collapsed }`. `compute_layout(width, prev)` applies per-mode hysteresis so modes fold into the kebab one at a time as the window narrows. `CULL_ORDER` (right-to-left): Visualizer, Crossfade, SFX, EQ, Consume, Shuffle, Repeat. `CULL_ENTER_WIDTHS` 1070→670 px, `CULL_HYSTERESIS_PX = 40`. Transport row collapses 5→3 buttons (prev / play-pause / next) at narrow widths.

Scroll-to-volume on wheel. Horizontal volume mode stacks sliders. Progress-track metadata mode builds the scrolling overlay + format-info container.

## Progress Bar (`progress_bar.rs`)

Custom `iced::advanced` seekable widget. `Vec<OverlaySegment>` for scrolling colored metadata (2 s pause, 30 px/s, 80 px loop gap). Handle rendered in a separate `with_layer()` with an expanded clip rect. Stale segments persist if not rebuilt after a metadata-toggle change.

## Key Widgets

| Widget | File | Purpose |
|--------|------|---------|
| Volume Slider | `volume_slider.rs` | Vertical/horizontal, `SliderVariant` |
| View Header | `view_header.rs` | Sort selector, search bar, shuffle, center-on-playing, columns dropdown |
| Base Slot List | `base_slot_list_layout.rs` | Shared layout scaffolding, `base_slot_list_empty_state()` |
| Scroll Indicator | `scroll_indicator.rs` | Transient scrollbar overlay, `wrap_with_scroll_indicator()`, drag-to-seek |
| Hover Overlay | `hover_overlay.rs` | Per-slot hover darkening + press scale + external `flash_at()`. Default radius = `ui_border_radius()` |
| Track Info Strip | `track_info_strip.rs` | Now-playing metadata (player bar + top bar + progress-track overlay). All three renderers share the `MetadataSegment` builder + `MetadataSegmentKind` enum |
| Marquee Text | `marquee_text.rs` | Scrolling overflow text, generic over message type |
| Hover Indicator | `hover_indicator.rs` | Canvas hover underline, `HoverExpand` for hot-zone expansion |
| Context Menu | `context_menu.rs` | Right-click menu widget + `LibraryContextEntry` and `StripContextEntry`. `QueueContextEntry` lives in `views/queue.rs` because its variants are queue-specific |
| Checkbox Dropdown | `checkbox_dropdown.rs` | Multi-checkbox column-visibility dropdown, generic over `Key` (controlled via `OpenMenu::CheckboxDropdown`) |
| Info Modal | `info_modal.rs` | Two-column property table for Get Info. `InfoModalItem` enum |
| Text Input Dialog | `text_input_dialog.rs` | Modal text input or confirmation. Save Queue uses `combo_box` |
| EQ Slider | `eq_slider.rs` | Vertical ±15 dB slider for 10-band EQ |
| Drag Column | `drag_column.rs` | In-queue drag-and-drop reorder (multi-selection batch aware) |
| Format Info | `format_info.rs` | Codec / bitrate split-string helper |
| Hamburger Menu | `hamburger_menu.rs` | App menu (quit, light/dark toggle, about) |
| Player Modes Menu | `player_modes_menu.rs` | Kebab-menu dropdown for culled mode toggles |
| Search Bar | `search_bar.rs` | Centralized search input with integrated clear |
| Link Text | `link_text.rs` | Hover-underlined clickable text (tight hitbox, accent on hover) |
| Metadata Pill | `metadata_pill.rs` | Composable artwork-panel metadata row builders |
| Artwork Split Handle | `artwork_split_handle.rs` | Draggable separator for artwork-column width |
| Default Playlist Chip | `default_playlist_chip.rs` | Pin-icon button in the Playlists/Queue header — opens the picker |
| Default Playlist Picker | `default_playlist_picker.rs` | Modal overlay (font-picker pattern) to pick the default playlist; state lives on `Nokkvi.default_playlist_picker` |
| Boat | `boat.rs` | Surfing-boat overlay for lines-mode visualizer. CPU-only — reads the shared bar buffer the shader already consumes. Physics is music-driven (cruise scales with spectrum presence, thrust stacks above cruise, anchor doodad drops on silence). Sprite + anchor are themed via `embedded_svg::themed_boat_svg` / `themed_anchor_svg` using the active visualizer `border_color` |

## 3D Buttons

`three_d_button.rs` / `three_d_icon_button.rs`: single bordered quad in rounded mode, 5-quad bevel in squared mode. `AnimatedPress` (`three_d_helpers.rs`) for press scale animation.

## Nav Bars

- **Top** (`nav_bar.rs`): tabs + format stats + hamburger. Metadata only when `TrackInfoDisplay::TopBar`. Progressive collapsing (album <900, artist <750, title <600). `HoverIndicator` underlines.
- **Side** (`side_nav_bar.rs`): vertical sidebar. `NavDisplayMode` { TextOnly, TextAndIcons, IconsOnly }.
- **None** layout: no nav chrome — only the active page + player bar render (minimalist mode).

## Layout Constants (`slot_list.rs`)

Single source of truth: `chrome_height_with_header()`, `queue_slot_list_start_y()`, `NAV_BAR_HEIGHT = 32`, `VIEW_HEADER_HEIGHT = 48`, `TAB_BAR_HEIGHT = 32`, `SLOT_SPACING = 3`. Slot count is computed dynamically: always odd, capped at `MAX_SLOT_COUNT = 29`.

## Slot Rendering

`SlotListRowContext` bundles per-slot args. `SlotListRowMetrics` derives sizes from active `slot_row_height()`. Center slot gets `flash_at`. Clickable stars (`slot_list_star_rating()`) and hearts (`slot_list_favorite_icon()`). Top-packing when items < slot_count. Multi-selection highlight via `selected_indices`; suppressed during active Ctrl/Shift modifier hold.

## SVG Icons (`embedded_svg.rs`)

Top-level module. The lookup table is **generated by `build.rs`** from the contents of `assets/icons/` — adding/removing an icon is a one-step change (drop or remove the file, rebuild). Unknown paths return `play.svg` with a warn log. `themed_logo_svg()` rewrites the Nokkvi logo's hex fills to the active theme (fg1, success, accent).

## Critical Pattern

**HoverOverlay wraps containers, not native buttons.** Buttons capture `ButtonPressed` early. Pattern: `mouse_area(HoverOverlay::new(container(...))).on_press(msg)`.
