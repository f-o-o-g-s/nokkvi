//! Playlists ViewModel - UI-specific view data for playlists

use crate::types::playlist::Playlist;

/// UI-specific view data for playlists
/// UI-projected data
#[derive(Debug, Clone)]
pub struct PlaylistUIViewData {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub duration: f32,
    pub song_count: u32,
    pub owner_name: String,
    pub public: bool,
    pub updated_at: String,
    /// Album IDs for 3x3 collage (up to 9 albums)
    pub artwork_album_ids: Vec<String>,
    /// User-uploaded custom cover reference, already collapsed through
    /// `Playlist::custom_image` — `Some` here always means a REAL uploaded
    /// image (the wire's `""`-when-none form never reaches this projection).
    /// Gates the custom-artwork display path and the "Reset Artwork" menu
    /// entry; the value itself is unused (fetches key on `pl-<id>`).
    pub uploaded_image: Option<String>,
    /// Pre-lowercased search index — see `crate::utils::search::Searchable`.
    pub searchable_lower: String,
}

impl From<Playlist> for PlaylistUIViewData {
    fn from(playlist: Playlist) -> Self {
        let searchable_lower =
            crate::utils::search::build_searchable_lower(&[&playlist.name, &playlist.comment]);
        let uploaded_image = playlist.custom_image().map(str::to_string);
        Self {
            id: playlist.id,
            name: playlist.name,
            comment: playlist.comment,
            duration: playlist.duration,
            song_count: playlist.song_count,
            owner_name: playlist.owner_name,
            public: playlist.public,
            updated_at: playlist.updated_at,
            artwork_album_ids: Vec::new(),
            uploaded_image,
            searchable_lower,
        }
    }
}

impl From<&Playlist> for PlaylistUIViewData {
    /// Delegates to the by-value impl (the single source of truth for this
    /// projection) at the cost of cloning the whole source `Playlist`,
    /// including fields the projection drops (`size`, `owner_id`,
    /// `created_at`).
    fn from(playlist: &Playlist) -> Self {
        Self::from(playlist.clone())
    }
}

impl crate::utils::search::Searchable for PlaylistUIViewData {
    fn matches_query(&self, query_lower: &str) -> bool {
        self.searchable_lower.contains(query_lower)
    }
}

impl crate::types::collage_artwork::CollageArtworkItem for PlaylistUIViewData {
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
        let playlist = Playlist {
            id: "pl-1".to_owned(),
            name: "Road Trip".to_owned(),
            comment: "windows down".to_owned(),
            duration: 5421.5,
            size: 123_456_789,
            song_count: 31,
            owner_name: "foogs".to_owned(),
            owner_id: "user-9".to_owned(),
            public: true,
            created_at: "2026-01-02T03:04:05Z".to_owned(),
            updated_at: "2026-06-07T08:09:10Z".to_owned(),
            uploaded_image: Some("al-cover-ref".to_owned()),
            external_image_url: None,
        };

        let by_ref = PlaylistUIViewData::from(&playlist);
        let by_value = PlaylistUIViewData::from(playlist.clone());

        assert_eq!(by_ref.id, by_value.id);
        assert_eq!(by_ref.name, by_value.name);
        assert_eq!(by_ref.comment, by_value.comment);
        assert_eq!(by_ref.duration, by_value.duration);
        assert_eq!(by_ref.song_count, by_value.song_count);
        assert_eq!(by_ref.owner_name, by_value.owner_name);
        assert_eq!(by_ref.public, by_value.public);
        assert_eq!(by_ref.updated_at, by_value.updated_at);
        assert_eq!(by_ref.artwork_album_ids, by_value.artwork_album_ids);
        assert_eq!(by_ref.uploaded_image, by_value.uploaded_image);
        assert_eq!(by_ref.searchable_lower, by_value.searchable_lower);
    }

    /// The projection must carry only the COLLAPSED custom-image form: the
    /// wire's `""`-when-none encoding never reaches `PlaylistUIViewData`.
    #[test]
    fn projection_collapses_empty_uploaded_image_to_none() {
        let mut playlist: Playlist = serde_json::from_value(serde_json::json!({
            "id": "pl-2", "name": "Empty", "uploadedImage": ""
        }))
        .expect("fixture must deserialize");
        assert_eq!(playlist.uploaded_image.as_deref(), Some(""));
        assert_eq!(PlaylistUIViewData::from(&playlist).uploaded_image, None);

        playlist.uploaded_image = Some("real-ref".to_owned());
        assert_eq!(
            PlaylistUIViewData::from(&playlist)
                .uploaded_image
                .as_deref(),
            Some("real-ref")
        );
    }
}
