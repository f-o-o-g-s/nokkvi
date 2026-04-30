//! Playback tab setting entries
//!
//! Contains: Playback (crossfade), Scrobbling, and Playlists sections.
//! Migrated from the General tab to reduce clutter.

use super::items::{SettingItem, SettingsEntry};

/// Data needed by the Playback tab builder
pub(crate) struct PlaybackSettingsData<'a> {
    pub crossfade_enabled: bool,
    pub crossfade_duration_secs: i64,
    /// Volume-normalization mode label ("Off" / "AGC" / "ReplayGain (Track)" / "ReplayGain (Album)")
    pub volume_normalization: &'a str,
    pub normalization_level: &'a str,
    /// Pre-amp dB applied on top of resolved ReplayGain (rounded to int for UI).
    pub replay_gain_preamp_db: i64,
    /// Fallback dB for tracks with no ReplayGain tags.
    pub replay_gain_fallback_db: i64,
    /// Whether untagged tracks fall through to AGC.
    pub replay_gain_fallback_to_agc: bool,
    /// Whether the resolver clamps gain so peak·gain ≤ 1.0.
    pub replay_gain_prevent_clipping: bool,
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as a fraction (0.25–0.90)
    pub scrobble_threshold: f64,
    pub quick_add_to_playlist: bool,
    pub default_playlist_name: &'a str,
    pub queue_show_default_playlist: bool,
}

/// Build settings entries for the Playback tab
pub(crate) fn build_playback_items(data: &PlaybackSettingsData) -> Vec<SettingsEntry> {
    const PLAY: &str = "assets/icons/circle-play.svg";
    const SCR: &str = "assets/icons/radio-tower.svg";
    const LIST: &str = "assets/icons/list-music.svg";

    // Map scrobble threshold fraction to percentage integer (e.g. 0.50 -> 50)
    let threshold_pct = (data.scrobble_threshold * 100.0).round() as i64;

    let mut items: Vec<SettingsEntry> = vec![
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
        SettingItem::enum_val(
            meta!(
                "general.volume_normalization",
                "Volume Normalization",
                "Off · ReplayGain (track or album) · AGC (real-time)"
            ),
            data.volume_normalization,
            "Off",
            vec!["Off", "ReplayGain (Track)", "ReplayGain (Album)", "AGC"],
        ),
    ];

    // AGC-only knob: target loudness applies only when AGC is selected.
    if data.volume_normalization == "AGC" {
        items.push(SettingItem::enum_val(
            meta!(
                "general.normalization_level",
                "AGC Target Level",
                "Quiet (headroom) · Normal · Loud (boost)"
            ),
            data.normalization_level,
            "Normal",
            vec!["Quiet", "Normal", "Loud"],
        ));
    }

    // ReplayGain-only knobs: appear when either RG mode is selected.
    let is_rg = matches!(
        data.volume_normalization,
        "ReplayGain (Track)" | "ReplayGain (Album)"
    );
    if is_rg {
        items.push(SettingItem::int(
            meta!(
                "general.replay_gain_preamp_db",
                "ReplayGain Pre-amp",
                "Boost on top of the tag value · 0 dB matches reference, +6 is typical for modern listeners"
            ),
            data.replay_gain_preamp_db,
            0,
            -15,
            15,
            1,
            "dB",
        ));
        items.push(SettingItem::int(
            meta!(
                "general.replay_gain_fallback_db",
                "Untagged Track Fallback",
                "dB applied when a track has no ReplayGain tags · ignored if Use AGC for Untagged is on"
            ),
            data.replay_gain_fallback_db,
            0,
            -15,
            15,
            1,
            "dB",
        ));
        items.push(SettingItem::bool_val(
            meta!(
                "general.replay_gain_fallback_to_agc",
                "Use AGC for Untagged Tracks",
                "Falls through to real-time AGC when a track has no ReplayGain tags"
            ),
            data.replay_gain_fallback_to_agc,
            false,
        ));
        items.push(SettingItem::bool_val(
            meta!(
                "general.replay_gain_prevent_clipping",
                "Prevent Clipping",
                "Clamp gain so track_peak × gain ≤ 1.0"
            ),
            data.replay_gain_prevent_clipping,
            true,
        ));
    }

    items.extend([
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
                "Click to choose a playlist · also settable from the Playlists header chip or right-click menu"
            ),
            if data.default_playlist_name.is_empty() {
                "Not set"
            } else {
                data.default_playlist_name
            },
            "Not set",
        ),
        SettingItem::bool_val(
            meta!(
                "general.queue_show_default_playlist",
                "Default Playlist Chip in Queue",
                "Display the default playlist chip in the queue view's header"
            ),
            data.queue_show_default_playlist,
            false,
        ),
    ]);

    items
}
