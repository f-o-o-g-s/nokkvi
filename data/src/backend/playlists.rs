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
    /// Whether this is a server-side smart playlist (collapsed through
    /// `Playlist::is_smart` — a `"rules": null` emission never reaches this
    /// projection as `true`). Gates the badge, the Edit-Rules/Edit-Playlist
    /// context-menu split, and every add-target exclusion.
    pub is_smart: bool,
    /// The RAW criteria substrate for smart rows (`None` on regular rows —
    /// the wire's `"rules": null` collapses here). The rules session seeds
    /// its byte-faithful working tree from this.
    pub rules: Option<serde_json::Value>,
    /// Server evaluation timestamp for smart playlists (the age-aware
    /// "evaluated …" stamp); `None` on regular or never-evaluated rows.
    pub evaluated_at: Option<String>,
    /// Whether a scanner-synced file (.nsp/.m3u) backs this playlist —
    /// drives the delete-resurrect honesty copy + Detach offer.
    pub is_file_backed: bool,
    /// Whether that file re-syncs its rules over API edits on every scan.
    pub sync: bool,
    /// The owning user's id — compared against the session `user_id` for
    /// the ownership gate (`is_owned`); NEVER compare owner names
    /// (Navidrome logins are case-insensitive).
    pub owner_id: String,
    /// Pre-lowercased search index — see `crate::utils::search::Searchable`.
    pub searchable_lower: String,
}

impl From<Playlist> for PlaylistUIViewData {
    fn from(playlist: Playlist) -> Self {
        let searchable_lower =
            crate::utils::search::build_searchable_lower(&[&playlist.name, &playlist.comment]);
        let uploaded_image = playlist.custom_image().map(str::to_string);
        let is_smart = playlist.is_smart();
        let is_file_backed = playlist.is_file_backed();
        let rules = if is_smart {
            playlist.rules.clone()
        } else {
            None
        };
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
            is_smart,
            rules,
            evaluated_at: playlist.evaluated_at,
            is_file_backed,
            sync: playlist.sync,
            owner_id: playlist.owner_id,
            searchable_lower,
        }
    }
}

impl From<&Playlist> for PlaylistUIViewData {
    /// Delegates to the by-value impl (the single source of truth for this
    /// projection) at the cost of cloning the whole source `Playlist`,
    /// including fields the projection drops (`size`, `created_at`).
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
            rules: Some(serde_json::json!({ "all": [ { "is": { "loved": true } } ] })),
            evaluated_at: Some("2026-07-01T10:00:00Z".to_owned()),
            path: "/music/Library/road_trip.nsp".to_owned(),
            sync: true,
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
        assert_eq!(by_ref.is_smart, by_value.is_smart);
        assert_eq!(by_ref.rules, by_value.rules);
        assert_eq!(by_ref.evaluated_at, by_value.evaluated_at);
        assert_eq!(by_ref.is_file_backed, by_value.is_file_backed);
        assert_eq!(by_ref.sync, by_value.sync);
        assert_eq!(by_ref.owner_id, by_value.owner_id);
        assert_eq!(by_ref.searchable_lower, by_value.searchable_lower);

        // The five smart-layer fields carry real values through the
        // projection (not defaults) — the parity assert above would pass
        // trivially if both impls dropped a field to its default.
        assert!(by_value.is_smart);
        assert!(
            by_value
                .rules
                .as_ref()
                .is_some_and(|r| r.get("all").is_some()),
            "the raw substrate rides the projection for smart rows"
        );
        assert_eq!(
            by_value.evaluated_at.as_deref(),
            Some("2026-07-01T10:00:00Z")
        );
        assert!(by_value.is_file_backed);
        assert!(by_value.sync);
        assert_eq!(by_value.owner_id, "user-9");
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
