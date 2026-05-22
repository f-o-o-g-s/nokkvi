#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryFilter {
    /// Filter by artist ID. Used on: Albums (`artist_id=`), Songs (`artists_id=`), Artists (`id=`).
    ArtistId { id: String, name: String },
    /// Filter by album ID. Used on: Songs (`album_id=`).
    AlbumId { id: String, title: String },
    /// Filter by genre name. Used on: Albums (`genre_id=`), Songs (`genre_id=`).
    /// Navidrome uses genre NAME as the filter key, not UUID.
    GenreId { id: String, name: String },
    /// Filter by one or more library (music folder) IDs. Reserved for future
    /// "show me everything in libraries X, Y" navigation surfaces — v1 wires
    /// the multi-library scope orthogonally through `library_ids: &[i32]` on
    /// each `load_*` API call so it composes with any other filter variant.
    LibraryIds(Vec<i32>),
}

impl LibraryFilter {
    /// Format for display in the search box
    pub fn display_text(&self) -> String {
        match self {
            Self::ArtistId { name, .. } => name.clone(),
            Self::AlbumId { title, .. } => title.clone(),
            Self::GenreId { name, .. } => name.clone(),
            Self::LibraryIds(ids) => match ids.as_slice() {
                [] => String::new(),
                [single] => format!("library {single}"),
                _ => format!("{} libraries", ids.len()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_text_for_library_ids_handles_empty() {
        let f = LibraryFilter::LibraryIds(Vec::new());
        assert_eq!(f.display_text(), "");
    }

    #[test]
    fn display_text_for_library_ids_handles_single() {
        let f = LibraryFilter::LibraryIds(vec![1]);
        assert_eq!(f.display_text(), "library 1");
    }

    #[test]
    fn display_text_for_library_ids_handles_multiple() {
        let f = LibraryFilter::LibraryIds(vec![1, 2, 3]);
        assert_eq!(f.display_text(), "3 libraries");
    }

    #[test]
    fn display_text_preserves_existing_variants() {
        let artist = LibraryFilter::ArtistId {
            id: "abc".to_string(),
            name: "Some Artist".to_string(),
        };
        assert_eq!(artist.display_text(), "Some Artist");

        let album = LibraryFilter::AlbumId {
            id: "xyz".to_string(),
            title: "Some Album".to_string(),
        };
        assert_eq!(album.display_text(), "Some Album");

        let genre = LibraryFilter::GenreId {
            id: "g1".to_string(),
            name: "Jazz".to_string(),
        };
        assert_eq!(genre.display_text(), "Jazz");
    }
}
