# Iced Architecture Patterns — Research from Mature Applications

> **Date**: 2026-02-06  
> **Projects Studied**: Sniffnet (32k★), Halloy (3.7k★), Flowsurface (1.3k★), Icebreaker (394★), COSMIC Clipboard Manager (177★)  
> **Goal**: Identify scalable patterns that could improve our Navidrome client

---

## Executive Summary

After studying 5 mature, actively-developed iced applications, a clear set of architectural patterns emerges. Our application follows all the right patterns (TEA architecture, message bubbling, view/update separation), and as of February 2026, **message namespacing is complete** — reducing the root `Message` enum from ~77 flat variants to 38 (25 flat + 13 wrappers), with 58 variants organized into 4 domain-specific enums.

Remaining areas where we could further align with best practices:

1. **Workspace/crate separation** for the data layer (Halloy, Flowsurface, Icebreaker)
2. **Screen-level component encapsulation** (all 5 projects — low priority for a 2-screen app)

---

## Pattern 1: Screen Enum (All 5 Projects)

### What They Do

Every mature iced app wraps their top-level UI states in a `Screen` enum:

```rust
// Icebreaker (hecrj — the iced creator himself)
pub enum Screen {
    Loading,
    Search(Search),
    Conversation(Conversation),
    Settings(Settings),
}

// Halloy
pub enum Screen {
    Dashboard(screen::Dashboard),
    Help(screen::Help),
    Welcome(screen::Welcome),
    Exit { pending_exit: HashSet<Server> },
}

// Flowsurface — uses a layout manager wrapping Dashboard instances
struct Flowsurface {
    layout_manager: LayoutManager, // contains multiple Dashboard instances
    sidebar: dashboard::Sidebar,
    // ...
}
```

Each screen **owns its own state, messages, and actions**. The root app only knows which screen is active.

### How We Differ

Our app uses a flat `Screen` enum (`Login`, `Home`) but the `Home` screen doesn't fully encapsulate its state. Instead, `Nokkvi` owns ALL state directly:

```rust
// Our current approach (main.rs):
pub struct Nokkvi {
    screen: Screen,           // Just Login or Home
    current_view: View,       // Flat field, not inside a screen struct
    shell: Option<ShellViewModel>,  // Global
    albums_page: AlbumsPage,  // Global (even when on Login screen)
    artists_page: ArtistsPage, // Global
    songs_page: SongsPage,    // Global
    // ... 20+ more fields
}
```

### Recommendation: **LOW PRIORITY** (current approach works fine)

Our app only has 2 real screens (Login → Home), so the flat approach is reasonable. Unlike Halloy/Icebreaker which navigate between fundamentally different screens, our Login screen is a brief transition. The real complexity is within the Home screen, which we handle well with page-level components.

**Verdict**: ✅ Our current approach is acceptable for a 2-screen app. No change needed.

---

## Pattern 2: Component Action Enums (Icebreaker, Halloy, Flowsurface)

### What They Do

Every component returns a structured `Action` enum from `update()`:

```rust
// Icebreaker — the canonical pattern from iced's creator
pub enum Action {
    None,
    Run(Task<Message>),
}

// Halloy Dashboard
fn update(...) -> (Task<Message>, Option<Event>)
// Where Event = QuitServer | Exit | OpenUrl | ToggleFullscreen | ...

// Flowsurface Dashboard
fn update(...) -> (Task<Message>, Option<Event>)
// Where Event = DistributeFetchedData | Notification | ResolveStreams
```

The key insight: **components never directly modify root state**. They return typed `Action`/`Event` values that the root interprets.

### How We Compare

We already use this exact pattern!

```rust
// Our views/albums.rs — same pattern as Icebreaker
pub enum AlbumsAction {
    PlayAlbum(String),
    LoadLargeArtwork(String),
    None,
}
fn update(&mut self, msg: AlbumsMessage) -> (Task<AlbumsMessage>, AlbumsAction)
```

**Verdict**: ✅ We already follow this pattern. Well done.

---

## Pattern 3: Workspace Crate Separation (Halloy, Flowsurface, Icebreaker)

### What They Do

All three complex apps use Cargo workspaces to separate concerns:

```
# Halloy
halloy/           ← GUI crate (iced-dependent)
├── data/         ← Pure data crate (no iced dependency)
├── irc/          ← IRC protocol crate
└── ipc/          ← IPC crate

# Flowsurface
flowsurface/      ← GUI crate
├── data/         ← Pure data + config crate
└── exchange/     ← Exchange protocol crate

# Icebreaker
icebreaker/       ← GUI crate
└── core/         ← LLM inference, chat persistence
```

The `data` crate contains:
- Domain types (our `types/`)
- Configuration parsing/persistence
- API/protocol clients
- Business logic
- **Zero iced dependencies**

### How We Differ

We keep everything in a single crate with module-level separation:
```
src/
├── types/        # Domain types (could be in a data crate)
├── services/     # API clients, persistence (could be in a data crate)
├── data/         # ViewModels (bridges services ↔ views)
├── views/        # Iced-dependent pages
├── widgets/      # Iced-dependent widgets
└── ...
```

### Recommendation: **LOW-MEDIUM PRIORITY** (nice-to-have)

Our single-crate approach works well at our current size (~103 .rs files). The workspace pattern primarily helps with:
- **Compilation speed**: Changes to `data/` don't recompile the UI
- **Testability**: Pure data crate is easier to unit test without iced
- **Reusability**: The data crate could serve a TUI or headless client

However, the migration would be significant. Our `data/` ViewModels bridge services and views and depend on both — they'd need to be split.

**Verdict**: 🔶 Valuable for the future, but the ROI isn't worth the refactoring cost right now. Revisit when compile times become painful or if we ever want a second frontend.

---

## Pattern 4: Flat Update Dispatch (Sniffnet, Halloy, Icebreaker)

### What They Do

The root `update()` maps each message to a single method call:

```rust
// Sniffnet — very clean, one method per message
fn update(&mut self, message: Message) -> Task<Message> {
    match message {
        Message::Style(style) => self.style(style),
        Message::Search(params) => self.search(params),
        Message::ChangeVolume(v) => self.change_volume(v),
        // Each arm calls a private method
    }
    Task::none()
}

// Icebreaker — delegates to screen.update(), maps actions at root
Message::Search(message) => {
    let Screen::Search(search) = &mut self.screen else { return Task::none() };
    let action = search.update(message);
    match action {
        search::Action::None => Task::none(),
        search::Action::Run(task) => task.map(Message::Search),
        search::Action::Boot(file) => {
            // Root-level side effect: screen transition
            let (conversation, task) = screen::Conversation::new(&self.library, file, backend);
            self.screen = Screen::Conversation(conversation);
            task.map(Message::Conversation)
        }
    }
}
```

### How We Compare

We already have a well-organized dispatch in `update/mod.rs` with domain-specific submodules. Our 33 root message variants (many of which are namespaced wrappers) are routed through a clean `match` block. This is **similar to Sniffnet's** approach.

One difference: Sniffnet keeps individual message handlers as private methods directly on the `Sniffer` struct. We separate them into submodules (`update/playback.rs`, `update/navigation.rs`, etc.), which is arguably **better** at our scale — Sniffnet's `sniffer.rs` is 2,408 lines!

**Verdict**: ✅ Our approach is best-in-class. Splitting handlers into submodules is cleaner than what Sniffnet does.

---

## Pattern 5: Subscription Organization (All Projects)

### What They Do

```rust
// Sniffnet — named subscription methods
fn subscription(&self) -> Subscription<Message> {
    Subscription::batch([
        self.keyboard_subscription(),
        self.mouse_subscription(),
        self.time_subscription(),
        Sniffer::window_subscription(),
    ])
}

// Halloy — conditional subscriptions based on screen
fn subscription(&self) -> Subscription<Message> {
    let screen_specific = match &self.screen {
        Screen::Dashboard(dashboard) => dashboard.subscription().map(Message::Dashboard),
        _ => Subscription::none(),
    };
    Subscription::batch([screen_specific, events(), ...])
}

// Icebreaker — screens own their subscriptions
fn subscription(&self) -> Subscription<Message> {
    let screen = match &self.screen {
        Screen::Conversation(c) => c.subscription().map(Message::Conversation),
        _ => Subscription::none(),
    };
    Subscription::batch([screen, hotkeys])
}
```

### How We Compare

Our subscriptions are all defined inline in `main.rs::subscription()` with conditional logic. This works fine, but there's an interesting pattern: **Icebreaker and Halloy let screens own their subscriptions**, meaning the visualizer animation subscription could live inside the visualizer widget rather than at the root.

**Verdict**: ✅ Our current approach works. The only takeaway is that if we add more screen-specific subscriptions, we should consider pushing them into the screen/page components.

---

## Pattern 6: Message Wrapping for Sub-Components (All Projects)

### What They Do

All projects use `.map(Message::ComponentName)` to namespace messages:

```rust
// Halloy
Message::Dashboard(message) => {
    let (command, event) = dashboard.update(message, ...);
    // Handle event at root level
    command.map(Message::Dashboard)
}

// Icebreaker
Message::Search(message) => {
    let action = search.update(message);
    match action {
        search::Action::Run(task) => task.map(Message::Search),
        // ...
    }
}
```

### How We Compare

✅ **Completed.** As of February 2026, we follow this pattern comprehensively:

```rust
// app_message.rs — 4 domain-specific enums (58 variants total)
pub enum Message {
    Artwork(ArtworkMessage),    // 18 variants: album/genre/playlist artwork pipelines
    Playback(PlaybackMessage),  // 26 variants: tick, seek, volume, track control, etc.
    Scrobble(ScrobbleMessage),  // 3 variants: submit, now-playing, result
    Hotkey(HotkeyMessage),      // 11 variants: global hotkey actions
    // + 9 component bubbling wrappers (Albums, Artists, Queue, etc.)
    // + 25 flat variants (navigation, data loading, window events, etc.)
}
```

Handler routing uses nested `match` arms:
```rust
Message::Artwork(msg) => {
    use ArtworkMessage;
    match msg {
        ArtworkMessage::Loaded(id, handle) => self.handle_artwork_loaded(id, handle),
        ArtworkMessage::LoadGenre(id, url, cred, ids) => { ... },
        // ...
    }
}
```

**Verdict**: ✅ This was our biggest architectural gap — now resolved. Root `Message` went from ~77 flat variants to 38 (25 flat + 13 wrappers), comparable to Halloy (~17) and Icebreaker (~16) when accounting for our larger feature surface.

---

## Pattern 7: Error Handling from Async Tasks (Halloy, Icebreaker)

### What They Do

Both use `Result` types in messages and handle errors gracefully:

```rust
// Icebreaker — clean grouped error handling
Message::Booted(Err(error))
| Message::Created(Err(error))
| Message::Saved(Err(error))
| Message::TitleChanged(Err(error))
| Message::ChatFetched(Err(error)) => {
    self.error = Some(dbg!(error));
    Action::None
}
```

### How We Compare

We handle errors in messages like `LoginResult(Result<ShellViewModel, String>)`, but many of our async tasks don't propagate errors at all — they just log them. Our `TaskManager` handles this with `spawn_result()` for automatic error logging.

**Verdict**: ✅ Our `TaskManager` approach is actually more centralized than what these projects do. It's fine.

---

## Pattern 8: State Machine Patterns (Icebreaker)

### What They Do

Icebreaker uses local state machines within screens:

```rust
enum State {
    Booting {
        file: File,
        logs: Vec<String>,
        progress: u32,
        _task: task::Handle,
    },
    Running {
        assistant: Assistant,
        sending: Option<task::Handle>,
    },
}
```

This makes invalid states unrepresentable. A `Booting` screen can't accidentally access `assistant`.

### How We Compare

We use a similar pattern at the app level (`Screen::Login` vs `Screen::Home`), but within our pages, we tend to use `Option<T>` rather than state enums. For example, our audio engine state is managed through various `Option` fields and flags.

**Verdict**: ✅ Fine for our use case. State machine enums are most valuable when there are multiple distinct lifecycle phases (boot → ready → error). Our pages don't have that complexity.

---

## Summary: Priority Action Items

| Priority | Pattern | Status | Action |
|----------|---------|--------|--------|
| ✅ Done | **Message Namespacing** | ✅ Complete | 58 variants moved into 4 domain enums (Feb 2026) |
| 🟡 Medium | **Workspace Crates** | ❌ Not used | Consider when compile times become a bottleneck |
| ⚪ Low | **Screen Encapsulation** | ✅ OK | Only 2 screens, current flat approach is acceptable |
| ✅ Done | **Action Bubbling** | ✅ Already implemented | No change needed |
| ✅ Done | **Flat Dispatch** | ✅ Best-in-class | Better than Sniffnet's 2.4k-line file |
| ✅ Done | **TaskManager** | ✅ Superior | More centralized than most projects |
| ✅ Done | **Subscription Organization** | ✅ Fine | Consider pushing to screens if more are added |

---

## Completed: Message Namespacing Refactoring (Feb 2026)

This was identified as our single biggest architectural gap and has been **fully resolved**.

### What Was Done

| Enum | Variants | Domain |
|------|----------|--------|
| `PlaybackMessage` | 26 | Tick, seek, volume, track control, gapless, player settings |
| `ArtworkMessage` | 18 | Shared album, genre pipeline, playlist pipeline |
| `HotkeyMessage` | 11 | Global keyboard shortcuts, starring, queue actions |
| `ScrobbleMessage` | 3 | Submit, now-playing, result |
| **Total namespaced** | **58** | |

### Root `Message` Enum: Before → After

- **Before**: ~77 flat variants — every playback, artwork, hotkey, and scrobble message lived at the root
- **After**: 38 variants (25 flat + 13 wrappers)
  - 4 domain wrappers: `Artwork(...)`, `Playback(...)`, `Scrobble(...)`, `Hotkey(...)`
  - 9 component bubbling wrappers: `Albums(...)`, `Artists(...)`, `Queue(...)`, etc.
  - 6 data loading triggers: `LoadAlbums`, `LoadQueue`, `LoadArtists`, `LoadGenres`, `LoadPlaylists`, `LoadSongs`
  - 19 misc: navigation, window events, wheel, view header, animation, config hot-reload

### Key Implementation Details

- Function references passed to services (e.g., `Message::LoadGenreArtwork`) were converted to closures: `|a, b, c, d| Message::Artwork(ArtworkMessage::LoadGenre(a, b, c, d))`
- Handler routing in `mod.rs` uses nested `match` arms within each `Message::Domain(msg)` wrapper
- Page-level message intercepts (e.g., `Message::Genres(GenresMessage::ArtworkLoaded(...))`) coexist alongside the domain wrappers — these serve a different purpose (page-internal state updates vs. cross-cutting pipeline messages)

---

## Appendix: Project Architecture Comparison

| Feature | **Us** | **Sniffnet** | **Halloy** | **Flowsurface** | **Icebreaker** |
|---------|--------|-------------|-----------|----------------|---------------|
| **Stars** | - | 32k | 3.7k | 1.3k | 394 |
| **Crate Structure** | Single | Single | Workspace (4) | Workspace (3) | Workspace (2) |
| **Main File Size** | 414 lines | 2408 lines | 1853 lines | 1210 lines | 385 lines |
| **Screen Enum** | Flat | Optional<RunningPage> | Screen enum | LayoutManager | Screen enum |
| **Action Bubbling** | ✅ | ❌ (direct mutation) | ✅ Event tuples | ✅ Event tuples | ✅ Action enum |
| **Message Count** | ~38 (namespaced) | ~60 | ~17 (namespaced) | ~18 (namespaced) | ~16 (namespaced) |
| **Update Split** | ✅ Submodules | ❌ All in sniffer.rs | ❌ All in main.rs | ❌ All in main.rs | ❌ All in main.rs |
| **Subscriptions** | Root-owned | Root-owned | Screen-delegated | Root-owned | Screen-delegated |
| **Data Separation** | modules | modules | data/ crate | data/ crate | core/ crate |
| **Persistence** | redb | Conf struct | filesystem | filesystem | SQLite |

### Key Takeaway

Our codebase is architecturally **more modular than Sniffnet** (which has a single 2400-line file) and **comparable to Halloy/Icebreaker** in message organization (~38 root variants vs their ~17/~16, reflecting our larger feature surface). The message namespacing refactoring (Feb 2026) moved 58 variants into 4 domain-specific enums, closing what was previously our biggest architectural gap.

Our `update/` submodule pattern is actually **superior** to what all 4 other projects do — they all keep their update logic in `main.rs` (Halloy's is 600 lines of match arms). Splitting into `update/playback.rs`, `update/navigation.rs`, etc. is a pattern these projects could benefit from adopting.
