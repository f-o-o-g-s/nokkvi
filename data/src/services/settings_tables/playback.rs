//! Playback-tab settings table.
//!
//! Empty in the foundation slice — per-tab follow-up commits will migrate
//! the `general.crossfade_*`, `general.volume_normalization`,
//! `general.replay_gain_*`, `general.scrobbl*`, and the playlist-related
//! keys here.

use crate::define_settings;

define_settings! {
    tab: crate::types::setting_def::Tab::Playback,
    settings_const: TAB_PLAYBACK_SETTINGS,
    contains_fn: tab_playback_contains,
    dispatch_fn: dispatch_playback_tab_setting,
    apply_fn: apply_toml_playback_tab,
    settings: []
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_playback_is_empty() {
        assert!(TAB_PLAYBACK_SETTINGS.is_empty());
        assert!(!tab_playback_contains("general.stable_viewport"));
    }
}
