#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryFilter {
    /// Filter by artist ID. Used on: Albums (`artist_id=`), Songs (`artists_id=`), Artists (`id=`).
    ArtistId { id: String, name: String },
    /// Filter by album ID. Used on: Songs (`album_id=`).
    AlbumId { id: String, title: String },
    /// Filter by genre name. Used on: Albums (`genre_id=`), Songs (`genre_id=`).
    /// Navidrome uses genre NAME as the filter key, not UUID.
    GenreId { id: String, name: String },
}

impl LibraryFilter {
    /// Format for display in the search box
    pub fn display_text(&self) -> String {
        match self {
            Self::ArtistId { name, .. } => name.clone(),
            Self::AlbumId { title, .. } => title.clone(),
            Self::GenreId { name, .. } => name.clone(),
        }
    }
}
