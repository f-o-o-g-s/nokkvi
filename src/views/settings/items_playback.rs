//! Playback tab setting entries.
//!
//! Contains five sections: Transitions (crossfade), Volume Normalization
//! (mode dropdown + AGC target knob + ReplayGain knobs), Scrobbling, Rating
//! Reminder (enable + conditional timing/percentage), and Playlists. Flat
//! rows come from `define_settings!` via `build_playback_tab_settings_items`.
//! The conditional AGC target-level knob, the four ReplayGain knobs (only
//! shown in RG modes), the Rating Reminder timing/percentage rows (only shown
//! when the reminder is enabled / in percentage mode), and the
//! `default_playlist_name` dialog sentinel row stay hand-written so the
//! conditional logic and the picker dialog construction live next to the rows
//! they gate.

// See `items_general.rs` for why the data struct lives in the data crate.
use nokkvi_data::services::{
    radio_scrobble::source::CredSource,
    settings_tables::playback::build_playback_tab_settings_items,
};
pub(crate) use nokkvi_data::types::settings_data::PlaybackSettingsData;

use super::{
    items::{ActivateKind, MacroRows, SettingItem, SettingMeta, SettingsEntry},
    sentinel::SentinelKind,
};

/// Status value for a radio-scrobble credential row. Config.toml is the GUI
/// write target, so Config/Redb read as "Saved"; an env var overrides config,
/// so it's called out (review #2).
fn radio_cred_value(source: CredSource) -> &'static str {
    match source {
        CredSource::Unset => "Not set · Enter to set",
        CredSource::Redb | CredSource::Config => "Saved · Enter to replace",
        CredSource::Env => "Set via env var (overrides config.toml)",
    }
}

/// Build settings entries for the Playback tab.
pub(crate) fn build_playback_items(data: &PlaybackSettingsData) -> Vec<SettingsEntry> {
    const TRANSITIONS: &str = "assets/icons/audio-waveform.svg";
    const NORMALIZATION: &str = "assets/icons/sliders-vertical.svg";
    const SCR: &str = "assets/icons/radio-tower.svg";
    const CHECK: &str = "assets/icons/check.svg";
    const LOGOUT: &str = "assets/icons/log-out.svg";
    const REMIND: &str = "assets/icons/star.svg";
    const LIST: &str = "assets/icons/list-music.svg";

    // Status-aware values for the radio-scrobble action rows so the user can
    // see what's configured (and from which source) at a glance.
    let lb_token_val = radio_cred_value(data.listenbrainz_source);
    let lf_creds_val = radio_cred_value(data.lastfm_credentials_source);
    let lf_connect_val = if data.lastfm_username.is_empty() {
        "Not connected · Enter to connect".to_string()
    } else {
        format!("Connected as {} · Enter to reconnect", data.lastfm_username)
    };

    let mut macro_rows = MacroRows::new(build_playback_tab_settings_items(data));

    let mut items: Vec<SettingsEntry> = vec![
        // --- Transitions ---
        SettingsEntry::Header {
            label: "Transitions",
            icon: TRANSITIONS,
        },
        macro_rows.take("general.crossfade_enabled"),
        macro_rows.take("general.crossfade_duration"),
        macro_rows.take("general.bit_perfect"),
        macro_rows.take("general.rewind_on_previous"),
        // --- Volume Normalization ---
        SettingsEntry::Header {
            label: "Volume Normalization",
            icon: NORMALIZATION,
        },
        macro_rows.take("general.volume_normalization"),
    ];

    // AGC-only knob: target loudness applies only when AGC is selected.
    if data.volume_normalization.as_ref() == "AGC" {
        items.push(SettingItem::enum_val(
            SettingMeta::new(
                "general.normalization_level",
                "AGC Target Level",
                "Quiet (headroom) · Normal · Loud (boost)",
            ),
            data.normalization_level.as_ref(),
            "Normal",
            vec!["Quiet", "Normal", "Loud"],
        ));
    }

    // ReplayGain-only knobs: appear when either RG mode is selected.
    let is_rg = matches!(
        data.volume_normalization.as_ref(),
        "ReplayGain (Track)" | "ReplayGain (Album)"
    );
    if is_rg {
        items.push(SettingItem::int(
            SettingMeta::new(
                "general.replay_gain_preamp_db",
                "ReplayGain Pre-amp",
                "Boost on top of the tag value · 0 dB matches reference, +6 is typical for modern listeners",
            ),
            data.replay_gain_preamp_db,
            0,
            -15,
            15,
            1,
            "dB",
        ));
        items.push(SettingItem::int(
            SettingMeta::new(
                "general.replay_gain_fallback_db",
                "Untagged Track Fallback",
                "dB applied when a track has no ReplayGain tags · ignored if Use AGC for Untagged is on",
            ),
            data.replay_gain_fallback_db,
            0,
            -15,
            15,
            1,
            "dB",
        ));
        items.push(SettingItem::bool_val(
            SettingMeta::new(
                "general.replay_gain_fallback_to_agc",
                "Use AGC for Untagged Tracks",
                "Falls through to real-time AGC when a track has no ReplayGain tags",
            ),
            data.replay_gain_fallback_to_agc,
            false,
        ));
        items.push(SettingItem::bool_val(
            SettingMeta::new(
                "general.replay_gain_prevent_clipping",
                "Prevent Clipping",
                "Clamp gain so track_peak × gain ≤ 1.0",
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
        macro_rows.take("general.scrobbling_enabled"),
        macro_rows.take("general.scrobble_threshold"),
        // --- Radio Scrobbling (direct to ListenBrainz) ---
        SettingsEntry::Header {
            label: "Radio Scrobbling",
            icon: SCR,
        },
        macro_rows.take("general.radio_scrobbling_enabled"),
        macro_rows.take("general.radio_scrobble_threshold_secs"),
        macro_rows.take("general.radio_now_playing_enabled"),
        SettingItem::text_with_icon(
            SettingMeta::new(
                SentinelKind::SetListenBrainzToken.to_key(),
                "ListenBrainz Token",
                "Radio Scrobbling",
            )
            .with_subtitle(
                "Paste your token from listenbrainz.org/settings (empty to disconnect). \
                 Saved to config.toml's [radio_scrobble] (plaintext, like navidrome.toml); a \
                 $NOKKVI_RADIO_LISTENBRAINZ_TOKEN env var overrides it. Radio only — library \
                 scrobbling uses Navidrome's keys.",
            ),
            lb_token_val,
            "",
            SCR,
        ),
        SettingItem::text_with_icon(
            SettingMeta::new(
                SentinelKind::VerifyListenBrainz.to_key(),
                "Verify ListenBrainz",
                "Radio Scrobbling",
            )
            .with_subtitle("Check the saved token and show your username."),
            "Press Enter to verify",
            "",
            CHECK,
        ),
        SettingItem::text_with_icon(
            SettingMeta::new(
                SentinelKind::SetLastfmCredentials.to_key(),
                "Last.fm API Credentials",
                "Radio Scrobbling",
            )
            .with_subtitle(
                "Enter your Last.fm app API key + secret (from last.fm/api). Saved to \
                 config.toml's [radio_scrobble] (plaintext); the $NOKKVI_RADIO_LASTFM_API_KEY/SECRET \
                 env vars override them. Radio only — library scrobbling uses Navidrome's keys.",
            ),
            lf_creds_val,
            "",
            SCR,
        ),
        SettingItem::text_with_icon(
            SettingMeta::new(
                SentinelKind::ConnectLastfm.to_key(),
                "Connect Last.fm",
                "Radio Scrobbling",
            )
            .with_subtitle("Authorize nokkvi in your browser to link your Last.fm account."),
            lf_connect_val.as_str(),
            "",
            CHECK,
        ),
        SettingItem::text_with_icon(
            SettingMeta::new(
                SentinelKind::DisconnectLastfm.to_key(),
                "Disconnect Last.fm",
                "Radio Scrobbling",
            )
            .with_subtitle("Clear the stored Last.fm session."),
            "Press Enter to disconnect",
            "",
            LOGOUT,
        ),
        // --- Rating Reminder ---
        SettingsEntry::Header {
            label: "Rating Reminder",
            icon: REMIND,
        },
        macro_rows.take("general.rating_reminder_enabled"),
        macro_rows.take("general.rating_change_notification_enabled"),
    ]);

    // The timing controls only matter once the reminder is enabled; the
    // percentage knob is further gated to the percentage trigger. The two
    // rows are taken unconditionally and pushed conditionally so `finish()`
    // can assert full consumption regardless of data — rows not pushed just
    // drop, emitting the same UI as before.
    let trigger_row = macro_rows.take("general.rating_reminder_trigger");
    let percent_row = macro_rows.take("general.rating_reminder_percent");
    if data.rating_reminder_enabled {
        items.push(trigger_row);
        if data.rating_reminder_trigger.as_ref() == "Percentage Played" {
            items.push(percent_row);
        }
    }

    items.extend([
        // --- Playlists ---
        SettingsEntry::Header {
            label: "Playlists",
            icon: LIST,
        },
        macro_rows.take("general.quick_add_to_playlist"),
        // `general.default_playlist_name` opens a picker dialog (sentinel
        // path); kept hand-written so the empty/Not-set fallback lives at
        // the row construction site.
        SettingItem::text(
            SettingMeta::new(
                "general.default_playlist_name",
                "Default Playlist",
                "Click to choose a playlist · also settable from the Playlists header chip or right-click menu",
            ),
            if data.default_playlist_name.is_empty() {
                "Not set"
            } else {
                data.default_playlist_name.as_ref()
            },
            "Not set",
        )
        .with_enter_hint()
        .with_activate(ActivateKind::PlaylistPicker),
        macro_rows.take("general.queue_show_default_playlist"),
    ]);

    macro_rows.finish();
    items
}
