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
    /// Delegates to the by-value impl (the single source of truth for this
    /// projection) at the cost of cloning the source `Genre`.
    fn from(genre: &Genre) -> Self {
        Self::from(genre.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the by-ref impl as a pure delegation to the by-value impl —
    /// the two projections must stay field-identical.
    #[test]
    fn from_ref_matches_from_value() {
        let genre = Genre {
            id: "genre-1".to_owned(),
            name: "Post-Rock".to_owned(),
            album_count: 7,
            song_count: 42,
        };

        let by_ref = GenreUIViewData::from(&genre);
        let by_value = GenreUIViewData::from(genre.clone());

        assert_eq!(by_ref.id, by_value.id);
        assert_eq!(by_ref.name, by_value.name);
        assert_eq!(by_ref.album_count, by_value.album_count);
        assert_eq!(by_ref.song_count, by_value.song_count);
        assert_eq!(by_ref.artwork_url, by_value.artwork_url);
        assert_eq!(by_ref.artwork_album_ids, by_value.artwork_album_ids);
        assert_eq!(by_ref.searchable_lower, by_value.searchable_lower);
    }
}
