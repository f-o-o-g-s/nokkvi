//! ListenBrainz `submit-listens` client for direct radio scrobbling.
//!
//! ListenBrainz accepts free-form `artist_name` / `track_name` (every MBID
//! field is optional), which is exactly what radio ICY metadata provides — so
//! radio tracks can be submitted directly with no library id and no
//! canonicalization. Auth is a single user token (no signing, no OAuth),
//! sent as `Authorization: Token <token>`.
//!
//! API reference: <https://listenbrainz.readthedocs.io/en/latest/users/api/core.html>

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use super::ScrobbleTrack;

/// Default ListenBrainz API root. Override for Maloja or a self-hosted
/// instance (both speak the same `submit-listens` protocol).
pub const DEFAULT_API_URL: &str = "https://api.listenbrainz.org";

/// Identifies nokkvi as the submitting client in `additional_info`.
const SUBMISSION_CLIENT: &str = "nokkvi";

/// A configured ListenBrainz endpoint + user token.
///
/// Cheap to clone (the HTTP client is an `Arc`); construct once per session and
/// reuse. The `http` client should carry a descriptive User-Agent.
#[derive(Clone)]
pub struct ListenBrainzClient {
    http: Arc<reqwest::Client>,
    api_url: String,
    token: String,
}

impl ListenBrainzClient {
    /// Construct against `api_url` (use [`DEFAULT_API_URL`] for listenbrainz.org)
    /// with the user's submission `token`. A trailing slash on `api_url` is
    /// normalized away.
    pub fn new(
        http: Arc<reqwest::Client>,
        api_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        let api_url = api_url.into().trim_end_matches('/').to_string();
        Self {
            http,
            api_url,
            token: token.into(),
        }
    }

    fn auth_header(&self) -> String {
        format!("Token {}", self.token)
    }

    /// Validate the configured token. ListenBrainz returns `valid: true` and
    /// the owning `user_name` for a good token.
    pub async fn validate_token(&self) -> Result<TokenValidation> {
        let url = format!("{}/1/validate-token", self.api_url);
        let resp = self
            .http
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .send()
            .await
            .context("Failed to send ListenBrainz validate-token request")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "ListenBrainz validate-token failed with status {status}: {body}"
            ));
        }
        serde_json::from_str(&body).with_context(|| {
            format!("Failed to parse ListenBrainz validate-token response: {body}")
        })
    }

    /// Submit a completed listen (`listen_type: "single"`). `listened_at` is the
    /// instant the track STARTED, in unix seconds (ListenBrainz convention).
    pub async fn submit_listen(&self, track: &ScrobbleTrack, listened_at: i64) -> Result<()> {
        let payload = build_payload(ListenType::Single, track, Some(listened_at), None);
        self.submit(&payload).await
    }

    /// Submit an ephemeral now-playing notification (`listen_type:
    /// "playing_now"`, no timestamp). Does NOT create a permanent listen —
    /// safe to call on every track change and to use for connection tests.
    pub async fn submit_playing_now(&self, track: &ScrobbleTrack) -> Result<()> {
        let payload = build_payload(ListenType::PlayingNow, track, None, None);
        self.submit(&payload).await
    }

    async fn submit(&self, payload: &SubmitPayload<'_>) -> Result<()> {
        let url = format!("{}/1/submit-listens", self.api_url);
        let resp = self
            .http
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .json(payload)
            .send()
            .await
            .context("Failed to send ListenBrainz submit-listens request")?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(anyhow!(
            "ListenBrainz submit-listens failed with status {status}: {body}"
        ))
    }
}

/// Response shape of `GET /1/validate-token`.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenValidation {
    pub valid: bool,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Copy)]
enum ListenType {
    Single,
    PlayingNow,
}

impl ListenType {
    fn as_str(self) -> &'static str {
        match self {
            ListenType::Single => "single",
            ListenType::PlayingNow => "playing_now",
        }
    }
}

// --- Wire payload (serialized to the submit-listens JSON body) ---------------

#[derive(Serialize)]
struct SubmitPayload<'a> {
    listen_type: &'a str,
    // Exactly one listen per submission (radio is one-track-at-a-time).
    payload: [ListenPayload<'a>; 1],
}

#[derive(Serialize)]
struct ListenPayload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    listened_at: Option<i64>,
    track_metadata: TrackMetadata<'a>,
}

#[derive(Serialize)]
struct TrackMetadata<'a> {
    artist_name: &'a str,
    track_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_name: Option<&'a str>,
    additional_info: AdditionalInfo<'a>,
}

#[derive(Serialize)]
struct AdditionalInfo<'a> {
    submission_client: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    submission_client_version: Option<&'a str>,
    media_player: &'a str,
    /// Originating radio station, surfaced as the listen's music-service name.
    #[serde(skip_serializing_if = "Option::is_none")]
    music_service_name: Option<&'a str>,
}

fn build_payload<'a>(
    listen_type: ListenType,
    track: &'a ScrobbleTrack,
    listened_at: Option<i64>,
    client_version: Option<&'a str>,
) -> SubmitPayload<'a> {
    SubmitPayload {
        listen_type: listen_type.as_str(),
        payload: [ListenPayload {
            listened_at,
            track_metadata: TrackMetadata {
                artist_name: &track.artist,
                track_name: &track.title,
                release_name: track.album.as_deref(),
                additional_info: AdditionalInfo {
                    submission_client: SUBMISSION_CLIENT,
                    submission_client_version: client_version,
                    media_player: SUBMISSION_CLIENT,
                    music_service_name: track.station_name.as_deref(),
                },
            },
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_track() -> ScrobbleTrack {
        ScrobbleTrack {
            artist: "Daft Punk".to_string(),
            title: "Around the World".to_string(),
            album: Some("Homework".to_string()),
            station_name: Some("SomaFM Groove Salad".to_string()),
        }
    }

    #[test]
    fn single_listen_payload_has_timestamp_and_full_metadata() {
        let track = full_track();
        let payload = build_payload(
            ListenType::Single,
            &track,
            Some(1_700_000_000),
            Some("0.11.3"),
        );
        let v = serde_json::to_value(&payload).unwrap();

        assert_eq!(v["listen_type"], "single");
        let listen = &v["payload"][0];
        assert_eq!(listen["listened_at"], 1_700_000_000_i64);

        let meta = &listen["track_metadata"];
        assert_eq!(meta["artist_name"], "Daft Punk");
        assert_eq!(meta["track_name"], "Around the World");
        assert_eq!(meta["release_name"], "Homework");

        let info = &meta["additional_info"];
        assert_eq!(info["submission_client"], "nokkvi");
        assert_eq!(info["submission_client_version"], "0.11.3");
        assert_eq!(info["media_player"], "nokkvi");
        assert_eq!(info["music_service_name"], "SomaFM Groove Salad");
    }

    #[test]
    fn playing_now_payload_omits_timestamp() {
        let track = full_track();
        let payload = build_payload(ListenType::PlayingNow, &track, None, Some("0.11.3"));
        let v = serde_json::to_value(&payload).unwrap();

        assert_eq!(v["listen_type"], "playing_now");
        // `listened_at` must be ABSENT (not null) for playing_now.
        assert!(
            v["payload"][0].get("listened_at").is_none(),
            "playing_now must omit listened_at, got: {v}"
        );
    }

    #[test]
    fn optional_fields_are_omitted_when_absent() {
        let track = ScrobbleTrack {
            artist: "A".to_string(),
            title: "B".to_string(),
            album: None,
            station_name: None,
        };
        let payload = build_payload(ListenType::Single, &track, Some(1), None);
        let meta = &serde_json::to_value(&payload).unwrap()["payload"][0]["track_metadata"];

        assert!(
            meta.get("release_name").is_none(),
            "blank album must be omitted"
        );
        let info = &meta["additional_info"];
        assert!(
            info.get("submission_client_version").is_none(),
            "absent version must be omitted (not null)"
        );
        assert!(
            info.get("music_service_name").is_none(),
            "absent station must be omitted"
        );
        // The required identity fields are always present.
        assert_eq!(info["submission_client"], "nokkvi");
        assert_eq!(info["media_player"], "nokkvi");
    }

    #[test]
    fn new_normalizes_trailing_slash_on_api_url() {
        let http = Arc::new(reqwest::Client::new());
        let client = ListenBrainzClient::new(http, "https://api.listenbrainz.org/", "tok");
        assert_eq!(client.api_url, "https://api.listenbrainz.org");
        assert_eq!(client.auth_header(), "Token tok");
    }

    /// Live integration test against the real ListenBrainz API. Non-destructive:
    /// it validates the token and sends an EPHEMERAL `playing_now` (which never
    /// becomes a permanent listen). A real `submit_listen` (which WOULD appear
    /// in listen history) only runs when `NOKKVI_LB_LIVE_SUBMIT=1`.
    ///
    /// Run with:
    ///   LISTENBRAINZ_TOKEN=$(grep '^token' ~/.config/listenbrainz-mpd/config.toml \
    ///       | sed -E 's/.*=[[:space:]]*//; s/"//g') \
    ///   cargo test -p nokkvi-data --  --ignored listenbrainz_live
    #[tokio::test]
    #[ignore = "needs a real LISTENBRAINZ_TOKEN env var; hits the network"]
    async fn listenbrainz_live_validate_and_playing_now() {
        let Ok(token) = std::env::var("LISTENBRAINZ_TOKEN") else {
            eprintln!("LISTENBRAINZ_TOKEN unset — skipping live test");
            return;
        };
        let http = Arc::new(
            reqwest::Client::builder()
                .user_agent("nokkvi-radio-scrobble-test/0 (+https://github.com/f-o-o-g-s/nokkvi)")
                .build()
                .expect("build http client"),
        );
        let client = ListenBrainzClient::new(http, DEFAULT_API_URL, token);

        let validation = client
            .validate_token()
            .await
            .expect("validate-token request");
        assert!(validation.valid, "token must be valid: {validation:?}");
        assert!(
            validation.user_name.is_some(),
            "valid token must carry a user_name"
        );
        eprintln!("validated as: {:?}", validation.user_name);

        let track = ScrobbleTrack {
            artist: "nokkvi radio-scrobble test".to_string(),
            title: "playing_now (ephemeral, not a real listen)".to_string(),
            album: None,
            station_name: Some("nokkvi test".to_string()),
        };
        client
            .submit_playing_now(&track)
            .await
            .expect("playing_now submit");

        if std::env::var("NOKKVI_LB_LIVE_SUBMIT").as_deref() == Ok("1") {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_secs() as i64;
            client
                .submit_listen(&track, now)
                .await
                .expect("single listen submit");
            eprintln!("submitted a REAL listen at {now}");
        }
    }
}
