---
trigger: glob
globs: src/widgets/**
---

# Widgets

## Rounded Mode

- `ROUNDED_MODE` (`AtomicBool` in `src/theme.rs`) is the global rounded-corners toggle
- `is_rounded_mode()` — read anywhere in widget/view code
- `set_rounded_mode(bool)` — called on startup restore and on settings change
- `ui_border_radius()` — returns `6.0` when rounded, `0.0` when squared; **use this everywhere instead of hardcoding**
- `three_d_button.rs` / `three_d_icon_button.rs`: single bordered quad when rounded, original 5-quad 3D bevel when squared. **Pressed scaling animation**: `AnimatedPress` wrapper scales down (e.g. 0.92×) on mouse press with smooth spring-back.
- **Transparent-border clipping gotcha**: Iced clips a container's background to its border radius **even when the border is transparent and 0px wide**. For full-width strip-style bars that sit flush against the window edge, do **not** set a border radius.

## Opacity Gradient & Slot Row Height

- `opacity_gradient` (`AtomicBool` in `src/theme.rs`): global toggle for non-center slot opacity fade
- Persisted as a `PlayerSettings` field; exposed in Settings → Theme
- `slot_row_height`: `SlotRowHeight` enum stored as `AtomicU8` variant index in `src/theme.rs`. Variants: Compact (50px), Default (70px), Comfortable (90px), Spacious (110px).
- Persisted as a `PlayerSettings` field; exposed in Settings → General → Application

## Player Bar (`player_bar.rs`)

- Responsive element culling at breakpoints: 1000px → 520px
- Transport controls always visible
- **Scroll-to-volume**: mouse wheel anywhere on the player bar adjusts volume
- **Horizontal volume mode**: `horizontal_volume` setting stacks volume sliders horizontally; both SFX and main volume sliders' combined height matches adjacent button height when both visible
- `GoToQueue` message — used by track info strip to navigate to queue view

## Progress Bar (`progress_bar.rs`)

- Custom `iced::advanced` seekable progress/scrub bar widget
- Click-to-seek with smooth visual feedback

## Volume Slider (`volume_slider.rs`)

- Custom `iced::advanced` slider widget for volume/balance control
- `SliderVariant` enum supports different visual modes
- Supports both vertical (default) and horizontal orientations via `horizontal_volume` setting

## View Header (`view_header.rs`)

- Generic slot list view header with sort mode selector, sort order toggle, search bar, shuffle button
- Reused across all views

## Base Slot List Layout (`base_slot_list_layout.rs`)

- Shared layout scaffolding for slot-list-based views
- `base_slot_list_empty_state()` renders empty states within the same widget tree structure to prevent focus loss

## Scroll Indicator (`scroll_indicator.rs`)

- Custom `iced::advanced` transient scroll indicator overlay for slot list views
- Proportionally-sized scrollbar handle on the right edge, rendered as transparent overlay in a `Stack` — no layout shift
- `wrap_with_scroll_indicator()` helper wraps a slot list element; automatically hidden when list fits in viewport
- Drag-to-seek support (click/drag on track area seeks)
- Handle scales with `slot_row_height`; opacity fades on idle, appears on hover/scroll
- Uses accent-family colors with 3D borders for visibility over any slot background

## Hover Overlay (`hover_overlay.rs`)

- Custom `iced::advanced` widget wrapping each slot list slot
- **Hover effect**: subtle darkening overlay on mouse enter/exit
- **Press animation**: scale-down effect on mouse press/release
- **External flash trigger**: `flash_at(Option<Instant>)` triggers the press animation from outside (used for center slot activation)

## Track Info Strip (`track_info_strip.rs`)

- Shared widget rendering now-playing metadata (title, artist, album, codec, kHz, kbps)
- Used by both the player bar and the top bar (side nav only)
- Controlled by `TrackInfoDisplay` setting (Off / PlayerBar / TopBar)
- **Clickable center metadata**: title/artist/album wrapped in `mouse_area` with optional `on_press` — navigates to queue view. Codec/sample-rate and bitrate sections remain non-clickable.
- **`info_field_widget()`**: shared helper for labeled scrolling metadata fields — single source of truth

## Format Info (`format_info.rs`)

- `format_audio_info()` produces strings like `"FLAC 44.1kHz · 1411kbps"`
- `format_audio_info_split()` returns `(left, right)` parts for split layouts
- Used by both the nav bar and the track info strip

## Marquee Text (`marquee_text.rs`)

- Custom `iced::advanced` widget for scrolling/ticker text that overflows its container
- **Generic over message type** — usable in any view
- Uses `FillPortion` for proportional gap distribution during scroll

## Side Nav Bar (`side_nav_bar.rs`)

- Vertical sidebar navigation layout (alternative to top bar)
- Enabled via `NavLayout::Side` setting
- `NavDisplayMode` controls content: TextOnly, TextAndIcons, IconsOnly

## Nav Bar (`nav_bar.rs`)

- Top-bar navigation: view tabs + audio format stats (kHz, bitrate) + hamburger menu
- Stats display live values from audio engine via atomics
- **Clickable info row**: metadata area wrapped in `mouse_area` — navigates to queue view
- Toast notifications rendered in the nav bar area

## Hamburger Menu (`hamburger_menu.rs`)

- Custom `iced::advanced` widget with overlay dropdown
- Click-outside-close behavior

## Context Menu (`context_menu.rs`)

- Generic right-click context menu widget (adapted from Halloy)
- Two entry enums: `LibraryContextEntry` for library views, `QueueContextEntry` for queue
- `library_entries()` — base entries (AddToQueue, AddToPlaylist, GetInfo)
- `library_entries_with_folder()` — adds ShowInFolder (used by Songs, Albums, Artists)
- Supports separator entries for visual grouping

## Info Modal (`info_modal.rs`)

- Feishin-style two-column property table for "Get Info" context menu action
- `InfoModalItem` enum: `Song`, `Album`, `Artist`, `Playlist` — each provides `properties()` → `Vec<(label, value)>`
- Per-row expand/collapse, hover copy button, clickable URLs, open in file manager
- `OpenFolder(path)` — direct path open (songs, albums with loaded tracks)
- `FetchAndOpenAlbumFolder(album_id)` — async fetch representative song path for albums without loaded tracks

## Text Input Dialog (`text_input_dialog.rs`)

- Modal overlay dialog with text input, submit/cancel, Enter/Escape handling
- Used for: Rename Playlist, Save Queue as Playlist, confirmation dialogs (visualizer/hotkey reset)
- **Confirmation mode**: no text input — just confirm/cancel buttons
- **Save Queue as Playlist** mode: `combo_box` dropdown listing existing playlists (with `+ New Playlist` option)

## Drag Column (`drag_column.rs`)

- Custom `iced::advanced` widget for in-queue drag-and-drop slot reordering

## Stable Viewport / Click-to-Focus

- `SlotListView` has `selected_offset: Option<usize>` — when set, item gets "center" styling without scrolling
- **Keyboard navigation** clears `selected_offset`; `get_effective_center_index()` returns `selected_offset` if set
- `CenterOnPlaying` (Shift+C) directly calls `handle_set_offset` — bypasses click-to-highlight path
- Auto-follow: on natural track changes, viewport follows even with stable viewport on

## Slot List Slot Rendering

- `SlotListRowContext` bundles: item_index, is_center, opacity, row_height, scale_factor
- Each slot wrapped in `HoverOverlay`; center slot receives `flash_at` for externally triggered flash
- **Clickable star ratings**: `slot_list_star_rating()` with `on_click` closure per star
- **Clickable hearts**: `slot_list_favorite_icon()` with `on_click` closure

## Layout Constants (`slot_list.rs`)

Shared layout constants: `NAV_BAR_HEIGHT`, `CHROME_HEIGHT`, `CHROME_HEIGHT_WITH_HEADER`, etc. Used by queue view, app_view, and cross-pane drag for slot sizing.

## SVG Icons (`embedded_svg.rs`)

- All SVG icons **embedded at compile time** via `include_str!` — fully portable binary
- `get_svg(path)` maps path strings to embedded content
- **Unknown paths silently fall back to the play icon** with a `warn!` log

### Adding a New Icon

1. Copy the SVG from `reference-lucide/icons/` to `assets/icons/`
2. Add a `const` with `include_str!` in `embedded_svg.rs`
3. Add a match arm in `get_svg()` mapping the path to the const
4. Reference the path in your view code

**If you skip steps 2–3, the icon compiles but renders as a play triangle at runtime.**
