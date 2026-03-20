use serde::{Deserialize, Serialize};

/// Artist model from Navidrome API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "albumCount")]
    pub album_count: Option<u32>,
    #[serde(rename = "songCount")]
    pub song_count: Option<u32>,
    #[serde(rename = "starred")]
    pub starred: Option<bool>,
    #[serde(rename = "starredAt")]
    pub starred_at: Option<String>,
    // Artist images from external sources (Last.fm, Spotify, etc.)
    #[serde(rename = "largeImageUrl")]
    pub large_image_url: Option<String>,
    #[serde(rename = "mediumImageUrl")]
    pub medium_image_url: Option<String>,
    #[serde(rename = "smallImageUrl")]
    pub small_image_url: Option<String>,
    // Additional fields from API
    #[serde(rename = "playCount")]
    pub play_count: Option<u32>,
    #[serde(rename = "playDate")]
    pub play_date: Option<String>,
    #[serde(rename = "size")]
    pub size: Option<u64>,
    #[serde(rename = "mbzArtistId")]
    pub mbz_artist_id: Option<String>,
    #[serde(rename = "biography")]
    pub biography: Option<String>,
    #[serde(rename = "similarArtists")]
    pub similar_artists: Option<Vec<SimilarArtist>>,
    #[serde(rename = "externalUrl")]
    pub external_url: Option<String>,
    #[serde(rename = "externalInfoUpdatedAt")]
    pub external_info_updated_at: Option<String>,
    #[serde(rename = "rating")]
    pub rating: Option<u32>,
}

/// Similar artist reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarArtist {
    pub id: String,
    pub name: String,
}

impl Artist {
    /// Get display name for the artist
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Get album count (defaults to 0)
    pub fn get_album_count(&self) -> u32 {
        self.album_count.unwrap_or(0)
    }

    /// Get song count (defaults to 0)
    pub fn get_song_count(&self) -> u32 {
        self.song_count.unwrap_or(0)
    }

    /// Check if artist is starred
    pub fn is_starred(&self) -> bool {
        self.starred.unwrap_or(false)
    }

    /// Get the best available image URL
    /// Priority: large > medium > small
    pub fn get_image_url(&self) -> Option<&str> {
        self.large_image_url
            .as_deref()
            .or(self.medium_image_url.as_deref())
            .or(self.small_image_url.as_deref())
    }
}

impl std::fmt::Display for Artist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({} albums, {} songs)",
            self.name,
            self.get_album_count(),
            self.get_song_count()
        )
    }
}
