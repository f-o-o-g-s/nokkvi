---
description: How to add a new slot list view to the application
---

# Add a New View

Steps to add a new slot-list-based view, in order.

1. Create `src/views/{name}.rs` (or `src/views/{name}/mod.rs` for complex views) with:
   - `{Name}Page` struct with `common: SlotListPageState`
   - `{Name}Message` enum (slot-list navigation variants)
   - `{Name}Action` enum with `None` at minimum
   - `update()` returning `(Task<{Name}Message>, {Name}Action)`
   - `view()` taking the ViewData struct by value — it is itself a struct of `&'a` borrows: `pub fn view<'a>(&'a self, data: {Name}ViewData<'a>) -> Element<'a, {Name}Message>`

2. Add to `src/views/mod.rs`:
   - Module declaration + re-exports
   - Search ID constant `{NAME}_SEARCH_ID`

   And in the view's own module file (the trait + macro live in `src/views/mod.rs`; the impls live with each view):
   - Explicit `impl super::ViewPage for {Name}Page` (pattern: `src/views/albums/mod.rs`)
   - `impl_has_common_action!({Name}Action { ... })` if the Action enum has SearchChanged/SortModeChanged/SortOrderChanged — invoke the macro rather than hand-writing `HasCommonAction`. The variadic list names the enum's `NavigateAndExpand*` variants; pass `, no_center` to skip the `CenterOnPlaying` arm, `, no_navigate_filter` for views with neither (e.g. `impl_has_common_action!(RadiosAction, no_navigate_filter);` in `src/views/radios.rs`)

3. Add `{name}_page: views::{Name}Page` to `Nokkvi` in `src/main.rs`. If the view is a top-level destination, also add a `View` variant and extend `View::ALL` (length-anchored), then decide its start-view eligibility in `View::start_view_option()` — the exhaustive match forces the call. Eligible views additionally need their name added to the `general.start_view` options in `data/src/services/settings_tables/general.rs` (iced-free, can't see `View`); the `view_metadata_tests` drift guard in `src/main.rs` pins the two lists together. Also opt the variant in or out of the IPC `switch-view` verb via `ipc_switchable` in `src/update/ipc.rs` — the wire-name parser and its error listing derive from `View::ALL`, so a `view_name` entry plus the opt-in is all it takes. The remaining `match`es over `View` (get-info / star-rating hotkeys, seek-settled artwork dispatch, roulette settle, view_page lookup) are exhaustive on purpose: the compiler walks you through each placement decision — keep them wildcard-free.

4. Add `Message::{Name}({Name}Message)` to `src/app_message.rs`.

5. Wire root dispatch in `src/update/mod.rs`:
   - Route `Message::{Name}(msg)` through the `dispatch_view_with_seek!` macro to a `handle_{name}` method (pattern: the `Message::Albums` arm), naming the view's `SlotList(ScrollSeek)` variant
   - Register the message type with `impl_view_chrome!({Name}Message { ... })` in `src/update/chrome.rs` and start `handle_{name}` with the mandatory prologue `dispatch_view_chrome(self, &msg, View::{Name})` (see `src/update/albums.rs::handle_albums`). This prologue carries the shared SetOpenMenu/roulette/SFX/artwork-drag handling — skipping it compiles fine but silently drops that chrome for the new view
   - In `handle_{name}`, map the `{Name}Action` returned by the page's `update()` to side effects

6. If the view has a paginated/async loader, add a typed loader inbox (Phase 2 pattern — see Albums/Songs/Artists/Genres/Playlists/Queue):
   - Define `{Name}LoaderMessage` in `src/app_message.rs` with `Loaded { ... }` / `PageLoaded(result, total_count)` variants for each loader result shape
   - Add `Message::{Name}Loader({Name}LoaderMessage)` to the root `Message`
   - Route in `src/update/mod.rs`: `Message::{Name}Loader(msg) => self.dispatch_{name}_loader(msg)`
   - Implement `dispatch_{name}_loader(msg)` in `src/update/{name}.rs`. Loader closures inside `shell_task(...)` construct `Message::{Name}Loader({Name}LoaderMessage::Loaded { ... })` instead of view-side variants, keeping the page's `{Name}Message` enum focused on user-driven UI events.
   - For the actual paged-fetch dispatch, implement `LoaderTarget` for a `{Name}Target` in `src/update/loader_target.rs` (`page_common`, `sort_mode_to_api`, buffer/artwork accessors) and call `self.load_paged::<{Name}Target>(...)` from your page handler — the page-fetch closure is passed at the call site, deliberately not threaded through the trait. Avoid hand-writing a `set_loading(true)` + defensive-gate + `shell_task` body — the shared `load_paged` body owns that invariant.

7. Render the page in `src/app_view.rs`.

8. Create the data/action handler at `src/update/{name}.rs`. For paginated loads always go through `load_paged` (step 6): `PaginatedFetch::from_common()` (`src/update/components/mod.rs`) only bundles view/sort/search/filter params — the needs_fetch duplicate-dispatch gate and the `set_loading(true)` invariant live in `Nokkvi::load_paged` (`src/update/loader_target.rs`).

9. If the view shows artwork, add an arm to the per-view match in `prefetch_viewport_artwork()` (`src/update/window.rs`, centralized) using the shared task builders in `src/update/components/artwork_prefetch.rs`.

10. Wrap the slot list in `wrap_with_scroll_indicator()` (`widgets/scroll_indicator.rs`).

11. Context menu: wrap rows in `context_menu()` with `LibraryContextEntry` / `QueueContextEntry`. Resolve batch targets via `evaluate_context_menu()` and `get_batch_target_indices()` / `get_queue_target_indices()`. Build payloads via `expansion::build_batch_payload()`.

12. Multi-selection: route clicks through `handle_slot_click()`; clear with `clear_multi_selection()` after every batch op. Add an opt-in checkbox column via `wrap_with_select_column()` + `compose_header_with_select()` (`widgets/slot_list.rs`) and a `{view}_show_select` flag in `ViewColumns` (`data/src/types/view_columns.rs` — shared by `LivePlayerSettings` / `PersistedPlayerSettings` / `TomlSettings` via their `view_columns` member) so the columns dropdown can toggle it. Every new `ViewColumns` field also gets declared in the `view_columns:` fields list of the consolidated `define_settings!` invocation in `data/src/services/settings_tables/columns.rs` — its exhaustive destructure turns an omission into a compile error (settings system: `.claude/rules/settings-view.md`; the per-view `define_view_columns!` dropdown side: `.claude/rules/ui-views.md`).

13. Toasts: `toast_success()` / `toast_error()` / `toast_warn()` / `toast_info()`.

14. Browsing panel: add a `BrowsingView` variant in `views/browsing_panel.rs` if the view should appear in split-view; wire lazy data load.

15. Icons: adding one is a three-part change — the Lucide SVG in `assets/icons/` (the path code references; build.rs registers both icon dirs automatically), its Phosphor counterpart in `assets/icons-phosphor/`, and a `NAME_MAP` row in `src/embedded_svg.rs` mapping the Lucide stem to the Phosphor file. The `icon_name_map_*` tests enforce all three (Phosphor is the default set). Details: `.claude/rules/widgets.md`.

16. Verify:
    - `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all-targets -- -D warnings`
    - `cargo test --workspace` (bare `cargo test` runs only the root crate and would skip the data-crate tests touched in step 12)
    - Slot navigation (↑/↓, focus, center activation)
    - Search filtering (immediate, no debounce)
    - Context menu (every entry functional)
    - Multi-selection (Ctrl+click, Shift+click range, batch actions)
    - Scroll indicator on long lists
