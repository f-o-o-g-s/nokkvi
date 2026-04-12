---
trigger: glob
globs: src/widgets/**
---

# Widgets

## Global Atomics (in `src/theme.rs`)

- `ROUNDED_MODE` (AtomicBool): `is_rounded_mode()` / `set_rounded_mode()`. `ui_border_radius()` → 6.0 or 0.0. **ALWAYS use `ui_border_radius()` instead of hardcoded border radii.**
- `opacity_gradient` (AtomicBool): non-center slot opacity fade. Settings → Theme → Appearance.
- `slot_row_height` (AtomicU8): `SlotRowHeight` enum (Compact 50px, Default 70px, Comfortable 90px, Spacious 110px). Settings → Interface → Layout.
- **Transparent-border clipping**: Iced clips background to border radius even when border is transparent/0px. ALWAYS leave border radius unset on flush-to-edge bars.

## Player Bar (`player_bar.rs`)

Responsive culling at breakpoints: format info (1000px) → visualizer (920px) → SFX (840px) → consume (680px) → shuffle (600px) → repeat (520px). Scroll-to-volume on wheel. Horizontal volume mode stacks sliders. Progress Track metadata mode builds scrolling overlay + format info container.

## Progress Bar (`progress_bar.rs`)

Custom `iced::advanced` seekable widget. `Vec<OverlaySegment>` for scrolling colored metadata (2s pause, 30px/s, 80px loop gap). Handle rendered in separate `with_layer()` with expanded clip rect. Stale segments persist if not rebuilt after metadata toggle changes.

## Key Widgets

| Widget | File | Purpose |
|--------|------|---------|
| Volume Slider | `volume_slider.rs` | Vertical/horizontal volume, `SliderVariant` |
| View Header | `view_header.rs` | Sort mode selector, search bar, shuffle button, center-on-playing button, tooltips |
| Base Slot List | `base_slot_list_layout.rs` | Shared layout scaffolding, `base_slot_list_empty_state()` |
| Scroll Indicator | `scroll_indicator.rs` | Transient scrollbar overlay, `wrap_with_scroll_indicator()`, drag-to-seek |
| Hover Overlay | `hover_overlay.rs` | Per-slot hover darkening + press scale animation + external `flash_at()`. Default border radius = `ui_border_radius()` (theme-aware). |
| Track Info Strip | `track_info_strip.rs` | Now-playing metadata, used by player bar and top bar, `info_field_widget()` shared helper |
| Marquee Text | `marquee_text.rs` | Scrolling overflow text, generic over message type |
| Hover Indicator | `hover_indicator.rs` | Canvas-based hover underline, `HoverExpand` for hot zone expansion |
| Context Menu | `context_menu.rs` | Right-click menu. `LibraryContextEntry` / `QueueContextEntry` / `StripContextEntry` |
| Info Modal | `info_modal.rs` | Two-column property table for Get Info. `InfoModalItem` enum per type. |
| Text Input Dialog | `text_input_dialog.rs` | Modal with text input or confirmation mode. Save Queue uses `combo_box`. |
| EQ Slider | `eq_slider.rs` | Vertical ±15 dB slider for 10-band EQ |
| Drag Column | `drag_column.rs` | In-queue drag-and-drop reordering (supports multi-selection batch) |
| Format Info | `format_info.rs` | Audio format display (codec, sample rate, bitrate) |
| Hamburger Menu | `hamburger_menu.rs` | App menu (quit, light/dark toggle, about) |

## 3D Buttons

`three_d_button.rs` / `three_d_icon_button.rs`: single bordered quad when rounded, 5-quad 3D bevel when squared. `AnimatedPress` (`three_d_helpers.rs`) for press scale animation.

## Nav Bars

- **Top** (`nav_bar.rs`): tabs + format stats + hamburger menu. Metadata only shown for `TrackInfoDisplay::TopBar` mode. Progressive metadata collapsing (album <900, artist <750, title <600). Hover underlines via `HoverIndicator`.
- **Side** (`side_nav_bar.rs`): `NavLayout::Side`, `NavDisplayMode` (TextOnly/TextAndIcons/IconsOnly).

## Layout Constants (`slot_list.rs`)

Single source of truth: `chrome_height_with_header()`, `queue_slot_list_start_y()`, `NAV_BAR_HEIGHT` (32), `VIEW_HEADER_HEIGHT` (48), `TAB_BAR_HEIGHT` (36), `SLOT_SPACING` (3).

## Slot Rendering

`SlotListRowContext` bundles per-slot args. Center slot gets `flash_at`. Clickable stars via `slot_list_star_rating()`, clickable hearts via `slot_list_favorite_icon()`. Top-packing when items < slot_count. Multi-selection highlight: `selected_indices` set renders selected slots with center-highlight styling; suppressed during active Ctrl/Shift modifier hold.
**Aesthetics**: Uses `SlotListRowMetrics` to compute sizing offsets dynamically based on the active structural layout (e.g., `slot_row_height()`).

## SVG Icons (`embedded_svg.rs`)

**Top-level module** in `src/embedded_svg.rs` (not in `widgets/`). Compile-time embedded via `include_str!`. `get_svg(path)` maps paths → content. Unknown paths fall back to play icon with `warn!` log. `themed_logo_svg()` returns the Nokkvi logo with fills remapped to active theme colors (fg1, success, accent). To add: copy SVG → `assets/icons/`, add `const` + `include_str!`, add match arm in `get_svg()`, add to `KNOWN` list + test assertions.

## Critical Pattern

**HoverOverlay Wraps Containers.** WHEN using HoverOverlay, ALWAYS wrap a container (`mouse_area(HoverOverlay::new(container(...)))`) rather than building over a native button. See gotchas.
