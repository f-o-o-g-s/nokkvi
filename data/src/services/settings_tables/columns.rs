//! Consolidated view-column copy-steps for all 7 slot-list views + the rules
//! preview.
//!
//! ONE `define_settings!` invocation owns every `<view>_show_<col>` boolean
//! (Queue 9, Albums 7, Songs 9, Artists 8, Genres 5, Playlists 6, Similar 6,
//! Preview 5 — 55 total) via the `view_columns:` clause, emitting the three
//! orchestrator
//! copy functions:
//!
//! - `apply_toml_columns_tab(ts, p)` — TOML → redb-backed
//!   `PersistedPlayerSettings` (called from `apply_toml_settings_to_internal`)
//! - `dump_columns_tab_player_settings(src, out)` — redb-backed → UI-facing
//!   `LivePlayerSettings` (called from `SettingsManager::get_player_settings`)
//! - `write_columns_tab_toml(ps, ts)` — UI-facing → TOML (called from
//!   `TomlSettings::from_player_settings_with_existing`)
//!
//! The `assert_exhaustive:` ident emits `assert_all_view_columns_covered`, a
//! never-called function that destructures `ViewColumns` with NO `..` rest
//! pattern — adding a field to `ViewColumns` without declaring it here is an
//! E0027 compile error, replacing the old runtime `VIEW_COLUMN_COUNTS`
//! parity table.
//!
//! Companion to `define_view_columns!` (UI crate, `src/views/mod.rs`), which
//! owns the per-view column enum + visibility struct + `ColumnPersist` impl.
//! The UI-side guards (`column_macro_covers_expected_field_count`,
//! `column_visibility_defaults_agree_with_data_crate_view_columns`) pin the
//! cross-crate field-set parity.
//!
//! The `settings:` list is deliberately empty — column visibility renders in
//! each view's header dropdown, not as Settings-tab rows, so nothing here is
//! dispatchable (`tab:` is required by the macro grammar but unused with an
//! empty settings list).
//!
//! WARNING (review #11): the grammar also emits `TAB_COLUMNS_SETTINGS`
//! (empty), `tab_columns_contains` (always false), an always-`None`
//! `dispatch_columns_tab_setting`, and an empty items builder. They look
//! identical to the four real tabs' artifacts but are permanent no-ops —
//! NEVER chain `dispatch_columns_tab_setting` into the settings dispatch
//! chain and never treat this invocation's `tab`/`data_type`/`mgr_type`
//! params as a pattern to copy. Only the three re-exported copy functions
//! above are real.

crate::define_settings! {
    // Unused with an empty settings list (the macro only transcribes `tab:`
    // into per-entry SettingDefs), but required by the grammar.
    tab: crate::types::setting_def::Tab::Interface,
    data_type: crate::types::settings_data::InterfaceSettingsData,
    mgr_type: crate::services::settings::SettingsManager,
    items_fn: build_columns_tab_settings_items,
    settings_const: TAB_COLUMNS_SETTINGS,
    contains_fn: tab_columns_contains,
    dispatch_fn: dispatch_columns_tab_setting,
    apply_fn: apply_toml_columns_tab,
    dump_fn: dump_columns_tab_player_settings,
    write_fn: write_columns_tab_toml,
    settings: [],
    view_columns: {
        fields: [
            // -- Queue (9) --
            queue_show_stars,
            queue_show_album,
            queue_show_duration,
            queue_show_love,
            queue_show_plays,
            queue_show_index,
            queue_show_thumbnail,
            queue_show_genre,
            queue_show_select,
            // -- Albums (7) --
            albums_show_stars,
            albums_show_songcount,
            albums_show_plays,
            albums_show_love,
            albums_show_index,
            albums_show_thumbnail,
            albums_show_select,
            // -- Songs (9) --
            songs_show_stars,
            songs_show_album,
            songs_show_duration,
            songs_show_plays,
            songs_show_love,
            songs_show_index,
            songs_show_thumbnail,
            songs_show_genre,
            songs_show_select,
            // -- Artists (8) --
            artists_show_stars,
            artists_show_albumcount,
            artists_show_songcount,
            artists_show_plays,
            artists_show_love,
            artists_show_index,
            artists_show_thumbnail,
            artists_show_select,
            // -- Genres (5) --
            genres_show_index,
            genres_show_thumbnail,
            genres_show_albumcount,
            genres_show_songcount,
            genres_show_select,
            // -- Playlists (6) --
            playlists_show_index,
            playlists_show_thumbnail,
            playlists_show_songcount,
            playlists_show_duration,
            playlists_show_updatedat,
            playlists_show_select,
            // -- Similar (6) --
            similar_show_index,
            similar_show_thumbnail,
            similar_show_album,
            similar_show_duration,
            similar_show_love,
            similar_show_select,
            // -- Preview (5) --
            preview_show_stars,
            preview_show_love,
            preview_show_plays,
            preview_show_genre,
            preview_show_duration,
        ],
        assert_exhaustive: assert_all_view_columns_covered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        player_settings::LivePlayerSettings, settings::PersistedPlayerSettings,
        toml_settings::TomlSettings, view_columns::ViewColumns,
    };

    /// Round-trip: set TOML → apply → dump → confirm the UI-facing struct
    /// receives the flipped value for every Albums column (spot-check view).
    #[test]
    fn columns_round_trip_through_apply_then_dump() {
        let ts = TomlSettings {
            view_columns: ViewColumns {
                albums_show_select: true,
                albums_show_index: false,
                albums_show_thumbnail: false,
                albums_show_stars: true,
                albums_show_songcount: false,
                albums_show_plays: true,
                albums_show_love: false,
                ..ViewColumns::default()
            },
            ..TomlSettings::default()
        };
        let mut p = PersistedPlayerSettings::default();
        apply_toml_columns_tab(&ts, &mut p);
        assert!(p.view_columns.albums_show_select);
        assert!(!p.view_columns.albums_show_index);
        assert!(!p.view_columns.albums_show_thumbnail);
        assert!(p.view_columns.albums_show_stars);
        assert!(!p.view_columns.albums_show_songcount);
        assert!(p.view_columns.albums_show_plays);
        assert!(!p.view_columns.albums_show_love);

        let mut out = LivePlayerSettings::default();
        dump_columns_tab_player_settings(&p, &mut out);
        assert!(out.view_columns.albums_show_select);
        assert!(!out.view_columns.albums_show_index);
        assert!(!out.view_columns.albums_show_thumbnail);
        assert!(out.view_columns.albums_show_stars);
        assert!(!out.view_columns.albums_show_songcount);
        assert!(out.view_columns.albums_show_plays);
        assert!(!out.view_columns.albums_show_love);
    }

    /// Write direction: UI → TOML for every Albums column.
    #[test]
    fn columns_write_back_to_toml() {
        let ps = LivePlayerSettings {
            view_columns: ViewColumns {
                albums_show_select: true,
                albums_show_index: false,
                albums_show_thumbnail: false,
                albums_show_stars: true,
                albums_show_songcount: false,
                albums_show_plays: true,
                albums_show_love: false,
                ..ViewColumns::default()
            },
            ..Default::default()
        };
        let mut ts = TomlSettings::default();
        write_columns_tab_toml(&ps, &mut ts);
        assert!(ts.view_columns.albums_show_select);
        assert!(!ts.view_columns.albums_show_index);
        assert!(!ts.view_columns.albums_show_thumbnail);
        assert!(ts.view_columns.albums_show_stars);
        assert!(!ts.view_columns.albums_show_songcount);
        assert!(ts.view_columns.albums_show_plays);
        assert!(!ts.view_columns.albums_show_love);
    }

    /// The two genre columns that the pre-macro hand-written apply body
    /// silently dropped must keep round-tripping through the consolidated
    /// apply (regression pin for the original silent-drop bug).
    #[test]
    fn queue_and_songs_genre_columns_apply_correctly() {
        let ts = TomlSettings {
            view_columns: ViewColumns {
                queue_show_genre: true,
                songs_show_genre: true,
                ..ViewColumns::default()
            },
            ..TomlSettings::default()
        };
        let mut p = PersistedPlayerSettings::default();
        apply_toml_columns_tab(&ts, &mut p);
        assert!(
            p.view_columns.queue_show_genre,
            "queue_show_genre must propagate through the consolidated apply"
        );
        assert!(
            p.view_columns.songs_show_genre,
            "songs_show_genre must propagate through the consolidated apply"
        );
    }

    /// Genres view sanity round-trip — proves the consolidated functions
    /// behave uniformly beyond the Albums spot-check.
    #[test]
    fn genres_columns_round_trip() {
        let ts = TomlSettings {
            view_columns: ViewColumns {
                genres_show_select: true,
                genres_show_index: false,
                genres_show_thumbnail: false,
                genres_show_albumcount: false,
                genres_show_songcount: false,
                ..ViewColumns::default()
            },
            ..TomlSettings::default()
        };
        let mut p = PersistedPlayerSettings::default();
        apply_toml_columns_tab(&ts, &mut p);
        assert!(p.view_columns.genres_show_select);
        assert!(!p.view_columns.genres_show_index);

        let mut out = LivePlayerSettings::default();
        dump_columns_tab_player_settings(&p, &mut out);
        assert!(out.view_columns.genres_show_select);
        assert!(!out.view_columns.genres_show_index);

        let ps = LivePlayerSettings {
            view_columns: ViewColumns {
                genres_show_select: false,
                genres_show_index: true,
                genres_show_thumbnail: true,
                genres_show_albumcount: true,
                genres_show_songcount: true,
                ..ViewColumns::default()
            },
            ..Default::default()
        };
        let mut ts2 = TomlSettings::default();
        write_columns_tab_toml(&ps, &mut ts2);
        assert!(!ts2.view_columns.genres_show_select);
        assert!(ts2.view_columns.genres_show_index);
    }
}
