---
paths:
  - "src/widgets/**"
  - "src/theme/**"
  - "src/embedded_svg.rs"
---

# Widgets

## Global Theme State (`src/theme/` — `state.rs`, `ui_mode.rs`, `radius.rs`, `colors.rs`, `font.rs`, `style.rs`; all re-exported as `theme::X` from `mod.rs`)

- **`DUAL_THEME` (`ArcSwap<ResolvedDualTheme>`)**: lock-free color reads (~12 ns/call). Color accessors do an atomic Arc clone — safe from any thread, including the visualizer.
- **`rounded_mode` (AtomicU8)** → `RoundedMode` enum: `Off` / `On` / `PlayerOnly` (tri-state, not a bool). `rounded_mode()` reads it; `is_rounded_mode()` is true only for `On`; `set_rounded_mode(RoundedMode)` stores it. `ui_border_radius()` → 6.0 or 0.0 (gated on `is_rounded_mode()`), while `ui_border_radius_player()` also rounds for `PlayerOnly` so the player chrome stays soft when the rest of the UI is flat. ALWAYS use these helpers instead of hardcoded radii.
- **`opacity_gradient` (AtomicBool)**: non-center slot opacity fade.
- **`slot_row_height` (AtomicU8)** → `SlotRowHeight` enum: Compact 50, Default 70, Comfortable 90, Spacious 110.

## Single-Active Overlay Menu (`Nokkvi.open_menu`)

Hamburger, player-bar kebab, view-header `checkbox_dropdown`, right-click context menus, and the nav-bar library-filter popover are all **controlled** widgets — no local `is_open` state. Each widget bubbles `Message::SetOpenMenu(Option<OpenMenu>)` to root, which atomically replaces the current menu (so opening one closes any other). `OpenMenu` variants: `Hamburger`, `PlayerModes`, `CheckboxDropdown { view, trigger_bounds }`, `CheckboxDropdownSimilar { trigger_bounds }` (browsing-panel-only Similar columns dropdown — has no matching `View` variant), `QueueSync { trigger_bounds }` (Queue-only server-sync push/pull action menu, built via `checkbox_dropdown::action_dropdown`), `Context { id, position }`, `LibrarySelector { trigger_bounds }` (multi-library filter popover anchored under the nav-bar trigger). Auto-closes on `SwitchView` and `WindowResized`.

Dismissal goes through `widgets::menu_dismiss::handle_dismiss()`: Escape closes with the event captured; an outside press closes **without** capturing, so the press still reaches a different menu's trigger in the widget tree (the trigger's open emit arrives after the close in iced's overlays-before-widget-tree dispatch order and wins — the click-to-switch UX). Each overlay supplies its own outside-press predicate (`press_began()` + bounds test for hamburger/kebab/context; `checkbox_dropdown` matches mouse presses only and exempts its trigger rect) — those predicate differences are historical behavior, keep them per-site.

## Menu Shadow Halo (`menu_constants.rs`)

Every custom `overlay::Overlay` impl that draws a `MENU_SHADOW`-bearing quad must inflate its `layout::Node` so the halo survives Iced's per-overlay `with_layer(layout.bounds(), …)` scissor (`core/src/overlay/nested.rs`). Use the helpers in `widgets::menu_constants`:

- **Leaf overlays** (`hamburger_menu`, `player_modes_menu` — draw everything via `renderer.fill_quad`, host no child `Element`): produce the inflated node via `inflate_for_shadow(size, position)`; recover the visible rect via `visible_menu_bounds(layout.bounds())`.
- **Child-forwarding overlays** (`checkbox_dropdown`, `context_menu` — host a real child `Element` that needs its own coordinate space): produce via `inflate_for_shadow_around_child(node, position)`; recover via `visible_menu_layout(layout)` (returns the inner child `Layout` to forward to the hosted widget).

`MENU_SHADOW_PADDING` is module-private by design — new overlays use the helpers, not the raw constant. The `const _: () = assert!(…)` invariants in `menu_constants.rs` pin the shadow geometry (padding covers worst-axis extent, offset stays vertical-only and non-negative); tuning `MENU_SHADOW` past those bounds yields a compile error pointing at the assertion to update.

## Player Bar (`player_bar.rs`)

Adaptive layout via `PlayerBarLayout { kebab_mode_count, wide_for_three_section }`, both width-driven + hysteretic in `compute_layout(width, prev)`. Modes fold into the kebab one at a time as the window narrows. `CULL_ORDER` (right-to-left, `[ModeId; 8]`): Visualizer, Crossfade, BitPerfect, SFX, EQ, Consume, Shuffle, Repeat. `CULL_ENTER_WIDTHS` 1070→670 px, `CULL_HYSTERESIS_PX = 40` (the two arrays are index-coupled and pinned by a `const _: () = assert!(len == len)` interlock). Transports are always the modern 3-button set (prev / play-or-pause toggle / next); there is no dedicated Stop button (Stop stays reachable via MPRIS + the `nokkvi stop` CLI/IPC verb). `effective_player_bar_layout()` is a render-only override of `compute_layout`'s output (it never recomputes the regime).

Scroll-to-volume on wheel (both music + SFX sliders publish a delta via `on_scroll`). Horizontal volume mode stacks sliders. `Nokkvi::mini_player_artwork()` is the gated resolver that surfaces the cached large-artwork handle only in `MiniPlayer` mode.

**`MiniPlayer` mode** renders a full-width "capsule" seek scrub (a `filled` progress bar, no handle, color-aware overlaid elapsed/duration + dimmed codec/bitrate end-caps via `capsule_scrub_labels` → `CapLabel`) framed by 1 px separators, above a responsive content row. `wide_for_three_section` (hysteretic band `MINI_THREE_SECTION_ENTER`/`EXIT`, deliberately BELOW the cull range so modes cull while the layout stays centered) selects: WIDE = three-section `[metadata (Fill, Start) | transports (Shrink, centered by equal Fill siblings) | modes+volume (Fill, End)]` with modes expanded/culling like the normal bar; COMPACT = single-cluster `[metadata | transports | divider | kebab | volume]` with all modes force-folded. Volume is rightmost in both. Bar height (`MINI_PLAYER_BAR_HEIGHT = 78`) is regime-independent. Per-control visibility = `mini_player_show_volume()` / `mini_player_show_modes()` (a MiniPlayer-only "Visible Controls" ToggleSet).

## Progress Bar (`progress_bar.rs`)

Custom `iced::advanced` seekable widget. The handle draws in its own `with_layer()` pass so it sits above neighboring quads; the seek tooltip renders through a real `overlay()` (`TooltipOverlay`) so it z-orders above other widgets. Drag-release publishes `Seek(progress * duration)` once; in-flight position keeps the handle smooth via `last_position + elapsed` interpolation. `filled(true)` switches to the MiniPlayer capsule look — a full-height track that is structurally handle-less (the handle never draws in filled mode) and carries `time_labels(CapLabel, CapLabel)` end-caps. `interactive(false)` (radio) disables seeking and is decoupled from `hide_handle`.

## Key Widgets

| Widget | File | Purpose |
|--------|------|---------|
| Volume Slider | `volume_slider.rs` | Vertical/horizontal, `SliderVariant` |
| View Header | `view_header.rs` | Sort selector, search bar, shuffle, center-on-playing, columns dropdown. `ViewHeaderConfig.sort_placeholder: Option<&str>` renders a grayed no-selection placeholder in the sort dropdown (queue shows "Unsorted" until a queue sort is applied). Opt-in auto-hide toolbar: `ViewHeaderConfig.collapsed` renders a collapsed strip per `CollapsedAppearance` instead of the full toolbar; `on_hover_enter`/`on_hover_exit` wrap the header in a hover-reveal `mouse_area`, `on_dropdown_open`/`on_dropdown_close` keep it revealed while the sort dropdown is open |
| Base Slot List | `base_slot_list_layout.rs` | Shared layout scaffolding (`base_slot_list_layout{,_with_handle}`, `BaseSlotListLayoutConfig`). Artwork panels take an `ArtworkPlaceholder` (`Blank` default / `RadioTower` for logo-less radio stations) for their art-less fill. `base_slot_list_empty_state()` lives in `widgets/mod.rs` and routes through it |
| Scroll Indicator | `scroll_indicator.rs` | `wrap_with_scroll_indicator()` + drag-to-seek, gated on the `ScrollbarVisibility` setting (`theme::scrollbar_visibility()`): `OnHover` = transient handle that fades in on hover/scroll; `Always` (default) = permanent track + handle, the wrapper reserves a right-edge gutter so the bar never floats over content; `Hidden` = nothing drawn, no gutter (wheel still scrolls) |
| Hover Overlay | `hover_overlay.rs` | Per-slot hover darkening + press scale + external `flash_at()`. Default radius = `ui_border_radius()`. `wash_enabled(false)` suppresses the hover/press color wash (press scale-down still fires) — used by foreign-palette rows like the theme-picker swatches; `on_accent_surface(true)` swaps the accent wash for a contrasting neutral pigment over already-`accent_bright()`-filled surfaces |
| Track Info Strip | `track_info_strip.rs` | Now-playing metadata. `build_now_playing_segments` returns `Vec<String>` (title / artist / album fragments + separators) that the merged-mode marquee concats |
| Marquee Text | `marquee_text.rs` | Scrolling overflow text, generic over message type |
| Context Menu | `context_menu.rs` | Right-click menu. `LibraryContextEntry` (`ShufflePlay` heads the list — one-shot shuffle, never touches shuffle mode) / `StripContextEntry` / `RadioContextEntry` (Edit, Copy Stream URL, Set Custom Artwork…, Reset Artwork [gated], Refresh Artwork, Delete); the queue's `QueueContextEntry` lives in `src/views/queue/mod.rs`. `PanelMenuEntry<Message>` (icon + label + message, constructors `refresh_artwork` / `set_custom_artwork` / `reset_artwork`) is the entries vocabulary of the large-artwork-panel menus — the `*_artwork_panel_with_*` helpers in `base_slot_list_layout.rs` take a `Vec<PanelMenuEntry>` and wrap in `context_menu` only when non-empty. Wrap helpers: `wrap_library_row` / `wrap_similar_row` (slot rows), `wrap_strip_context_menu` (the three now-playing strip placements — player-bar, top strip, merged nav-bar; takes `StripContextAction` / `SetOpenMenu` variant constructors as fn pointers, returns the bare strip when radio is active) |
| Checkbox Dropdown | `checkbox_dropdown.rs` | Shared header-anchored dropdown chassis, generic over `Key`. Hosts two row families off one widget: **checkbox rows** (`checkbox_dropdown` / `library_selector_popover`, stay-open multi-toggle) and **action rows** (`action_dropdown`, close-on-click one-shot actions with leading icon + label + optional consequence subtitle, `close_on_click=true` publishes `on_open_change(None)` on select). The Queue server-sync (push/pull) menu uses `action_dropdown` |
| Info Modal | `info_modal.rs` | Two-column property table for Get Info (`InfoModalItem`) |
| About Modal | `about_modal.rs` | Metadata + diagnostics, theme-adaptive logo, Ko-fi tip link |
| Text Input Dialog | `text_input_dialog.rs` | Modal text input or confirmation. Save Queue uses `combo_box`; `TextInputDialogState.secure` masks the primary input for secret entry (scrobble tokens) |
| EQ Slider | `eq_slider.rs` | Vertical ±15 dB slider for 10-band EQ |
| EQ Modal | `eq_modal.rs` | 10-band EQ overlay with preset picker (`update/eq_modal.rs`). State lives on `Nokkvi.eq_modal: EqModalState` (extracted as a sibling struct so the EQ overlay doesn't drift WindowState fields) |
| Slot List Page | `slot_list_page.rs` | `SlotListPageState` + unified `SlotListPageMessage` dispatcher. Auto-hide toolbar: transient reveals (hover, open sort dropdown, hotkey reveal timer, focused-but-empty search) are gated on `window_focused` (window Focused/Unfocused events) so the toolbar collapses behind another app; a non-empty search filter stays revealed unconditionally. Activation is `ActivateCenter(bool)` — `true` forces a one-shot Shuffle Play (Ctrl+Enter). (`SlotListConfig` lives in `slot_list.rs`; its `hover_wash: bool` field — default `true`, cleared via `.without_hover_wash()` — forwards to each row's `HoverOverlay::wash_enabled`, used by the theme picker so swatch rows keep their own palette) |
| Slot List View | `slot_list_view.rs` | Scroll-position state owned by the view (decoupled from `SlotListPageState`) |
| Visualizer | `visualizer/` | Pipeline + shader + wgsl modules (see `visualizer.md`) |
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
| Boat | `boat.rs` (+ `boat_physics.rs` / `boat_tests.rs`) | Surfing-boat overlay. CPU-only — reads a shared bar buffer. Two uses: the lines-mode over-cover boat (`boat_overlay(..., None)` — the `None` trail-offset keeps its drop-anchor doodad) and the Harbour Trawl seascape (`harbour_sea::trawl_scene` calls `boat_overlay` with a trail offset for the trawling longship) |
| Harbour Sea | `harbour_sea.rs` | Procedural day/night seascape for the Harbour Trawl panel's artwork slot. `sea_bars(phase)` yields one coherent bar array per tick (advanced by the boat tick at `SEA_DRIFT_HZ`, no audio needed), fed to a canvas sea + the reused `boat_overlay` longship, with SVG sun/moon layers. Rendered via `trawl_scene(...)` from `views/harbour.rs` |
| Menu Chrome | `menu_chrome.rs` | Shared overlay-menu vocabulary: `fill()`, `border()`, `container_style()` accessors consumed by the four overlay menus (hamburger / player_modes / checkbox_dropdown / context_menu) so the `bg1 + border + ui_radius_md + MENU_SHADOW` recipe lives at one site |
| Menu Dismiss | `menu_dismiss.rs` | Shared overlay-menu dismissal: `handle_dismiss()` (Escape closes + captures; outside press closes WITHOUT capturing — the click-to-switch invariant lives + is unit-pinned here) and `press_began()`; each overlay passes its own outside-press predicate |
| Slider Drag | `slider_drag.rs` | Shared drag machinery for the settings/volume/EQ sliders: `project_fraction()` (axis projection with half-handle inset; `handle = 0.0` for the vertical volume level-meter), `SliderDragState` (press/drag/release with `>=`-threshold move gate and strictly-`>` trailing release gate), `grab_interaction()`. Deliberately Shell-free — each slider keeps its own publish/capture/redraw (settings captures on release; volume/EQ do not) and its own `State` type (tree::Tag identity) |
| Modal Button | `modal_button.rs` | `modal_icon_button(icon, size, on_press)` — the shared `mouse_area(HoverOverlay(container(svg)))` chassis used by About / Info modal headers |
| Pill Segmented Button | `pill_segmented_button.rs` | Horizontal chip group used by Settings Bool / Enum / ToggleSet widgets. Flat 1 px outline + `theme::bg0()` fill in flat mode; `ui_radius_pill()` corners in rounded mode; selected chip uses `accent_bright()` fill |
| Checkbox Glyph | `checkbox_glyph.rs` | Shared menu-checkbox visual recipe (filled `accent_bright()` rounded square + `check.svg` when checked; outlined `fg2()` square when unchecked). Two adapters off one set of consts: `element()` for `checkbox_dropdown`'s composed rows, `draw()` for `player_modes_menu`'s hand-painted `fill_quad` path — mirrors the `menu_constants`/`menu_chrome` split. The slot-list multi-select boxes and the `text_input_dialog` native checkbox are deliberately distinct families |
| Settings Slider | `settings_slider.rs` | Draggable settings slider (flat 6 px track + 14 px handle; same visual family as `progress_bar.rs`). Emits a `[0.0, 1.0]` fraction that the settings handler maps via `SettingValue::set_fraction`; presses outside the visible bar pass through to the row button |
| Sizes | `sizes.rs` | Shared widget-geometry constants (`ICON_BUTTON_SIZE = 40`, `TOOLBAR_BUTTON_SIZE = 44`, the modal icon sizes). Only obviously-duplicated sizes live here — one-off literals stay in their owning widget |
| Scroll Into View | `scroll_into_view.rs` | Measured scroll-into-view: `center_in_scrollable()` runs a widget `Operation` that reads the real laid-out bounds of an `Id`-tagged target + its scrollable in one tree walk, then centers it — replaces estimated per-row heights for the variable-height settings detail pane |

## Modal Frame Style

`theme::modal_frame_style(theme)` returns the `container::Style` for every overlay modal panel — `bg0_hard()` fill, 1 px `accent_bright()` outline, `ui_radius_lg()` corners. Routed by `about_modal`, `info_modal`, `eq_modal`, `text_input_dialog`, `default_playlist_picker`, and `trawl_modal` so a future tweak (e.g. switching the outline onto `border()` for a chrome-quiet variant) lands at one site.

The settings **font + theme pickers** are the exception: they share their own chrome via `render_picker_modal()` (`src/views/settings/view.rs`) — a dimmed backdrop (press → Escape, wheel → slot Up/Down) behind a centered `bg0_hard()` + 1.5 px `accent()` panel with an X-back title bar, a search bar, and a caller-built slot-list body. `render_font_modal` / `render_theme_modal` differ only in title/placeholder/search-input-id and their row renderer; the theme picker passes `.without_hover_wash()` so each row stays in its own palette (selection shows via a per-row accent ring).

## Nav Bars

- **Top** (`nav_bar.rs`): tabs + format stats + hamburger. Metadata only when `TrackInfoDisplay::TopBar`. Progressive collapsing (album <900, artist <750, title <600). `flat_tab_container_style` paints the full-cell `accent_bright()` active fill; the right-edge indicator strip from the pre-redesign was removed. `NAV_TABS` is the single source of truth for which tabs render — `NAV_TABS[i] == NavView::ALL[i]` is pinned by a runtime test.
- **Side** (`side_nav_bar.rs`): vertical sidebar. `NavDisplayMode` { TextOnly, TextAndIcons, IconsOnly }. Same active-fill recipe as the top nav.
- **None** layout: no nav chrome — only the active page + player bar render (minimalist mode).

## Layout Constants (`slot_list.rs`)

Single source of truth: `chrome_height_with_header(collapsed_header: bool)`, `theme::nav_bar_height()` (32 flat / 44 rounded — the old `NAV_BAR_HEIGHT` const is gone), `view_header_chrome()` (derives from `view_header::HEADER_HEIGHT = 50` + 1 px separator — replaces the old `VIEW_HEADER_HEIGHT` const), `TAB_BAR_HEIGHT = 32`, `SLOT_SPACING = 0` (flat redesign: rows touch). When the auto-hide toolbar is collapsed, `collapsed_view_header_chrome()` substitutes for `view_header_chrome()` and dispatches on `CollapsedAppearance`: Hairline = `theme::autohide_toolbar_height_px()` + separator, Hidden = `view_header::HIDDEN_CATCH_HEIGHT` (3 px invisible hover catch-zone, no separator), CountStrip = `view_header::COUNT_STRIP_HEIGHT` (24 px) + separator — so a collapsed view reclaims the header height as extra slot rows. Slot count is computed dynamically: always odd, capped at `MAX_SLOT_COUNT = 29`. Cross-pane drag uses structural cursor → slot resolution via per-slot `mouse_area` (see `slot_list.rs::SlotHoverCallback`) rather than chrome math, so there is no `queue_slot_list_start_y` helper.

## Slot Rendering

`SlotListRowContext` bundles per-slot args. `SlotListRowMetrics` derives sizes from active `slot_row_height()`. Center slot gets `flash_at`. Clickable stars (`slot_list_star_rating()`) and hearts (`slot_list_favorite_icon()`). Top-packing when items < slot_count.

**Selection is border-only** (theme-modal style): multi-selected rows (via `selected_indices`, always shown) and the lone click/keyboard cursor keep the normal row background + theme text and are marked solely by a 2 px accent ring — `theme::selection_ring_on(bg)`, contrast-floored to ≥ 3:1 against the row bg (`SELECTION_RING_MIN_CONTRAST`, all-themes gauntlet-pinned) and immune to the opacity gradient. The loud `selected_fill_resolved()` fill + forced-legible ink belongs ONLY to the now-playing row (breathing glow), expanded-parent headers (calmer `playing_fill()`), and `SlotListSlotStyle::drag_preview()` (the floating drag ghost). The fallback *center* ring is what's suppressed during an active Ctrl/Shift modifier hold or while a multi-selection exists.

The list **shell** (`slot_list_background_container`) runs edge-to-edge (rows touch the header strip above + player bar below). Rounded mode adds a 1 px `theme::border()` outline that seals the touching row hairlines, but the shell's corners stay **square in every mode** — a rounded corner under `clip(true)` would leave the base theme background bleeding through the unpainted wedge (the scrollbar doesn't reliably cover it).

**Always derive static-icon color via `slot_list_static_icon_color(style, fallback, opacity)`** when embedding a tinted SVG / text / pill inside a row renderer (lock glyphs, sub-index labels, empty heart/star outlines, radio-tower icons, etc.). On the opaque loud-fill rows (`style.forces_legible_text` — now-playing / expanded-parent / drag-preview ghost; border-only selections stay `false`) it returns the row's forced-legible `style.text_color` (contrast-guarded `Color::BLACK` or `Color::WHITE` via `theme::legible_text_on()`, ≥ 4.58:1 against any fill); otherwise the `fallback` color with `opacity` applied to its alpha. The icon thus stays readable against the highlight fill in lockstep with the row's text. Hardcoding a `theme::fg*()` color in a row renderer breaks contrast on the loud-fill rows.

## SVG Icons (`embedded_svg.rs`)

Top-level module. `build.rs` walks **both** `assets/icons/` (the canonical Lucide set, the path every view references) and `assets/icons-phosphor/` (the Phosphor alternate), emitting a combined `lookup()` + `KNOWN_PATHS` array under `OUT_DIR`. Adding/removing an icon in either dir is a one-step change (drop or remove the file, rebuild).

`embedded_svg.rs` adds the **icon-set remap layer** on top of that generated table. The selectable set is `IconSet` (`nokkvi_data::types::player_settings`, re-exported from `player_settings/appearance.rs`); the active value lives on a `theme` atomic (`icon_set()` / `set_icon_set()`, default `IconSet::Phosphor`). `get_svg(path)`:

- When the active set is **Phosphor** (the default), it first remaps a Lucide path to its Phosphor equivalent via `NAME_MAP` (Lucide-stem → Phosphor-file, sorted, `binary_search_by_key`). A mapped-but-missing Phosphor file **falls through to the Lucide content** — *not* the `play.svg` fallback. Most entries use the Phosphor **Regular** weight; the filled transport + rating glyphs (`play`, `pause`, `skip-back`, `skip-forward`, `heart-filled`, `star-filled`) deliberately resolve to the **Fill** weight.
- When the active set is **Lucide**, the remap is skipped (one atomic load) and the direct lookup is used.
- A path with no entry at all returns `play.svg` with a warn log — the silent failure mode the `all_svg_paths_in_source_are_registered` test exists to catch. Companion tests pin the map sorted, cover every Lucide icon, confirm every Phosphor target ships, and assert `get_svg` honors the active set end-to-end.

The boat's drop-anchor doodad and the Queue/Playlists nav glyphs **follow the active set** (changing the set bumps `theme_generation()`, rebuilding the cached handles): `themed_anchor_svg()` strokes the Lucide open-path anchor or fills the Phosphor solid glyph with the visualizer border color, and `anchor_svg_ring_top_fraction()` returns the set-specific rope-hook fraction (Lucide `2/24`, Phosphor `40/256`).

`themed_logo_svg()` rewrites the Nokkvi longship logo's hex fills to the active theme via role accessors: body (sail + hull) → `logo_body()` (fg0), shields → `logo_shields()` (accent), wood (mast + yard) → `logo_wood()` (warning). All read the theme's dark palette regardless of light/dark mode, so the mark is mode-stable; the near-black group outline (`LOGO_TOKEN_OUTLINE`) stays fixed. `themed_boat_svg()` reshapes the same logo for the lines-mode boat overlay (uniform themed outline, baked tilt/mirror/flip transforms derived from `LOGO_VIEWBOX`).

## HoverOverlay + native buttons

`HoverOverlay::new(button)` works in some places — e.g. `player_bar.rs:845` (inside `player_control_button`) actively wraps a `button` and the hover/press visual fires correctly because commit `d2f22a0` added `shell.request_redraw()` to `HoverOverlay::update`. The canonical pattern still applies for clickable cells (slot rows, header icons, modal-icon buttons), but the absolute "never wraps a button" framing in older notes is too strict.
