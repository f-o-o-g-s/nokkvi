//! Pure domain types — data models shared between backend and UI
//!
//! Entity types (Album, Artist, Song, Genre, Playlist), queue/sort modes,
//! hotkey configuration, user settings, and thread-safe reactive containers.

pub mod album;
pub mod artist;
pub mod batch;
pub mod collage_artwork;
pub mod error;
pub mod filter;
pub mod genre;
pub mod hotkey_config;
pub mod info_modal;
pub mod mode_toggle;
pub mod paged_buffer;
pub mod player_settings;
pub mod playlist;
pub mod playlist_edit;
pub mod progress;
pub mod queue;
pub mod queue_sort_mode;
pub mod radio_station;
pub mod reactive;
pub mod setting_def;
pub mod setting_item;
pub mod setting_value;
pub mod settings;
pub mod settings_data;
pub mod settings_side_effect;
pub mod song;
pub mod song_pool;
pub mod song_source;
pub mod sort_mode;
pub mod theme_file;
pub mod toast;
pub mod toml_settings;
pub mod toml_views;
pub mod view_preferences;

pub use mode_toggle::ModeToggleEffect;
pub use song_source::SongSource;

pub fn deserialize_starred<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StarredValue {
        Bool(bool),
        String(String),
    }

    match Option::<StarredValue>::deserialize(deserializer)? {
        Some(StarredValue::Bool(b)) => Ok(b),
        Some(StarredValue::String(s)) => Ok(!s.is_empty()),
        None => Ok(false),
    }
}

pub fn deserialize_starred_opt<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StarredValue {
        Bool(bool),
        String(String),
    }

    match Option::<StarredValue>::deserialize(deserializer)? {
        Some(StarredValue::Bool(b)) => Ok(Some(b)),
        Some(StarredValue::String(s)) => Ok(Some(!s.is_empty())),
        None => Ok(None),
    }
}
