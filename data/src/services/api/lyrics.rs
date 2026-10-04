//! Subsonic API client for `getLyricsBySongId` (OpenSubsonic Song Lyrics v2).
//!
//! Returns structured, optionally word-level-timed lyrics keyed by song id — no
//! matching required. Word-level (`cueLine`/`cue`) and `kind` are only populated
//! when called with `enhanced=true`, which is why the resolve chain always does.

use anyhow::Result;

use crate::{
    services::api::client::ApiClient,
    types::lyrics::{StructuredCue, StructuredCueLine, StructuredLine, StructuredLyrics},
};

/// Inner payload of the `getLyricsBySongId` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct LyricsListInner {
    #[serde(rename = "lyricsList")]
    lyrics_list: Option<LyricsListBody>,
}

#[derive(Debug, serde::Deserialize)]
struct LyricsListBody {
    // Navidrome marshals via Go `encoding/json`, so this is always a real JSON
    // array (unlike the XML-bridge endpoints that need `deserialize_one_or_many`).
    #[serde(rename = "structuredLyrics", default)]
    structured_lyrics: Vec<WireStructured>,
}

#[derive(Debug, serde::Deserialize)]
struct WireStructured {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    synced: bool,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    line: Vec<WireLine>,
    #[serde(rename = "cueLine", default)]
    cue_line: Vec<WireCueLine>,
}

#[derive(Debug, serde::Deserialize)]
struct WireLine {
    #[serde(default)]
    start: Option<u32>,
    #[serde(default)]
    value: String,
}

#[derive(Debug, serde::Deserialize)]
struct WireCueLine {
    #[serde(default)]
    index: usize,
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
    #[serde(default)]
    cue: Vec<WireCue>,
}

#[derive(Debug, serde::Deserialize)]
struct WireCue {
    #[serde(default)]
    start: u32,
    // `value` is the exact word text; `byteStart`/`byteEnd` are line-relative
    // inclusive offsets we deliberately ignore (see LrcDocument::from_structured).
    #[serde(default)]
    value: String,
}

impl From<WireStructured> for StructuredLyrics {
    fn from(w: WireStructured) -> Self {
        StructuredLyrics {
            synced: w.synced,
            offset_ms: w.offset.unwrap_or(0),
            kind: w.kind,
            lines: w
                .line
                .into_iter()
                .map(|l| StructuredLine {
                    start_ms: l.start,
                    value: l.value,
                })
                .collect(),
            cue_lines: w
                .cue_line
                .into_iter()
                .map(|cl| StructuredCueLine {
                    index: cl.index,
                    agent_id: cl.agent_id,
                    cues: cl
                        .cue
                        .into_iter()
                        .map(|c| StructuredCue {
                            start_ms: c.start,
                            value: c.value,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[derive(Clone)]
pub struct LyricsApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl LyricsApiService {
    /// Create with a pre-authenticated `ApiClient`.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch structured lyrics for a song id. Pass `enhanced = true` to request
    /// word-level (`cueLine`/`cue`) timing where the server has it. A failed
    /// envelope is an error (checked reader): its empty list would otherwise
    /// read as "no lyrics" and be cached for the session.
    pub async fn get_lyrics_by_song_id(
        &self,
        id: &str,
        enhanced: bool,
    ) -> Result<Vec<StructuredLyrics>> {
        let inner: LyricsListInner = crate::services::api::subsonic::subsonic_get_envelope_checked(
            &self.client.http_client(),
            &self.server_url,
            "getLyricsBySongId",
            &self.subsonic_credential,
            &[
                ("id", id),
                ("enhanced", if enhanced { "true" } else { "false" }),
            ],
            "getLyricsBySongId",
        )
        .await?;

        Ok(inner
            .lyrics_list
            .map(|body| body.structured_lyrics.into_iter().map(Into::into).collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::subsonic::{SubsonicEnvelope, tests::one_shot_server};

    async fn fetch_from(base_url: String) -> Result<Vec<StructuredLyrics>> {
        let url = url::Url::parse(&base_url).expect("one-shot server url");
        LyricsApiService::new(ApiClient::new(url, String::new()), base_url, "u=x".into())
            .get_lyrics_by_song_id("s1", true)
            .await
    }

    /// Navidrome reports an error (an unknown id, a failed lyrics read) as HTTP
    /// 200 with a failed envelope. It must be an error: as an empty list the
    /// resolve chain would read it as "this song has no lyrics" and cache that
    /// for the session. A song without lyrics is an ok envelope, still empty.
    #[tokio::test]
    async fn a_failed_envelope_is_an_error_not_an_empty_list() {
        const FAILED: &str = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"data not found"}}}"#;
        let err = fetch_from(one_shot_server("200 OK", FAILED).await)
            .await
            .expect_err("a failed envelope must be an error");
        assert!(
            format!("{err:#}").contains("data not found"),
            "got: {err:#}"
        );

        const NO_LYRICS: &str =
            r#"{"subsonic-response":{"status":"ok","version":"1.16.1","lyricsList":{}}}"#;
        let lyrics = fetch_from(one_shot_server("200 OK", NO_LYRICS).await)
            .await
            .expect("an ok envelope with no lyrics is not an error");
        assert!(lyrics.is_empty());
    }

    #[test]
    fn parses_line_level_structured_lyrics() {
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "lyricsList": {
                    "structuredLyrics": [{
                        "displayArtist": "Beach House",
                        "displayTitle": "Myth",
                        "lang": "eng",
                        "offset": 0,
                        "synced": true,
                        "line": [
                            { "start": 45470, "value": "Drifting in and out" },
                            { "start": 48540, "value": "You see the road" }
                        ]
                    }]
                }
            }
        }"#;

        let parsed: SubsonicEnvelope<LyricsListInner> = serde_json::from_str(json).expect("parse");
        let list: Vec<StructuredLyrics> = parsed
            .response
            .lyrics_list
            .map(|b| b.structured_lyrics.into_iter().map(Into::into).collect())
            .unwrap_or_default();

        assert_eq!(list.len(), 1);
        assert!(list[0].synced);
        assert_eq!(list[0].lines.len(), 2);
        assert_eq!(list[0].lines[0].start_ms, Some(45470));
        assert_eq!(list[0].lines[1].value, "You see the road");
        assert!(list[0].cue_lines.is_empty());
    }

    #[test]
    fn parses_enhanced_word_level_lyrics() {
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "lyricsList": {
                    "structuredLyrics": [{
                        "synced": true,
                        "kind": "main",
                        "line": [{ "start": 1000, "value": "Hello world" }],
                        "cueLine": [{
                            "index": 0,
                            "agentId": "v1",
                            "cue": [
                                { "start": 1000, "value": "Hello ", "byteStart": 0, "byteEnd": 5 },
                                { "start": 1500, "value": "world", "byteStart": 6, "byteEnd": 10 }
                            ]
                        }]
                    }]
                }
            }
        }"#;

        let parsed: SubsonicEnvelope<LyricsListInner> = serde_json::from_str(json).expect("parse");
        let list: Vec<StructuredLyrics> = parsed
            .response
            .lyrics_list
            .map(|b| b.structured_lyrics.into_iter().map(Into::into).collect())
            .unwrap_or_default();

        assert_eq!(list[0].kind.as_deref(), Some("main"));
        assert_eq!(list[0].cue_lines.len(), 1);
        assert_eq!(list[0].cue_lines[0].index, 0);
        assert_eq!(list[0].cue_lines[0].cues.len(), 2);
        // cue.value used directly — trailing space preserved from the wire.
        assert_eq!(list[0].cue_lines[0].cues[0].value, "Hello ");
        assert_eq!(list[0].cue_lines[0].cues[1].start_ms, 1500);
    }

    #[test]
    fn parses_plain_untimed_lyrics() {
        // What Navidrome sends for a `USLT` frame (or a Vorbis/FLAC/MP4
        // `unsyncedlyrics` property): `synced: false` and lines carrying only
        // `value`. `WireLine.start` is already optional, so each line must
        // arrive with `start_ms: None` rather than a fabricated 0.
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "lyricsList": {
                    "structuredLyrics": [{
                        "displayArtist": "Beach House",
                        "displayTitle": "Myth",
                        "lang": "eng",
                        "synced": false,
                        "kind": "main",
                        "line": [
                            { "value": "Drifting in and out" },
                            { "value": "You see the road" }
                        ]
                    }]
                }
            }
        }"#;

        let parsed: SubsonicEnvelope<LyricsListInner> = serde_json::from_str(json).expect("parse");
        let list: Vec<StructuredLyrics> = parsed
            .response
            .lyrics_list
            .map(|b| b.structured_lyrics.into_iter().map(Into::into).collect())
            .unwrap_or_default();

        assert_eq!(list.len(), 1);
        assert!(!list[0].synced);
        assert_eq!(list[0].kind.as_deref(), Some("main"));
        assert_eq!(list[0].lines.len(), 2);
        assert!(list[0].lines.iter().all(|l| l.start_ms.is_none()));
        assert_eq!(list[0].lines[1].value, "You see the road");
    }

    #[test]
    fn empty_lyrics_list_is_empty() {
        let json = r#"{"subsonic-response":{"status":"ok","lyricsList":{}}}"#;
        let parsed: SubsonicEnvelope<LyricsListInner> = serde_json::from_str(json).expect("parse");
        let list: Vec<StructuredLyrics> = parsed
            .response
            .lyrics_list
            .map(|b| b.structured_lyrics.into_iter().map(Into::into).collect())
            .unwrap_or_default();
        assert!(list.is_empty());
    }
}
