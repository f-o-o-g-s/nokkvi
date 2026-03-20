//! Pure domain types — data models shared between backend and UI
//!
//! Entity types (Album, Artist, Song, Genre, Playlist), queue/sort modes,
//! hotkey configuration, user settings, and thread-safe reactive containers.

pub mod album;
pub mod artist;
pub mod collage_artwork;
pub mod genre;
pub mod hotkey_config;
pub mod info_modal;
pub mod paged_buffer;
pub mod player_settings;
pub mod playlist;
pub mod playlist_edit;
pub mod progress;
pub mod queue;
pub mod queue_sort_mode;
pub mod reactive;
pub mod settings;
pub mod song;
pub mod song_pool;
pub mod sort_mode;
pub mod toast;
pub mod view_preferences;
