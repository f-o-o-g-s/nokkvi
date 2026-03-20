# Design Inspirations & Naming Brainstorm

A living document collecting all design influences, technical ancestry, and naming ideas for the client. Intended to provide context when the project eventually gets a proper name.

---

## Core Technology Stack

| Component | Technology | Notes |
|:----------|:-----------|:------|
| Language | **Rust** | Core identity — all application code is Rust |
| GUI Framework | **Iced** | Elm Architecture (MVU), GPU-rendered |
| Server | **Navidrome** | Self-hosted music server with Subsonic-compatible API |
| Audio Output | **rodio** (cpal/ALSA) | Cross-platform. Migrated from direct PipeWire bindings (`77639fe`) |
| Visualizer FFT | **RustFFT** | Pure-Rust. Migrated from vendored C/FFTW3 cava (`28d42f5`) |
| AI Authorship | **Claude** | Effectively 100% AI-authored codebase with human-level edits being slim to none |

---

## Design Inspirations

### rmpc (Rust MPD Client)

The most direct layout influence. [rmpc](https://github.com/mierak/rmpc) is a terminal-based Rust client for MPD (Music Player Daemon).

**What we took:**
- **Top tab bar navigation** — Queue, Albums, Artists, Songs, Playlists, Genres as labeled tabs across the top of the window. Our `nav_bar.rs` is a GUI adaptation of their terminal tab row. rmpc defines these as configurable `PaneType` variants (`Queue`, `Artists`, `Albums`, `Playlists`, `Directories`, `Cava`, etc.) — we hardcode `NAV_TABS` in the same order.
- **View-based architecture** — rmpc uses "panes" (not pages); we use views rendered into a content area below the nav bar, same concept.
- **Cava visualizer** — rmpc integrates Cava as a terminal pane. We initially did the same (vendored `cavacore`), then migrated to a pure-Rust `SpectrumEngine` using RustFFT while preserving all of Cava's user-facing settings (noise reduction, monstercat smoothing, waves mode).
- **MPD consume mode** — We emulate MPD's consume mode logic (remove tracks from queue after playing).

### StepMania / DDR (Dance Dance Revolution)

The original client's UI heritage. Two major patterns survive:

**Slot List (originally "Wheel View"):**
- The signature navigation widget — a vertically centered list showing a fixed number of slots (default 9), with the active item in the center.
- Directly inspired by DDR's music select wheel, which uses modulo arithmetic for infinite wrapping (you can see the end of the list before the beginning).
- The current implementation uses clamped navigation (no wrapping), but the visual pattern — opacity gradient fading away from center, scrolling items past a fixed focal point — is pure DDR.
- Originally called a "wheel view" (`WheelPageState`, `base_wheel_layout`) — fully renamed to the `SlotList*` family during a past refactoring. No wheel references remain in the codebase.

**Settings / Options Menu:**
- Drill-down navigation with categorized headers: Level 1 picks a category (General, Playback, Hotkeys, Theme, Visualizer), Level 2 shows all items within that category grouped under section headers with icons.
- Left/Right arrows increment/decrement values inline, Enter activates edit mode, Escape goes back — all straight from StepMania's options screen.
- Sound effects on navigation (expand/collapse sounds, enter sounds) reinforce the arcade feel.

**Sound Effects:**
- DDR-inspired sound effects for UI interactions (navigation clicks, menu transitions). The SFX engine (`data/src/audio/sfx_engine.rs`) plays short audio cues on user actions.

### Neovim / Vim Colorscheme Ecosystem

The entire theming engine is built around the neovim colorscheme naming convention and curated schemes from vim/nvim plugin authors.

**Color naming convention** (from `theme_config.rs`):
- Background layers: `bg0_hard`, `bg0`, `bg0_soft`, `bg1`, `bg2`, `bg3`, `bg4`
- Foreground layers: `fg0`, `fg1`, `fg2`, `fg3`, `fg4`
- Named color groups: `AquaConfig { normal, bright }`, plus red, green, yellow, blue, purple, orange — each with normal/bright variants
- This is the exact naming scheme from [gruvbox.nvim](https://github.com/morhetz/gruvbox) / [gruvbox-material.nvim](https://github.com/sainnhe/gruvbox-material)

**Primary inspiration — Gruvbox** (by morhetz):
- The default palette and design foundation. Gruvbox colors are hardcoded as fallback defaults throughout.
- The "Gruvbox hardware aesthetic" defines the entire visual language: warm retro tones, high readability, distinct light/dark mode palettes.

**Bundled theme presets** — all ports of popular neovim colorschemes:

| Theme | nvim Origin | Character |
|:------|:------------|:----------|
| Gruvbox (Dark Hard Blue/Red) | morhetz/gruvbox | Warm retro, flagship default |
| Everforest | sainnhe/everforest | Comfortable green/forest tones |
| Catppuccin | catppuccin/nvim | Mocha/Latte pastel flavors |
| Nord | arcticicestudio/nord | Arctic blue, cool minimalism |
| Dracula | dracula/vim | High-contrast purple/dark |
| Kanagawa | rebelot/kanagawa.nvim | Hokusai wave-inspired |
| Cryo | — | Icy blue (custom) |
| Ember Dusk | — | Warm fiery/parchment (custom) |
| Bio-Luminal Swamplab | — | High-intensity neon green/teal (custom) |

### Feishin (TypeScript Navidrome Client)

Reference for API patterns and feature scope.

**What we took:**
- Enum patterns for the Navidrome-native API, especially the GetInfo context menu and how they structure API response types.
- Feature parity targets (what a Navidrome client should support).

**What we deliberately avoided:**
- Their page-based layout with dense information panels — we consider it too cluttered. Our slot list approach shows far less information per screen but makes navigation faster and more focused.

### Other Technical References

| Influence | What We Took |
|:----------|:-------------|
| **audioMotion** | Physics-based peak gravity for visualizer peaks (`fall_accel`, `fade` modes) |
| **Fooyin** | Decoupling UI from backend engines — influenced `SettingsManager` and `VolumeSender` isolation |
| **Symphonia** | Pure-Rust audio decoding — standardized as the backend codec engine |
| **Waybar** | The nav bar comment in code literally says "Waybar-style flat navigation bar" — three-section layout (tabs / track info / format info) |

### Legacy: Qt6 / Quickshell Prototype

The project evolved from a Qt6 QML prototype. Patterns refined during migration:
- MVVM → Elm Architecture (MVU)
- QML list views → Rust-native slot list with optimized slot counts
- C++ CAVA reference → matched stability using async Iced pipelines, then full Rust replacement

---

## Naming Brainstorm

> **Status**: Ongoing. Brainstormed across three AI agents (Claude × 3, Gemini 3.1 × 3) with extensive collision checking against npm, GitHub, crates.io, and music software namespaces.

> [!IMPORTANT]
> This is a **streaming GUI Navidrome client**, not a CLI tool. Names should work as a binary name and window title, but don't optimize for `--flags` aesthetics. Subsonic compatibility is a factor but the client is Navidrome-specific.

> [!IMPORTANT]
> **No humor-first names.** Previous S-tier humor picks (Barf, Zarf, Futz) are archived. The name should evoke: **cold / iron / unique**. A unique Unix GUI application for music, built in Rust/Iced, specifically for Navidrome. No S-opening names.

### 🔬 The Breakthrough: Illegal Consonant Clusters

The critical discovery (Claude + Gemini session 3): **names using consonant clusters that don't exist in English bypass the native speaker's pattern-matcher entirely**. When a made-up word uses legal English phonotactics, the brain maps it to the nearest English word (rimvald→Rimworld, brynd→Bryn, grundr→Grindr). But clusters like **Kv-**, **Hv-**, **Tv-**, **Kj-**, **Fj-**, **Bj-**, **Kn-** force the brain to accept the word as genuinely foreign.

**Why Kvarst worked when dozens of others failed:**
1. **Kv-** opening — illegal in English. No pattern match possible.
2. **-arst** ending — almost nothing in English ends with broad "ah" + "rst" (only "karst," a rare geology term).
3. The brain gives up trying to map it to a dictionary word and accepts it as a distinctive name.

**The formula**: `[Illegal consonant cluster] + [Broad vowel] + [Harsh stop ending]`

### ⚡ S-Tier (The Name)

| Name | Chars | Cluster | Why it works |
|:-----|:------|:--------|:-------------|
| **Nokkvi** | 7 | — | ASCII adaptation of Old Norse *nökkvi* (nǫkkvi) = small boat/vessel. The -ee ending Anglicizes the pronunciation without Anglicizing the word — tells English speakers "this ends like knee." The double-k preserves Norse orthographic weight. Inherits the Arapaho *nek-* convergence (water in two unrelated language families). Nautical, humble, specific. Lends itself to a small boat icon/SVG. Zero collisions. |
| **Nekuuwu** | 7 | — | Not an illegal-cluster name — a completely different lineage. Built from Arapaho *nek* (water) + *-oowu'* (flowing water verb), with independent Old Norse and Japanese convergence. The double-u is the Arapaho long vowel. Phonologically alien to English through vowel duration rather than consonant collision. Reads as neither English nor any single recognizable language. Zero collisions. ⚠️ Contains "uwu" substring — see backstory for mitigation. |
| **Kworv** | 5 | Kw- | Kw- is the PNW labialized velar — the defining consonant of every Salish and Tlingit language, written /kʷ/ in linguistics. English spells this sound "qu" (queen, quest) but never "kw." The K-spelling marks it as non-English on sight. Combined with hverfa (to turn) + terminal -v. Five characters, one syllable, the labialized turning. Zero collisions. |
| **Qvarsk** | 6 | Qv- | Q-without-U is visually unmistakable in any font, terminal, or browser tab. Uvular stops (/q/) are *the* PNW consonant — every Salish, Tlingit, and Haida language has them; English has zero. Combined with -arsk (validated Norse harsh-stop ending). PNW onset meets Norse coda. Zero collisions. |
| **Phyorv** | 6 | Phy- | Greek Phy- (φύσις, nature/growth) + fjörðr (sea) + hverfa (turn). The Y vowel opens a brief glimpse of something pronounceable before -orv slams the door. Six chars, tight, alien but not hostile. The best balance of foreign and rollable in the entire list. Zero collisions. |
| **Phjorv** | 6 | Phj- | Ph- (Greek) fused with Fj- (Norse) into a cluster that exists in neither tradition. fjörðr (sea) + -v terminal. The j forces the tongue into a position no English speaker has rehearsed. Six chars, maximally dense. Reads as streamy/nautical — a fjord made of aspiration. Zero collisions. |
| **Phnarsk** | 7 | Phn- | Phn- doesn't exist in any natural language as a word-initial cluster. The Ph- invokes Greek, then the -n- detonates it. -arsk ending grounds it in the Norse harsh-stop family. Sounds like iron being stress-fractured. Zero collisions. |
| **Phorvth** | 7 | Ph-...-rvth | Ph- reads Greek but the -rvth ending is alien to every European language. No English word contains the -rvth cluster. Beautiful backstory but the four-consonant coda is genuinely hard to say aloud. Zero collisions. |
| **Verphwo** | 7 | V-...-phwo | *Hverfa* (to turn) rearranged. V- invites you in, -rphwo ambushes you. The -wo ending kills the French association of -wa. Conceptually strong but visually reads like a typo as a window title. Zero collisions. |

---

**Nokkvi — The Small Boat (Old Norse nökkvi × Arapaho nek × Gros Ventre)**

*Nokkvi* is Old Norse *nökkvi* (nǫkkvi) — a small, primitive boat — adapted to ASCII with an English-readable ending. Where every other S-tier name is invented, this one is *translated*. The word already exists. It already means exactly the right thing. It just needed its diacritics stripped and its pronunciation made visible.

1. **Old Norse — nökkvi / nǫkkvi (a small boat)**: A *nökkvi* was the humblest vessel in the Norse fleet. Not a longship (*langskip*), not a merchant ship (*knörr*), not a warship (*snekkja*). A small, primitive boat with one or two pairs of oars. It was the craft ordinary people used — fishermen, farmers crossing fjords, anyone who needed to get across water without ceremony. The word appears in the sagas as a humble conveyance, often in contrast to the grand ships of kings. *Nökkvi* is also attested as a heiti (poetic synonym) for "ship" in skaldic poetry, and it appears as a dwarf's name in the *Völuspá* — one of the dvergar (dwarves) listed in the Dvergatal. The vessel and the craftsman share a name.

2. **The nek- convergence (Old Norse × Arapaho/Gros Ventre)**: The *nek-* root in Old Norse denotes a vessel — the thing that carries you through water. In Arapaho/Gros Ventre (Algonquian family), *nek/neč* means the water itself. Two completely unrelated language families, separated by an ocean and ten thousand years of linguistic evolution, produced the same consonant cluster for concepts inseparable from each other: the water and the boat. This is the kind of accident that makes a name feel inevitable rather than constructed.

3. **The -ee ending**: Old Norse *-vi* (/-vi/) ended in a short /i/ vowel. Modern English has no convention for representing this — "-vi" reads as "vye" (rhyming with "sky") to most English speakers. The -ee spelling (*Nokkvi*) corrects this: it tells the reader "this ends like knee, free, tree." It's an explicit pronunciation guide embedded in the spelling. The tradeoff is that the word is no longer raw Old Norse — it's been adapted, visibly. But this adaptation is a feature: it signals that the word was resurfaced from an old language and made accessible, not that it was invented in a vacuum.

4. **The double-k**: Norse orthography uses geminate (doubled) consonants to indicate a preceding short vowel. The *-kk-* in *nökkvi* tells you the O is short and the consonant is held. In *Nokkvi*, the double-k preserves this visual weight — it's the heaviest part of the word, the keel of the name. It also prevents the eye from reading "Nokvee" (too close to "no-key") — the doubled consonant forces the reader to slow down at the center of the word.

5. **The icon potential**: A *nökkvi* is visually simple — a hull, an oar, maybe a small mast. It's the kind of shape that reduces to a clean SVG with 4-6 paths: a curved hull, a straight oar, concentric ripples beneath. The boat-on-water icon naturally merges with audio waveforms — ripples become sound waves, the hull becomes a vessel carrying music. No other S-tier name offers this kind of direct visual identity.

**The synthesis**: *Nokkvi* is the small boat — the humble Norse vessel that carried ordinary people across water, adapted for a modern screen. It's the client: a small Rust program that carries your music through the stream, without ceremony, without a grand fleet behind it. The *nek-* root ties it to Arapaho water by accident, the double-k anchors it visually, and the -ee ending makes it pronounceable to anyone who sees it. It's the only S-tier name that already means what it needs to mean.

