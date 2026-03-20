//! Playback tab setting entries
//!
//! Contains: Playback (crossfade), Scrobbling, and Playlists sections.
//! Migrated from the General tab to reduce clutter.

use super::items::{SettingItem, SettingsEntry};

/// Data needed by the Playback tab builder
pub(crate) struct PlaybackSettingsData<'a> {
    pub crossfade_enabled: bool,
    pub crossfade_duration_secs: i64,
    pub volume_normalization: bool,
    pub normalization_level: &'a str,
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as a fraction (0.25–0.90)
    pub scrobble_threshold: f64,
    pub quick_add_to_playlist: bool,
    pub default_playlist_name: &'a str,
}

/// Build settings entries for the Playback tab
pub(crate) fn build_playback_items(data: &PlaybackSettingsData) -> Vec<SettingsEntry> {
    const PLAY: &str = "assets/icons/circle-play.svg";
    const SCR: &str = "assets/icons/radio-tower.svg";
    const LIST: &str = "assets/icons/list-music.svg";

    // Map scrobble threshold fraction to percentage integer (e.g. 0.50 -> 50)
    let threshold_pct = (data.scrobble_threshold * 100.0).round() as i64;

    vec![
        // --- Playback ---
        SettingsEntry::Header {
            label: "Playback",
            icon: PLAY,
        },
        SettingItem::bool_val(
            meta!(
                "general.crossfade_enabled",
                "Crossfade",
                "Fade between tracks instead of gapless transitions"
            ),
            data.crossfade_enabled,
            false,
        ),
        SettingItem::int(
            meta!(
                "general.crossfade_duration",
                "Crossfade Duration",
                "Duration of crossfade between tracks"
            ),
            data.crossfade_duration_secs,
            5,
            1,
            15,
            1,
            "s",
        ),
        SettingItem::bool_val(
            meta!(
                "general.volume_normalization",
                "Volume Normalization",
                "Automatic gain control · normalizes loudness across tracks"
            ),
            data.volume_normalization,
            false,
        ),
        SettingItem::enum_val(
            meta!(
                "general.normalization_level",
                "Normalization Level",
                "Target loudness: Quiet (headroom), Normal, Loud (boost)"
            ),
            data.normalization_level,
            "Normal",
            vec!["Quiet", "Normal", "Loud"],
        ),
        // --- Scrobbling ---
        SettingsEntry::Header {
            label: "Scrobbling",
            icon: SCR,
        },
        SettingItem::bool_val(
            meta!(
                "general.scrobbling_enabled",
                "Scrobbling Enabled",
                "Report listening activity to server"
            ),
            data.scrobbling_enabled,
            true,
        ),
        SettingItem::int(
            meta!(
                "general.scrobble_threshold",
                "Scrobble Threshold",
                "% of track duration needed to scrobble"
            ),
            threshold_pct,
            50,
            25,
            90,
            5,
            "%",
        ),
        // --- Playlists ---
        SettingsEntry::Header {
            label: "Playlists",
            icon: LIST,
        },
        SettingItem::bool_val(
            meta!(
                "general.quick_add_to_playlist",
                "Quick Add to Playlist",
                "Skip the playlist picker dialog · uses default playlist"
            ),
            data.quick_add_to_playlist,
            false,
        ),
        SettingItem::text(
            meta!(
                "general.default_playlist_name",
                "Default Playlist",
                "Set via right-click → 'Set as Default Playlist' on any playlist"
            ),
            if data.default_playlist_name.is_empty() {
                "Not set"
            } else {
                data.default_playlist_name
            },
            "Not set",
        ),
    ]
}
