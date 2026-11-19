---
paths:
  - "src/theme/**"
  - "src/widgets/**"
  - "src/views/**"
  - "src/{app_view,theme_config,test_helpers}.rs"
  - "src/state/dynamic_accent.rs"
  - "src/update/{dynamic_accent,cross_pane_drag}.rs"
  - "src/update/tests/{dynamic_accent,redesign_chrome,settings,milkdrop}.rs"
---

# Gotchas: Theme & Accent

- **A filled accent surface is `accent_fill()` with `on_accent_fill()` ink, never `bg0_hard()` or `fg*()`**: under the dynamic accent the fill shade only clears 3:1 against the chrome, so `bg0_hard()` ink on it can drop under AA, and a light fill on a dark palette hides `fg0()`. `on_accent_fill()` is chosen to clear 4.5:1 on BOTH fill shades. A one-off fill with its own ink can use `legible_text_on(fill)` instead (≥ 4.58:1 always; the dialog check mark and drag badge do). Same for an opaque accent `selection` under `fg0()` text in a `text_input` — use `selection_color()`.
- **Accent TEXT stays on `accent()` / `accent_bright()`**: those are the text shade (4.5:1 against `bg0_hard`..`bg2` under the dynamic accent). Moving a text site to `accent_fill()` drops it to the fill floor.
- **The active lyric line is marked by hue, not lightness**: on every shipped theme its accent sits at or below the lightness of the dimmed lines around it, so a grey cover accent (black, white or grey album) drew it as a dimmed line. `lyrics_accent()` reads the `AccentRoles.lyrics` role, which `dynamic_accent::apply` sets to the theme's own accent whenever the cover's `lyric_line_hue` (the smaller of its hue separation from `fg0` and its own chroma) misses `LYRICS_HUE_SEPARATION` (`every_theme_marks_the_lyric_line_by_hue` pins that every shipped accent clears it). Separation alone is not enough: a grey line sits a tinted `fg0`'s whole chroma away from it, so grey covers kept a grey line on 11 of the 23 shipped themes (Everforest, Gruvbox, the Catppuccins, Solarized, both Tokyo Nights among them). Text drawn over `fg0` text that must stand out takes the same check.
- **The UI test binary always runs on the built-in default theme** (`load_active_theme_file` under `cfg(test)`, which also skips seeding the user's themes dir): it once read the theme of whoever ran it, so a palette comparison passed on Svalbard and failed on 11 other shipped themes. An invariant that must hold on every palette sweeps `theme::tests::all_builtin_palettes` (the pure functions take the palette as arguments); never lean on the live theme for it.
- **`hover_tint_on_accent()` measures, it does not key on the mode**: it returns whichever of `bg0_hard()` / `fg0()` contrasts more with the `accent_fill()` fill (the surfaces that opt in via `on_accent_surface` are fill-shade surfaces).
- **`read_dark_color` is the theme file's palette, not the one on screen**: it skips the dynamic accent on purpose. Cover colours reach a themed MilkDrop preset only through `cover_milkdrop()` (Visualizer From Album Art), and the logo is a per-theme mark, so following the accent alone never rebuilds a preset or recolors the logo.
- **A test that lets cover colors reach `theme::set_cover_colors` (or its test shorthand `set_dynamic_accent`) takes `THEME_MODE_LOCK` and leaves the global cleared**: the overlay is process-global like the other theme state. `Nokkvi::dynamic_accent_show` only writes `None` back from the app that set a palette (`DynamicAccentState.applied`), so the hundreds of unrelated tests that tick a default app never touch it. The other side of the same race: a test that reads the live accent (or anything derived from it, like `selected_fill_resolved()` or a `SlotListSlotStyle`) more than once and compares takes the lock too, or a concurrent accent test can swap it in between.
- **A palette is only shown against the artwork handle it was read from**: `DynamicAccentState.seeds` stores `(handle id, palette)` and the tick requires that id to still be the album's mini or large handle. That is what lets a late `AccentExtracted` be cached without a staleness check, makes a refreshed cover re-read, and stops the large cover landing after the mini from re-reading the album mid-track.
