---
trigger: glob
globs: src/widgets/**
---

# Widgets

## Global Theme State (`src/theme/` — `state.rs`, `ui_mode.rs`, `radius.rs`, `colors.rs`, `font.rs`, `style.rs`; all re-exported as `theme::X` from `mod.rs`)

- **`DUAL_THEME` (`ArcSwap<ResolvedDualTheme>`)**: lock-free color reads (~12 ns/call). Color accessors do an atomic Arc clone — safe from any thread, including the visualizer.
- **`rounded_mode` (AtomicU8)** → `RoundedMode` enum: `Off` / `On` / `PlayerOnly` (tri-state, not a bool). `rounded_mode()` reads it; `is_rounded_mode()` is true only for `On`; `set_rounded_mode(RoundedMode)` stores it. `ui_border_radius()` → 6.0 or 0.0 (gated on `is_rounded_mode()`), while `ui_border_radius_player()` also rounds for `PlayerOnly` so the player chrome stays soft when the rest of the UI is flat. ALWAYS use these helpers instead of hardcoded radii.
- **`opacity_gradient` (AtomicBool)**: non-center slot opacity fade.
- **`slot_row_height` (AtomicU8)** → `SlotRowHeight` enum: Compact 50, Default 70, Comfortable 90, Spacious 110.
- **Transparent-border clipping**: Iced clips background to border radius even with a 0px transparent border. Leave radius unset on flush-to-edge bars.

## Single-Active Overlay Menu (`Nokkvi.open_menu`)

Hamburger, player-bar kebab, view-header `checkbox_dropdown`, right-click context menus, and the nav-bar library-filter popover are all **controlled** widgets — no local `is_open` state. Each widget bubbles `Message::SetOpenMenu(Option<OpenMenu>)` to root, which atomically replaces the current menu (so opening one closes any other). `OpenMenu` variants: `Hamburger`, `PlayerModes`, `CheckboxDropdown { view, trigger_bounds }`, `CheckboxDropdownSimilar { trigger_bounds }` (browsing-panel-only Similar columns dropdown — has no matching `View` variant), `Context { id, position }`, `LibrarySelector { trigger_bounds }` (multi-library filter popover anchored under the nav-bar trigger). Auto-closes on `SwitchView` and `WindowResized`.

Dismissal goes through `widgets::menu_dismiss::handle_dismiss()`: Escape closes with the event captured; an outside press closes **without** capturing, so the press still reaches a different menu's trigger in the widget tree (the trigger's open emit arrives after the close in iced's overlays-before-widget-tree dispatch order and wins — the click-to-switch UX). Each overlay supplies its own outside-press predicate (`press_began()` + bounds test for hamburger/kebab/context; `checkbox_dropdown` matches mouse presses only and exempts its trigger rect) — those predicate differences are historical behavior, keep them per-site.

## Menu Shadow Halo (`menu_constants.rs`)

Every custom `overlay::Overlay` impl that draws a `MENU_SHADOW`-bearing quad must inflate its `layout::Node` so the halo survives Iced's per-overlay `with_layer(layout.bounds(), …)` scissor (`core/src/overlay/nested.rs`). Use the helpers in `widgets::menu_constants`:

- **Leaf overlays** (`hamburger_menu`, `player_modes_menu` — draw everything via `renderer.fill_quad`, host no child `Element`): produce the inflated node via `inflate_for_shadow(size, position)`; recover the visible rect via `visible_menu_bounds(layout.bounds())`.
- **Child-forwarding overlays** (`checkbox_dropdown`, `context_menu` — host a real child `Element` that needs its own coordinate space): produce via `inflate_for_shadow_around_child(node, position)`; recover via `visible_menu_layout(layout)` (returns the inner child `Layout` to forward to the hosted widget).

`MENU_SHADOW_PADDING` is module-private by design — new overlays use the helpers, not the raw constant. The four `const _: () = assert!(…)` invariants in `menu_constants.rs` pin the shadow geometry (padding covers worst-axis extent, offset stays vertical-only and non-negative); tuning `MENU_SHADOW` past those bounds yields a compile error pointing at the assertion to update.

## Player Bar (`player_bar.rs`)

Adaptive layout via `PlayerBarLayout { kebab_mode_count, wide_for_three_section }`, both width-driven + hysteretic in `compute_layout(width, prev)`. Modes fold into the kebab one at a time as the window narrows. `CULL_ORDER` (right-to-left): Visualizer, Crossfade, SFX, EQ, Consume, Shuffle, Repeat. `CULL_ENTER_WIDTHS` 1070→670 px, `CULL_HYSTERESIS_PX = 40` (the two arrays are index-coupled and pinned by a `const _: () = assert!(len == len)` interlock). Transports are always the modern 3-button set (prev / play-or-pause toggle / next); there is no dedicated Stop button (Stop stays reachable via MPRIS + the `nokkvi stop` CLI/IPC verb). `effective_player_bar_layout()` is a render-only override of `compute_layout`'s output (it never recomputes the regime).

Scroll-to-volume on wheel (both music + SFX sliders publish a delta via `on_scroll`). Horizontal volume mode stacks sliders. `Nokkvi::mini_player_artwork()` is the gated resolver that surfaces the cached large-artwork handle only in `MiniPlayer` mode.

**`MiniPlayer` mode** renders a full-width "capsule" seek scrub (a `filled` progress bar, no handle, color-aware overlaid elapsed/duration + dimmed codec/bitrate end-caps via `capsule_scrub_labels` → `CapLabel`) framed by 1 px separators, above a responsive content row. `wide_for_three_section` (hysteretic band `MINI_THREE_SECTION_ENTER`/`EXIT`, deliberately BELOW the cull range so modes cull while the layout stays centered) selects: WIDE = three-section `[metadata (Fill, Start) | transports (Shrink, centered by equal Fill siblings) | modes+volume (Fill, End)]` with modes expanded/culling like the normal bar; COMPACT = single-cluster `[metadata | transports | divider | kebab | volume]` with all modes force-folded. Volume is rightmost in both. Bar height (`MINI_PLAYER_BAR_HEIGHT = 78`) is regime-independent. Per-control visibility = `mini_player_show_volume()` / `mini_player_show_modes()` (a MiniPlayer-only "Visible Controls" ToggleSet).

## Progress Bar (`progress_bar.rs`)

Custom `iced::advanced` seekable widget. Track + handle rendered in separate `with_layer()` passes so the tooltip and handle survive the per-overlay scissor. Drag-release publishes `Seek(progress * duration)` once; in-flight position keeps the handle smooth via `last_position + elapsed` interpolation. `filled(true)` switches to the MiniPlayer capsule look — a full-height track that is structurally handle-less (the handle never draws in filled mode) and carries `time_labels(CapLabel, CapLabel)` end-caps. `interactive(false)` (radio) disables seeking and is decoupled from `hide_handle`.

## Key Widgets

| Widget | File | Purpose |
|--------|------|---------|
| Volume Slider | `volume_slider.rs` | Vertical/horizontal, `SliderVariant` |
| View Header | `view_header.rs` | Sort selector, search bar, shuffle, center-on-playing, columns dropdown |
| Base Slot List | `base_slot_list_layout.rs` | Shared layout scaffolding, `base_slot_list_empty_state()` |
| Scroll Indicator | `scroll_indicator.rs` | Transient scrollbar overlay, `wrap_with_scroll_indicator()`, drag-to-seek |
| Hover Overlay | `hover_overlay.rs` | Per-slot hover darkening + press scale + external `flash_at()`. Default radius = `ui_border_radius()` |
| Track Info Strip | `track_info_strip.rs` | Now-playing metadata. `build_now_playing_segments` returns `Vec<String>` (title / artist / album fragments + separators) that the merged-mode marquee concats |
| Marquee Text | `marquee_text.rs` | Scrolling overflow text, generic over message type |
| Context Menu | `context_menu.rs` | Right-click menu. `LibraryContextEntry` / `QueueContextEntry` / `StripContextEntry`. Wrap helpers: `wrap_library_row` / `wrap_similar_row` (slot rows), `wrap_strip_context_menu` (the three now-playing strip placements — player-bar, top strip, merged nav-bar; takes `StripContextAction` / `SetOpenMenu` variant constructors as fn pointers, returns the bare strip when radio is active) |
| Checkbox Dropdown | `checkbox_dropdown.rs` | Multi-checkbox column-visibility dropdown, generic over `Key` (controlled via `OpenMenu::CheckboxDropdown`) |
| Info Modal | `info_modal.rs` | Two-column property table for Get Info. `InfoModalItem` enum |
| About Modal | `about_modal.rs` | Metadata + diagnostics, theme-adaptive logo, Ko-fi tip link |
| Text Input Dialog | `text_input_dialog.rs` | Modal text input or confirmation. Save Queue uses `combo_box` |
| EQ Slider | `eq_slider.rs` | Vertical ±15 dB slider for 10-band EQ |
| EQ Modal | `eq_modal.rs` | 10-band EQ overlay with preset picker (`update/eq_modal.rs`). State lives on `Nokkvi.eq_modal: EqModalState` (extracted as a sibling struct so the EQ overlay doesn't drift WindowState fields) |
| Slot List Page | `slot_list_page.rs` | `SlotListPageState` + unified `SlotListPageMessage` dispatcher |
| Slot List View | `slot_list_view.rs` | Scroll-position state owned by the view (decoupled from `SlotListPageState`) |
| Visualizer | `visualizer/` | Pipeline + shader + wgsl modules (see `.agent/rules/visualizer.md`) |
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
| Library Filter Trigger | `library_filter_trigger.rs` | Nav-bar button anchoring the multi-library selector popover. Renders a count badge via `badge_pip::draw_badge_pip` when a subset is active. Auto-hidden on single-library servers. `FILTERED_CHASSIS_WIDTH` const pins the filtered render's wider chassis |
| Badge Pip | `badge_pip.rs` | Tiny "active-state" pip drawn in the top-right of an icon button. Shared between the kebab `player_modes_menu` and `library_filter_trigger` |
| Boat | `boat.rs` (+ `boat_physics.rs` / `boat_tests.rs`) | Surfing-boat overlay for lines-mode visualizer. CPU-only — reads the shared bar buffer the shader already consumes |
| Menu Chrome | `menu_chrome.rs` | Shared overlay-menu vocabulary: `fill()`, `border()`, `container_style()` accessors consumed by the four overlay menus (hamburger / player_modes / checkbox_dropdown / context_menu) so the `bg1 + border + ui_radius_md + MENU_SHADOW` recipe lives at one site |
| Menu Dismiss | `menu_dismiss.rs` | Shared overlay-menu dismissal: `handle_dismiss()` (Escape closes + captures; outside press closes WITHOUT capturing — the click-to-switch invariant lives + is unit-pinned here) and `press_began()`; each overlay passes its own outside-press predicate |
| Slider Drag | `slider_drag.rs` | Shared drag machinery for the settings/volume/EQ sliders: `project_fraction()` (axis projection with half-handle inset; `handle = 0.0` for the vertical volume level-meter), `SliderDragState` (press/drag/release with `>=`-threshold move gate and strictly-`>` trailing release gate), `grab_interaction()`. Deliberately Shell-free — each slider keeps its own publish/capture/redraw (settings captures on release; volume/EQ do not) and its own `State` type (tree::Tag identity) |
| Modal Button | `modal_button.rs` | `modal_icon_button(icon, size, on_press)` — the shared `mouse_area(HoverOverlay(container(svg)))` chassis used by About / Info modal headers |
| Pill Segmented Button | `pill_segmented_button.rs` | Horizontal chip group used by Settings Bool / Enum / ToggleSet widgets. Flat 1 px outline + `theme::bg0()` fill in flat mode; `ui_radius_pill()` corners in rounded mode; selected chip uses `accent_bright()` fill |

## Modal Frame Style

`theme::modal_frame_style(theme)` returns the `container::Style` for every overlay modal panel — `bg0_hard()` fill, 1 px `accent_bright()` outline, `ui_radius_lg()` corners. Routed by `about_modal`, `info_modal`, `eq_modal`, `text_input_dialog`, and `default_playlist_picker` so a future tweak (e.g. switching the outline onto `border()` for a chrome-quiet variant) lands at one site.

## Nav Bars

- **Top** (`nav_bar.rs`): tabs + format stats + hamburger. Metadata only when `TrackInfoDisplay::TopBar`. Progressive collapsing (album <900, artist <750, title <600). `flat_tab_container_style` paints the full-cell `accent_bright()` active fill; the right-edge indicator strip from the pre-redesign was removed. `NAV_TABS` is the single source of truth for which tabs render — `NAV_TABS[i] == NavView::ALL[i]` is pinned by a runtime test.
- **Side** (`side_nav_bar.rs`): vertical sidebar. `NavDisplayMode` { TextOnly, TextAndIcons, IconsOnly }. Same active-fill recipe as the top nav.
- **None** layout: no nav chrome — only the active page + player bar render (minimalist mode).

## Layout Constants (`slot_list.rs`)

Single source of truth: `chrome_height_with_header()`, `theme::nav_bar_height()` (32 flat / 44 rounded — the old `NAV_BAR_HEIGHT` const is gone), `view_header_chrome()` (derives from `view_header::HEADER_HEIGHT = 50` + 1 px separator — replaces the old `VIEW_HEADER_HEIGHT` const), `TAB_BAR_HEIGHT = 32`, `SLOT_SPACING = 0` (flat redesign: rows touch). Slot count is computed dynamically: always odd, capped at `MAX_SLOT_COUNT = 29`. Cross-pane drag uses structural cursor → slot resolution via per-slot `mouse_area` (see `slot_list.rs::SlotHoverCallback`) rather than chrome math, so there is no `queue_slot_list_start_y` helper.

## Slot Rendering

`SlotListRowContext` bundles per-slot args. `SlotListRowMetrics` derives sizes from active `slot_row_height()`. Center slot gets `flash_at`. Clickable stars (`slot_list_star_rating()`) and hearts (`slot_list_favorite_icon()`). Top-packing when items < slot_count. Multi-selection highlight via `selected_indices`; suppressed during active Ctrl/Shift modifier hold.

**Always derive static-icon color via `slot_list_static_icon_color(style, fallback, opacity)`** when embedding a tinted SVG / text / pill inside a row renderer (lock glyphs, sub-index labels, empty heart/star outlines, radio-tower icons, etc.). The helper returns `bg0_hard()` on dark-text rows (selected / highlighted / centered) and the `fallback` color (with `opacity` applied to its alpha) otherwise, so the icon stays readable against the light selected-row fill in lockstep with the row's text. Hardcoding a `theme::fg*()` color in a row renderer breaks contrast under selection.

## SVG Icons (`embedded_svg.rs`)

Top-level module. The lookup table is **generated by `build.rs`** from the contents of `assets/icons/` — adding/removing an icon is a one-step change (drop or remove the file, rebuild). Unknown paths return `play.svg` with a warn log. `themed_logo_svg()` rewrites the Nokkvi longship logo's hex fills to the active theme via three role accessors: body (sail + hull) → `logo_body()` (fg0), shields → `logo_shields()` (accent), wood (mast + yard) → `logo_wood()` (warning). All read the theme's dark palette regardless of light/dark mode, so the mark is mode-stable.

## HoverOverlay + native buttons

`HoverOverlay::new(button)` works in some places — e.g. `player_bar.rs:674` (inside `player_control_button`) actively wraps a `button` and the hover/press visual fires correctly because commit `d2f22a0` added `shell.request_redraw()` to `HoverOverlay::update`. The canonical pattern is still `mouse_area(HoverOverlay::new(container(...))).on_press(msg)` for clickable cells (slot rows, header icons, modal-icon buttons), but the absolute "never wraps a button" framing in older notes is too strict.
