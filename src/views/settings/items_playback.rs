//! Playback tab setting entries.
//!
//! Contains six sections: Transitions (crossfade), Fading (smooth track
//! starts + pause/resume/stop transport fades + the M8 content-aware overlap
//! knobs — skip-silence, gap/overlap trim, bar-snap), Volume Normalization
//! (mode dropdown + AGC target knob + ReplayGain knobs), Scrobbling, Rating
//! Reminder (enable + conditional timing/percentage), and Playlists. Flat
//! rows come from `define_settings!` via `build_playback_tab_settings_items`.
//! The conditional AGC target-level knob, the four ReplayGain knobs (only
//! shown in RG modes), the Rating Reminder timing/percentage rows (only shown
//! when the reminder is enabled / in percentage mode), and the
//! `default_playlist_name` dialog sentinel row stay hand-written so the
//! conditional logic and the picker dialog construction live next to the rows
//! they gate. The three fade duration rows follow the same
//! take-unconditionally-push-conditionally convention (shown only while
//! their enable / skip mode is on).

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
    const FADING: &str = "assets/icons/blend.svg";
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
        macro_rows.take("general.lyrics_enabled"),
        macro_rows.take("general.lyrics_fetch_online"),
        macro_rows.take("general.lyrics_backdrop_blur"),
        macro_rows.take("general.crossfade_duration"),
        macro_rows.take("general.crossfade_curve"),
        macro_rows.take("general.crossfade_min_track"),
        macro_rows.take("general.crossfade_album_gapless"),
        macro_rows.take("general.bit_perfect"),
        macro_rows.take("general.rewind_on_previous"),
        // --- Fading ---
        SettingsEntry::Header {
            label: "Fading",
            icon: FADING,
        },
        macro_rows.take("general.smooth_track_starts"),
        macro_rows.take("general.fade_on_pause"),
    ];

    // Each fade duration knob only matters while its enable is on. Taken
    // unconditionally, pushed conditionally — same convention as the Rating
    // Reminder timing rows, so `finish()` stays data-independent.
    let pause_ms_row = macro_rows.take("general.fade_pause_ms");
    if data.fade_on_pause {
        items.push(pause_ms_row);
    }
    items.push(macro_rows.take("general.fade_on_stop"));
    let stop_ms_row = macro_rows.take("general.fade_stop_ms");
    if data.fade_on_stop {
        items.push(stop_ms_row);
    }
    // The skip-fade duration knob only matters while a skip-fade mode is
    // selected — same take-unconditionally-push-conditionally convention.
    // Matched positively against the two real modes (not `!= "Off"`) so the
    // test-default sentinel data keeps the row gated off too.
    items.push(macro_rows.take("general.fade_on_skip"));
    let skip_secs_row = macro_rows.take("general.fade_skip_secs");
    if matches!(data.fade_on_skip.as_ref(), "Boundary Fade" | "Crossfade") {
        items.push(skip_secs_row);
    }
    items.push(macro_rows.take("general.fade_radio_transitions"));
    // M8 content-aware overlap knobs close out the Fading section.
    items.push(macro_rows.take("general.skip_silence"));
    items.push(macro_rows.take("general.crossfade_offset"));
    items.push(macro_rows.take("general.crossfade_bar_snap"));

    items.extend([
        // --- Volume Normalization ---
        SettingsEntry::Header {
            label: "Volume Normalization",
            icon: NORMALIZATION,
        },
        macro_rows.take("general.volume_normalization"),
    ]);

    // AGC-only knob: target loudness applies only when AGC is selected.
    if data.volume_normalization.as_ref() == "AGC" {
        items.push(SettingItem::enum_val(
            SettingMeta::new(
                "general.normalization_level",
                "AGC Target Level",
                "Volume Normalization",
            )
            .with_subtitle("Quiet (headroom) · Normal · Loud (boost)"),
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
                "Volume Normalization",
            )
            .with_subtitle(
                "Boost on top of the tag value · 0 dB matches reference, +6 is typical \
                 for modern listeners",
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
                "Volume Normalization",
            )
            .with_subtitle(
                "dB applied when a track has no ReplayGain tags · ignored if Use AGC \
                 for Untagged is on",
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
                "Volume Normalization",
            )
            .with_subtitle("Falls through to real-time AGC when a track has no ReplayGain tags"),
            data.replay_gain_fallback_to_agc,
            false,
        ));
        items.push(SettingItem::bool_val(
            SettingMeta::new(
                "general.replay_gain_prevent_clipping",
                "Prevent Clipping",
                "Volume Normalization",
            )
            .with_subtitle("Clamp gain so track_peak × gain ≤ 1.0"),
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
        macro_rows.take("general.love_change_notification_enabled"),
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
                "Playlists",
            )
            .with_subtitle(
                "Click to choose a playlist · also settable from the Playlists header \
                     chip or right-click menu",
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
