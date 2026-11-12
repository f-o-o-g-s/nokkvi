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
    // Signed purely for consistency with our own `Library::id` (i32). Upstream
    // is `int` on both `model.Library.ID` and `model.Album.LibraryID` (64-bit
    // on amd64) backed by an autoincrement rowid, so neither a negative value
    // nor an i32 overflow is reachable — this is tidiness, not a fix.
    //
    // Emitted by the native `/api/album` only; Subsonic never sends it. Nothing
    // in nokkvi reads this field today, so it is deserialize-only. Library ids
    // are exempt from Navidrome's canonical-id migration (they stay integers),
    // so this stays an integer too.
    #[serde(rename = "libraryId")]
    pub library_id: Option<i32>,
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
    /// The notes Navidrome's agents fetched for the album (Last.fm's wiki
    /// summary, or another agent's). Empty until some client has asked
    /// `getAlbumInfo2` about the album: see [`AlbumInfo`].
    #[serde(rename = "description")]
    pub description: Option<String>,
    /// The agents' page for the album, filled together with `description`.
    #[serde(rename = "externalUrl")]
    pub external_url: Option<String>,
    #[serde(rename = "starred", default)]
    #[serde(deserialize_with = "crate::types::deserialize_starred_opt")]
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

    /// Navidrome 0.64 image info (six flat keys; default on older servers).
    #[serde(flatten)]
    pub image: crate::types::image_info::ImageInfo,
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

/// What Subsonic `getAlbumInfo2` reports about one album: the notes and the
/// page Navidrome's agents fetched (Last.fm, or another configured agent).
///
/// Navidrome fills this lazily, only when a client asks: the native
/// `/api/album` row carries the same `description` / `externalUrl` columns,
/// but they stay empty for an album no client has asked about. Each field is
/// `None` when the server sent it empty or not at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlbumInfo {
    /// The agent's notes as plain text ([`plain_notes`]). Last.fm's are the
    /// opening of the album's wiki, a few hundred characters that can stop
    /// mid-sentence.
    pub notes: Option<String>,
    /// The agent's page for the album. Subsonic names the key `lastFmUrl`,
    /// but Navidrome fills it from whichever agent answered, so it is not
    /// always a Last.fm link. Most albums have a page; far fewer have notes.
    pub external_url: Option<String>,
}

/// An album's notes as the views show them, from either wire (`notes` on
/// `getAlbumInfo2`, `description` on the native row): tags stripped, since an
/// agent's text can carry an `<a>` link, and `None` when nothing is left.
pub fn plain_notes(raw: Option<&str>) -> Option<String> {
    let text = crate::utils::html::strip_html_tags(raw?);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

impl std::fmt::Display for Album {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} by {}", self.name, self.display_artist())
    }
}
