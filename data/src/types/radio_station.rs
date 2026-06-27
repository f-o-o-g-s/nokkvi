use serde::{Deserialize, Serialize};

use crate::utils::search::Searchable;

/// Navidrome internet radio station (from getInternetRadioStations.view)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RadioStation {
    pub id: String,
    pub name: String,
    #[serde(rename = "streamUrl")]
    pub stream_url: String,
    #[serde(rename = "homePageUrl")]
    pub home_page_url: Option<String>,
    /// OpenSubsonic cover-art token for an admin-uploaded station logo.
    ///
    /// Navidrome's `getInternetRadioStations` (OpenSubsonic radio extension)
    /// flattens an `OpenSubsonicRadio` block into each station, whose `coverArt`
    /// is set **only when the station has an uploaded image**
    /// (`server/subsonic/radio.go`: `if g.UploadedImage != "" { coverArt = … }`)
    /// — otherwise it is the empty string, and on pre-extension / legacy-client
    /// responses the field is absent entirely (→ `None`). So a non-EMPTY value is
    /// a reliable "this station has a real logo" signal: a `getCoverArt?id=<token>`
    /// URL built from it will never return Navidrome's generic
    /// `album-placeholder.webp`. Gate on [`Self::logo_cover_art`], never on
    /// `is_some()`.
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
}

impl RadioStation {
    /// The uploaded-logo cover-art token, or `None` when this station has no
    /// logo. Collapses the `absent → None` and `present-but-empty → Some("")`
    /// cases Navidrome can emit (see [`Self::cover_art`]) into a single
    /// "is there a real logo?" check, so callers never request art for a
    /// logo-less station (which would fetch the generic placeholder).
    pub fn logo_cover_art(&self) -> Option<&str> {
        self.cover_art.as_deref().filter(|token| !token.is_empty())
    }
}

impl Searchable for RadioStation {
    /// Compute on the fly. Stations are typically a few dozen at most, so the
    /// per-call lowercase allocation is negligible. Caching would require
    /// either a `#[serde(skip)]` field with post-deserialize repopulation or
    /// changing the on-disk JSON shape — neither is justified at this scale.
    fn matches_query(&self, query_lower: &str) -> bool {
        self.name.to_lowercase().contains(query_lower)
            || self.stream_url.to_lowercase().contains(query_lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-empty `coverArt` (Navidrome emits it only for stations that have
    /// an uploaded logo) deserializes and is surfaced by `logo_cover_art`.
    #[test]
    fn coverart_present_yields_logo_token() {
        let json = r#"{
            "id": "1",
            "name": "SomaFM",
            "streamUrl": "http://example/stream",
            "homePageUrl": "http://example",
            "coverArt": "ra-1_18f0c3"
        }"#;
        let station: RadioStation = serde_json::from_str(json).expect("deserialize");
        assert_eq!(station.cover_art.as_deref(), Some("ra-1_18f0c3"));
        assert_eq!(station.logo_cover_art(), Some("ra-1_18f0c3"));
    }

    /// Navidrome sends `coverArt: ""` (not omitted) for a logo-less station on a
    /// non-legacy client. That must NOT be treated as a logo — otherwise every
    /// art-less station would request `getCoverArt` and receive the generic
    /// placeholder.
    #[test]
    fn coverart_empty_string_is_not_a_logo() {
        let json = r#"{
            "id": "2",
            "name": "No Logo FM",
            "streamUrl": "http://example/stream2",
            "coverArt": ""
        }"#;
        let station: RadioStation = serde_json::from_str(json).expect("deserialize");
        assert_eq!(station.cover_art.as_deref(), Some(""));
        assert_eq!(
            station.logo_cover_art(),
            None,
            "empty coverArt must gate out"
        );
    }

    /// Pre-extension Navidrome / legacy-client responses omit `coverArt`
    /// entirely; the `Option` field must deserialize to `None`, not error.
    #[test]
    fn coverart_absent_yields_none() {
        let json = r#"{
            "id": "3",
            "name": "Old Server FM",
            "streamUrl": "http://example/stream3"
        }"#;
        let station: RadioStation = serde_json::from_str(json).expect("deserialize");
        assert_eq!(station.cover_art, None);
        assert_eq!(station.logo_cover_art(), None);
    }
}
