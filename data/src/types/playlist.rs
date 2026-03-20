//! Playlist model from Navidrome API

use serde::{Deserialize, Serialize};

/// Playlist model from Navidrome API
/// Data from Native API (/api/playlist)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "comment", default)]
    pub comment: String,
    #[serde(rename = "duration", default)]
    pub duration: f32,
    #[serde(rename = "size", default)]
    pub size: i64,
    #[serde(rename = "songCount", default)]
    pub song_count: u32,
    #[serde(rename = "ownerName", default)]
    pub owner_name: String,
    #[serde(rename = "ownerId", default)]
    pub owner_id: String,
    #[serde(rename = "public", default)]
    pub public: bool,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

impl Playlist {
    /// Get display name for the playlist
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Get song count
    pub fn get_song_count(&self) -> u32 {
        self.song_count
    }

    /// Get duration in seconds
    pub fn get_duration(&self) -> f32 {
        self.duration
    }
}

impl std::fmt::Display for Playlist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({} songs)", self.name, self.song_count)
    }
}
