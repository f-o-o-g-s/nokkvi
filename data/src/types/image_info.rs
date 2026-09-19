//! Navidrome 0.64 image info: six FLAT optional keys on native-API album,
//! artist and playlist JSON (`model/artwork.go` `ItemImage`, embedded
//! untagged, every key `omitempty`).
//!
//! Three states: resolved (a hash and friends), known absent (only
//! `imageAbsent: true`), not resolved yet (no keys). No keys is also what
//! every server older than 0.64 sends, so [`ImageInfo::default`] means "keep
//! today's behavior".

use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};

/// The image info a 0.64+ server attaches to an entity. Embedded with
/// `#[serde(flatten)]`; each key is parsed leniently (a malformed value
/// reads as absent) so one odd value can never fail a whole page of rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageInfo {
    /// Content hash of the image bytes (XXH3-64, 16 lowercase hex). Use
    /// [`ImageInfo::valid_hash`], never the raw field.
    #[serde(
        rename = "imageHash",
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_hash: Option<String>,
    /// The server settled that this entity has no artwork anywhere.
    #[serde(
        rename = "imageAbsent",
        default,
        deserialize_with = "lenient_flag",
        skip_serializing_if = "is_false"
    )]
    pub image_absent: bool,
    /// ThumbHash placeholder (base64). Parsed and kept, not rendered.
    #[serde(
        rename = "thumbHash",
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub thumb_hash: Option<String>,
    /// `#rrggbb`. Use [`ImageInfo::dominant_rgb`], never the raw field.
    #[serde(
        rename = "dominantColor",
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub dominant_color: Option<String>,
    /// The source image's width in pixels.
    #[serde(
        rename = "imageWidth",
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_width: Option<u32>,
    /// The source image's height in pixels.
    #[serde(
        rename = "imageHeight",
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_height: Option<u32>,
}

impl ImageInfo {
    /// The image hash when it is exactly 16 lowercase hex characters (the
    /// server's XXH3-64 form); anything else is treated as no hash.
    pub fn valid_hash(&self) -> Option<&str> {
        self.image_hash.as_deref().filter(|h| {
            h.len() == 16
                && h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }

    /// The dominant color as RGB when it is a strict `#rrggbb` (either hex
    /// case); anything else is treated as no color.
    pub fn dominant_rgb(&self) -> Option<[u8; 3]> {
        let hex = self.dominant_color.as_deref()?.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        Some([channel(0)?, channel(2)?, channel(4)?])
    }
}

// Serde's `skip_serializing_if` hands the field by reference.
fn is_false(b: &bool) -> bool {
    !*b
}

/// Read any JSON value and keep it only when it has the expected type.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

/// [`lenient`] for a flag: anything but a JSON `true` reads as false.
fn lenient_flag<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(lenient::<D, bool>(deserializer)?.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::types::album::Album;

    fn album(extra: serde_json::Value) -> Album {
        let mut v = json!({ "id": "al1", "name": "A" });
        if let (Some(obj), Some(more)) = (v.as_object_mut(), extra.as_object()) {
            obj.extend(more.clone());
        }
        serde_json::from_value(v).expect("album parses")
    }

    /// A resolved 0.64 album carries all six keys.
    #[test]
    fn resolved_album_parses_all_six_keys() {
        let a = album(json!({
            "imageHash": "0123456789abcdef",
            "thumbHash": "1QcSHQRnh493V4dIh4eXh1h4kJUI",
            "dominantColor": "#12ab3f",
            "imageWidth": 1200,
            "imageHeight": 1000
        }));
        assert_eq!(
            a.image,
            ImageInfo {
                image_hash: Some("0123456789abcdef".into()),
                image_absent: false,
                thumb_hash: Some("1QcSHQRnh493V4dIh4eXh1h4kJUI".into()),
                dominant_color: Some("#12ab3f".into()),
                image_width: Some(1200),
                image_height: Some(1000),
            }
        );
        assert_eq!(a.image.valid_hash(), Some("0123456789abcdef"));
        assert_eq!(a.image.dominant_rgb(), Some([0x12, 0xab, 0x3f]));
    }

    #[test]
    fn absent_album_parses_the_flag_only() {
        let a = album(json!({ "imageAbsent": true }));
        assert!(a.image.image_absent);
        assert_eq!(a.image.valid_hash(), None);
    }

    /// No keys (a pre-0.64 server, or not resolved yet) is the default, and
    /// unknown extra keys are still ignored.
    #[test]
    fn old_server_album_is_default() {
        let a = album(json!({ "someFutureKey": { "x": 1 } }));
        assert_eq!(a.image, ImageInfo::default());
    }

    /// A malformed value reads as absent instead of failing the row.
    #[test]
    fn malformed_values_read_as_absent() {
        let a = album(json!({
            "imageHash": 5,
            "imageAbsent": "yes",
            "imageWidth": "wide",
            "imageHeight": -3,
            "dominantColor": null
        }));
        assert_eq!(a.image, ImageInfo::default());
        assert_eq!(a.name, "A", "the rest of the row still parsed");
    }

    #[test]
    fn valid_hash_is_exactly_16_lowercase_hex() {
        let with = |h: &str| ImageInfo {
            image_hash: Some(h.to_owned()),
            ..ImageInfo::default()
        };
        assert_eq!(
            with("0123456789abcdef").valid_hash(),
            Some("0123456789abcdef")
        );
        assert_eq!(with("0123456789abcde").valid_hash(), None, "15 chars");
        assert_eq!(with("0123456789abcdef0").valid_hash(), None, "17 chars");
        assert_eq!(with("0123456789ABCDEF").valid_hash(), None, "uppercase");
        assert_eq!(with("0123456789abcdeg").valid_hash(), None, "a g");
        assert_eq!(with("").valid_hash(), None);
        assert_eq!(ImageInfo::default().valid_hash(), None);
    }

    #[test]
    fn dominant_rgb_is_strict_hash_rrggbb() {
        let with = |c: &str| ImageInfo {
            dominant_color: Some(c.to_owned()),
            ..ImageInfo::default()
        };
        assert_eq!(with("#12ab3F").dominant_rgb(), Some([0x12, 0xab, 0x3f]));
        assert_eq!(with("#000000").dominant_rgb(), Some([0, 0, 0]));
        assert_eq!(with("#fff").dominant_rgb(), None);
        assert_eq!(with("red").dominant_rgb(), None);
        assert_eq!(with("12ab3f").dominant_rgb(), None);
        assert_eq!(with("#12ab3g").dominant_rgb(), None);
        assert_eq!(with("#+2ab3f").dominant_rgb(), None);
        assert_eq!(ImageInfo::default().dominant_rgb(), None);
    }

    /// Serializing an old-server album adds none of the six keys.
    #[test]
    fn old_server_album_serializes_without_image_keys() {
        let a = album(json!({}));
        let v = serde_json::to_value(&a).expect("serializes");
        for key in [
            "imageHash",
            "imageAbsent",
            "thumbHash",
            "dominantColor",
            "imageWidth",
            "imageHeight",
        ] {
            assert!(v.get(key).is_none(), "{key} leaked into {v}");
        }
    }

    /// Artists and playlists carry the same keys.
    #[test]
    fn artist_and_playlist_parse_image_info() {
        let artist: crate::types::artist::Artist = serde_json::from_value(json!({
            "id": "ar1", "name": "X", "imageAbsent": true
        }))
        .expect("artist parses");
        assert!(artist.image.image_absent);
        let playlist: crate::types::playlist::Playlist = serde_json::from_value(json!({
            "id": "pl1", "name": "P", "imageHash": "fedcba9876543210"
        }))
        .expect("playlist parses");
        assert_eq!(playlist.image.valid_hash(), Some("fedcba9876543210"));
    }
}
