use serde::{Deserialize, Serialize};

/// Genre model from Navidrome API
/// Combines data from Native API (/api/genre) and Subsonic API (getGenres)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genre {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    /// Album count - populated from Subsonic API
    #[serde(rename = "albumCount", default)]
    pub album_count: u32,
    /// Song count - populated from Subsonic API
    #[serde(rename = "songCount", default)]
    pub song_count: u32,
}

impl Genre {
    /// Get display name for the genre
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Get album count
    pub fn get_album_count(&self) -> u32 {
        self.album_count
    }

    /// Get song count
    pub fn get_song_count(&self) -> u32 {
        self.song_count
    }
}

impl std::fmt::Display for Genre {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({} albums, {} songs)",
            self.name, self.album_count, self.song_count
        )
    }
}
