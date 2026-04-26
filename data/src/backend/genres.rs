//! Genres — UI view data and collage artwork support

use crate::types::genre::Genre;

/// UI-specific view data for genres
/// UI-projected data
#[derive(Debug, Clone)]
pub struct GenreUIViewData {
    pub id: String,
    pub name: String,
    pub album_count: u32,
    pub song_count: u32,
    /// Mini artwork URL (first album in genre)
    pub artwork_url: Option<String>,
    /// Album IDs for 3x3 collage (up to 9 albums)
    pub artwork_album_ids: Vec<String>,
    /// Pre-lowercased search index — see `crate::utils::search::Searchable`.
    pub searchable_lower: String,
}

impl From<Genre> for GenreUIViewData {
    fn from(genre: Genre) -> Self {
        let searchable_lower = crate::utils::search::build_searchable_lower(&[&genre.name]);
        Self {
            id: genre.id,
            name: genre.name,
            album_count: genre.album_count,
            song_count: genre.song_count,
            artwork_url: None,
            artwork_album_ids: Vec::new(),
            searchable_lower,
        }
    }
}

impl From<&Genre> for GenreUIViewData {
    fn from(genre: &Genre) -> Self {
        let searchable_lower = crate::utils::search::build_searchable_lower(&[&genre.name]);
        Self {
            id: genre.id.clone(),
            name: genre.name.clone(),
            album_count: genre.album_count,
            song_count: genre.song_count,
            artwork_url: None,
            artwork_album_ids: Vec::new(),
            searchable_lower,
        }
    }
}

impl crate::utils::search::Searchable for GenreUIViewData {
    fn matches_query(&self, query_lower: &str) -> bool {
        self.searchable_lower.contains(query_lower)
    }
}

impl crate::types::collage_artwork::CollageArtworkItem for GenreUIViewData {
    fn id(&self) -> &str {
        &self.id
    }

    fn artwork_album_ids(&self) -> &[String] {
        &self.artwork_album_ids
    }
}
