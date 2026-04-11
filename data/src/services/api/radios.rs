//! Internet Radio Stations API Service
//!
//! Wraps the Subsonic `getInternetRadioStations.view` endpoint.
//! Radio stations are simple objects: id, name, streamUrl, homePageUrl.
//!
//! NOTE from Claude: Built this ahead of Gemini's Phase 4 to unblock
//! the data crate work. Follows the exact GenresApiService pattern.

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::debug;

use crate::{services::api::client::ApiClient, types::radio_station::RadioStation};

/// Subsonic API response for getInternetRadioStations
#[derive(Debug, serde::Deserialize)]
struct SubsonicRadiosResponse {
    #[serde(rename = "subsonic-response")]
    subsonic_response: SubsonicResponseInner,
}

#[derive(Debug, serde::Deserialize)]
struct SubsonicResponseInner {
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

pub struct RadiosApiService {
    client: Arc<ApiClient>,
    server_url: String,
    subsonic_credential: String,
}

impl RadiosApiService {
    /// Create with a pre-authenticated ApiClient
    pub fn new_with_client(
        client: ApiClient,
        server_url: String,
        subsonic_credential: String,
    ) -> Self {
        Self {
            client: Arc::new(client),
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch all internet radio stations from the Subsonic API.
    ///
    /// Returns a flat list — the Subsonic API has no pagination for radio stations.
    pub async fn load_radio_stations(&self) -> Result<Vec<RadioStation>> {
        let response = crate::services::api::subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getInternetRadioStations",
            &self.subsonic_credential,
            &[],
        )
        .await
        .context("Failed to fetch internet radio stations")?;

        let body = response
            .text()
            .await
            .context("Failed to read radio stations response")?;

        let parsed: SubsonicRadiosResponse = serde_json::from_str(&body).with_context(|| {
            format!(
                "Failed to parse radio stations JSON: {}",
                &body[..body.len().min(200)]
            )
        })?;

        let mut stations = Vec::new();

        if let Some(radio_obj) = parsed.subsonic_response.internet_radio_stations
            && let Some(station_value) = radio_obj.internet_radio_station
        {
            // Handle both array and single object cases (Subsonic JSON quirk)
            let station_array: Vec<RadioStation> = if station_value.is_array() {
                serde_json::from_value(station_value)?
            } else {
                vec![serde_json::from_value(station_value)?]
            };

            stations = station_array;
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

impl Clone for RadiosApiService {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            server_url: self.server_url.clone(),
            subsonic_credential: self.subsonic_credential.clone(),
        }
    }
}
