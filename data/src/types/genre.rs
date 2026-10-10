use serde::{Deserialize, Serialize};

use crate::types::filter::LibraryFilter;

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

/// A genre as the play / queue / Trawl pipeline carries it: the tag id the
/// song fetch filters on, plus the name that labels, toasts and logs show.
///
/// The id is the `/api/genre` row's tag id (a hash of the lowercased name),
/// so build one only from such a row; a genre known only by its name has no
/// `GenreRef`. Fetching by the id (`genre_id`) is an indexed join since
/// Navidrome 0.64, and it covers every casing of the name the row's counts
/// include, which the bare `genre=<name>` match did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenreRef {
    pub id: String,
    pub name: String,
}

impl GenreRef {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }

    /// The `/api/song` filter for this genre's songs (`genre_id=<tag id>`).
    pub fn songs_filter(&self) -> LibraryFilter {
        LibraryFilter::GenreId {
            id: self.id.clone(),
            name: self.name.clone(),
        }
    }
}

impl From<&Genre> for GenreRef {
    fn from(genre: &Genre) -> Self {
        Self::new(genre.id.clone(), genre.name.clone())
    }
}
