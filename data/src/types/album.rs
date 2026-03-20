use serde::{Deserialize, Serialize};

/// A participant artist with a role (e.g., composer, lyricist, producer).
/// Deserialized from Navidrome's `participants` field on album/song responses.
#[derive(Debug, Clone, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode)]
pub struct Participant {
    pub id: String,
    pub name: String,
    #[serde(rename = "subRole")]
    pub sub_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "albumArtist")]
    pub album_artist: Option<String>,
    #[serde(rename = "artist")]
    pub artist: Option<String>,
    #[serde(rename = "albumArtistId")]
    pub album_artist_id: Option<String>,
    #[serde(rename = "artistId")]
    pub artist_id: Option<String>,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
    #[serde(rename = "songCount")]
    pub song_count: Option<u32>,
    #[serde(rename = "duration")]
    pub duration: Option<f64>, // seconds (can be fractional)
    #[serde(rename = "maxYear")]
    pub max_year: Option<u32>,
    #[serde(rename = "year")]
    pub year: Option<u32>,
    #[serde(rename = "genre")]
    pub genre: Option<String>,
    #[serde(rename = "genres")]
    pub genres: Option<Vec<Genre>>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(rename = "playDate")]
    pub play_date: Option<String>,
    #[serde(rename = "playCount")]
    pub play_count: Option<u32>,
    // Additional fields from API response
    #[serde(rename = "libraryId")]
    pub library_id: Option<u32>,
    #[serde(rename = "libraryPath")]
    pub library_path: Option<String>,
    #[serde(rename = "libraryName")]
    pub library_name: Option<String>,
    #[serde(rename = "date")]
    pub date: Option<String>,
    #[serde(rename = "minYear")]
    pub min_year: Option<u32>,
    #[serde(rename = "maxOriginalYear")]
    pub max_original_year: Option<u32>,
    #[serde(rename = "minOriginalYear")]
    pub min_original_year: Option<u32>,
    #[serde(rename = "originalDate")]
    pub original_date: Option<String>,
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
    #[serde(rename = "compilation")]
    pub compilation: Option<bool>,
    #[serde(rename = "comment")]
    pub comment: Option<String>,
    #[serde(rename = "starred")]
    pub starred: Option<bool>,
    #[serde(rename = "starredAt")]
    pub starred_at: Option<String>,
    #[serde(rename = "rating")]
    pub rating: Option<u32>,
    /// Album update timestamp - used for cache invalidation
    /// When artwork is updated, this changes and triggers re-download
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(rename = "size")]
    pub size: Option<u64>,
    #[serde(rename = "mbzAlbumId")]
    pub mbz_album_id: Option<String>,
    /// MusicBrainz release type (Album, EP, Single, etc.)
    #[serde(rename = "mbzAlbumType")]
    pub mbz_album_type: Option<String>,
    /// Dynamic tags from Navidrome (disctotal, media, releasecountry, etc.)
    #[serde(rename = "tags")]
    pub tags: Option<std::collections::HashMap<String, Vec<String>>>,
    /// Role-based participants (composer, lyricist, producer, etc.)
    #[serde(default)]
    pub participants: Option<std::collections::HashMap<String, Vec<Participant>>>,

    /// Cached display artist (computed once at load time to avoid repeated allocations)
    /// This eliminates the memory leak from calling .display_artist().to_string() on every scroll
    #[serde(skip)]
    pub display_artist_cached: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genre {
    pub name: String,
}

impl Album {
    /// Get display name for the album
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Get display artist (prefers albumArtist over artist)
    pub fn display_artist(&self) -> &str {
        self.album_artist
            .as_deref()
            .or(self.artist.as_deref())
            .unwrap_or("Unknown Artist")
    }

    /// Check if album is starred
    pub fn is_starred(&self) -> bool {
        self.starred.unwrap_or(false)
    }
}

impl std::fmt::Display for Album {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} by {}", self.name, self.display_artist())
    }
}
