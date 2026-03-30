//! TOML-serializable view sort preferences for the `[views]` section of config.toml.
//!
//! Flat key structure for readability: `albums_sort = "recently_added"`.

use serde::{Deserialize, Serialize};

use crate::types::{
    queue::SortPreferences,
    queue_sort_mode::QueueSortMode,
    sort_mode::SortMode,
    view_preferences::AllViewPreferences,
};

/// View sort preferences in config.toml.
///
/// Uses snake_case string keys for TOML readability. Each sort mode is the
/// lowercase/snake_case representation of the enum variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TomlViewPreferences {
    pub albums_sort: String,
    pub albums_ascending: bool,
    pub artists_sort: String,
    pub artists_ascending: bool,
    pub songs_sort: String,
    pub songs_ascending: bool,
    pub genres_sort: String,
    pub genres_ascending: bool,
    pub playlists_sort: String,
    pub playlists_ascending: bool,
    pub queue_sort: String,
    pub queue_ascending: bool,
}

impl Default for TomlViewPreferences {
    fn default() -> Self {
        Self {
            albums_sort: SortMode::RecentlyAdded.to_toml_key().to_string(),
            albums_ascending: false,
            artists_sort: SortMode::Name.to_toml_key().to_string(),
            artists_ascending: true,
            songs_sort: SortMode::RecentlyAdded.to_toml_key().to_string(),
            songs_ascending: false,
            genres_sort: SortMode::Name.to_toml_key().to_string(),
            genres_ascending: true,
            playlists_sort: SortMode::UpdatedAt.to_toml_key().to_string(),
            playlists_ascending: false,
            queue_sort: QueueSortMode::Album.to_toml_key().to_string(),
            queue_ascending: true,
        }
    }
}

impl TomlViewPreferences {
    /// Build from `AllViewPreferences` (for migration from redb).
    pub fn from_all_view_prefs(avp: &AllViewPreferences) -> Self {
        Self {
            albums_sort: avp.albums.sort_mode.to_toml_key().to_string(),
            albums_ascending: avp.albums.sort_ascending,
            artists_sort: avp.artists.sort_mode.to_toml_key().to_string(),
            artists_ascending: avp.artists.sort_ascending,
            songs_sort: avp.songs.sort_mode.to_toml_key().to_string(),
            songs_ascending: avp.songs.sort_ascending,
            genres_sort: avp.genres.sort_mode.to_toml_key().to_string(),
            genres_ascending: avp.genres.sort_ascending,
            playlists_sort: avp.playlists.sort_mode.to_toml_key().to_string(),
            playlists_ascending: avp.playlists.sort_ascending,
            queue_sort: avp.queue.sort_mode.to_toml_key().to_string(),
            queue_ascending: avp.queue.sort_ascending,
        }
    }

    /// Convert back to `AllViewPreferences`.
    pub fn to_all_view_prefs(&self) -> AllViewPreferences {
        use crate::types::queue::QueueSortPreferences;

        AllViewPreferences {
            albums: SortPreferences::new(
                SortMode::from_toml_key(&self.albums_sort),
                self.albums_ascending,
            ),
            artists: SortPreferences::new(
                SortMode::from_toml_key(&self.artists_sort),
                self.artists_ascending,
            ),
            songs: SortPreferences::new(
                SortMode::from_toml_key(&self.songs_sort),
                self.songs_ascending,
            ),
            genres: SortPreferences::new(
                SortMode::from_toml_key(&self.genres_sort),
                self.genres_ascending,
            ),
            playlists: SortPreferences::new(
                SortMode::from_toml_key(&self.playlists_sort),
                self.playlists_ascending,
            ),
            queue: QueueSortPreferences::new(
                QueueSortMode::from_toml_key(&self.queue_sort),
                self.queue_ascending,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip() {
        let prefs = TomlViewPreferences::default();
        let toml_str = toml::to_string_pretty(&prefs).expect("serialize");
        let parsed: TomlViewPreferences = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.albums_sort, "recently_added");
        assert!(!parsed.albums_ascending);
        assert_eq!(parsed.artists_sort, "name");
        assert!(parsed.artists_ascending);
    }

    #[test]
    fn minimal_toml_uses_defaults() {
        let minimal = r#"
            albums_sort = "name"
            albums_ascending = true
        "#;
        let parsed: TomlViewPreferences = toml::from_str(minimal).expect("deserialize");
        assert_eq!(parsed.albums_sort, "name");
        assert!(parsed.albums_ascending);
        // Unspecified fields use defaults
        assert_eq!(parsed.artists_sort, "name");
        assert!(!parsed.songs_ascending);
    }
}
