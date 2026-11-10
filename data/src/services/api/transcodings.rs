//! Transcodings API Service — Navidrome native `/api/transcoding`.
//!
//! Route registered at `reference-navidrome/server/nativeapi/native_api.go`
//! (`rx(r, "/transcoding", …)`); rows are `model/transcoding.go`
//! `Transcoding` objects. Any user may read them (the `command` comes back
//! blank for a regular one); only an admin with `EnableTranscodingConfig`
//! may write them, and nokkvi never does. It reads one to name the format a
//! transcoding forced on nokkvi's player sends.

use anyhow::Result;
use serde::Deserialize;

use crate::services::api::{client::ApiClient, parse};

/// One `GET /api/transcoding/{id}` row. Only the fields nokkvi reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TranscodingRow {
    pub id: String,
    pub name: String,
    /// The format it sends (`mp3`, `opus`, `aac`, ...).
    pub target_format: String,
    /// Its bit rate (kbps) when the player sets none.
    pub default_bit_rate: i64,
}

#[derive(Clone)]
pub struct TranscodingsApiService {
    client: ApiClient,
}

impl TranscodingsApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient) -> Self {
        Self { client }
    }

    /// One transcoding row. A missing one is
    /// [`crate::types::error::NokkviError::NotFound`].
    pub async fn get(&self, transcoding_id: &str) -> Result<TranscodingRow> {
        let body = self
            .client
            .get(&format!("/api/transcoding/{transcoding_id}"), &[])
            .await?;
        parse::parse_json_with_preview(&body, "transcoding")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transcoding_row_parses_with_a_blank_command() {
        let row: TranscodingRow = serde_json::from_str(
            r#"{"id":"t1","name":"mp3 audio","targetFormat":"mp3","defaultBitRate":192,
                "command":"","createdAt":"2026-01-01T00:00:00Z"}"#,
        )
        .expect("parses");
        assert_eq!(row.target_format, "mp3");
        assert_eq!(row.default_bit_rate, 192);
    }
}
