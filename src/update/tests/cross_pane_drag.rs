//! Cross-pane drag (browsing panel → queue) — structural-resolution tests.
//!
//! Pins the contract that cursor → slot → item resolution flows from the
//! per-slot `mouse_area` hover state instead of chrome reconstruction.
//! Each test exercises one piece of that contract:
//!
//! - `HoverEnterSlot` / `HoverExitSlot` update the view's `hovered_slot`
//!   field and shrug off the cross-view ordering races a coalesced enter+exit
//!   pair can trigger.
//! - `compute_queue_drop_slot` reads the queue's hovered slot directly —
//!   no `cursor_y` argument, no chrome math, no slot-count divergence.
//! - `handle_cross_pane_drag_pressed` reads the active browser view's
//!   hovered slot — the press at the "wrong-track-dragged" scenario from
//!   the math-fix branch is now structurally unreachable because the
//!   handler asks the rendered widget tree instead of reconstructing slot
//!   positions from chrome constants.
//!
//! Bug-class coverage relative to the math-fix commit
//! (`worktree-fix-crosspane-drag-vertical-artwork::c19e7ae`):
//!
//! 1. Missing vertical-artwork chrome in `queue_slot_list_start_y` —
//!    structurally unreachable: no chrome math here.
//! 2. Stored vs inline `slot_count` divergence — structurally unreachable:
//!    `HoveredSlot::Item { item_index }` is baked at render time with the
//!    same `effective_center` the slots use, so no separate slot→item
//!    resolution runs at consume time.
//! 3. Nav-mode-insensitive press chrome math → wrong-track-dragged —
//!    structurally unreachable: `handle_cross_pane_drag_pressed` reads
//!    `hovered_slot.item_index()` published by the slot's own
//!    `mouse_area`, so the press resolves to whichever item the user is
//!    actually pointing at.
//! 4. Hand-coded `EDIT_BAR_HEIGHT = 32` vs the queue view's actual 45/33
//!    — structurally unreachable: indicator y is derived from the hovered
//!    slot's index inside the slot list's own coordinate space (see
//!    `slot_list_view_with_drag`), so the constant doesn't exist.

use crate::{
    test_helpers::{make_queue_song, make_song, test_app},
    views,
    widgets::{HoveredSlot, SlotListPageMessage},
};

fn populate_queue(app: &mut crate::Nokkvi, n: usize) {
    for i in 0..n {
        let id = format!("q{i}");
        app.library
            .queue_songs
            .push(make_queue_song(&id, &id, "ar", "Al"));
    }
}

fn open_browsing_panel(app: &mut crate::Nokkvi, view: views::BrowsingView) {
    let mut panel = views::BrowsingPanel::new();
    panel.active_view = view;
    app.browsing_panel = Some(panel);
}

#[test]
fn hover_enter_records_slot_on_queue_page() {
    // Per-slot `mouse_area::on_enter` publishes HoverEnterSlot; the queue
    // page's update handler stores it on `slot_list.hovered_slot`. This
    // is what `compute_queue_drop_slot` reads later.
    let mut app = test_app();
    populate_queue(&mut app, 20);

    let payload = HoveredSlot::Item {
        slot_index: 4,
        item_index: 4,
    };
    let total = app.library.queue_songs.len();
    app.queue_page
        .common
        .handle(SlotListPageMessage::HoverEnterSlot(payload), total);

    assert_eq!(
        app.queue_page.common.slot_list.hovered_slot,
        Some(payload),
        "HoverEnterSlot must populate `hovered_slot` so downstream readers \
         (compute_queue_drop_slot, drop indicator) see what the user is \
         pointing at"
    );
}

#[test]
fn hover_exit_clears_only_if_payload_matches() {
    // Adjacent-slot moves coalesce into a per-slot exit + per-slot enter
    // pair, dispatched in widget-tree order. If the exit for slot A
    // arrives AFTER the enter for slot B, we must NOT clear B's state.
    // The handler uses `Some == payload` equality, not a blanket clear.
    let mut app = test_app();
    populate_queue(&mut app, 20);

    let slot_a = HoveredSlot::Item {
        slot_index: 3,
        item_index: 3,
    };
    let slot_b = HoveredSlot::Item {
        slot_index: 4,
        item_index: 4,
    };
    let total = app.library.queue_songs.len();

    // Simulate: enter A, then enter B (cursor moved from A → B already
    // landed on B before A's exit fired), then exit A (late delivery).
    app.queue_page
        .common
        .handle(SlotListPageMessage::HoverEnterSlot(slot_a), total);
    app.queue_page
        .common
        .handle(SlotListPageMessage::HoverEnterSlot(slot_b), total);
    app.queue_page
        .common
        .handle(SlotListPageMessage::HoverExitSlot(slot_a), total);

    assert_eq!(
        app.queue_page.common.slot_list.hovered_slot,
        Some(slot_b),
        "late HoverExitSlot for the previous slot must NOT clear the new \
         slot's state — otherwise rapid cursor movement between adjacent \
         slots flickers `hovered_slot` to None and the drop indicator \
         vanishes mid-drag"
    );

    // Now exit B → must clear.
    app.queue_page
        .common
        .handle(SlotListPageMessage::HoverExitSlot(slot_b), total);
    assert_eq!(app.queue_page.common.slot_list.hovered_slot, None);
}

#[test]
fn compute_queue_drop_slot_returns_hovered_item_index() {
    // The fundamental contract: `compute_queue_drop_slot` IS just a
    // structural read of `queue_page.common.slot_list.hovered_slot`. No
    // cursor coordinate. No chrome math. The item index baked into the
    // hover payload at render time IS the insertion index.
    let mut app = test_app();
    populate_queue(&mut app, 50);
    app.queue_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 6,
        item_index: 27,
    });

    assert_eq!(
        app.compute_queue_drop_slot(),
        Some(27),
        "drop slot must echo the hovered item index — structural read, \
         no derivation"
    );
}

#[test]
fn compute_queue_drop_slot_empty_slot_appends() {
    // Hovering a trailing empty slot (queue end or top-packing tail) maps
    // to insert-at-total — the "append" position.
    let mut app = test_app();
    populate_queue(&mut app, 8);
    app.queue_page.common.slot_list.hovered_slot = Some(HoveredSlot::Empty { slot_index: 9 });

    assert_eq!(
        app.compute_queue_drop_slot(),
        Some(8),
        "Empty hovered slot must map to total_items so the drop appends \
         past the last entry"
    );
}

#[test]
fn compute_queue_drop_slot_none_when_no_hover() {
    // No queue slot hovered → no drop target. The release handler
    // interprets this as "release outside queue slots" and cancels.
    let mut app = test_app();
    populate_queue(&mut app, 10);
    app.queue_page.common.slot_list.hovered_slot = None;

    assert_eq!(
        app.compute_queue_drop_slot(),
        None,
        "no hovered queue slot must yield no drop target — drag is then \
         cancelled rather than appended (no chrome heuristic to fill in)"
    );
}

#[test]
fn compute_queue_drop_slot_is_unaffected_by_window_resize() {
    // The bug class the math fix patched: any change in chrome
    // (artwork mode, nav layout, edit bar, select header, window size)
    // shifted `slot_list_start_y` and broke cursor → slot mapping.
    //
    // Structurally, none of those inputs feed `compute_queue_drop_slot`
    // now: it only reads `hovered_slot`. Resizing the window from
    // 1920×1080 down to a 600×400 portrait — the same scenario the math
    // fix had to handle with vertical-artwork chrome compensation — must
    // not change the result for a fixed hover payload.
    let mut app = test_app();
    populate_queue(&mut app, 100);
    app.queue_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 2,
        item_index: 42,
    });

    app.window.width = 1920.0;
    app.window.height = 1080.0;
    let wide = app.compute_queue_drop_slot();

    app.window.width = 600.0;
    app.window.height = 400.0;
    let narrow = app.compute_queue_drop_slot();

    assert_eq!(wide, Some(42));
    assert_eq!(
        narrow, wide,
        "chrome / window-size variation must not perturb the drop slot — \
         the value flows from the hover payload only"
    );
}

#[test]
fn compute_queue_drop_slot_is_unaffected_by_viewport_offset() {
    // Bug 2 from the math-fix commit: forward (cursor→slot) and
    // back-projection (item→slot) used different `effective_center` at
    // any `viewport_offset > 0`, putting the drop line several slots
    // away from the cursor.
    //
    // Structurally, `item_index` is baked into the hover payload at the
    // moment `build_slot_list_slots` rendered the slot — using the same
    // `effective_center` the slot got. The two computations can no
    // longer disagree. Verify by hovering an item far past where the
    // viewport starts and asserting we get the baked index back.
    let mut app = test_app();
    populate_queue(&mut app, 13_500);
    app.queue_page.common.slot_list.viewport_offset = 5_000;
    app.queue_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 4,
        item_index: 5_001,
    });

    assert_eq!(
        app.compute_queue_drop_slot(),
        Some(5_001),
        "scrolled viewport must NOT reapply effective_center — the hover \
         payload was built with the correct value already"
    );
}

#[test]
fn compute_queue_drop_slot_ignores_stale_stored_slot_count() {
    // Bug 2's stored-vs-inline slot_count divergence concretely: in
    // Auto-vertical-artwork split-view, `resync_slot_counts` writes a
    // slot_count based on the full content-pane width, but the queue
    // pane renders against `content_pane_width * 0.55`. At certain
    // aspect ratios the two diverge (live 13, stored 5 in the user-
    // reported case). The math-fix had to sync the stored value before
    // calling `slot_to_item_index`.
    //
    // Structurally, `compute_queue_drop_slot` never calls
    // `slot_to_item_index` — it reads `hovered_slot.item_index()`
    // straight from a payload that was baked with the inline (correct)
    // value. Stash a deliberately stale `slot_count = 5` and verify the
    // drop slot is unchanged.
    let mut app = test_app();
    populate_queue(&mut app, 20);
    app.queue_page.common.slot_list.viewport_offset = 18;
    app.queue_page.common.slot_list.slot_count = 5; // stale on purpose
    app.queue_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 12,
        item_index: 17,
    });

    assert_eq!(
        app.compute_queue_drop_slot(),
        Some(17),
        "stored slot_count value MUST NOT enter the calculation — the \
         hover payload's item_index is authoritative"
    );
}

#[test]
fn press_resolves_from_browser_hovered_item_index() {
    // The "wrong-track-dragged" regression from the math-fix branch
    // (bug 3): the press handler used to reconstruct slot from cursor Y
    // with hardcoded chrome that missed vertical-artwork extent AND
    // over-counted NAV_BAR in nav=None/Side layouts. A press at visual
    // slot 0 resolved to a different item.
    //
    // Structural fix: the press handler reads the active browser view's
    // `hovered_slot.item_index()` — populated by the slot's own
    // `mouse_area::on_enter`, so it MATCHES what the user is pointing
    // at by construction.
    let mut app = test_app();
    let songs: Vec<_> = (0..50)
        .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "ar"))
        .collect();
    app.library.songs.set_from_vec(songs);
    open_browsing_panel(&mut app, views::BrowsingView::Songs);

    // Cursor is over visual slot 0 of the songs view — item 0.
    app.songs_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 0,
        item_index: 0,
    });

    let _ = app.handle_cross_pane_drag_pressed();

    assert_eq!(
        app.cross_pane_drag_pressed_item,
        Some(0),
        "press over visual slot 0 must resolve to item 0 — the rendered \
         widget tree IS the source of truth"
    );
    assert!(
        app.cross_pane_drag_press_origin.is_some(),
        "press origin must be recorded so the threshold check in \
         handle_cross_pane_drag_moved can activate the drag"
    );
}

#[test]
fn press_resolves_from_browser_hovered_item_at_high_offset() {
    // Same property at a scrolled viewport: the press handler still
    // reads the baked-in item_index, no chrome math, so the cursor maps
    // to the correct item no matter how far the user scrolled.
    let mut app = test_app();
    let songs: Vec<_> = (0..2_000)
        .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "ar"))
        .collect();
    app.library.songs.set_from_vec(songs);
    app.songs_page.common.slot_list.viewport_offset = 1_337;
    open_browsing_panel(&mut app, views::BrowsingView::Songs);
    app.songs_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 7,
        item_index: 1_340,
    });

    let _ = app.handle_cross_pane_drag_pressed();
    assert_eq!(app.cross_pane_drag_pressed_item, Some(1_340));
}

#[test]
fn press_no_op_when_no_browser_slot_hovered() {
    // Cursor not over any browser slot (chrome, queue pane, etc.) → no
    // drag arms. Otherwise a click anywhere in the window could begin
    // an off-target drag.
    let mut app = test_app();
    let songs: Vec<_> = (0..50)
        .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "ar"))
        .collect();
    app.library.songs.set_from_vec(songs);
    open_browsing_panel(&mut app, views::BrowsingView::Songs);
    app.songs_page.common.slot_list.hovered_slot = None;

    let _ = app.handle_cross_pane_drag_pressed();
    assert_eq!(app.cross_pane_drag_pressed_item, None);
    assert_eq!(app.cross_pane_drag_press_origin, None);
}

#[test]
fn press_no_op_on_empty_trailing_slot() {
    // The browser view has a trailing empty slot under the cursor. With
    // no item to drag, the press handler is a no-op (otherwise the drop
    // preview would be empty and the AddCenterToQueue dispatch would
    // resolve to an unintended center).
    let mut app = test_app();
    let songs: Vec<_> = (0..3)
        .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "ar"))
        .collect();
    app.library.songs.set_from_vec(songs);
    open_browsing_panel(&mut app, views::BrowsingView::Songs);
    app.songs_page.common.slot_list.hovered_slot = Some(HoveredSlot::Empty { slot_index: 7 });

    let _ = app.handle_cross_pane_drag_pressed();
    assert_eq!(app.cross_pane_drag_pressed_item, None);
    assert_eq!(app.cross_pane_drag_press_origin, None);
}

#[test]
fn press_no_op_when_browsing_panel_closed() {
    // No browsing panel → no cross-pane drag possible.
    let mut app = test_app();
    app.browsing_panel = None;
    // Hover state on songs is irrelevant when no panel is open.
    app.songs_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 2,
        item_index: 2,
    });

    let _ = app.handle_cross_pane_drag_pressed();
    assert_eq!(app.cross_pane_drag_pressed_item, None);
    assert_eq!(app.cross_pane_drag_press_origin, None);
}

#[test]
fn press_reads_from_active_browser_view_not_a_different_one() {
    // The browser panel surfaces one tab at a time. Press must read the
    // active tab's hover state, not whatever the user last hovered on a
    // different tab.
    let mut app = test_app();
    let albums: Vec<_> = (0..10)
        .map(|i| crate::test_helpers::make_album(&format!("a{i}"), &format!("A{i}"), "ar"))
        .collect();
    app.library.albums.set_from_vec(albums);
    open_browsing_panel(&mut app, views::BrowsingView::Albums);

    // Albums (active) has NO hover.
    app.albums_page.common.slot_list.hovered_slot = None;
    // Songs (inactive) has a stale hover — must be ignored.
    app.songs_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 2,
        item_index: 2,
    });

    let _ = app.handle_cross_pane_drag_pressed();
    assert_eq!(
        app.cross_pane_drag_pressed_item, None,
        "press must consult ONLY the active browser view's hover state \
         — a stale hover on an inactive tab must not arm the drag"
    );
}

#[test]
fn press_no_op_with_ctrl_or_shift_modifier() {
    // Ctrl/Shift held = user multi-selecting, not starting a drag. The
    // button's on_press (mouse-release) handles the selection click; we
    // must not arm the drag state machine.
    let mut app = test_app();
    let songs: Vec<_> = (0..10)
        .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "ar"))
        .collect();
    app.library.songs.set_from_vec(songs);
    open_browsing_panel(&mut app, views::BrowsingView::Songs);
    app.songs_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 2,
        item_index: 2,
    });

    app.window.keyboard_modifiers = iced::keyboard::Modifiers::CTRL;
    let _ = app.handle_cross_pane_drag_pressed();
    assert_eq!(app.cross_pane_drag_pressed_item, None);

    app.window.keyboard_modifiers = iced::keyboard::Modifiers::SHIFT;
    let _ = app.handle_cross_pane_drag_pressed();
    assert_eq!(app.cross_pane_drag_pressed_item, None);
}
