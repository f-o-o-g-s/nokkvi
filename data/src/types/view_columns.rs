//! Canonical per-view column-visibility toggles.
//!
//! One struct owns all 55 `<view>_show_<col>` booleans (Queue 9, Albums 7,
//! Songs 9, Artists 8, Genres 5, Playlists 6, Similar 6, Preview 5) and the
//! ONE `Default` impl that every consumer shares:
//!
//! - [`PersistedPlayerSettings`][crate::types::settings::PersistedPlayerSettings]
//!   and [`TomlSettings`][crate::types::toml_settings::TomlSettings] embed it
//!   with `#[serde(flatten)]`, so every key stays a TOP-LEVEL
//!   `<view>_show_<col>` entry on both wire formats (redb JSON and
//!   config.toml `[settings]`). The flat shape is pinned by the
//!   `*_column_keys_stay_flat_*` tests next to those structs — field names
//!   here are wire keys and must never change without a migration.
//! - [`LivePlayerSettings`][crate::types::player_settings::LivePlayerSettings]
//!   embeds it as a plain field, so its derived `Default` carries the real
//!   shipped column defaults instead of all-`false`.
//!
//! The container-level `#[serde(default)]` fills missing keys from
//! [`ViewColumns::default`], replacing the per-field `default_true`
//! attributes the flat fields used to carry — serde-fill and struct default
//! can no longer diverge per field.
//!
//! Fields NOT owned here (not columns): `queue_show_default_playlist`
//! (header chip), `strip_show_*`, `mini_player_show_*`, and the
//! `*_artwork_overlay` toggles.

use serde::{Deserialize, Serialize};

/// Per-view column-visibility toggles shared by the persisted, TOML, and
/// live settings structs. See the module docs for the wire-format contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewColumns {
    // -- Queue view column toggles --
    /// Whether the queue's stars rating column is visible (default: true).
    /// Subject to a separate responsive width gate — see queue.rs.
    pub queue_show_stars: bool,
    /// Whether the queue's album column is visible (default: true).
    pub queue_show_album: bool,
    /// Whether the queue's duration column is visible (default: true).
    pub queue_show_duration: bool,
    /// Whether the queue's love (heart) column is visible (default: true).
    pub queue_show_love: bool,
    /// Whether the queue's plays column is visible (default: false).
    /// When sort = MostPlayed, the column auto-shows regardless of this toggle.
    pub queue_show_plays: bool,
    /// Whether the queue's leading row-index column is visible (default: true).
    pub queue_show_index: bool,
    /// Whether the queue's leading thumbnail column is visible (default: true).
    pub queue_show_thumbnail: bool,
    /// Whether the queue's genre is shown stacked under the album in the
    /// album column slot (default: false). When sort = Genre, the genre
    /// auto-shows regardless of this toggle. When the album column is
    /// hidden, the genre takes its slot at album-size font.
    pub queue_show_genre: bool,
    /// Leading multi-select checkbox column (default: false).
    pub queue_show_select: bool,

    // -- Albums view column toggles --
    /// Stars column. Auto-shows when sort = Rating regardless of toggle.
    pub albums_show_stars: bool,
    /// Song count column.
    pub albums_show_songcount: bool,
    /// Plays column. Auto-shows when sort = MostPlayed regardless of toggle.
    pub albums_show_plays: bool,
    /// Heart (favorite) column.
    pub albums_show_love: bool,
    /// Leading row-index column.
    pub albums_show_index: bool,
    /// Leading thumbnail column.
    pub albums_show_thumbnail: bool,
    /// Leading multi-select checkbox column (default: false).
    pub albums_show_select: bool,

    // -- Songs view column toggles --
    /// Stars column. Auto-shows when sort = Rating regardless of toggle.
    pub songs_show_stars: bool,
    /// Album column.
    pub songs_show_album: bool,
    /// Duration column.
    pub songs_show_duration: bool,
    /// Plays column. Auto-shows when sort = MostPlayed regardless of toggle.
    pub songs_show_plays: bool,
    /// Heart (favorite) column.
    pub songs_show_love: bool,
    /// Leading row-index column.
    pub songs_show_index: bool,
    /// Leading thumbnail column.
    pub songs_show_thumbnail: bool,
    /// Genre stacked under album in the album column slot. Auto-shows when
    /// sort = Genre regardless of toggle. Replaces the album slot at
    /// album-size font when the album column is hidden.
    pub songs_show_genre: bool,
    /// Leading multi-select checkbox column (default: false).
    pub songs_show_select: bool,

    // -- Artists view column toggles --
    /// Stars column. Auto-shows when sort = Rating regardless of toggle.
    pub artists_show_stars: bool,
    /// Album count column.
    pub artists_show_albumcount: bool,
    /// Song count column.
    pub artists_show_songcount: bool,
    /// Plays column. Auto-shows when sort = MostPlayed regardless of toggle.
    pub artists_show_plays: bool,
    /// Heart (favorite) column.
    pub artists_show_love: bool,
    /// Leading row-index column.
    pub artists_show_index: bool,
    /// Leading thumbnail column.
    pub artists_show_thumbnail: bool,
    /// Leading multi-select checkbox column (default: false).
    pub artists_show_select: bool,

    // -- Genres view column toggles --
    /// Leading row-index column.
    pub genres_show_index: bool,
    /// Thumbnail column on parent genre rows; also drives whether nested
    /// child album rows in the genre→album expansion render their artwork.
    pub genres_show_thumbnail: bool,
    /// Album-count column.
    pub genres_show_albumcount: bool,
    /// Song-count column.
    pub genres_show_songcount: bool,
    /// Leading multi-select checkbox column (default: false).
    pub genres_show_select: bool,

    // -- Playlists view column toggles --
    /// Leading row-index column.
    pub playlists_show_index: bool,
    /// Leading thumbnail column.
    pub playlists_show_thumbnail: bool,
    /// Song-count column. Auto-shows when sort = SongCount regardless of toggle.
    pub playlists_show_songcount: bool,
    /// Duration column. Auto-shows when sort = Duration regardless of toggle.
    pub playlists_show_duration: bool,
    /// Updated-at column. Auto-shows when sort = UpdatedAt regardless of toggle.
    pub playlists_show_updatedat: bool,
    /// Leading multi-select checkbox column (default: false).
    pub playlists_show_select: bool,

    // -- Similar view column toggles (Find Similar / Top Songs results) --
    /// Leading row-index column.
    pub similar_show_index: bool,
    /// Leading thumbnail column.
    pub similar_show_thumbnail: bool,
    /// Album column.
    pub similar_show_album: bool,
    /// Duration column.
    pub similar_show_duration: bool,
    /// Heart (favorite) column.
    pub similar_show_love: bool,
    /// Leading multi-select checkbox column (default: false).
    pub similar_show_select: bool,

    // -- Smart-playlist preview column toggles (rules editor's results pane) --
    /// Star-rating column (default: true).
    pub preview_show_stars: bool,
    /// Love (heart) column (default: true).
    pub preview_show_love: bool,
    /// Play-count column (default: true).
    pub preview_show_plays: bool,
    /// Genre column (default: true).
    pub preview_show_genre: bool,
    /// Duration column (default: true). Was always rendered before the
    /// preview gained configurable columns, so it defaults on.
    pub preview_show_duration: bool,
}

impl Default for ViewColumns {
    fn default() -> Self {
        Self {
            queue_show_stars: true,
            queue_show_album: true,
            queue_show_duration: true,
            queue_show_love: true,
            queue_show_plays: false,
            queue_show_index: true,
            queue_show_thumbnail: true,
            queue_show_genre: false,
            queue_show_select: false,
            albums_show_stars: false,
            albums_show_songcount: true,
            albums_show_plays: false,
            albums_show_love: true,
            albums_show_index: true,
            albums_show_thumbnail: true,
            albums_show_select: false,
            songs_show_stars: false,
            songs_show_album: true,
            songs_show_duration: true,
            songs_show_plays: false,
            songs_show_love: true,
            songs_show_index: true,
            songs_show_thumbnail: true,
            songs_show_genre: false,
            songs_show_select: false,
            artists_show_stars: true,
            artists_show_albumcount: true,
            artists_show_songcount: true,
            artists_show_plays: true,
            artists_show_love: true,
            artists_show_index: true,
            artists_show_thumbnail: true,
            artists_show_select: false,
            genres_show_index: true,
            genres_show_thumbnail: true,
            genres_show_albumcount: true,
            genres_show_songcount: true,
            genres_show_select: false,
            playlists_show_index: true,
            playlists_show_thumbnail: true,
            playlists_show_songcount: false,
            playlists_show_duration: false,
            playlists_show_updatedat: false,
            playlists_show_select: false,
            similar_show_index: true,
            similar_show_thumbnail: true,
            similar_show_album: true,
            similar_show_duration: true,
            similar_show_love: true,
            similar_show_select: false,
            preview_show_stars: true,
            preview_show_love: true,
            preview_show_plays: true,
            preview_show_genre: true,
            preview_show_duration: true,
        }
    }
}
