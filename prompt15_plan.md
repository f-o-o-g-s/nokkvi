# PROMPT 15 — Phased Implementation Plan (Red/Green TDD)

**Feature:** Queue stars column always visible + responsive hide (Part A) and reusable column-visibility dropdown in the view header (Part B).
**Source:** `newfeatbugs_claude.md` § PROMPT 15.

## Decisions locked in

- **Ship Part A first** as a standalone PR. Part B is a follow-up PR.
- **Responsive primitive:** `iced::widget::responsive(|size| ...)` measuring the queue panel itself — *not* `data.window_width`. Reason: the queue can be in split-view (Ctrl+E from queue, see `.agent/rules/ui-views.md` §Browsing Panel), at which point the queue panel is roughly half the window. Using `window_width` would mis-fire the breakpoint.
- **Lucide icon for Part B:** `columns-3-cog` (already present at `reference-lucide/icons/columns-3-cog.svg` — no `git pull` needed).
- **Part B widget:** new reusable `checkbox_dropdown` widget in `src/widgets/checkbox_dropdown.rs`. Follows the `context_menu`-style trigger-element + overlay pattern (see `.agent/rules/widgets.md` §Key Widgets). Aligns with project rule "always prefer the most robust, DRY, and scalable solution" (`code-standards.md`).
- **Settings routing:** queue column visibility is an **Interface** setting → `WriteGeneralSetting` action → `config.toml`. Lives in `views/settings/items_interface.rs` per `.agent/rules/settings-view.md`.

## Open questions

1. **Breakpoint value** for hiding the stars column based on **queue panel width** (not window). Suggested starting value: `400.0` px. Will refine after seeing it in motion.
2. Should Part B's column visibility state be **per-view** (queue-only for now) or **shared** across slot-list views from day one? Recommend per-view; lift to shared only when a second view actually needs it.

## Reference: existing patterns this plan reuses

- Responsive culling pattern → `src/widgets/player_bar.rs:44–51` (breakpoint constants) and `:446–453` (visibility flags + conditional `.push()`). We adapt this to use `responsive(|size| ...)` instead of `window_width`.
- Stars cell widget → `src/widgets/slot_list.rs:922–982` (`slot_list_star_rating`).
- Stars column gate today → `src/views/queue.rs:746` (`show_rating_column = current_sort_mode == QueueSortMode::Rating`) and rendering at `:863–876`.
- View header → `src/widgets/view_header.rs:15–36` (signature) — we extend with one optional param.
- Embedded SVG 3-part registration → `src/embedded_svg.rs` (const + match arm + KNOWN). Test enforcement at `:360–384`.
- Settings persistence helper → `src/update/settings.rs:70–100` (`persist_bool_setting`).
- TEA test helper → `src/update/tests*.rs` using `test_app()` from `test_helpers.rs`.
- TDD protocol → `.agent/rules/code-standards.md` §Red-Green TDD.

---

# Part A — Stars column always visible + responsive hide

## Phase 1 — Stars visible across all sort modes

**Goal:** Remove the sort-mode gate. Stars column renders for every `QueueSortMode`.

**Plumbing first** (so tests compile per CLAUDE.md TDD protocol):
- Extract the visibility decision into a small pure helper in `src/views/queue.rs`:
  ```rust
  fn rating_column_visible(_sort: QueueSortMode) -> bool {
      // Phase 1: starts as `sort == Rating` (no behavior change yet)
      // Phase 1 green: returns `true`
  }
  ```
  Replace the inline `let show_rating_column = ...` at `queue.rs:746` with a call to this helper.
- No new types, no new messages.

**Red — write tests first** (`src/update/tests_queue_columns.rs`, new file):
- `rating_column_visible_for_all_sort_modes`:
  - For each `QueueSortMode` variant (Album, Artist, Title, Duration, Genre, Rating), assert `rating_column_visible(mode) == true`.
  - Initial helper body returns `sort == Rating` → 5/6 assertions fail → red confirmed.

**Green:**
- Change helper body to `true`.
- Update `title_portion` logic at `queue.rs:746–747` so it stays at `35` (formerly the "with rating" branch) unconditionally.

**Verify:**
- `cargo test -p nokkvi -- rating_column`
- `cargo clippy --all-targets -- -D warnings`
- `cargo +nightly fmt --all`
- Manual: launch app, cycle queue sort modes, confirm stars column always renders and clicks set ratings correctly.

**Risk:** title_portion balance may need a small tweak (35 vs 40) at typical widths now that the column is permanent. Visual check, not testable.

---

## Phase 2 — Responsive hide at narrow queue-panel widths

**Goal:** Stars column hides when the queue *panel* is narrower than `BREAKPOINT_HIDE_QUEUE_STARS`. Works correctly in split-view (where queue panel ≠ window).

**Plumbing first:**
- Add `const BREAKPOINT_HIDE_QUEUE_STARS: f32 = 400.0;` near the top of `src/views/queue.rs` (mirroring the constant style at `player_bar.rs:44–51`).
- Extend the helper signature:
  ```rust
  fn rating_column_visible(_sort: QueueSortMode, panel_width: f32) -> bool { ... }
  ```
- Wrap the queue's slot-list render block in `iced::widget::responsive(|size| { ... })` so the inner closure has access to `size.width` (the queue panel's measured width). Pass `size.width` into `rating_column_visible(...)`. Confirm the wrap doesn't break widget-tree stability (`.agent/rules/gotchas.md` §Widget Tree & Focus): keep the root widget type stable across renders.

**Red:**
- Extend `tests_queue_columns.rs`:
  - `rating_column_hidden_below_breakpoint`: `rating_column_visible(Album, 399.0) == false`.
  - `rating_column_visible_at_and_above_breakpoint`: `rating_column_visible(Album, 400.0) == true` and `rating_column_visible(Album, 1200.0) == true`. (Boundary is `>=`.)
  - `responsive_hide_overrides_sort_mode`: `rating_column_visible(Rating, 399.0) == false` — width wins over sort.
- Tests fail because helper still returns `true` unconditionally → red confirmed.

**Green:**
- Helper body: `panel_width >= BREAKPOINT_HIDE_QUEUE_STARS`.
- Title portion adjusts at the breakpoint: when stars hide, give title that 5 portions back (35 → 40).

**Verify:**
- `cargo test`, clippy, fmt.
- Manual narrow-window test: drag the window down; stars column should drop out smoothly.
- Manual split-view test: open Ctrl+E browsing panel from queue; queue panel narrows → stars drops *independent of* total window width. This is the key correctness check the plan is built around.

**End of Part A — ship as a single PR:**
- Suggested commit message: `feat(queue): always show stars column with responsive hide`
- PR title: same.

---

# Part B — Reusable column-visibility dropdown

## Phase 3 — Reusable `checkbox_dropdown` widget

**Goal:** Self-contained `checkbox_dropdown` widget usable by any view header. No queue wiring yet.

**Files:**
- New: `src/widgets/checkbox_dropdown.rs`
- Modified: `src/widgets/mod.rs` (export)
- New: tests inline `#[cfg(test)] mod tests` for any pure helpers.

**Design:**
- API:
  ```rust
  pub fn checkbox_dropdown<'a, Message: Clone + 'a>(
      trigger_icon: &'static str,         // "assets/icons/columns-3-cog.svg"
      tooltip: &'static str,              // "Show/hide columns"
      items: Vec<(String, bool)>,         // (label, current_state)
      is_open: bool,                      // open/close state owned by caller
      on_open_toggle: Message,            // dispatched when trigger clicked
      on_item_toggle: impl Fn(usize) -> Message + 'a,
      on_dismiss: Message,                // dispatched on outside click / Escape
  ) -> Element<'a, Message>
  ```
- Renders trigger as `header_icon_button` (40×40, mirroring view_header style at `view_header.rs:213–262`).
- When `is_open`, renders an iced `overlay` (or absolutely positioned `Container`, mirroring whatever pattern `context_menu.rs` uses — read it before deciding) anchored below the trigger, listing each item as a `mouse_area` row with a checkbox-style icon (use existing `assets/icons/check.svg` if present, else add `square-check.svg` from lucide) + label.
- HoverOverlay wraps a Container, never a button (`.agent/rules/gotchas.md` §Widget Tree).
- Width unset → flush content, leave border radius unset only if flush-to-edge (it isn't here, so use `ui_border_radius()`).

**Plumbing first:**
- Add `columns-3-cog` icon registration to `src/embedded_svg.rs`:
  1. Copy `reference-lucide/icons/columns-3-cog.svg` → `assets/icons/columns-3-cog.svg`.
  2. `const COLUMNS_3_COG: &str = include_str!("../assets/icons/columns-3-cog.svg");` (~line 180).
  3. Match arm in `get_svg()`: `"assets/icons/columns-3-cog.svg" => COLUMNS_3_COG,` (~line 50).
  4. Add `"assets/icons/columns-3-cog.svg",` to `KNOWN` array (~line 270).
- If using a checkbox icon not yet embedded, repeat the 4-step registration for it.
- Run `cargo test --bin nokkvi -- embedded_svg` to catch silent fallbacks (`.agent/rules/gotchas.md` §Assets & Icons).

**Red:**
- `checkbox_dropdown_widget_module_compiles`: trivial — module + public function exist.
- Pure-helper unit tests inside the widget module (e.g., a `summarize_visible_count` helper that returns `"3 of 4"` or similar for the trigger tooltip): assert formatting cases.
- The widget's interactive surface isn't easily unit-testable through Iced — visual/integration verification covers it.

**Green:**
- Implement the widget module. Keep it generic; do not import from `views/`.

**Verify:**
- `cargo test`, clippy, fmt.
- `cargo test --bin nokkvi -- embedded_svg` to confirm the new icon is registered.

---

## Phase 4 — Column visibility state, persistence, and queue wiring

**Goal:** Queue gains a `column_visibility` state, a dropdown control in its view header that uses the Phase 3 widget, and persistence to `config.toml`.

**Files:**
- `data/src/types/toml_settings.rs` — extend Interface section with `[interface.queue_columns]` table or flat keys (e.g. `queue_show_stars = true`). Pick whichever matches the existing toml shape; read the file first.
- `data/src/services/toml_settings_io.rs` (or wherever bool getters/setters live) — add `update_queue_show_stars(bool)` setter using `update_config_value()` (NOT `update_theme_value()` — see `.agent/rules/gotchas.md` §Config & Persistence).
- `src/views/queue.rs` — add `pub column_visibility: QueueColumnVisibility` and `pub column_dropdown_open: bool` to `QueuePage`. Define `QueueColumnVisibility { stars: bool }` with `Default { stars: true }`. Load initial state from settings in `QueuePage::new`.
- `src/app_message.rs` — add to `QueueMessage`:
  - `ToggleColumnDropdown`
  - `ToggleColumnVisible(QueueColumn)` where `QueueColumn` is a small enum (just `Stars` for now, easy to extend).
  - `DismissColumnDropdown`
- `src/update/queue.rs` — handlers:
  - `ToggleColumnDropdown` flips `column_dropdown_open`.
  - `ToggleColumnVisible(Stars)` flips `column_visibility.stars` AND `shell_spawn`s a `update_queue_show_stars` persist call.
  - `DismissColumnDropdown` sets `column_dropdown_open = false`.
- `src/widgets/view_header.rs` — add an optional param:
  ```rust
  on_columns: Option<ColumnsControl<'a, Message>>,
  // where ColumnsControl carries items, is_open, on_open_toggle, on_item_toggle, on_dismiss
  ```
  Render the `checkbox_dropdown` widget into `header_row` when `Some`.
- `src/views/queue.rs` — at the call site for `view_header(...)`, pass `Some(ColumnsControl { ... })` constructed from queue state and `QueueMessage` variants. Pass `None` from the other ~7 views (they don't need this yet).
- `src/views/queue.rs` Phase 2 helper extension: take the visibility flag too:
  ```rust
  fn rating_column_visible(_sort: QueueSortMode, panel_width: f32, user_visible: bool) -> bool {
      user_visible && panel_width >= BREAKPOINT_HIDE_QUEUE_STARS
  }
  ```

**Plumbing first:** add the `QueueColumnVisibility` struct, the `QueueColumn` enum, the `QueueMessage` variants, the `ColumnsControl` param type, the toml setter, and a dummy handler body that compiles. **No behavioral changes yet.**

**Red — append to `src/update/tests_queue_columns.rs`:**
- `default_queue_column_visibility_shows_stars`: `app.queue_page.column_visibility.stars == true`.
- `toggle_column_visible_flips_state`: dispatch `QueueMessage::ToggleColumnVisible(Stars)`, assert `false`. Dispatch again, assert `true`.
- `toggle_column_dropdown_opens_then_closes`: dispatch `QueueMessage::ToggleColumnDropdown` twice, assert open/closed/open.
- `dismiss_column_dropdown_closes`: open it, dispatch `DismissColumnDropdown`, assert closed.
- `user_invisible_overrides_responsive_show`: `rating_column_visible(Album, 1200.0, false) == false`.
- `responsive_hide_still_wins_over_user_visible`: `rating_column_visible(Album, 200.0, true) == false`.
- TOML round-trip test in `data/`: write `queue_show_stars = false`, read back, assert.

All red because handlers are still empty stubs.

**Green:**
- Wire the handlers (mutate state, call `shell_spawn` to persist).
- Make `rating_column_visible` consult the new flag.
- Render `checkbox_dropdown` from `view_header` when `on_columns` is `Some`.

**Verify:**
- `cargo test`, clippy, fmt.
- Manual: open queue, click `columns-3-cog` icon, see dropdown with "Stars" toggle. Toggle off → column disappears. Restart app → preference survives. Inspect `~/.config/nokkvi/config.toml` to confirm the key.
- Split-view manual check: dropdown opens correctly even when queue panel is narrow (overlay should not clip out of bounds — adjust if so).

---

## Phase 5 — Settings GUI mirror entry

**Goal:** User can also flip the toggle from Settings → Interface, consistent with existing toggle conventions (`new-feature-checklist.md` §Cross-Cutting "Settings" line).

**Files:**
- `src/views/settings/items_interface.rs` — add a new `SettingItem` for "Queue: show stars column" with `SettingValue::Bool(...)` and a clear `subtitle` (`SettingMeta` requires it, per `.agent/rules/settings-view.md`).
- `src/update/settings.rs` — handle the new entry via existing `persist_bool_setting()` helper, calling the same `update_queue_show_stars` setter from Phase 4. Dispatch a `QueueColumnVisibilityChanged` (or reuse `ToggleColumnVisible(Stars)`) so the queue's in-memory state flips too.

**Red:**
- `settings_toggle_stars_column_persists`: dispatch the settings message → assert `app.queue_page.column_visibility.stars` flipped AND a persist task was spawned. Use `test_app()`-friendly checks; if asserting on the spawn closure is awkward, fall back to asserting state mutation only and rely on the Phase 4 round-trip test for persistence coverage.

**Green:**
- Wire the entry, route through `WriteGeneralSetting` action so it goes to `config.toml`.

**Verify:**
- `cargo test`, clippy, fmt.
- Manual: settings → Interface → toggle stars; return to queue; stars matches. Header dropdown also reflects the new state. Both paths flip the same flag with no double-write.

---

## Phase 6 — Final pass

- `cargo +nightly fmt --all`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `cargo build --release`
- `cargo test --bin nokkvi -- embedded_svg` (icon registration sanity)
- Visual sweep:
  - Cycle every `QueueSortMode` — stars column always renders above breakpoint.
  - Cross the breakpoint by resizing the window (single-pane) and by toggling Ctrl+E split-view (queue narrows mid-render). Smooth transition both ways.
  - Toggle off via header dropdown → off via settings → on via header → on via settings. Both UIs stay consistent. Restart app → final state restored from `config.toml`.
  - Confirm no widget-tree-stability regressions: text input focus survives toggles (this is the `.agent/rules/gotchas.md` §Widget Tree & Focus risk for the `responsive(...)` wrap).
- Conventional commits (per phase or grouped per PR):
  - PR 1 (Part A): `feat(queue): always show stars column with responsive hide`
  - PR 2 (Part B): can be one commit or split:
    - `feat(widgets): add reusable checkbox_dropdown widget`
    - `feat(queue): per-column visibility state and header dropdown`
    - `feat(settings): queue stars column visibility toggle`
- Check whether any `.agent/rules/` files need updating (per `code-standards.md` §Rule Maintenance). Likely candidates if behavior shifts: `widgets.md` (new widget entry in the Key Widgets table) and `ui-views.md` (queue column visibility note).

---

## Cross-cutting checklist (from `.agent/rules/new-feature-checklist.md`)

- [x] TEA pattern (Message + Action + handler)
- [x] Update handler in `update/queue.rs`, root dispatch already routes `Message::Queue(...)`
- [x] Persistence: `config.toml` via `update_config_value()` (Interface routing)
- [x] Tests for observable state mutations BEFORE implementation (TDD)
- [x] Settings entry with `SettingMeta` + subtitle
- [x] Lucide icon registered in `embedded_svg.rs` (const + match + KNOWN)
- [x] Reuse existing patterns (`view_header`, `header_icon_button`, `persist_bool_setting`, `responsive`)
- [N/A] Cross-view sync, MPRIS, scrobbling, hotkeys, multi-selection (not applicable — purely a column toggle)
- [N/A] Artwork prefetch (no new artwork surface)
- [N/A] Batch payload (single-toggle action)
