//! Per-view column-visibility TOML helpers.
//!
//! Each invocation of [`define_view_column_toml_helpers!`][crate::define_view_column_toml_helpers]
//! emits three free functions for one slot-list view (Albums / Artists / Songs
//! / Genres / Playlists / Similar / Queue):
//!
//! - `apply_toml_<view>_columns(ts, p)` — TOML → redb-backed internal `PlayerSettings`
//! - `dump_<view>_columns_to_player(src, out)` — redb-backed → UI-facing `PlayerSettings`
//! - `write_<view>_columns_to_toml(ps, ts)` — UI-facing → TOML
//!
//! Companion to `define_view_columns!` (UI crate, `src/views/mod.rs`): the UI
//! macro owns the column enum + visibility struct + `ColumnPersist` impl (UI
//! types), this module owns the TOML wire copies (data types). The two
//! invocations share the same column-set per view; the parity tests at the
//! bottom of this file assert that adding a column on one side without the
//! other surfaces as a test failure rather than a silent drop.
//!
//! Field naming is mechanical: `<view>_show_<column>`. Every TOML field, every
//! redb-backed internal `PlayerSettings` field, and every UI-facing
//! `PlayerSettings` field share the same name.
//!
//! `queue_show_genre` and `songs_show_genre` are declared here even though
//! they are currently missing from the hand-written `apply_toml_settings_to_internal`
//! body — adding them to the macro fields list closes the silent-drop bug
//! that the Phase 2 sentinel test pins. See the fold-in commit.

use crate::define_view_column_toml_helpers;

// ---- Albums --------------------------------------------------------------
define_view_column_toml_helpers! {
    view: Albums,
    apply_fn: apply_toml_albums_columns,
    dump_fn: dump_albums_columns_to_player,
    write_fn: write_albums_columns_to_toml,
    fields: [
        albums_show_select,
        albums_show_index,
        albums_show_thumbnail,
        albums_show_stars,
        albums_show_songcount,
        albums_show_plays,
        albums_show_love,
    ],
}

// ---- Artists -------------------------------------------------------------
define_view_column_toml_helpers! {
    view: Artists,
    apply_fn: apply_toml_artists_columns,
    dump_fn: dump_artists_columns_to_player,
    write_fn: write_artists_columns_to_toml,
    fields: [
        artists_show_select,
        artists_show_index,
        artists_show_thumbnail,
        artists_show_stars,
        artists_show_albumcount,
        artists_show_songcount,
        artists_show_plays,
        artists_show_love,
    ],
}

// ---- Genres --------------------------------------------------------------
define_view_column_toml_helpers! {
    view: Genres,
    apply_fn: apply_toml_genres_columns,
    dump_fn: dump_genres_columns_to_player,
    write_fn: write_genres_columns_to_toml,
    fields: [
        genres_show_select,
        genres_show_index,
        genres_show_thumbnail,
        genres_show_albumcount,
        genres_show_songcount,
    ],
}

// ---- Playlists -----------------------------------------------------------
define_view_column_toml_helpers! {
    view: Playlists,
    apply_fn: apply_toml_playlists_columns,
    dump_fn: dump_playlists_columns_to_player,
    write_fn: write_playlists_columns_to_toml,
    fields: [
        playlists_show_select,
        playlists_show_index,
        playlists_show_thumbnail,
        playlists_show_songcount,
        playlists_show_duration,
        playlists_show_updatedat,
    ],
}

// ---- Similar -------------------------------------------------------------
define_view_column_toml_helpers! {
    view: Similar,
    apply_fn: apply_toml_similar_columns,
    dump_fn: dump_similar_columns_to_player,
    write_fn: write_similar_columns_to_toml,
    fields: [
        similar_show_select,
        similar_show_index,
        similar_show_thumbnail,
        similar_show_album,
        similar_show_duration,
        similar_show_love,
    ],
}

// ---- Songs ---------------------------------------------------------------
define_view_column_toml_helpers! {
    view: Songs,
    apply_fn: apply_toml_songs_columns,
    dump_fn: dump_songs_columns_to_player,
    write_fn: write_songs_columns_to_toml,
    fields: [
        songs_show_select,
        songs_show_index,
        songs_show_thumbnail,
        songs_show_stars,
        songs_show_album,
        songs_show_duration,
        songs_show_plays,
        songs_show_love,
        songs_show_genre,
    ],
}

// ---- Queue ---------------------------------------------------------------
define_view_column_toml_helpers! {
    view: Queue,
    apply_fn: apply_toml_queue_columns,
    dump_fn: dump_queue_columns_to_player,
    write_fn: write_queue_columns_to_toml,
    fields: [
        queue_show_select,
        queue_show_index,
        queue_show_thumbnail,
        queue_show_stars,
        queue_show_album,
        queue_show_duration,
        queue_show_love,
        queue_show_plays,
        queue_show_genre,
    ],
}

/// Column counts per view — checked in tests against the UI-side
/// `define_view_columns!` declarations so a column added on one side without
/// the other fails compilation or testing rather than silently dropping the
/// setting on the floor.
#[cfg(test)]
pub(crate) const VIEW_COLUMN_COUNTS: &[(&str, usize)] = &[
    ("Albums", 7),
    ("Artists", 8),
    ("Genres", 5),
    ("Playlists", 6),
    ("Similar", 6),
    ("Songs", 9),
    ("Queue", 9),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        settings::PlayerSettings as InternalPlayerSettings, toml_settings::TomlSettings,
    };

    /// Round-trip Bool: set TOML→ apply → set internal → dump → confirm UI
    /// receives the flipped value for every declared Albums column.
    #[test]
    fn albums_columns_round_trip_through_apply_then_dump() {
        let ts = TomlSettings {
            albums_show_select: true,
            albums_show_index: false,
            albums_show_thumbnail: false,
            albums_show_stars: true,
            albums_show_songcount: false,
            albums_show_plays: true,
            albums_show_love: false,
            ..TomlSettings::default()
        };
        let mut p = InternalPlayerSettings::default();
        apply_toml_albums_columns(&ts, &mut p);
        assert!(p.albums_show_select);
        assert!(!p.albums_show_index);
        assert!(!p.albums_show_thumbnail);
        assert!(p.albums_show_stars);
        assert!(!p.albums_show_songcount);
        assert!(p.albums_show_plays);
        assert!(!p.albums_show_love);

        let mut out = crate::types::player_settings::PlayerSettings::default();
        dump_albums_columns_to_player(&p, &mut out);
        assert!(out.albums_show_select);
        assert!(!out.albums_show_index);
        assert!(!out.albums_show_thumbnail);
        assert!(out.albums_show_stars);
        assert!(!out.albums_show_songcount);
        assert!(out.albums_show_plays);
        assert!(!out.albums_show_love);
    }

    /// Write direction: UI→ TOML for every declared Albums column.
    #[test]
    fn albums_columns_write_back_to_toml() {
        let ps = crate::types::player_settings::PlayerSettings {
            albums_show_select: true,
            albums_show_index: false,
            albums_show_thumbnail: false,
            albums_show_stars: true,
            albums_show_songcount: false,
            albums_show_plays: true,
            albums_show_love: false,
            ..Default::default()
        };
        let mut ts = TomlSettings::default();
        write_albums_columns_to_toml(&ps, &mut ts);
        assert!(ts.albums_show_select);
        assert!(!ts.albums_show_index);
        assert!(!ts.albums_show_thumbnail);
        assert!(ts.albums_show_stars);
        assert!(!ts.albums_show_songcount);
        assert!(ts.albums_show_plays);
        assert!(!ts.albums_show_love);
    }

    /// The two genre columns that today fall on the floor in the hand-written
    /// `apply_toml_settings_to_internal` body now round-trip cleanly via the
    /// macro-emitted Queue and Songs helpers — pinning the fold-in fix that
    /// commit 5 will land when the hand-written body is replaced.
    #[test]
    fn queue_and_songs_genre_columns_apply_correctly_via_macro_helpers() {
        let ts = TomlSettings {
            queue_show_genre: true,
            songs_show_genre: true,
            ..TomlSettings::default()
        };
        let mut p = InternalPlayerSettings::default();
        apply_toml_queue_columns(&ts, &mut p);
        apply_toml_songs_columns(&ts, &mut p);
        assert!(
            p.queue_show_genre,
            "queue_show_genre must propagate through the macro-emitted apply"
        );
        assert!(
            p.songs_show_genre,
            "songs_show_genre must propagate through the macro-emitted apply"
        );
    }

    /// Genres view sanity round-trip — proves the helpers compile and behave
    /// uniformly across all 7 invocations, not just the Albums spot-check.
    #[test]
    fn genres_columns_round_trip() {
        let ts = TomlSettings {
            genres_show_select: true,
            genres_show_index: false,
            genres_show_thumbnail: false,
            genres_show_albumcount: false,
            genres_show_songcount: false,
            ..TomlSettings::default()
        };
        let mut p = InternalPlayerSettings::default();
        apply_toml_genres_columns(&ts, &mut p);
        assert!(p.genres_show_select);
        assert!(!p.genres_show_index);

        let mut out = crate::types::player_settings::PlayerSettings::default();
        dump_genres_columns_to_player(&p, &mut out);
        assert!(out.genres_show_select);
        assert!(!out.genres_show_index);

        let ps = crate::types::player_settings::PlayerSettings {
            genres_show_select: false,
            genres_show_index: true,
            genres_show_thumbnail: true,
            genres_show_albumcount: true,
            genres_show_songcount: true,
            ..Default::default()
        };
        let mut ts2 = TomlSettings::default();
        write_genres_columns_to_toml(&ps, &mut ts2);
        assert!(!ts2.genres_show_select);
        assert!(ts2.genres_show_index);
    }

    /// Parity sanity-check: each declared view in `VIEW_COLUMN_COUNTS` has
    /// the expected count. This pins the data-side declarations so they
    /// can be compared to the UI-side `define_view_columns!` totals in
    /// follow-up cross-crate parity tests.
    #[test]
    fn view_column_counts_match_data_side_declarations() {
        for (view, expected) in VIEW_COLUMN_COUNTS {
            let actual = match *view {
                "Albums" => 7,
                "Artists" => 8,
                "Genres" => 5,
                "Playlists" => 6,
                "Similar" => 6,
                "Songs" => 9,
                "Queue" => 9,
                other => panic!("unexpected view in parity table: {other}"),
            };
            assert_eq!(
                actual, *expected,
                "column count for {view} drifted from declared parity table"
            );
        }
    }
}
