//! Typed side-effect descriptions emitted by `define_settings!`'s
//! `on_dispatch:` hook so the data crate stays iced-free while the UI crate
//! still gets to run iced-aware effects (toasts, atomic writes, follow-up
//! `Task<Message>` dispatch) after a setter persists.
//!
//! The variants are constructed in `data/src/services/settings_tables/*.rs`
//! and consumed in `src/update/settings.rs::handle_settings_general`. Adding a
//! new variant means: declare it here, return it from a setting's
//! `on_dispatch:` closure, and add a match arm in
//! `dispatch_settings_side_effect` to run the iced-side work.

use crate::types::toast::ToastLevel;

#[derive(Debug, Clone)]
pub enum SettingsSideEffect {
    /// No side effect — the setter's persistence is the whole story. This is
    /// the default for entries that do not declare an `on_dispatch:` clause.
    None,
    /// Flip the UI-crate `theme::set_light_mode()` atomic and write
    /// `settings.light_mode` to `config.toml`. Used by `general.light_mode`,
    /// which has no redb persistence path of its own — its truth lives in the
    /// theme module + `config.toml`.
    SetLightModeAtomic(bool),
    /// Push a toast at the requested severity.
    Toast { level: ToastLevel, message: String },
    /// Re-fetch the artists list. Used by
    /// `general.show_album_artists_only` so the visible artist filter
    /// reflects the new toggle without a manual refresh.
    LoadArtists,
    /// Trigger the UI-crate verbose-config writer chain: write the full TOML
    /// (or strip-to-sparse), emit a result toast, then ask the manager to
    /// flush every TOML section in one pass via `write_all_toml_public`.
    WriteVerboseConfig { enabled: bool },
}
