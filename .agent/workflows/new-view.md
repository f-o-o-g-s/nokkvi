---
description: How to add a new slot list view to the application
---

# Add a New View

Follow these steps in order to add a new slot-list-based view.

## Steps

1. Create `src/views/{name}.rs` (or `src/views/{name}/mod.rs` for complex views) with:
   - `{Name}Page` struct with `common: SlotListPageState`
   - `{Name}Message` enum with slot list navigation variants
   - `{Name}Action` enum with `None` variant at minimum
   - `update()` method returning `(Task<{Name}Message>, {Name}Action)`
   - `view()` method receiving `&{Name}ViewData`

2. Add to `src/views/mod.rs`:
   - Module declaration and re-exports
   - Search ID constant (`{NAME}_SEARCH_ID`)
   - Add explicit `impl ViewPage for {Name}Page` block
   - If the view has expansion, implement `HasCommonAction` on the action enum

3. Add `{Name}Page` field to `Nokkvi` in `src/main.rs`

4. Add `Message::{Name}({Name}Message)` variant to `src/app_message.rs`

5. Add routing in `src/update/mod.rs`:
   - Dispatch `Message::{Name}(msg)` to `self.{name}_page.update(msg)`
   - Handle returned `{Name}Action` variants

6. Add view rendering in `src/app_view.rs`

7. Create handler in `src/update/{name}.rs` for data loading and action handling

8. If the view displays artwork, add prefetch logic in `update/window.rs` (centralized artwork prefetch dispatch)

9. Wrap slot list in `wrap_with_scroll_indicator()` from `widgets/scroll_indicator.rs`

10. Add context menu: wrap slot buttons in `context_menu()` with `LibraryContextEntry` / `QueueContextEntry` actions, add `ContextMenuAction(usize, LibraryContextEntry)` variant. Batch-aware via `evaluate_context_menu()` and `get_batch_target_indices()`.

11. Add multi-selection support: `handle_slot_click()` with keyboard modifiers in click handler, `clear_multi_selection()` after batch actions. Use `build_batch_payload()` and `get_queue_target_indices()` to simplify batch payload construction.

12. Add toast notifications for user-facing actions using `toast_*()` helpers

13. If the view should appear in the browsing panel, add a `BrowsingView` variant and lazy data load check

14. Verify:
    - `cargo +nightly fmt --all -- --check`
    - `cargo clippy` clean
    - `cargo test` passing
    - Slot list navigation works (↑/↓, focus, center activation)
    - Search filtering works (/ hotkey, immediate results)
    - Context menu works (right-click, all entries functional)
    - Multi-selection works (Ctrl+click, Shift+click range, batch actions)
    - Scroll indicator appears on long lists
