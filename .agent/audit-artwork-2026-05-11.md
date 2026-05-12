# Artwork-column audit — fix spec (2026-05-11)

Source review: three parallel subagent audits of the day's six commits
(`9c932de`, `a113380`, `91e34d0`, `7789624`, `e285d74`, `3145ac7`) covering the
new artwork-column modes (Always-Vertical Native + Stretched, user-tunable
Auto-mode max-percent, side-nav chrome pane-width fix, vertical-header padding).
This doc is the single source of truth for the cleanup branch
`artwork-audit-fixes`. Worktree agents should treat each numbered finding as
their work item.

## Lane plan

| Lane | Worktree | Owns | Depends on |
|------|----------|------|------------|
| **Phase 1** | _main branch, serial_ | `data/src/types/player_settings/artwork.rs`, `data/src/types/settings.rs`, `data/src/types/toml_settings.rs`, `data/src/services/settings.rs`, `data/src/services/settings_tables/interface.rs`, `src/theme.rs` | — |
| **W** | `worktrees/lane-w` | `src/widgets/base_slot_list_layout.rs`, `src/widgets/artwork_split_handle.rs`, `src/widgets/artwork_split_handle_vertical.rs`, view files using `base_slot_list_layout_with_handle` | Phase 1 (uses `is_stretched()`, `is_vertical()`) |
| **S** | `worktrees/lane-s` | `src/views/settings/items_interface.rs` | Phase 1 (uses `is_stretched()`) |
| **A** | `worktrees/lane-a` | `src/widgets/side_nav_bar.rs`, `src/app_view.rs`, `src/app_message.rs`, `src/update/mod.rs` | — |
| **T** | `worktrees/lane-t` | `src/update/tests/` (new file) | — |

All four Phase 2 lanes are file-disjoint after Phase 1 lands.

## Findings (10 total, do them all)

### Phase 1 — foundation

#### #4 — Triple-source-of-truth on artwork defaults and clamps `high impact`

Today the magic literal `0.40` lives in four places per knob, and the clamp
ranges (`0.30..=0.70`, `0.10..=0.80`, `0.05..=0.80`) live in three places per
knob:

| Knob | `data/types/settings.rs` (default fn) | `data/types/toml_settings.rs` (default fn) | `data/services/settings.rs` (setter clamp) | `data/services/settings_tables/interface.rs` (macro ui_meta) | `src/theme.rs` (atomic init + clamp) |
|---|---|---|---|---|---|
| `artwork_column_width_pct` | `0.40` :400-402 | `0.40` :243-245 | `0.05, 0.80` :437 | — (no slider; pixel-drag only) | `0x3ECC_CCCD` :154, `0.05, 0.80` :783 |
| `artwork_auto_max_pct` | `0.40` :404-406 | `0.40` :247-249 | `0.30, 0.70` :442 | `default: 0.40_f64, min: 0.30_f64, max: 0.70_f64, step: 0.05_f64` :316-317 | `0x3ECC_CCCD` :156, `0.30, 0.70` :800 |
| `artwork_vertical_height_pct` | `0.40` :408-410 | `0.40` :251-253 | `0.10, 0.80` :447 | `default: 0.40_f64, min: 0.10_f64, max: 0.80_f64, step: 0.05_f64` :335-336 | `0x3ECC_CCCD` :158, `0.10, 0.80` :816 |

**Fix:** in `data/src/types/player_settings/artwork.rs`, declare these
`pub const` values (one block per knob):

```rust
pub const ARTWORK_COLUMN_WIDTH_PCT_DEFAULT: f32 = 0.40;
pub const ARTWORK_COLUMN_WIDTH_PCT_MIN: f32 = 0.05;
pub const ARTWORK_COLUMN_WIDTH_PCT_MAX: f32 = 0.80;

pub const ARTWORK_AUTO_MAX_PCT_DEFAULT: f32 = 0.40;
pub const ARTWORK_AUTO_MAX_PCT_MIN: f32 = 0.30;
pub const ARTWORK_AUTO_MAX_PCT_MAX: f32 = 0.70;

pub const ARTWORK_VERTICAL_HEIGHT_PCT_DEFAULT: f32 = 0.40;
pub const ARTWORK_VERTICAL_HEIGHT_PCT_MIN: f32 = 0.10;
pub const ARTWORK_VERTICAL_HEIGHT_PCT_MAX: f32 = 0.80;
```

Each consumer references those constants (`pct.clamp(MIN, MAX)`,
`AtomicU32::new(DEFAULT.to_bits())`, `default: f64::from(DEFAULT)`). The
`.to_bits()` const trick is safe because `f32::to_bits` is `const` on stable.

#### #6 — Predicate methods on `ArtworkColumnMode`

`is_stretched` and `is_vertical` are currently inlined as OR-matches at four
sites and (worse) as a string-compare in the UI settings builder. Define them
on the enum so a future variant forces the compile error in one place:

```rust
impl ArtworkColumnMode {
    /// Cover/Fill stretch applies (any "Stretched" variant).
    pub fn is_stretched(self) -> bool {
        matches!(self, Self::AlwaysStretched | Self::AlwaysVerticalStretched)
    }

    /// Artwork stacks above the slot list (any "Vertical" variant).
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::AlwaysVerticalNative | Self::AlwaysVerticalStretched)
    }

    /// Artwork shown to the right (any non-vertical "Always" variant).
    pub fn is_always_horizontal(self) -> bool {
        matches!(self, Self::AlwaysNative | Self::AlwaysStretched)
    }
}
```

Phase 1 only **defines** them. Lanes W and S consume them.

#### #10 — Document `Never`-variant placement in theme atomic encoding

`src/theme.rs:728-748` encodes mode as `0=Auto, 1=AlwaysNative,
2=AlwaysStretched, 3=Never, 4=AlwaysVerticalNative, 5=AlwaysVerticalStretched`.
`Never` sitting between two Always-modes is a future-agent trap. Both `match`
arms (load + store) are correctly bidirectional, so this is purely a comment
fix; do not renumber (would break existing redb state).

Add a comment above the `artwork_column_mode()` function:

```rust
// Encoding NOTE: 0=Auto, 1=AlwaysNative, 2=AlwaysStretched, 3=Never (kept
// where it is for redb back-compat — do not renumber), 4=AlwaysVerticalNative,
// 5=AlwaysVerticalStretched. New variants must be appended at 6+; both the
// load and the store match must list every value.
```

### Phase 2 lane W — widget layer

#### #1 — Parameterize `ArtworkSplitHandle` over an `Axis` enum `high impact`

`src/widgets/artwork_split_handle.rs` and `artwork_split_handle_vertical.rs`
are 95% line-for-line copies. Only meaningful diffs: an axis (`_x` vs `_y`), a
sign flip (`-dpct` vs `+dpct`), a `Length::Fixed(width)` vs `Length::Fill` in
`size()`, the default `min_pct` (0.05 vs 0.10), `ResizingHorizontally` vs
`ResizingVertically`, and which theme atomic the convenience constructor reads.

**Fix:** introduce `enum Axis { X, Y }` and a single `ArtworkSplitHandle<A>` or
`ArtworkSplitHandle { axis: Axis, ... }`. The two convenience constructors
(`new_horizontal(width)` / `new_vertical(width)`) stay as ~5-line wrappers
that pick the right atomic. Delete `artwork_split_handle_vertical.rs`. Update
every view's call site.

Be deliberate about the sign asymmetry: handle-below-artwork moves the same
direction as the cursor (+dpct); handle-left-of-artwork moves opposite (-dpct).
Name this in `Axis::pct_sign()` or equivalent so it's not a comment-as-truth.

#### #2 — Stable root widget across horizontal/vertical layout branches `medium impact`

`base_slot_list_layout.rs:775,894` returns `row![...]` for horizontal and
`column![...]` for vertical. CLAUDE.md is explicit: changing root widget
between renders destroys `text_input` focus. Switching artwork-mode while a
search input is focused will currently drop the cursor.

**Fix:** wrap both branches in an outer `column![...]` (with `padding(0)` so
it's a no-op) or a `container(...)`. Inner `row`/`column` swap is invisible to
focus tracking.

#### #9 — Make `vertical_artwork_chrome` exhaustive `low impact`

`base_slot_list_layout.rs:267` has `_ => 0.0`. Today only
`ArtworkOrientation::Horizontal` falls through, but the wildcard would also
swallow a future variant.

**Fix:** spell the variant out:

```rust
match orientation {
    Some(ArtworkLayout { orientation: Horizontal, .. }) | None => 0.0,
    Some(ArtworkLayout { orientation: Vertical, .. }) => /* vertical chrome */,
}
```

Workspace lint `match_wildcard_for_single_variants = "deny"` already enforces
this elsewhere; reach the same standard here.

#### #6 (consumer) — Replace OR-match predicates in `base_slot_list_layout.rs`

After Phase 1 lands the methods, replace:
- `let is_always = matches!(mode, AlwaysNative | AlwaysStretched | …);` →
  `mode.is_always_horizontal()` (or the appropriate predicate per call site)
- `let is_always_vertical = matches!(mode, AlwaysVerticalNative | AlwaysVerticalStretched);` →
  `mode.is_vertical()`

at every call site in `base_slot_list_layout.rs` (lines 340, 721, 833, plus
any other location grep turns up).

### Phase 2 lane S — settings UI

#### #6 (consumer) — Replace string-compare in `items_interface.rs:150-152`

```rust
if data.artwork_column_mode == "Always (Stretched)"
    || data.artwork_column_mode == "Always (Vertical Stretched)"
```

becomes

```rust
let mode = ArtworkColumnMode::from_label(data.artwork_column_mode);
if mode.is_stretched() { ... }
```

(The `data.artwork_column_mode` field is a `&str` label; route through
`from_label` to reach the enum, then call the predicate.)

#### #8 — Refresh stale module doc-comment

`src/views/settings/items_interface.rs:3-8` claims "11 flat rows" and a single
stretched-mode trigger. After today's commits it's 13 rows and the conditional
fires for both stretched modes. Rewrite the comment to reflect reality, and
mention the two new sliders (`general.artwork_auto_max_pct`,
`general.artwork_vertical_height_pct`) alongside the existing knobs.

### Phase 2 lane A — app integration

#### #3 — Promote `SIDE_NAV_WIDTH + 2.0` to a named constant

The 2-pixel border literal lives in two places that must agree:
- `src/app_view.rs:108` — `content_pane_width()` returns
  `physical_window_width - (SIDE_NAV_WIDTH + 2.0)`
- `src/widgets/side_nav_bar.rs:365` — outer container width is
  `Length::Fixed(SIDE_NAV_WIDTH + 2.0)`

**Fix:** in `side_nav_bar.rs`, add

```rust
/// Width of the side-nav border (`bg0_hard` rule on the right edge).
pub(crate) const SIDE_NAV_BORDER: f32 = 2.0;
/// Total horizontal footprint of the side-nav bar (icons + border).
pub(crate) const SIDE_NAV_TOTAL_WIDTH: f32 = SIDE_NAV_WIDTH + SIDE_NAV_BORDER;
```

Replace both `SIDE_NAV_WIDTH + 2.0` sites with `SIDE_NAV_TOTAL_WIDTH`.

#### #7 — Delete dead `Message::ArtworkColumnDrag{Change,Commit}` variants

`src/app_message.rs:742,744` define the variants and `src/update/mod.rs:623-628`
dispatches them, but no widget or view emits them — the current emitters are
per-view-namespaced (`Message::Albums(AlbumsMessage::ArtworkColumnDrag(...))`).
The two root variants are vestiges of an earlier flat-message design.

**Fix:** delete the two variants and the two dispatch arms. The shared handler
`handle_artwork_column_drag` (`update/mod.rs:640-661`) stays — it is still
called from the per-view arms.

Grep `ArtworkColumnDragChange|ArtworkColumnDragCommit` after deletion to
confirm zero remaining references.

### Phase 2 lane T — handler test

#### #5 — Red-green TDD test for `handle_artwork_vertical_drag` `medium impact`

CLAUDE.md mandates red-green TDD for handlers. The new
`handle_artwork_vertical_drag` (`update/mod.rs:666-687`) has no test.

**Fix:** add `src/update/tests/artwork_drag.rs` (and register it in
`src/update/tests/mod.rs`). Two tests:

1. `vertical_drag_change_updates_atomic_only`: build `test_app()`, snapshot
   `artwork_vertical_height_pct()`, dispatch
   `Message::Albums(AlbumsMessage::ArtworkColumnVerticalDrag(DragEvent::Change(0.55)))`
   through `app.update(msg)`, assert atomic now reads `0.55` (within f32
   tolerance), and assert no persistence task was spawned (no spawn = nothing
   to verify positively; rely on the absence of `app_service` mutation).
2. `vertical_drag_commit_updates_atomic_and_persists`: same shape, dispatch
   `DragEvent::Commit(0.62)`, assert atomic reads `0.62`. (The persistence
   call goes through `shell_spawn` which test_app can't intercept; assert at
   least the observable atomic.)

Symmetric pair for `handle_artwork_column_drag` (horizontal) would also be
welcome but is not in audit scope; do not add it.

## Phase 3 — merge + CI gate

After all four worktree lanes are committed and merged into
`artwork-audit-fixes`:

```bash
cargo +nightly fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

All four must pass. Then hand off to the owner for smoke testing.

## Out of scope (intentionally skipped)

- Lifting `ArtworkColumnDrag`/`ArtworkColumnVerticalDrag` into a shared
  `ArtworkMessage` sub-enum — would force wrapping at every emitter site;
  current per-view variant is idiomatic TEA.
- Auto-deriving the per-view drag-message variants via macro — premature at
  7 views; revisit at 10+.
- The `+2` touches in `update/playback.rs` and `update/settings.rs` —
  load-bearing hydration symmetry, do not remove.
- `app_view.rs::content_pane_width()` split-fraction logic — correct as-is.
- The `e285d74` rename `BOTTOM_PAD → TOP_PAD` — clean, internally consistent.
