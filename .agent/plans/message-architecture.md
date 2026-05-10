# Message Architecture — §4 #9 (staged refactor plan)

Closes §4 #9: per-view message enum parallelism, bubble-only intercepts, and the
positional `view_header()` parameter explosion.

**This is a multi-phase plan. Phases MUST execute in strict C → B → A order.**
Each phase must pass CI before the next begins. Attempting to run phases in
parallel will cause merge conflicts and semantic inconsistencies.

Within each phase, the 8-view migrations CAN be parallelized across worktrees.

Last verified baseline: **2026-05-10, `main @ HEAD`**.

Research source: `~/nokkvi-audit-results/` §4 #9 + live codebase audit 2026-05-10.

Do NOT begin this plan until the `loaders-and-chrome.md` batch is merged to main
(Lanes B and C of that plan both touch handler files; Phase B of this plan also
touches handler files — sequential ordering prevents confusion).

---

## 1. The Problem

Three coupled drift surfaces affect all 8 slot-list views:

### Surface 1: Positional `view_header()` signature (14–17 params)
Adding a new toolbar button requires editing all 8 view files. A recent commit
adding `on_roulette` touched all 8. A new `on_something` will too.

### Surface 2: ~22 bubble-only variants stamped across 8 enums
`SetOpenMenu`, `Roulette`, `ArtworkColumnDrag` — the page's `update()` never
processes these. They exist only to be intercepted by the handler prologue and
re-emitted as root `Message` variants. The handler prologue extracts them (§3 #11
will simplify this with `dispatch_view_chrome`), but the variants must still exist
per-view for the message routing to compile.

### Surface 3: ~60 stamped slot-list navigation variants
`SlotListNavigateUp`, `SlotListNavigateDown`, `SlotListSetOffset`, `SlotListActivateCenter`,
`SlotListClickPlay`, `SearchQueryChanged`, `CenterOnPlaying`, etc. — identical across
all 8 view enums. Each `handle_*` function has 8+ match arms that delegate identically.

---

## 2. Proposals

### Proposal C: `ViewHeaderConfig` struct (eliminates Surface 1)

Replace the 14–17 positional `Option<Message>` parameters of `view_header()` with a
single config struct + enum for optional buttons:

```rust
pub struct ViewHeaderConfig<'a, V, M> {
    pub current_view: V,
    pub view_options: &'a [V],
    pub sort_ascending: bool,
    pub search_query: &'a str,
    pub filtered_count: usize,
    pub total_count: usize,
    pub item_type: &'a str,
    pub search_input_id: &'static str,
    pub on_view_selected: Box<dyn Fn(V) -> M + 'a>,
    pub on_search_change: Box<dyn Fn(String) -> M + 'a>,
    pub show_search: bool,
    pub buttons: Vec<HeaderButton<M>>,
}

pub enum HeaderButton<M> {
    SortToggle(M),
    Refresh(M),
    CenterOnPlaying(M),
    Add { label: &'static str, msg: M },
    Roulette(M),
    Trailing(Element<'static, M>),
}
```

`view_header()` destructures the config struct and iterates `buttons` instead of
matching positional Options. Adding a new button type requires:
1. Add `HeaderButton::NewButton(M)` variant
2. Handle it in `view_header()` render loop
3. Views that want it push it to their `buttons` vec; views that don't are untouched

**Impact**: Eliminates the 8-view touch-fanout for toolbar changes. Future button
additions require **0 view-file edits** for views that don't use the button.

**Files changed**:
- `src/widgets/view_header.rs` — new struct, new enum, refactor the fn body
- All 8 view files (`views/{albums,artists,genres,playlists,queue,radios,similar,songs}/view.rs` or standalone files) — swap positional args for struct construction

**Risk**: Low. Pure mechanical refactor, no semantic change. Build validates correctness.

---

### Proposal B: Bubble-only variants → root Message directly (eliminates Surface 2)

Remove the ~22 bubble-only per-view variants by having the `view()` function
construct root `Message` variants directly rather than per-view wrapping variants.

**Current state** (albums/view.rs):
```rust
// view_header call passes:
on_roulette: Some(AlbumsMessage::Roulette),  // bubble-only variant
// → handle_albums intercepts this:
if matches!(msg, AlbumsMessage::Roulette) {
    return Task::done(Message::Roulette(RouletteMessage::Start(View::Albums)));
}
```

**After Proposal B** (albums/view.rs):
```rust
// view_header call passes (after Proposal C's config struct):
HeaderButton::Roulette(Message::Roulette(RouletteMessage::Start(View::Albums))),
// → AlbumsMessage::Roulette variant deleted
// → intercept in handle_albums deleted
// → root dispatcher ArtworkColumnDrag arm deleted
```

**Key insight**: `view()` already has the view context (`View::Albums`), so it can
construct the correct root `Message::Roulette(RouletteMessage::Start(View::Albums))`
directly. The per-view `Roulette` variant exists only because `view()` previously
returned `Element<'a, AlbumsMessage>` — the bubble-only message needed to be
wrapped in the per-view type. After Proposal C, if view_header accepts `M = Message`
for its button messages, the per-view wrapping is no longer needed.

**Dependencies**: Requires Proposal C to be complete first (the config struct enables
passing mixed-type messages through a single `buttons: Vec<HeaderButton<M>>`).

**Files changed**:
- 8 per-view `*Message` enums — delete 3 bubble-only variants each (`SetOpenMenu`, `Roulette`, `ArtworkColumnDrag`)
- 8 per-view handler files — delete 3 intercept blocks each (already simplified by §3 #11 / dispatch_view_chrome)
- 8 view files — update button construction to pass root `Message` directly
- `src/update/mod.rs` — delete 7 `ArtworkColumnDrag` match arms

**Risk**: Medium. Requires the view function's type parameter to support passing `Message`
directly for bubble-only buttons while still using `AlbumsMessage` for page-owned actions.
This may require a mixed-message approach (e.g., `view()` returns `Element<'a, Message>`
for bubble-only buttons and maps page messages up). Investigate the iced `.map()` API.

**Alternative approach**: Instead of changing `view()` return type, teach the view_header
to accept `Option<Message>` (root) alongside `impl Fn(...) -> M + 'a` (page) for each slot.
This keeps bubble-only items as direct `Message` and everything else as `AlbumsMessage`.

---

### Proposal A: `SlotListPageMessage` carrier (eliminates Surface 3)

Collapse ~60 stamped slot-list navigation variants into a single shared enum:

```rust
// In src/widgets/slot_list_page.rs or a new src/types/slot_list_message.rs
pub enum SlotListPageMessage<S = SortMode> {
    NavigateUp,
    NavigateDown,
    SetOffset(usize, iced::keyboard::Modifiers),
    ScrollSeek(usize),
    ActivateCenter,
    ClickPlay(usize),
    SelectionToggle(usize),
    SelectAllToggle,
    AddCenterToQueue,
    SearchQueryChanged(String),
    SearchFocused(bool),
    SortModeSelected(S),
    ToggleSortOrder,
    RefreshViewData,
    CenterOnPlaying,
    // ... etc
}
```

Each per-view `*Message` gets a single `SlotList(SlotListPageMessage)` variant:
```rust
pub enum AlbumsMessage {
    SlotList(SlotListPageMessage),     // ← replaces ~14 variants
    ContextMenuAction(usize, LibraryContextEntry),
    TracksLoaded(Result<...>),
    // ... only view-specific variants remain
}
```

`SlotListPageState` gets a `handle(SlotListPageMessage) -> (Task<SlotListPageMessage>, SlotListPageAction)` method. Each per-view handler delegates:
```rust
AlbumsMessage::SlotList(msg) => {
    let (task, action) = self.albums_page.common.handle(msg);
    // handle action (navigate, play, etc.)
    task.map(|m| Message::Albums(AlbumsMessage::SlotList(m)))
}
```

**Dependencies**: Requires both Proposals C and B to be complete first.
**Queue complication**: Queue uses `QueueSortMode`, not `SortMode`. Use `SlotListPageMessage<S = SortMode>` generic, or a separate `QueueSlotListPageMessage`. Decide before starting.
**Radios complication**: Radios hand-rolls sort (in-place), not via `impl_has_common_action!`. May need exemption or trait amendment.
**Risk**: Large. ~1000+ LOC changes across all 8 view enums + 8 handlers + tests.

---

## 3. Execution plan

### Phase 1 — Proposal C: `ViewHeaderConfig` struct

Effort: S–M. Internal parallelism: 8 view file changes can be split across 2–4 agents.

**Prerequisite**: None (standalone). Can start immediately after loaders-and-chrome lands.

**Agents**:
- Foundation agent: adds struct + enum to `view_header.rs`, refactors the fn, migrates `albums/view.rs` as reference
- Parallel agents: each migrates 1–2 view files to the new struct API

**What to be careful about**:
- `trailing_button: Option<Element<'a, Message>>` (current positional) becomes `HeaderButton::Trailing(Element<...>)` — may need `Element<'a, M>` if M != Message
- The `show_search: bool` flag becomes a config field
- Similar uses `SimilarMessage` not a view-based `*Message` — it may need its own variant or a separate `view_header_similar()`

**CI gate**: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`

---

### Phase 2 — Proposal B: Bubble-only variants → root Message

Effort: M. Internal parallelism: 8 view files can split, 7 handler files can split.

**Prerequisite**: Phase 1 (Proposal C) merged and green on CI.

**Investigation before writing prompts**:
1. Check how iced handles mixed-type messages in a single Element tree (`.map()` API)
2. Decide: does `view()` return `Element<'a, AlbumsMessage>` with some buttons mapped to root? Or does it return `Element<'a, Message>` entirely?
3. The approach affects whether the 8 view enums change (if view returns `Element<AlbumsMessage>`, bubble-only variants might still be needed as passthrough; if it returns `Element<Message>`, the per-view variants can go)

**Recommended approach**: Keep `view() -> Element<'a, AlbumsMessage>` signature. In the `ViewHeaderConfig`, bubble-only buttons (`HeaderButton::Roulette(M)`) receive an `AlbumsMessage::Roulette` that IS STILL a per-view variant — but now the variant is constructed in `view.rs` rather than being passed as a raw variant to the old positional parameter. This doesn't eliminate the variant yet; it eliminates the `handle_*` intercept because after §3 #11, the intercept is already in `dispatch_view_chrome`. The *real* win of B is the `ArtworkColumnDrag` root dispatch arms (7 of them in `mod.rs`) and the `SetOpenMenu` intercepts being pushed down into the trait.

**Clarification**: Phase B's benefit may be primarily `ArtworkColumnDrag` (if it exists as a root-level arm in mod.rs separate from per-view variants). Re-audit the actual ArtworkColumnDrag dispatch before writing Phase B prompts.

---

### Phase 3 — Proposal A: `SlotListPageMessage` carrier

Effort: L. Internal parallelism: each view can be migrated independently.

**Prerequisite**: Phases 1 and 2 merged and green on CI.

**Before starting**: Write a detailed sub-plan that addresses:
1. Queue's `QueueSortMode` vs `SortMode` — generic parameter decision
2. Radios' in-place sort — exemption strategy
3. Similar's different dispatch semantics — exemption or inclusion
4. Test migration plan (navigation.rs has 255 tests; many assert on `AlbumsMessage::SlotList*` variants that will change to `AlbumsMessage::SlotList(SlotListPageMessage::Navigate*)`)

**Rollout order**: Albums → Artists → Songs → Genres → Playlists → (Queue, Radios, Similar as last)

---

## 4. Audit tracker updates

After Phase 1 completes:
- Flag §4 row 9 as 🟡 partial (Phase C done)

After Phase 2 completes:
- Flag §4 row 9 as 🟡 partial (Phases C+B done)

After Phase 3 completes:
- §4 row 9 → ✅ done

---

## 5. What this plan does NOT include

- §3 #10 / §3 #11 / §3 #9 — those are the `loaders-and-chrome.md` plan; land those first
- §7 #9 / §4 #1 (ViewPage slot_list dispatch) — already closed in Batch 2
- Any new UI features or behavioral changes — this is structural refactoring only
- Removing `HasViewChrome` / `dispatch_view_chrome` (added by §3 #11) — those stay even after Phase B, since they handle `play_view_sfx` and any remaining view-chrome dispatch

---

## Notes for plan author (Phase 2 investigation)

Before writing the Phase 2 fanout prompts, run this research:

```bash
# Find ArtworkColumnDrag dispatch in mod.rs
grep -n "ArtworkColumnDrag" /home/foogs/nokkvi/src/update/mod.rs

# Count per-view ArtworkColumnDrag variants
grep -rn "ArtworkColumnDrag" /home/foogs/nokkvi/src/views/

# Understand the iced .map() API for message conversion
grep -rn "\.map(|" /home/foogs/nokkvi/src/views/ | head -20
```

This will clarify whether Phase B is best done by:
a) Deleting per-view bubble-only variants + routing root Message through view_header config
b) Keeping per-view variants but consolidating the root dispatch arms

The answer determines the Phase 2 agent prompts.
