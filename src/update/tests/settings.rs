//! Tests for settings dispatch update handlers.
//!
//! Interface-tab keys (`strip_*`, `*_artwork_overlay`, etc.) now route through
//! `define_settings!` in `nokkvi_data::services::settings_tables::interface`.
//! That module owns the dispatch + apply round-trip tests at the data layer.
//! The theme-atomic re-sync that used to happen in-line in
//! `handle_settings_general` is now driven by `PlayerSettingsLoaded` after the
//! manager-dispatched persist completes — covered separately by the playback
//! handler tests and exercised end-to-end at runtime.
