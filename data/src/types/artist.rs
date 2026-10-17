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
    #[serde(rename = "starred", default)]
    #[serde(deserialize_with = "crate::types::deserialize_starred_opt")]
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

    /// Navidrome 0.64 image info (six flat keys; default on older servers).
    #[serde(flatten)]
    pub image: crate::types::image_info::ImageInfo,
    /// Per-role counts (`albumartist`, `artist`, `composer`, ...; Navidrome
    /// 0.56+, empty on older servers). `album_count` above sums every role,
    /// so a featured-only artist still counts the albums they appear on;
    /// `stats["albumartist"]` is what says they have albums of their own.
    #[serde(default)]
    pub stats: std::collections::HashMap<String, ArtistRoleStats>,
}

/// One role's counts for an artist (`Artist::stats`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtistRoleStats {
    #[serde(rename = "albumCount", default)]
    pub album_count: u32,
    #[serde(rename = "songCount", default)]
    pub song_count: u32,
}

/// Similar artist reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarArtist {
    pub id: String,
    pub name: String,
}

impl Artist {
    /// The outside poster link the server passed along for the artist
    /// (large, else medium): what the big panel shows, and what the
    /// thumbnail shows when the server has no image of its own
    /// (`image_info::artist_mini_entry`). One pick for both, because the
    /// sizes can come from different agents with different pictures (a
    /// Deezer small and medium beside a Last.fm large).
    pub fn poster(&self) -> Option<&str> {
        [&self.large_image_url, &self.medium_image_url]
            .into_iter()
            .filter_map(|link| link.as_deref())
            .find(|link| !link.is_empty())
    }

    /// Whether the artist has albums of their own (the album-artist role),
    /// which is what the Artists view lists and expands. On a server that
    /// sends no per-role stats, any album counts.
    pub fn is_album_artist(&self) -> bool {
        match self.stats.get("albumartist") {
            Some(role) => role.album_count > 0,
            None => self.stats.is_empty() && self.get_album_count() > 0,
        }
    }

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

/// What Subsonic `getArtistInfo2` reports about one artist: the external
/// info Navidrome's agents fetched (Last.fm, or another configured agent).
///
/// Navidrome fills this lazily, only when a client asks: the native
/// `/api/artist` row carries the same `biography` / `externalUrl` columns, but
/// they stay empty for an artist no client has asked about. Every field is
/// `None` when the server sent it empty or not at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtistInfo {
    /// The agent's biography, sanitized by Navidrome. Its Last.fm agent
    /// strips Last.fm's trailing "Read more" link; another agent's text can
    /// still carry an `<a>` tag, as the native row's `biography` can.
    pub biography: Option<String>,
    /// The agent's page for the artist. Subsonic names the key `lastFmUrl`,
    /// but Navidrome fills it from whichever agent answered first, so it is
    /// not always a Last.fm link.
    pub external_url: Option<String>,
    pub musicbrainz_id: Option<String>,
    pub small_image_url: Option<String>,
    pub medium_image_url: Option<String>,
    pub large_image_url: Option<String>,
}
