---
paths:
  - "src/{config_writer,theme_config,visualizer_config,main}.rs"
  - "src/theme/**"
  - "src/update/{settings,config,playback,player_bar,default_playlist_picker,playlists,text_input_dialog,theater}.rs"
  - "src/update/tests_player_settings.rs"
  - "src/views/settings/**"
  - "src/widgets/eq_modal.rs"
  - "data/src/utils/paths.rs"
  - "data/src/credentials.rs"
  - "data/src/services/radio_scrobble/source.rs"
  - "data/src/services/{toml_settings_io,settings,theme_loader}.rs"
  - "data/src/services/settings_tables/**"
  - "data/src/backend/settings.rs"
  - "data/src/types/player_settings*.rs"
  - "data/src/types/player_settings/**"
  - "data/src/types/toml_*.rs"
  - "data/src/types/setting*.rs"
---

# Gotchas: Config & Persistence

- **Every TOML read-modify-write goes through `edit_config_doc` or `edit_existing_config_doc`** (`data/src/utils/paths.rs`): the first for config.toml (a missing file starts empty), the second for theme files (a missing file is an error). Both return `Err` and write nothing when the file can't be read or parsed, so a typo stays the user's to fix instead of being "repaired" by a rewrite holding only the edited keys. Surface that `Err` at the UI boundary with a toast.
- **Scalar settings live on `Nokkvi.settings` only**: no second copy elsewhere in the app state. A control that changes one writes `self.settings.<field>` synchronously and then persists (and pushes to the engine where needed), so the field is current between reloads; `handle_player_settings_loaded` replaces it wholesale. Do not add a second copy. A setting read before `PlayerSettingsLoaded` lands needs its persisted default hand-restored in `Nokkvi::default()` (`LivePlayerSettings` derives an all-zero `Default`); `live_controls_start_at_their_persisted_defaults` pins the live controls. Still held elsewhere, restored from settings on load: per-view column flags (on the pages, `restore_from`), EQ gains (`playback.eq_state`) and custom EQ presets (`eq_modal.custom_presets`).
- **Font is global, not per-theme**: `font_family` lives in `LivePlayerSettings` / `TomlSettings` and routes to `config.toml`. EQ modal `pick_list` must explicitly receive the active app font.
