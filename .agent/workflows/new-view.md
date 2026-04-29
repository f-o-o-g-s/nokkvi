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
   - `view()` taking `&{Name}ViewData`

2. Add to `src/views/mod.rs`:
   - Module declaration + re-exports
   - Search ID constant `{NAME}_SEARCH_ID`
   - Explicit `impl ViewPage for {Name}Page`
   - `impl HasCommonAction for {Name}Action` if it has SearchChanged/SortModeChanged/SortOrderChanged

3. Add `{name}_page: views::{Name}Page` to `Nokkvi` in `src/main.rs`.

4. Add `Message::{Name}({Name}Message)` to `src/app_message.rs`.

5. Wire root dispatch in `src/update/mod.rs`:
   - Forward `Message::{Name}(msg)` to `self.{name}_page.update(msg)`
   - Map returned `{Name}Action` variants to side effects

6. Render the page in `src/app_view.rs`.

7. Create the data/action handler at `src/update/{name}.rs`. Use `PaginatedFetch::from_common()` from `update/components.rs` for paginated loads — needs_fetch gating is built in.

8. If the view shows artwork, dispatch prefetch from `update/window.rs` (centralized).

9. Wrap the slot list in `wrap_with_scroll_indicator()` (`widgets/scroll_indicator.rs`).

10. Context menu: wrap rows in `context_menu()` with `LibraryContextEntry` / `QueueContextEntry`. Resolve batch targets via `evaluate_context_menu()` and `get_batch_target_indices()` / `get_queue_target_indices()`. Build payloads via `expansion::build_batch_payload()`.

11. Multi-selection: route clicks through `handle_slot_click()`; clear with `clear_multi_selection()` after every batch op.

12. Toasts: `toast_success()` / `toast_error()` / `toast_warn()` / `toast_info()`.

13. Browsing panel: add a `BrowsingView` variant in `views/browsing_panel.rs` if the view should appear in split-view; wire lazy data load.

14. Icons: drop SVGs into `assets/icons/`. The build.rs generator picks them up automatically — no manual registration.

15. Verify:
    - `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all-targets -- -D warnings`
    - `cargo test`
    - Slot navigation (↑/↓, focus, center activation)
    - Search filtering (immediate, no debounce)
    - Context menu (every entry functional)
    - Multi-selection (Ctrl+click, Shift+click range, batch actions)
    - Scroll indicator on long lists
