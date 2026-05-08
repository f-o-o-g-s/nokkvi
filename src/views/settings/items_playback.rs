//! Playback tab setting entries.
//!
//! Contains: Playback (crossfade + volume normalization), Scrobbling, and
//! Playlists sections. 7 flat rows come from `define_settings!` via
//! `build_playback_tab_settings_items`. The conditional AGC target-level
//! knob, the four ReplayGain knobs (only shown in RG modes), and the
//! `default_playlist_name` dialog sentinel row stay hand-written so the
//! mode-conditional logic and the picker dialog construction live next to
//! each other.

// See `items_general.rs` for why the data struct lives in the data crate.
use nokkvi_data::services::settings_tables::playback::build_playback_tab_settings_items;
pub(crate) use nokkvi_data::types::settings_data::PlaybackSettingsData;

use super::items::{SettingItem, SettingsEntry};

/// Build settings entries for the Playback tab.
pub(crate) fn build_playback_items(data: &PlaybackSettingsData) -> Vec<SettingsEntry> {
    const PLAY: &str = "assets/icons/circle-play.svg";
    const SCR: &str = "assets/icons/radio-tower.svg";
    const LIST: &str = "assets/icons/list-music.svg";

    let mut macro_rows = build_playback_tab_settings_items(data);
    let mut take = |key: &str| -> SettingsEntry {
        let pos = macro_rows
            .iter()
            .position(|e| matches!(e, SettingsEntry::Item(it) if it.key.as_ref() == key))
            .unwrap_or_else(|| panic!("missing macro row for {key}"));
        macro_rows.remove(pos)
    };

    let mut items: Vec<SettingsEntry> = vec![
        // --- Playback ---
        SettingsEntry::Header {
            label: "Playback",
            icon: PLAY,
        },
        take("general.crossfade_enabled"),
        take("general.crossfade_duration"),
        take("general.volume_normalization"),
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
        take("general.scrobbling_enabled"),
        take("general.scrobble_threshold"),
        // --- Playlists ---
        SettingsEntry::Header {
            label: "Playlists",
            icon: LIST,
        },
        take("general.quick_add_to_playlist"),
        // `general.default_playlist_name` opens a picker dialog (sentinel
        // path); kept hand-written so the empty/Not-set fallback lives at
        // the row construction site.
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
        take("general.queue_show_default_playlist"),
    ]);

    items
}
