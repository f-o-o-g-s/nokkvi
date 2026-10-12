//! Players API Service — Navidrome native `/api/player`.
//!
//! Route registered at `reference-navidrome/server/nativeapi/native_api.go`
//! (`rx(r, "/player", api.ds.Player(), true)`); rows are `model/player.go`
//! `Player` objects. nokkvi uses it only to hold its OpenSubsonic API key
//! (see [`crate::services::api_key`]): a create carries the key, a `PUT` with
//! a body of just `{"apiKey": …}` replaces or (empty) revokes it and leaves
//! every other column alone (`persistence/player_repository.go` `Update`, whose
//! column list is the JSON body's keys).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::services::api::{client::ApiClient, parse};

/// One `GET /api/player` row (`model/player.go`). Only the fields nokkvi reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayerRow {
    pub id: String,
    pub name: String,
    pub client: String,
    pub user_agent: String,
    pub user_id: String,
    pub user_name: String,
    /// RFC 3339 timestamp of the player's last request.
    pub last_seen: String,
    pub transcoding_id: String,
    pub max_bit_rate: i64,
    pub report_real_path: bool,
    pub scrobble_enabled: bool,
    pub has_api_key: bool,
}

/// The per-player settings a new player is created with. Navidrome fills
/// every field a create leaves out with Go's zero value, so `scrobbleEnabled`
/// is always sent: an omitted one turns scrobbling off for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSettings {
    pub transcoding_id: String,
    pub max_bit_rate: i64,
    pub report_real_path: bool,
    pub scrobble_enabled: bool,
}

impl Default for PlayerSettings {
    /// What Navidrome gives a player it registers itself
    /// (`core/players.go` `Register`): scrobbling on, no transcoding, no
    /// bit-rate cap. `reportRealPath` follows a server option there; nokkvi
    /// reads no paths from `/rest`, so it stays off.
    fn default() -> Self {
        Self {
            transcoding_id: String::new(),
            max_bit_rate: 0,
            report_real_path: false,
            scrobble_enabled: true,
        }
    }
}

/// `POST /api/player` body.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NewPlayer<'a> {
    name: &'a str,
    client: &'a str,
    api_key: &'a str,
    transcoding_id: &'a str,
    max_bit_rate: i64,
    report_real_path: bool,
    scrobble_enabled: bool,
}

/// `PUT /api/player/{id}` body that touches the key alone.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyOnly<'a> {
    api_key: &'a str,
}

/// `POST` answer: the new row's id.
#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Clone)]
pub struct PlayersApiService {
    client: ApiClient,
}

impl PlayersApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient) -> Self {
        Self { client }
    }

    /// Every player the session user may see: their own, or every user's for
    /// an admin.
    pub async fn list(&self) -> Result<Vec<PlayerRow>> {
        let body = self.client.get("/api/player", &[]).await?;
        parse::parse_json_with_preview(&body, "players")
    }

    /// One player row, which the session user owns or (an admin) may see.
    /// A missing one is [`crate::types::error::NokkviError::NotFound`].
    pub async fn get(&self, player_id: &str) -> Result<PlayerRow> {
        let body = self
            .client
            .get(&format!("/api/player/{player_id}"), &[])
            .await?;
        parse::parse_json_with_preview(&body, "player")
    }

    /// Create a player for the session user carrying `api_key`; returns its
    /// id. The server requires a key on a create (a hand-made player is
    /// reachable only through one) and rejects one not shaped
    /// `nds_` + 22 base62 characters.
    pub async fn create(
        &self,
        name: &str,
        client: &str,
        api_key: &str,
        settings: &PlayerSettings,
    ) -> Result<String> {
        let body = self
            .client
            .post_json(
                "/api/player",
                &NewPlayer {
                    name,
                    client,
                    api_key,
                    transcoding_id: &settings.transcoding_id,
                    max_bit_rate: settings.max_bit_rate,
                    report_real_path: settings.report_real_path,
                    scrobble_enabled: settings.scrobble_enabled,
                },
            )
            .await?;
        let created: Created = parse::parse_json_with_preview(&body, "created player")?;
        if created.id.is_empty() {
            anyhow::bail!("the server created a player without an id");
        }
        Ok(created.id)
    }

    /// Put `api_key` on player `player_id`, replacing its key, or revoke the
    /// key when `api_key` is empty. Only the key changes. Setting a key is
    /// owner-only on the server, even for an admin; a missing player is
    /// [`crate::types::error::NokkviError::NotFound`] and another user's is
    /// [`crate::types::error::NokkviError::Forbidden`].
    pub async fn set_api_key(&self, player_id: &str, api_key: &str) -> Result<()> {
        let endpoint = format!("/api/player/{player_id}");
        self.client
            .put_json(&endpoint, &KeyOnly { api_key })
            .await
            .with_context(|| {
                if api_key.is_empty() {
                    "revoking the player's API key"
                } else {
                    "setting the player's API key"
                }
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_player_body_always_carries_scrobble_enabled() {
        let settings = PlayerSettings::default();
        let body = serde_json::to_value(NewPlayer {
            name: "nokkvi (desk)",
            client: "nokkvi",
            api_key: "nds_aaaaaaaaaaaaaaaaaaaaaa",
            transcoding_id: &settings.transcoding_id,
            max_bit_rate: settings.max_bit_rate,
            report_real_path: settings.report_real_path,
            scrobble_enabled: settings.scrobble_enabled,
        })
        .expect("serializes");
        assert_eq!(
            body,
            serde_json::json!({
                "name": "nokkvi (desk)",
                "client": "nokkvi",
                "apiKey": "nds_aaaaaaaaaaaaaaaaaaaaaa",
                "transcodingId": "",
                "maxBitRate": 0,
                "reportRealPath": false,
                "scrobbleEnabled": true,
            })
        );
    }

    /// A key-only `PUT` names nothing but `apiKey`, so the server's column
    /// list is that one key and every other setting on the row survives.
    #[test]
    fn key_only_body_names_only_the_key() {
        let body = serde_json::to_value(KeyOnly { api_key: "" }).expect("serializes");
        assert_eq!(body, serde_json::json!({ "apiKey": "" }));
    }

    #[test]
    fn player_rows_parse_with_missing_fields() {
        let rows: Vec<PlayerRow> = serde_json::from_str(
            r#"[{"id":"p1","name":"nokkvi [nokkvi]","client":"nokkvi","userAgent":"nokkvi",
                 "userId":"u1","userName":"alice","lastSeen":"2026-10-11T10:00:00Z",
                 "transcodingId":"t1","maxBitRate":192,"reportRealPath":false,
                 "scrobbleEnabled":false,"hasApiKey":false},
                {"id":"p2"}]"#,
        )
        .expect("parses");
        assert_eq!(rows[0].max_bit_rate, 192);
        assert!(!rows[0].scrobble_enabled);
        assert_eq!(rows[1].id, "p2");
        assert!(!rows[1].has_api_key);
    }
}
