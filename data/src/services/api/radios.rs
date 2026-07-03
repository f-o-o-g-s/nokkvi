//! Internet Radio Stations API Service
//!
//! Wraps the Subsonic `getInternetRadioStations.view` endpoint.
//! Radio stations are simple objects: id, name, streamUrl, homePageUrl.
//!
//! NOTE from Claude: Built this ahead of Gemini's Phase 4 to unblock
//! the data crate work. Follows the exact GenresApiService pattern.

use anyhow::Result;
use tracing::debug;

use crate::{services::api::client::ApiClient, types::radio_station::RadioStation};

/// Inner payload of the Subsonic `getInternetRadioStations` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct RadiosInner {
    #[serde(rename = "internetRadioStations")]
    internet_radio_stations: Option<SubsonicRadioStations>,
}

#[derive(Debug, serde::Deserialize)]
struct SubsonicRadioStations {
    /// Subsonic XML→JSON can return a single object or an array.
    /// Using `serde_json::Value` + manual parsing handles both cases
    /// (same pattern as genres.rs).
    #[serde(rename = "internetRadioStation")]
    internet_radio_station: Option<serde_json::Value>,
}

#[derive(Clone)]
pub struct RadiosApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl RadiosApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch all internet radio stations from the Subsonic API.
    ///
    /// Returns a flat list — the Subsonic API has no pagination for radio stations.
    pub async fn load_radio_stations(&self) -> Result<Vec<RadioStation>> {
        let inner: RadiosInner = crate::services::api::subsonic::subsonic_get_envelope(
            &self.client.http_client(),
            &self.server_url,
            "getInternetRadioStations",
            &self.subsonic_credential,
            &[],
            "radio stations",
        )
        .await?;

        let mut stations = Vec::new();

        if let Some(radio_obj) = inner.internet_radio_stations
            && let Some(station_value) = radio_obj.internet_radio_station
        {
            // Subsonic can return a single object or an array (JSON quirk);
            // `deserialize_one_or_many` absorbs that.
            stations = crate::services::api::subsonic::deserialize_one_or_many(station_value)?;
        }

        debug!(
            " RadiosApiService: Loaded {} radio stations",
            stations.len()
        );

        Ok(stations)
    }

    /// Create a new internet radio station
    pub async fn create_radio_station(
        &self,
        name: &str,
        stream_url: &str,
        homepage_url: Option<&str>,
    ) -> Result<()> {
        let mut params = vec![("name", name), ("streamUrl", stream_url)];
        if let Some(url) = homepage_url {
            params.push(("homepageUrl", url));
        }

        crate::services::api::subsonic::subsonic_post_ok(
            &self.client.http_client(),
            &self.server_url,
            "createInternetRadioStation",
            &self.subsonic_credential,
            &params,
            "Failed to create internet radio station",
        )
        .await
    }

    /// Delete an internet radio station
    pub async fn delete_radio_station(&self, id: &str) -> Result<()> {
        crate::services::api::subsonic::subsonic_post_ok(
            &self.client.http_client(),
            &self.server_url,
            "deleteInternetRadioStation",
            &self.subsonic_credential,
            &[("id", id)],
            "Failed to delete internet radio station",
        )
        .await
    }

    /// Upload a custom logo image for a radio station. Navidrome stores it
    /// server-side; the next `getInternetRadioStations` response then carries
    /// a `coverArt` token (`ra-<id>_<ts>`) for the station, which nokkvi's
    /// existing logo pipeline renders. Requires `EnableArtworkUpload`
    /// (default true) or admin — a refusal surfaces as
    /// [`NokkviError::Forbidden`].
    ///
    /// Uses Navidrome native API: POST /api/radio/:id/image
    /// (`multipart/form-data`, one part named `image`).
    ///
    /// [`NokkviError::Forbidden`]: crate::types::error::NokkviError::Forbidden
    pub async fn upload_image(
        &self,
        station_id: &str,
        bytes: Vec<u8>,
        filename: &str,
    ) -> Result<()> {
        self.client
            .post_multipart(
                &format!("/api/radio/{station_id}/image"),
                "image",
                bytes,
                filename,
            )
            .await?;
        Ok(())
    }

    /// Delete a radio station's custom logo, reverting to the automatic
    /// artwork (ICY now-playing capture / tower glyph client-side).
    ///
    /// Uses Navidrome native API: DELETE /api/radio/:id/image
    pub async fn delete_image(&self, station_id: &str) -> Result<()> {
        self.client
            .delete(&format!("/api/radio/{station_id}/image"))
            .await
    }

    /// Update an internet radio station
    pub async fn update_radio_station(
        &self,
        id: &str,
        name: &str,
        stream_url: &str,
        homepage_url: Option<&str>,
    ) -> Result<()> {
        let mut params = vec![("id", id), ("name", name), ("streamUrl", stream_url)];
        if let Some(url) = homepage_url {
            params.push(("homepageUrl", url));
        }

        crate::services::api::subsonic::subsonic_post_ok(
            &self.client.http_client(),
            &self.server_url,
            "updateInternetRadioStation",
            &self.subsonic_credential,
            &params,
            "Failed to update internet radio station",
        )
        .await
    }
}
