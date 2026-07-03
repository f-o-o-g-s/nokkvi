//! Playlist model from Navidrome API

use serde::{Deserialize, Serialize};

/// Playlist model from Navidrome API
/// Data from Native API (/api/playlist)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "comment", default)]
    pub comment: String,
    #[serde(rename = "duration", default)]
    pub duration: f32,
    #[serde(rename = "size", default)]
    pub size: i64,
    #[serde(rename = "songCount", default)]
    pub song_count: u32,
    #[serde(rename = "ownerName", default)]
    pub owner_name: String,
    #[serde(rename = "ownerId", default)]
    pub owner_id: String,
    #[serde(rename = "public", default)]
    pub public: bool,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
    /// Reference to a user-uploaded custom cover image, set by
    /// `POST /api/playlist/:id/image` and cleared by the matching DELETE.
    /// Navidrome always emits the key (`""` when none set); pre-feature
    /// servers omit it entirely (→ `None`). Gate on [`Self::custom_image`],
    /// never on `is_some()` — the empty-string form must not count.
    #[serde(rename = "uploadedImage", default)]
    pub uploaded_image: Option<String>,
    /// External image URL (M3U `#EXTIMG` import / plugin-managed). Emitted
    /// with `omitempty`, so it is absent when unset. Parsed for completeness
    /// but NOT treated as "has custom art": the image DELETE endpoint does
    /// not clear it, so keying Set/Reset on it would break reset symmetry
    /// (nokkvi could never remove it). v1 custom-artwork gating keys on
    /// [`Self::custom_image`] only.
    #[serde(rename = "externalImageUrl", default)]
    pub external_image_url: Option<String>,
}

impl Playlist {
    /// Get display name for the playlist
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Get song count
    pub fn get_song_count(&self) -> u32 {
        self.song_count
    }

    /// The user-uploaded custom cover reference, or `None` when this playlist
    /// has no uploaded image. Collapses the `absent → None` and
    /// `present-but-empty → Some("")` cases Navidrome emits (see
    /// [`Self::uploaded_image`]) into a single "is there a custom cover?"
    /// check — mirrors `RadioStation::logo_cover_art`.
    pub fn custom_image(&self) -> Option<&str> {
        self.uploaded_image.as_deref().filter(|s| !s.is_empty())
    }
}

impl std::fmt::Display for Playlist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({} songs)", self.name, self.song_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the `radio_station.rs` test trio: present / empty-string /
    /// absent forms of the wire field, plus the collapsing accessor.
    #[test]
    fn uploaded_image_present_yields_custom_image() {
        let json = r#"{
            "id": "p1",
            "name": "Mix",
            "uploadedImage": "al-p1-cover-ref",
            "externalImageUrl": "https://example/img.png"
        }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert_eq!(p.uploaded_image.as_deref(), Some("al-p1-cover-ref"));
        assert_eq!(p.custom_image(), Some("al-p1-cover-ref"));
        assert_eq!(
            p.external_image_url.as_deref(),
            Some("https://example/img.png")
        );
    }

    /// Navidrome always emits `uploadedImage` (json, no omitempty) — `""`
    /// when no custom image is set. That must NOT count as custom art.
    #[test]
    fn uploaded_image_empty_string_is_not_custom() {
        let json = r#"{ "id": "p2", "name": "Mix", "uploadedImage": "" }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert_eq!(p.uploaded_image.as_deref(), Some(""));
        assert_eq!(p.custom_image(), None, "empty uploadedImage must gate out");
        assert_eq!(p.external_image_url, None);
    }

    /// Pre-feature servers omit both keys entirely; the `Option` fields must
    /// deserialize to `None`, not error — additive wire-safety.
    #[test]
    fn uploaded_image_absent_yields_none() {
        let json = r#"{ "id": "p3", "name": "Mix" }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert_eq!(p.uploaded_image, None);
        assert_eq!(p.custom_image(), None);
        assert_eq!(p.external_image_url, None);
    }
}
