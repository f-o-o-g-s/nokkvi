# Cleanup batch — fanout plan (Bugs B1/B2/B3/B6/B8/B9 + View::ALL)

Closes `.agent/audit-progress.md` §7 #1 (the 6-bug batch) and §7 #2 (View::ALL + wildcard sweep). The two are bundled because each is small (S effort), they touch disjoint files (with one or two trivial overlaps), and §7 #1 says the rationale: "real visible bugs you'd actually feel running the app" + "foundational structural fix" — a healthy palate cleanser after the two L-effort typestate clusters that just landed.

Last verified baseline: **2026-05-08, `main @ HEAD = 8965832`**.

Source audit details: `~/nokkvi-audit-results/_SYNTHESIS.md` §5 (B1-B11), §4 #1 (View drift), §3a in `drift-match-arms.md` (full wildcard inventory).

---

## 1. Goal & rubric

Six visible bugs and one structural drift fix, parallelizable. Each lane is a single conceptual change — no architecture, no design surface.

| Bug / item | Class | Sites | Failure today |
|---|---|---:|---|
| **B1** | `HoverOverlay::new(button(...))` re-trip of documented gotcha | 3 | press-scale animation misfires (button captures `ButtonPressed` first) |
| **B2** | `radius: 4.0.into()` bypasses `theme::ui_border_radius()` | 5 | ignores squared-mode theme toggle |
| **B3** | Queue header widget-tree shape varies across edit / playlist-context / read-only | 3 modes × 1 file | `text_input::Id` focus invalidated when shape changes (iced reconciles positionally) |
| **B6** | Hamburger menu `match item_index { 0 => …, 4 => Quit }` paired with separate `MENU_ITEM_COUNT = 5` const | 1 | reordering items silently fires the wrong action |
| **B8** | Test name says "albums" but body operates on Artists | 1 | misleading test signal — agent reading the failure searches the wrong code |
| **B9** | Comment claims `*LoaderMessage` migration is partial; Phase 2 is fully landed (commits `31374ec..bc53b17`) | 1 | misleads future agents into thinking the loader split is incomplete |
| **§7 #2** | 8 wildcard `_ =>` arms in `match` over `View` enum | ~8 | new `View` variant silently swallowed, no compile error |

Rubric (in order): (1) close visible bugs, (2) convert silent-drift sites to compile errors, (3) keep blast radius small per lane, (4) no scope creep into adjacent audit items.

---

## 2. Lane decomposition (parallel)

Six independent lanes. No required ordering — all six can run concurrently in their own worktrees.

| Lane | Scope | Files touched | Commits (est.) | Effort |
|---|---|---|---:|---|
| **A** (HoverOverlay re-trip) | B1 | `src/widgets/nav_bar.rs`, `src/widgets/side_nav_bar.rs`, `src/views/login.rs` | 1 | S |
| **B** (themed radii) | B2 | `src/views/login.rs`, `src/widgets/info_modal.rs` | 1 | S |
| **C** (queue header shape) | B3 | `src/views/queue/view.rs` | 1 | S |
| **D** (hamburger menu indexed match) | B6 | `src/widgets/hamburger_menu.rs` | 1 | S |
| **E** (test rename + stale comment) | B8 + B9 | `src/update/tests/navigation.rs`, `src/update/mod.rs` | 1–2 | XS |
| **F** (View::ALL + wildcard sweep) | §7 #2 | `src/main.rs`, `src/widgets/nav_bar.rs`, `src/update/navigation.rs`, `src/update/window.rs`, `src/update/components.rs`, `src/views/sort_api.rs`, `src/update/playback.rs`, `src/update/hotkeys/navigation.rs` | 2–4 | M |

### 2.1 Conflict zones

| File | Lane(s) | Resolution |
|---|---|---|
| `src/views/login.rs` | A (line 301) and B (lines 226, 253, 282) | Different methods/sections — rebase clean |
| `src/widgets/nav_bar.rs` | A (line 327, body edit) and F (declaration of `NavView::ALL` near the enum at line 31) | Different parts of the file — rebase clean |

No other overlaps. Recommended merge order: any. If A merges before B, B rebases trivially on the unchanged radii lines. If F merges first, A's nav_bar.rs body edit is independent of the new `NavView::ALL` const.

---

## 3. Per-lane scope (sites verified at baseline `8965832`)

### Lane A — B1, three `HoverOverlay::new(button(...))` re-trips

**The rule** (`.agent/rules/gotchas.md:40` and `widgets.md:84`): *HoverOverlay wraps containers, not native buttons — buttons capture `ButtonPressed` early. Pattern: `mouse_area(HoverOverlay::new(container(content))).on_press(msg)`.*

The pattern is followed correctly in `views/queue/view.rs` (e.g., `icon_btn` at line 222 — see it as the canonical reference). Three sites still violate it:

| File | Line (baseline) | Current shape | Target shape |
|---|---:|---|---|
| `src/widgets/nav_bar.rs` | 327 | `HoverOverlay::new(button(tab_content(...)).on_press(SwitchView(view)).padding(...).style(...))` | `mouse_area(HoverOverlay::new(container(tab_content(...)).padding(...).style(container_style_equiv))).on_press(SwitchView(view))` |
| `src/widgets/side_nav_bar.rs` | 252 | `HoverOverlay::new(button(content).on_press(SwitchView(view)).padding(0).width(...).height(...).style(tab_style))` | `mouse_area(HoverOverlay::new(container(content).padding(0).width(...).height(...).style(container_style_equiv))).on_press(SwitchView(view))` |
| `src/views/login.rs` | 301 | `HoverOverlay::new(button(text(...)).on_press(LoginPressed).padding(14).width(...).style(button::Style)))` | `mouse_area(HoverOverlay::new(container(text(...)).padding(14).width(...).style(container::Style)))..on_press(LoginPressed)` |

The translation from `button::Style` to `container::Style`:
- `background: Some(...)` → identical in `container::Style`.
- `text_color: ...` → `container::Style::text_color = Some(...)`.
- `border: ...` → identical.
- `shadow: ...` → identical.
- The button's hover/pressed status branches collapse: `HoverOverlay` already paints the press-scale animation; the underlying container draws the static visual. If the existing `button::Style` closure produced different styles for `Status::Hovered`/`Status::Pressed`, pick the resting style for the container — `HoverOverlay` provides the press visual.

Add `iced::widget::mouse_area` to imports where missing. `Interaction::Pointer` on the `mouse_area` keeps the cursor change.

**Verification of the fix**: in nokkvi each site has nearby `HoverOverlay::new(container(...))` sibling code (e.g., `nav_bar.rs:307` is already the rounded-mode branch using `mouse_area(HoverOverlay::new(container(...)))`), so the target shape is reachable by mirroring the file's own existing pattern.

### Lane B — B2, five hardcoded `radius: 4.0.into()` sites

`theme::ui_border_radius()` (defined `src/theme.rs:285`, returns `iced::border::Radius`) is the theme-aware accessor that respects the app's squared-mode toggle. Five sites bypass it:

| File | Line (baseline) | Context |
|---|---:|---|
| `src/views/login.rs` | 226 | Server URL `text_input` border |
| `src/views/login.rs` | 253 | Username `text_input` border |
| `src/views/login.rs` | 282 | Password `text_input` border |
| `src/widgets/info_modal.rs` | 559 | Scrollable rail border |
| `src/widgets/info_modal.rs` | 565 | Scrollable scroller border |

Replace `radius: 4.0.into(),` → `radius: theme::ui_border_radius(),` at each site. Confirm the surrounding `iced::Border { ... }` literal has `theme` in scope (it should — these files already use `theme::accent()`, `theme::bg3()`, etc.). No other changes.

**Do NOT** touch the deliberate non-themed radii in this PR: the `radius: 12.0.into()` on the login *card* (`login.rs:343`) is the card's distinct visual, intentionally separate from input radii. Leave it.

### Lane C — B3, queue header widget-tree shape

`src/views/queue/view.rs:166-453` builds a header that varies depth across three modes:

```rust
// Edit mode (line ~340):
column![edit_bar, sep_bottom, header].into()
// Playlist-context mode (line ~450):
column![playlist_bar, sep_bottom, header].into()
// Read-only mode (line ~452):
header   // <-- no wrapper, depth differs
```

iced reconciles widgets positionally. When the user toggles edit mode, the parent of the search input changes shape; `text_input::Id` focus is invalidated, the user loses keyboard focus mid-edit.

**Fix**: render an unconditional `column![extra, sep, header]` shape, with zero-sized placeholders in the read-only branch.

```rust
let extra: Element<'a, QueueMessage> = if let Some((ref name, _)) = data.edit_mode_info {
    // ... build edit_bar ...
    edit_bar.into()
} else if let Some(ref ctx) = data.playlist_context_info {
    // ... build playlist_bar ...
    playlist_bar.into()
} else {
    iced::widget::Space::new(Length::Shrink, Length::Fixed(0.0)).into()
};
let sep: Element<'a, QueueMessage> = if data.edit_mode_info.is_some() || data.playlist_context_info.is_some() {
    crate::theme::horizontal_separator(1.0)
} else {
    iced::widget::Space::new(Length::Shrink, Length::Fixed(0.0)).into()
};
let header: Element<'a, QueueMessage> = column![extra, sep, header].into();
```

(Pseudocode — implementer chooses the cleanest restructuring of the existing `if … else if … else` ladder. The invariant is: every branch produces a 3-child `column!` of the same shape.)

`Space::new(width, height)` is the iced 0.14 placeholder; `Length::Fixed(0.0)` keeps it visually invisible. Verify the import — `iced::widget::Space` may need adding to the file's `use` block.

### Lane D — B6, hamburger menu indexed match + parallel const

`src/widgets/hamburger_menu.rs:401-407`:

```rust
let action = match item_index {
    0 => Some(MenuAction::ToggleLightMode),
    1 => Some(MenuAction::ToggleSoundEffects),
    2 => Some(MenuAction::OpenSettings),
    3 => Some(MenuAction::About),
    4 => Some(MenuAction::Quit),
    _ => None,
};
```

Paired with separate consts at `:315-316`:

```rust
const MENU_ITEM_COUNT: usize = 5;
const SEPARATOR_INDEX: usize = 4;
```

And a parallel labels array at `:456-476` (5 tuples).

**Fix**: introduce a single `const MENU_ITEMS: &[MenuAction]` slice. Derive `MENU_ITEM_COUNT` from `MENU_ITEMS.len()`. Replace the indexed `match` with `MENU_ITEMS.get(item_index).copied()`.

```rust
const MENU_ITEMS: &[MenuAction] = &[
    MenuAction::ToggleLightMode,
    MenuAction::ToggleSoundEffects,
    MenuAction::OpenSettings,
    MenuAction::About,
    MenuAction::Quit,
];
const MENU_ITEM_COUNT: usize = MENU_ITEMS.len();
```

Then `:401`:

```rust
let action = MENU_ITEMS.get(item_index).copied();
```

`SEPARATOR_INDEX` stays a manual `4` because it expresses "draw separator before this item index" (not derivable from `MENU_ITEMS` alone). Anchor it with a const-assert so reordering breaks the build:

```rust
const _: () = assert!(SEPARATOR_INDEX < MENU_ITEM_COUNT);
const _: () = assert!(matches!(MENU_ITEMS[MENU_ITEM_COUNT - 1], MenuAction::Quit));
```

(`matches!` in `const` requires Rust 1.79+; nokkvi already requires nightly for fmt, but `const matches!` is on stable since 1.79 — verify the workspace's `rust-toolchain` allows it. Fall back to a runtime debug_assert in a `#[test]` if not.)

The labels array at `:456-476` stays a literal — its tuples carry per-item runtime state (`is_light_mode`, `sfx_enabled`) that doesn't fit in a const `&[MenuAction]`. Add a `debug_assert_eq!(items.len(), MENU_ITEM_COUNT)` inside `draw()` so the labels array can never silently fall out of sync.

`MenuAction` must be `Copy` for `MENU_ITEMS.get(...).copied()`. If it isn't already, add `#[derive(Copy, Clone)]` (it likely already is — confirm before committing).

### Lane E — B8 (rename test) + B9 (delete stale comment)

**B8** — `src/update/tests/navigation.rs:1043`:

```rust
#[test]
fn albums_loaded_re_pins_selected_offset_for_artist() {
    // ... body operates on app.artists_page and ArtistsMessage::AlbumsLoaded ...
}
```

Audit-suggested rename: `artists_albums_loaded_re_pins_selected_offset_in_artists_view`. The new name says: "in the Artists view, when AlbumsLoaded fires for an artist's children, the selected offset is re-pinned." Test body unchanged.

**B9** — `src/update/mod.rs:230-238`:

The comment block currently reads:

```rust
// -----------------------------------------------------------------
// Loader Results (per-domain *LoaderMessage)
//
// These route to per-domain `dispatch_<domain>_loader` helpers in
// `update/<domain>.rs`. Phase 1 wires all six; Genres is the
// proof-of-concept and is fully migrated. The other five are stubs
// (`unimplemented!()`) until Phase 2 fills them in — currently
// unreachable because no fire site constructs the new variants
// for those domains.
// -----------------------------------------------------------------
```

Per audit-progress §5 row B9: Phase 2 is complete (commits `31374ec..bc53b17` landed pre-audit). The "stubs" / "unreachable" claim is now false. Replace with a one-line accurate description, e.g.:

```rust
// -----------------------------------------------------------------
// Loader Results (per-domain *LoaderMessage) — route to
// dispatch_<domain>_loader helpers in update/<domain>.rs.
// -----------------------------------------------------------------
```

Or delete the block outright if the section header is clear from the variant names. Implementer's call. Verify the `Note: the loader-result variants have migrated to ...` comments at `:250`, `:260`, `:268` are still accurate (they should be — they describe past migration, not current incompleteness).

Two trivial edits — one or two commits, implementer's call (one bundled commit is fine for a worktree).

### Lane F — §7 #2, View::ALL + wildcard `_ =>` sweep

Two layered changes per audit §4 #1.1 and §4 #1.2:

**(F.1) Add `View::ALL` and `NavView::ALL`** with length-anchored const-asserts.

`src/main.rs` after the `View` enum at line 53:

```rust
impl View {
    pub const ALL: &'static [View] = &[
        View::Albums,
        View::Queue,
        View::Songs,
        View::Artists,
        View::Genres,
        View::Playlists,
        View::Radios,
        View::Settings,
    ];
}

// Length anchor: adding a variant without extending `ALL` fails to compile.
const _: [(); 8 - View::ALL.len()] = [];
const _: [(); View::ALL.len() - 8] = [];
```

(The double const-assert pins `View::ALL.len() == 8` exactly; a single subtraction would pass either direction. Two opposite-direction const arrays force equality.)

`src/widgets/nav_bar.rs` after the `NavView` enum at line 31:

```rust
impl NavView {
    pub const ALL: &'static [NavView] = &[
        NavView::Queue,
        NavView::Albums,
        NavView::Artists,
        NavView::Songs,
        NavView::Genres,
        NavView::Playlists,
        NavView::Radios,
    ];
}

const _: [(); 7 - NavView::ALL.len()] = [];
const _: [(); NavView::ALL.len() - 7] = [];
```

`BrowsingView::ALL` already exists at `src/views/browsing_panel.rs:77` per `grep` — leave it.

**(F.2) Replace 8 wildcard `_ =>` arms over `View` matches** with explicit listings. The audit's `drift-match-arms.md` §3a enumerates them. After code drift since the audit, the current line numbers are:

| File | Current line | Wildcard | Explicit replacement |
|---|---:|---|---|
| `src/update/navigation.rs` | 171 | `_ => self.prefetch_viewport_artwork(),` (inside `handle_switch_view`) | `View::Albums \| View::Artists \| View::Songs \| View::Genres \| View::Playlists \| View::Radios => self.prefetch_viewport_artwork(),` (Queue/Settings handled above by `if`-guarded arms) |
| `src/update/navigation.rs` | 484 | `_ => {}` (NavigateAndFilter filter assignment) | `View::Queue \| View::Playlists \| View::Radios \| View::Settings => {}` |
| `src/update/navigation.rs` | 493 | `_ => Task::none(),` (NavigateAndFilter load) | `View::Queue \| View::Playlists \| View::Radios \| View::Settings => Task::none(),` |
| `src/update/navigation.rs` | 1086 | `_ => None,` (browser pane View → BrowsingView) | `View::Queue \| View::Playlists \| View::Radios \| View::Settings => None,` |
| `src/update/navigation.rs` | 1120 | `_ => {}` (browser pane filter) | `View::Queue \| View::Playlists \| View::Radios \| View::Settings => {}` |
| `src/update/navigation.rs` | 1129 | `_ => Task::none(),` (browser pane load) | `View::Queue \| View::Playlists \| View::Radios \| View::Settings => Task::none(),` |
| `src/update/window.rs` | 115 | `_ => Task::none(),` (prefetch_viewport_artwork) | `View::Playlists \| View::Settings => Task::none(),` (Genres/Playlists fall through to the `if !is_empty()` guards above; the wildcard catches Settings + the `else` branches of Genres/Playlists) — see notes below |
| `src/update/components.rs` | 681 | `_ => None,` (View → BrowsingView) | `View::Queue \| View::Playlists \| View::Radios \| View::Settings => None,` |

**Caveat about `update/window.rs:115`**: the match has guarded arms (`View::Genres if !self.library.genres.is_empty()`, `View::Playlists if !self.library.playlists.is_empty()`). When the guard is false, control falls through to the next arm. Replacing `_ =>` with an explicit listing must also account for the guard fall-through. Two equivalent shapes:

  (a) Add unguarded sibling arms: `View::Genres \| View::Playlists => Task::none(), View::Settings => Task::none(),` — explicit and keeps current behavior.

  (b) Restructure to `View::Genres => { if !empty { … } else { Task::none() } }` and explicit Settings — slightly more verbose but the falsy guard path is no longer implicit.

  Pick (a) for minimum churn unless clippy complains.

**Beyond the audit's headline 8**: while sweeping, also fix the additional View-match wildcards visible in the same audit table:

| File | Current line | Wildcard | Notes |
|---|---:|---|---|
| `src/update/playback.rs` | 1209 | `_ => Task::none(),` (start_view_task data-load) | View match — replace with `crate::View::Queue \| crate::View::Radios \| crate::View::Settings => Task::none(),` |
| `src/update/hotkeys/navigation.rs` | 315 | `_ => Task::none(),` (CenterOnPlaying in-buffer dispatch) | replace with explicit `View::Playlists \| View::Settings => Task::none(),` |
| `src/update/hotkeys/navigation.rs` | 336 | `_ => Task::none(),` (CenterOnPlaying off-buffer fallback) | replace with `View::Queue \| View::Playlists \| View::Radios \| View::Settings => Task::none(),` |
| `src/views/sort_api.rs` | 81 | `(_, _) => "name",` (final tuple catch-all) | replace with `(View::Queue \| View::Radios \| View::Settings, _) => "name",` — leaves the per-view `(V::Foo, _)` defaults at 42, 49, 64, 70, 77 alone (those are deliberate per-view fallbacks, documented in the doc comment) |

**Out of scope for Lane F**: the per-view `(V::Albums, _) => "recentlyAdded"` etc. wildcards in `sort_api.rs` (lines 42, 49, 64, 70, 77). These are intentional per-view defaults documented in the file's doc comment ("Per-view fallbacks preserve historical behavior"). The audit's recommendation for them is the SortMode TABLE-based encoding (Drift #6), a separate audit item.

**Line-number drift note**: the lines above were re-verified at `8965832`. Before editing, agents should confirm with a quick grep — code may have moved by another small amount.

`src/update/playback.rs:1190` is a `match settings.start_view.as_str()` (string match, NOT a `View` enum match). Leave it. The audit's `drift-match-arms.md` §3a explicitly excludes it.

---

## 4. Verification (every lane)

Per `.agent/workflows/build-test.md` and `commit.md`. Run after each commit slice:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before pushing the slice. Lane F should additionally run `cargo test --bin nokkvi -- view` to exercise the View-related tests if any exist.

---

## 5. What each lane does NOT do

- **Lane A**: does NOT touch any `HoverOverlay::new(container(...))` site that already follows the rule. Does NOT introduce a `NotAButton` marker trait (audit suggested it as a long-term defense, but it's a separate item — out of scope here).
- **Lane B**: does NOT touch deliberate non-themed radii (login card `12.0.into()`; any `pill-shaped` badges). Does NOT introduce a `dyn ThemeRadius` abstraction.
- **Lane C**: does NOT refactor the queue-header builder beyond making its widget-tree shape unconditional. The 942-LOC `view()` split is a separate audit item.
- **Lane D**: does NOT introduce a per-item enum variant carrying labels — that's a bigger refactor. Goal is just "single source of truth for the click-dispatch order."
- **Lane E**: does NOT migrate test bodies, does NOT touch other comments in `update/mod.rs` even if stale-looking. Scope is exactly the named test + the named comment block.
- **Lane F**: does NOT migrate slot_list / roulette per-View dispatch onto `ViewPage` (audit §7 #9 — separate L-effort lane). Does NOT introduce table-driven `VIEW_LOADERS`. Does NOT introduce the `enum ItemKind` (§7 #5). Does NOT touch the per-view `(V::Foo, _) => default` arms in `sort_api.rs` (Drift #6, separate). Does NOT add `Screen::ALL` or other enum ALLs beyond `View` and `NavView`.

No new dependencies in any lane (per `code-standards.md`). No reformatting outside touched files. No drive-by docstring rewrites unrelated to the bug.

---

## 6. After the lanes land

Append commit refs to `.agent/audit-progress.md`:
- §7 row 1 (Bug fixes batch) → flip to ✅ done with commit refs from Lanes A–E.
- §7 row 2 (View::ALL + wildcards) → flip to ✅ done with commit refs from Lane F.
- §5 rows B1, B2, B3, B6, B8, B9 → flip to ✅ done.
- §4 row 1 (View enum match-block fanout) → flip to 🟡 partial (wildcards closed; the per-View dispatch onto `ViewPage` from §7 #9 remains open). Update the note to reflect that Lane F is the partial close.
- §4 row 14 (Missing `View::ALL` / `NavView::ALL`) → flip to ✅ done.

Skip the `Co-Authored-By` trailer on every commit per global instructions.

---

## Fanout Prompts

### bug-hover-overlay

worktree: ~/nokkvi-cleanup-bug-hover-overlay
branch: fix/cleanup-hover-overlay
effort: max
permission-mode: bypassPermissions

````
Task: fix B1 — three `HoverOverlay::new(button(...))` re-trips of the documented gotcha.

Plan doc: /home/foogs/nokkvi/.agent/plans/cleanup-batch.md (sections 1, 2, 3 "Lane A").

Working directory: ~/nokkvi-cleanup-bug-hover-overlay (this worktree). Branch: fix/cleanup-hover-overlay. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` should show `8965832` or a descendant on `main`.
- `grep -rn 'HoverOverlay::new' --include='*.rs' src/` enumerate sites. The three this lane fixes are in `src/widgets/nav_bar.rs`, `src/widgets/side_nav_bar.rs`, and `src/views/login.rs`. If the line numbers in the plan have shifted by more than ~5 lines, locate by structural context (`button(...)` immediately wrapped by `HoverOverlay::new(`) before editing.
- Read `.agent/rules/gotchas.md` (the HoverOverlay rule line) and `.agent/rules/widgets.md` (the same rule, restated). Read `src/views/queue/view.rs:222-250` (the `icon_btn` closure) for the canonical correct shape used elsewhere in the codebase.

### 2. Fix each site

The pattern is:

```
HoverOverlay::new(
    button(content)
        .on_press(MSG)
        .padding(P)
        .style(BUTTON_STYLE),
)
```

Becomes:

```
mouse_area(
    HoverOverlay::new(
        container(content)
            .padding(P)
            .style(CONTAINER_STYLE_EQUIV),
    ),
)
.on_press(MSG)
.interaction(iced::mouse::Interaction::Pointer)
```

`button::Style` → `container::Style` field translation:
- `background: Some(...)` — identical.
- `text_color: ...` (button) → `text_color: Some(...)` (container; `Option<Color>`).
- `border: ...` — identical.
- `shadow: ...` — identical.

If the existing `button::Style` closure branched on `Status::Hovered` / `Status::Pressed`, pick the resting/`Active` style for the container — `HoverOverlay` provides the press-scale animation; the underlying container draws the static visual.

#### Site 1: `src/widgets/nav_bar.rs:327`

The flat-mode tab. Currently `HoverOverlay::new(button(tab_content(...)).on_press(NavBarMessage::SwitchView(view)).padding(tab_padding).height(Length::Fill).style(tab_style))`. Translate `tab_style` (a `button::Style` closure) to a `container::Style` closure. The rounded-mode branch above (line 307) already uses `mouse_area(HoverOverlay::new(container(...)))` — mirror its shape exactly.

#### Site 2: `src/widgets/side_nav_bar.rs:252`

Side nav vertical tab. Same translation. `tab_style` becomes a `container::Style` closure with the same active/inactive logic, dropping the `Status::Hovered`/`Status::Pressed` branches if any.

#### Site 3: `src/views/login.rs:301`

Login button. Currently `HoverOverlay::<'_, LoginMessage>::new(button(text(if login_in_progress { "Connecting..." } else { "Login" }).width(Length::Fill).align_x(Center)).on_press(LoginMessage::LoginPressed).padding(14).width(input_width).style(...))`. The button's style (lines ~315-326) sets `background: Some(theme::accent().into())`, `text_color: theme::bg0_hard()`, `border: { color: theme::accent_border_light(), width: 1.0, radius: theme::ui_border_radius() }`. Translate to a `container::Style` closure with those same fields. Wrap in `mouse_area(...).on_press(LoginMessage::LoginPressed).interaction(Pointer)`.

### 3. Imports

Add `mouse_area` (and `container` if missing) to each file's `iced::widget::{...}` use list.

### 4. Verify

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass.

### 5. Commit

One commit. Suggested message:

    fix(widgets): wrap HoverOverlay around containers, not buttons (B1)

    Three sites violated the documented gotcha — native button captures
    ButtonPressed before HoverOverlay's match arm runs, so the press-scale
    animation misfires:

    - src/widgets/nav_bar.rs:327 (flat-mode tab)
    - src/widgets/side_nav_bar.rs:252 (vertical side-nav tab)
    - src/views/login.rs:301 (login button)

    Each now uses the canonical mouse_area(HoverOverlay::new(container(...)))
    shape with the press emitted by mouse_area, mirroring the existing
    correct usage in src/views/queue/view.rs::icon_btn.

    Closes audit B1 (.agent/audit-progress.md §5).

Skip the `Co-Authored-By` trailer per ~/.claude/CLAUDE.md.

### 6. Update audit tracker

Append the commit ref to `.agent/audit-progress.md` §5 row B1 and flip the status to ✅ done. Do not flip §7 #1 unless every other bug in the batch (B2, B3, B6, B8, B9) has also landed.

## What NOT to touch

- Other `HoverOverlay::new` sites that already wrap a `container(...)` correctly.
- The `NotAButton` marker trait the audit suggested as long-term defense — separate item.
- B2's hardcoded radii in the same `login.rs` file — Lane B's territory.
- F's NavView::ALL declaration in `nav_bar.rs` — Lane F's territory.

## If blocked

- If `button::Style` has dynamic per-status visuals that don't translate cleanly to a single static container style: stop and report what you saw. The correct fallback may be to add a `if is_pressed { ... }` branch inside the container closure based on a `mouse_area` interaction observer, but the existing `icon_btn` precedent in `views/queue/view.rs` suggests static styles work fine.
- If the build fails on a `tab_style` closure type mismatch: the `button::Style` and `container::Style` types are incompatible — you must rewrite the closure body, not just rebind it.

## Reporting

End with: commit ref + subject, files changed, line counts.
````

### bug-themed-radii

worktree: ~/nokkvi-cleanup-bug-themed-radii
branch: fix/cleanup-themed-radii
effort: max
permission-mode: bypassPermissions

````
Task: fix B2 — five hardcoded `radius: 4.0.into()` sites bypass `theme::ui_border_radius()`.

Plan doc: /home/foogs/nokkvi/.agent/plans/cleanup-batch.md (section 3 "Lane B").

Working directory: ~/nokkvi-cleanup-bug-themed-radii (this worktree). Branch: fix/cleanup-themed-radii. Worktree pre-created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `8965832` or a descendant on `main`.
- `grep -rn 'radius: 4.0.into()' --include='*.rs' src/` should return exactly 5 sites:
  - `src/views/login.rs:226`
  - `src/views/login.rs:253`
  - `src/views/login.rs:282`
  - `src/widgets/info_modal.rs:559`
  - `src/widgets/info_modal.rs:565`
- If the count is off, locate before editing — code may have shifted slightly.

### 2. Replace each site

At each of the 5 lines: `radius: 4.0.into(),` → `radius: theme::ui_border_radius(),`.

`theme::ui_border_radius()` is defined at `src/theme.rs:285` and returns `iced::border::Radius`, which is exactly the field type. Both files already use other `theme::*` accessors in scope, so no import change needed.

### 3. Verify

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

### 4. Commit

One commit. Suggested message:

    fix(views,widgets): use theme::ui_border_radius for input borders (B2)

    Five hardcoded `radius: 4.0.into()` border radii bypassed
    theme::ui_border_radius() and ignored squared-mode:

    - src/views/login.rs:226,253,282 (server URL / username / password
      text_input borders)
    - src/widgets/info_modal.rs:559,565 (scrollable rail and scroller
      borders)

    Login card radius (12.0) is intentionally distinct and unchanged.

    Closes audit B2 (.agent/audit-progress.md §5).

Skip the `Co-Authored-By` trailer.

### 5. Update audit tracker

Append the commit ref to `.agent/audit-progress.md` §5 row B2 and flip the status to ✅ done.

## What NOT to touch

- Login card `radius: 12.0.into()` at `login.rs:343` — intentional non-themed radius for the card visual.
- Any other hardcoded radii not in the 5-site list.
- Lane A's HoverOverlay changes in the same `login.rs` file (different lines).

## If blocked

- If a site's radius is genuinely meant to be unthemed (intentional pill/badge): leave it AND add a one-line `// intentional non-themed radius — <reason>` comment. Document the why in your final report so we can decide whether to update the audit notes.

## Reporting

End with: commit ref + subject, the 5 lines changed.
````

### bug-queue-header

worktree: ~/nokkvi-cleanup-bug-queue-header
branch: fix/cleanup-queue-header
effort: max
permission-mode: bypassPermissions

````
Task: fix B3 — queue header widget-tree shape morphs across edit / playlist-context / read-only modes, invalidating `text_input::Id` focus.

Plan doc: /home/foogs/nokkvi/.agent/plans/cleanup-batch.md (section 3 "Lane C").

Working directory: ~/nokkvi-cleanup-bug-queue-header (this worktree). Branch: fix/cleanup-queue-header. Worktree pre-created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `8965832` or a descendant on `main`.
- Read `src/views/queue/view.rs:165-455`. The header builder branches three ways:
  - `if let Some(... edit_mode_info) = ... { ... column![edit_bar, sep_bottom, header].into() }` (~line 340)
  - `else if let Some(ref ctx) = data.playlist_context_info { ... column![playlist_bar, sep_bottom, header].into() }` (~line 450)
  - `else { header }` (~line 452)
- The third branch produces a 1-deep widget tree; the other two produce 3-deep. iced reconciles positionally — when a user toggles edit mode, the parent of the search input changes shape and `text_input::Id` focus is lost.

### 2. Restructure for unconditional 3-deep shape

Refactor the `if … else if … else` so every branch produces the same `column![extra, sep, header]` shape. Pseudocode:

```rust
let extra: Element<'a, QueueMessage> = if let Some((ref name, _)) = data.edit_mode_info {
    // ... existing edit_bar construction ...
    edit_bar.into()
} else if let Some(ref ctx) = data.playlist_context_info {
    // ... existing playlist_bar construction ...
    playlist_bar.into()
} else {
    iced::widget::Space::new(Length::Shrink, Length::Fixed(0.0)).into()
};
let sep: Element<'a, QueueMessage> = if data.edit_mode_info.is_some() || data.playlist_context_info.is_some() {
    crate::theme::horizontal_separator(1.0)
} else {
    iced::widget::Space::new(Length::Shrink, Length::Fixed(0.0)).into()
};
let header: Element<'a, QueueMessage> = column![extra, sep, header].into();
```

(Adapt to the existing variable names — `header` is the inner header from `view_header(...)` already at line 100ish; rename to avoid the shadow if needed, e.g., `inner_header`.)

The implementation MUST satisfy:
- Three children always: `column![extra, sep, header]`.
- In read-only mode, `extra` and `sep` are zero-sized `Space::new(Length::Shrink, Length::Fixed(0.0))`.
- Visual output unchanged in all three modes (the zero-sized spacers must not introduce extra padding/spacing).

Verify the import — `iced::widget::Space` may need adding to the file's existing `use iced::widget::{...}` list at line 9.

### 3. Verify

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

### 4. Commit

One commit. Suggested message:

    fix(views): stabilize queue header widget-tree shape across modes (B3)

    The queue header morphed depth across edit / playlist-context /
    read-only modes — `column![extra, sep, header]` in two branches,
    bare `header` in the third. iced reconciles positionally, so the
    parent of the search `text_input` changed shape on edit-mode
    toggle and the input lost focus mid-edit.

    Always render `column![extra, sep, header]`; the read-only branch
    uses zero-sized `Space::new(Length::Shrink, Length::Fixed(0.0))`
    placeholders so visual output is unchanged but widget-tree depth
    is constant.

    Closes audit B3 (.agent/audit-progress.md §5).

Skip the `Co-Authored-By` trailer.

### 5. Update audit tracker

Append the commit ref to `.agent/audit-progress.md` §5 row B3 and flip the status to ✅ done.

## What NOT to touch

- The 942-LOC `view()` split (audit recommends `build_header`/`build_render_item`/`build_artwork_column`) — separate audit item.
- Any other widget-tree shape in the file.
- `compose_header_with_select` (line ~460) — it wraps the result of this branch and is unaffected.

## If blocked

- If `Space::new(...)` is not the iced 0.14 spelling, the compiler error will give the right type. Substitute with `iced::widget::Space::with_height(0.0)` or whichever the workspace's iced version uses. If genuinely stuck, stop and report.
- If the visual output regresses (e.g., extra spacing introduced by the column's default spacing of the Space children): zero out via `column![extra, sep, header].spacing(0)` or set `Length::Fixed(0.0)` on both axes of the Space.

## Reporting

End with: commit ref + subject, restructured shape (a brief snippet showing the new branch structure), confirmation that `cargo test` includes the queue tests.
````

### bug-hamburger-menu

worktree: ~/nokkvi-cleanup-bug-hamburger
branch: fix/cleanup-hamburger-menu
effort: max
permission-mode: bypassPermissions

````
Task: fix B6 — hamburger menu indexed `match` paired with separate `MENU_ITEM_COUNT` const allows silent action drift on reorder.

Plan doc: /home/foogs/nokkvi/.agent/plans/cleanup-batch.md (section 3 "Lane D").

Working directory: ~/nokkvi-cleanup-bug-hamburger (this worktree). Branch: fix/cleanup-hamburger-menu. Worktree pre-created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `8965832` or a descendant on `main`.
- Read `src/widgets/hamburger_menu.rs:308-318` (the constants), `:399-408` (the indexed match), `:455-477` (the parallel labels array).

### 2. Introduce `MENU_ITEMS` slice

Replace the indexed match with a single source of truth for the click-dispatch order. After the existing constants block (~line 318), add:

```rust
/// Click-dispatch order for hamburger menu items. Index is the visual
/// position; `SEPARATOR_INDEX` is the position before which the divider
/// is drawn. Reordering this slice is the only way to reorder the menu.
const MENU_ITEMS: &[MenuAction] = &[
    MenuAction::ToggleLightMode,
    MenuAction::ToggleSoundEffects,
    MenuAction::OpenSettings,
    MenuAction::About,
    MenuAction::Quit,
];
```

Replace the existing `const MENU_ITEM_COUNT: usize = 5;` with:

```rust
const MENU_ITEM_COUNT: usize = MENU_ITEMS.len();
```

Add a const-assert pinning the separator anchor. After the constants block:

```rust
const _: () = assert!(SEPARATOR_INDEX < MENU_ITEM_COUNT);
const _: () = assert!(matches!(MENU_ITEMS[MENU_ITEM_COUNT - 1], MenuAction::Quit));
```

(`matches!` in a `const` context requires Rust 1.79+. Verify nokkvi's `Cargo.toml` `rust-version` or `rust-toolchain` allows it. If not, downgrade to a `#[test]` that asserts the same — `assert_eq!(MENU_ITEMS.last(), Some(&MenuAction::Quit))` — and call out the reason in the commit body.)

`MenuAction` must be `Copy` for `MENU_ITEMS.get(...).copied()` to work. If it isn't already `Copy`, derive it (`MenuAction` is a small unit-variant enum — adding `Copy` is fine). Verify before editing.

### 3. Replace the indexed match (line ~401-407)

Currently:

```rust
let action = match item_index {
    0 => Some(MenuAction::ToggleLightMode),
    1 => Some(MenuAction::ToggleSoundEffects),
    2 => Some(MenuAction::OpenSettings),
    3 => Some(MenuAction::About),
    4 => Some(MenuAction::Quit),
    _ => None,
};
```

Replace with:

```rust
let action = MENU_ITEMS.get(item_index).copied();
```

### 4. Anchor the labels array

The `items: [(&str, bool); MENU_ITEM_COUNT]` array at line ~456 carries per-item runtime labels (state-dependent like "Dark Mode" vs "Light Mode"). Leave it as a literal — the label text varies and doesn't fit in a `const &[MenuAction]`. But add a `debug_assert_eq!(items.len(), MENU_ITEM_COUNT, "labels array out of sync with MENU_ITEMS")` at the top of `draw()` after the array literal so the parallel arrays cannot silently fall out of sync.

### 5. Verify

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

### 6. Commit

One commit. Suggested message:

    fix(widgets): single-source MENU_ITEMS slice for hamburger menu (B6)

    The indexed `match item_index { 0 => ToggleLightMode, ... 4 => Quit }`
    was paired with a separate `MENU_ITEM_COUNT = 5` const and a
    parallel labels array. Reordering items today silently fires the
    wrong action — the constants and the indices are not coupled.

    Introduce `const MENU_ITEMS: &[MenuAction]` as the single source of
    truth for click-dispatch order. Derive `MENU_ITEM_COUNT` from
    `MENU_ITEMS.len()`. Replace the indexed match with
    `MENU_ITEMS.get(item_index).copied()`. Anchor `SEPARATOR_INDEX`
    and the Quit-at-end invariant with const-asserts. The labels
    array stays a literal (per-item runtime-derived text) but a
    `debug_assert_eq!` pins its length to `MENU_ITEM_COUNT`.

    Closes audit B6 (.agent/audit-progress.md §5).

Skip the `Co-Authored-By` trailer.

### 7. Update audit tracker

Append the commit ref to `.agent/audit-progress.md` §5 row B6 and flip the status to ✅ done. Also update §4 row 11 (Hamburger menu match-arms) to ✅ done.

## What NOT to touch

- The labels-with-runtime-state `items` array — keep it as a literal (the labels are state-derived, not constants).
- The `MenuOverlay::layout` / `MenuOverlay::draw` rendering logic beyond the new `debug_assert_eq!`.
- The `MENU_WIDTH` / `MENU_ITEM_HEIGHT` / etc. visual constants.

## If blocked

- If `MenuAction` cannot be `Copy` for some reason (e.g., contains a `String`): drop the `.copied()` and use `.cloned()` or `MENU_ITEMS[item_index].clone()` after a bounds check.
- If `const matches!` isn't supported by the workspace's MSRV: substitute a `#[test]` that asserts the same invariant. Note this in the commit body.

## Reporting

End with: commit ref + subject, the new `MENU_ITEMS` declaration, confirmation that the click test passes (or note if no test exists for hamburger clicks).
````

### bug-rename-and-comment

worktree: ~/nokkvi-cleanup-rename-comment
branch: fix/cleanup-rename-and-comment
effort: max
permission-mode: bypassPermissions

````
Task: fix B8 (rename misnamed test) and B9 (delete stale comment about *LoaderMessage migration).

Plan doc: /home/foogs/nokkvi/.agent/plans/cleanup-batch.md (section 3 "Lane E").

Working directory: ~/nokkvi-cleanup-rename-comment (this worktree). Branch: fix/cleanup-rename-and-comment. Worktree pre-created — do NOT run `git worktree add`.

## What to do

### B8: rename misnamed test

`src/update/tests/navigation.rs:1043` has:

```rust
#[test]
fn albums_loaded_re_pins_selected_offset_for_artist() {
```

The body operates on `app.artists_page` and dispatches `ArtistsMessage::AlbumsLoaded`. The test name says "albums_loaded ... for_artist" which is confusing; the audit-suggested rename is `artists_albums_loaded_re_pins_selected_offset_in_artists_view`.

Rename the function. No other change. Confirm there are no other references to the old name (`grep -rn 'albums_loaded_re_pins_selected_offset_for_artist' .` should return zero hits after the rename).

### B9: delete or rewrite stale comment

`src/update/mod.rs:230-238` currently reads (verify the exact text before editing):

```
// -----------------------------------------------------------------
// Loader Results (per-domain *LoaderMessage)
//
// These route to per-domain `dispatch_<domain>_loader` helpers in
// `update/<domain>.rs`. Phase 1 wires all six; Genres is the
// proof-of-concept and is fully migrated. The other five are stubs
// (`unimplemented!()`) until Phase 2 fills them in — currently
// unreachable because no fire site constructs the new variants
// for those domains.
// -----------------------------------------------------------------
```

Per `.agent/audit-progress.md` §5 row B9, Phase 2 is fully landed (commits `31374ec..bc53b17` pre-audit). The "stubs (`unimplemented!()`) until Phase 2" claim is now false.

Replace with a one-line accurate description, e.g.:

```
// -----------------------------------------------------------------
// Loader Results (per-domain *LoaderMessage) — route to
// dispatch_<domain>_loader helpers in update/<domain>.rs.
// -----------------------------------------------------------------
```

Or delete the block entirely if the section is self-evident from the variant names. Implementer's call. Verify the `Note: ... migrated to ...` comments at `:250`, `:260`, `:268` are still accurate (they describe past completed migration, which is still true).

### Verify

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

`cargo test` exercises the renamed test by its new name automatically (pre-rename it ran by the old name; post-rename it runs by the new name; cargo cares about the function existence, not its identity).

### Commit

One commit covering both fixes:

    docs: rename misleading test, refresh stale loader comment (B8 + B9)

    - Rename `albums_loaded_re_pins_selected_offset_for_artist` to
      `artists_albums_loaded_re_pins_selected_offset_in_artists_view`.
      Test body operates on Artists; old name said "albums".
    - Refresh the comment at update/mod.rs:230-238 that still claims
      Phase 2 of the *LoaderMessage migration is incomplete. Phase 2
      landed in commits 31374ec..bc53b17 (pre-audit).

    Closes audit B8 + B9 (.agent/audit-progress.md §5).

(Acceptable to use two commits if you prefer. Either way, skip the `Co-Authored-By` trailer.)

### Update audit tracker

Append the commit ref(s) to `.agent/audit-progress.md` §5 rows B8 and B9, flip both to ✅ done.

## What NOT to touch

- Any other comment in `update/mod.rs` even if it looks stale — scope is exactly the named block at 230-238.
- Other test names in `tests/navigation.rs` — scope is exactly the named test.
- The body of the renamed test.

## If blocked

- If the comment text has drifted from what's quoted in the plan: read the current text and apply the same logic — Phase 2 is done, so any "stubs"/"unimplemented"/"Phase 2 will fill these in" language is wrong and should be removed.
- If `cargo test` fails for an unrelated reason: stop and report.

## Reporting

End with: commit ref(s), the new test name, the rewritten/deleted comment text.
````

### refactor-view-all

worktree: ~/nokkvi-cleanup-view-all
branch: refactor/view-all-anchor
effort: max
permission-mode: bypassPermissions

````
Task: implement §7 #2 — add `View::ALL` and `NavView::ALL` length-anchored constants, then replace every wildcard `_ =>` arm in `match` blocks over `View` with explicit listings.

Plan doc: /home/foogs/nokkvi/.agent/plans/cleanup-batch.md (sections 3 "Lane F", and the audit details in `~/nokkvi-audit-results/drift-match-arms.md` §3a).

Working directory: ~/nokkvi-cleanup-view-all (this worktree). Branch: refactor/view-all-anchor. Worktree pre-created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `8965832` or a descendant on `main`.
- `grep -rn 'View::ALL\|impl View {' --include='*.rs' src/` should return zero hits (no existing `View::ALL`).
- `grep -rn 'NavView::ALL\|impl NavView {' --include='*.rs' src/` should return zero hits.
- `grep -n 'pub enum View' src/main.rs` finds the enum at line 53.
- `grep -n 'pub enum NavView' src/widgets/nav_bar.rs` finds the enum at line 31.

### 2. Add `View::ALL` (slice 1)

In `src/main.rs`, after the `View` enum (line 53), add:

```rust
impl View {
    /// Every `View` variant. Length-anchored — see the `const _:` lines below.
    pub const ALL: &'static [View] = &[
        View::Albums,
        View::Queue,
        View::Songs,
        View::Artists,
        View::Genres,
        View::Playlists,
        View::Radios,
        View::Settings,
    ];
}

// Length anchor: adding a `View` variant without extending `ALL` fails to
// compile. Both directions are needed — a single subtraction passes if
// either side is too small.
const _: [(); 8 - View::ALL.len()] = [];
const _: [(); View::ALL.len() - 8] = [];
```

Variant order in `ALL`: match the declaration order in the enum (Albums, Queue, Songs, Artists, Genres, Playlists, Radios, Settings).

### 3. Add `NavView::ALL` (slice 1, same commit)

In `src/widgets/nav_bar.rs`, after the `NavView` enum (line 31), add:

```rust
impl NavView {
    pub const ALL: &'static [NavView] = &[
        NavView::Queue,
        NavView::Albums,
        NavView::Artists,
        NavView::Songs,
        NavView::Genres,
        NavView::Playlists,
        NavView::Radios,
    ];
}

const _: [(); 7 - NavView::ALL.len()] = [];
const _: [(); NavView::ALL.len() - 7] = [];
```

`BrowsingView::ALL` already exists at `src/views/browsing_panel.rs:77` — leave it untouched.

### 4. Verify slice 1

```
cargo build && cargo clippy --all-targets -- -D warnings
```

If both pass, commit slice 1:

    refactor(views): add View::ALL and NavView::ALL length-anchored slices

    Foundational anchor for the View / NavView enum touch-fanout. The
    paired const-asserts force a build break if a variant is added
    without extending ALL — converting one silent-drift class into
    a compiler error.

    Pairs with the wildcard sweep that follows in this branch.

    Part of audit §7 #2 (.agent/audit-progress.md).

### 5. Wildcard sweep — `update/navigation.rs` (slice 2)

Six `_ =>` arms in `match` blocks where the scrutinee is a `View` value. Current line numbers (verify with grep before editing):

- Line ~171, inside `handle_switch_view`, the `_ => self.prefetch_viewport_artwork(),` after the per-view `if-empty` arms. Replace with:

  ```rust
  View::Albums | View::Artists | View::Songs | View::Genres | View::Playlists | View::Radios => self.prefetch_viewport_artwork(),
  ```

  Queue and Settings are already handled by the unguarded earlier arms.

- Line ~484, inside `handle_navigate_and_filter`, the `_ => {}` filter-assignment fall-through. Replace with:

  ```rust
  View::Queue | View::Playlists | View::Radios | View::Settings => {}
  ```

- Line ~493, inside `handle_navigate_and_filter`, the `_ => Task::none(),` load fall-through. Replace with:

  ```rust
  View::Queue | View::Playlists | View::Radios | View::Settings => Task::none(),
  ```

- Line ~1086, inside `handle_browser_pane_navigate_and_filter`, the `_ => None,` View → BrowsingView fall-through. Replace with:

  ```rust
  View::Queue | View::Playlists | View::Radios | View::Settings => None,
  ```

- Line ~1120, same function, the `_ => {}` filter fall-through. Replace with:

  ```rust
  View::Queue | View::Playlists | View::Radios | View::Settings => {}
  ```

- Line ~1129, same function, the `_ => Task::none(),` load fall-through. Replace with:

  ```rust
  View::Queue | View::Playlists | View::Radios | View::Settings => Task::none(),
  ```

NOTE the wildcards at `update/navigation.rs:638, 767, 841, 975` are NOT View matches — they match `&self.pending_expand` (`PendingExpand` enum variants). Leave them alone — they're out of scope for this lane.

Verify and commit slice 2:

```
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
```

    refactor(update): replace View wildcard arms in navigation.rs

    Six `_ =>` arms in `match` blocks over `View` are now explicit
    `View::X | View::Y => …` listings. Adding a future `View`
    variant fails to compile in each of these blocks rather than
    silently falling into the catch-all body.

    Touchpoints: handle_switch_view, handle_navigate_and_filter
    (filter + load), handle_browser_pane_navigate_and_filter
    (View → BrowsingView + filter + load).

    Part of audit §7 #2.

### 6. Wildcard sweep — `update/window.rs`, `update/components.rs`, `update/playback.rs`, `update/hotkeys/navigation.rs`, `views/sort_api.rs` (slice 3)

Five files, several wildcards. Combine into one slice unless one of them changes the test surface (none should).

- `src/update/window.rs:115`, inside `prefetch_viewport_artwork`. The match has guarded arms (`View::Genres if !is_empty`, `View::Playlists if !is_empty`). Replace `_ => Task::none(),` with explicit unguarded sibling arms covering the falsy guard branches AND Settings:

  ```rust
  View::Genres | View::Playlists | View::Settings => Task::none(),
  ```

  (Genres/Playlists are caught by the guarded arms when their library buffer is non-empty; this unguarded arm handles the empty case + Settings. Verify the resulting match is exhaustive.)

- `src/update/components.rs:681`, inside the `View → BrowsingView` mapping for browsing-pane open. Replace `_ => None,` with:

  ```rust
  crate::View::Queue | crate::View::Playlists | crate::View::Radios | crate::View::Settings => None,
  ```

- `src/update/playback.rs:1209`, inside `start_view_task` data-load match. Replace `_ => Task::none(),` with:

  ```rust
  crate::View::Queue | crate::View::Radios | crate::View::Settings => Task::none(),
  ```

  (Note: `:1190` immediately above is a `match settings.start_view.as_str()` string match — not a `View` enum match. Leave it.)

- `src/update/hotkeys/navigation.rs:315`, inside `CenterOnPlaying` in-buffer dispatch. Replace `_ => Task::none(),` with:

  ```rust
  View::Playlists | View::Settings => Task::none(),
  ```

  (Genres/Albums/Artists/Songs/Radios/Queue are all listed above.)

- `src/update/hotkeys/navigation.rs:336`, inside `CenterOnPlaying` off-buffer fallback. Replace `_ => Task::none(),` with:

  ```rust
  View::Queue | View::Playlists | View::Radios | View::Settings => Task::none(),
  ```

- `src/views/sort_api.rs:81`, the final `(_, _) => "name",` catch-all in the `(View, SortMode) → API string` tuple match. Replace with:

  ```rust
  (View::Queue | View::Radios | View::Settings | View::Login, _) => "name",
  ```

  Wait — verify `View::Login` exists. The plan and main.rs show only 8 variants without Login (Login is `Screen::Login`, not `View`). Use only the actual variants. Replace with:

  ```rust
  (View::Queue | View::Radios | View::Settings, _) => "name",
  ```

  The per-view `(V::Albums, _) => "recentlyAdded"` etc. wildcards at lines 42, 49, 64, 70, 77 are intentional per-view defaults (documented in the doc comment "Per-view fallbacks preserve historical behavior"). Leave them alone.

Verify and commit slice 3:

```
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check
```

    refactor: replace View wildcard arms across update + sort_api

    Five files swept. `_ =>` arms over `View` matches replaced by
    explicit `View::X | View::Y => …` listings — adding a future
    variant becomes a compile error rather than silently falling
    through.

    Files: update/window.rs, update/components.rs, update/playback.rs,
    update/hotkeys/navigation.rs, views/sort_api.rs.

    Per-view `(V::Foo, _) => default` arms in sort_api.rs are
    intentional per-view fallbacks (documented in the file's doc
    comment) and out of scope for this sweep — that's audit Drift #6.

    Part of audit §7 #2.

### 7. Final verification

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass. The drift-prevention test idea: optionally add `tests::view_all_covers_every_variant()` somewhere natural — a test that does `match v { View::A => true, View::B => true, ... }` for each variant in `View::ALL` so the compiler enforces equivalence between `View::ALL` and the variant set. If you can place this concisely in a test module without ceremony, do it; otherwise the const-asserts already cover the length anchor and that's enough.

### 8. Update audit tracker

Append commit refs to `.agent/audit-progress.md`:
- §7 row 2 (`View::ALL` + replace 8 wildcard `_ =>` arms) → flip to ✅ done with the slice 1+2+3 commit refs.
- §4 row 1 (View enum match-block fanout) → flip to 🟡 partial with note: "Wildcards eliminated; per-View dispatch onto `ViewPage` (§7 #9) remains open."
- §4 row 14 (Missing `View::ALL` / `NavView::ALL`) → flip to ✅ done with the slice 1 ref.

## What NOT to touch

- `update/navigation.rs:638, 767, 841, 975` — these are PendingExpand wildcards, not View. Out of scope.
- The per-view `(V::Albums, _) => "recentlyAdded"` style defaults in `sort_api.rs` (lines 42, 49, 64, 70, 77) — intentional and documented. Out of scope (covered by Drift #6, separate item).
- `update/playback.rs:1190` — string match, not enum. Audit explicitly excludes it.
- `Screen::ALL` or `BrowsingView::ALL` — out of scope; only View and NavView.
- Any per-view dispatch migration onto `ViewPage` (separate audit item §7 #9).
- Any introduction of `enum ItemKind` (§7 #5).
- Any rewiring of `*_OPTIONS` arrays (Drift #6).

## If blocked

- If a wildcard's body cannot be straightforwardly reproduced with an explicit listing because the scrutinee is `&self.current_view` while inside a closure / nested match: stop and report. Most likely the surrounding context still works with `View::X | View::Y =>` syntax, but rare cases may need restructuring.
- If `update/window.rs:115`'s guarded `if !is_empty()` arms make the explicit listing tricky (because falsy guards fall through): use option (a) from the plan — add explicit unguarded sibling arms covering the falsy + Settings cases. If the resulting match isn't exhaustive, the compiler will tell you which variants you missed.
- If clippy fires `match_same_arms` on the explicit listings: that's the desired signal that some listings could merge — feel free to use `View::A | View::B | View::C => …` to consolidate identical bodies. Do NOT `#[allow]`.
- If a test fails because it constructed a wildcard-relying scenario: investigate before adjusting. The new explicit shape should be behavior-preserving; a test that relied on the wildcard was likely also tied to a now-explicit branch.

## Reporting

End with:
- The three (or fewer) commit refs and subjects.
- Total files changed and total wildcards eliminated (count the `_ =>` deletions to confirm; should be ~12 total: 6 in nav.rs, 1 in window.rs, 1 in components.rs, 1 in playback.rs, 2 in hotkeys/nav.rs, 1 in sort_api.rs).
- Confirmation that `View::ALL.len() == 8` and `NavView::ALL.len() == 7` const-asserts compile.
````
