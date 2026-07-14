//! Subsonic API client for the OpenSubsonic `indexBasedQueue` extension
//! (`savePlayQueueByIndex` / `getPlayQueueByIndex`).
//!
//! Backs the manual queue push/pull sync with the Navidrome server. The
//! index-based variant is used deliberately: the legacy `savePlayQueue`
//! identifies the current track by *id*, which restores to the wrong
//! instance when a queue holds the same song twice — the index variant keys
//! the playhead on position, matching nokkvi's own position-based queue
//! model. All requests POST a form body (never GET query strings), so a
//! multi-thousand-row queue can't die at a reverse proxy's URL-length limit.

use anyhow::Result;

use crate::{services::api::client::ApiClient, types::song::Song};

/// Inner payload of the Subsonic `getPlayQueueByIndex` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]). The field is
/// absent entirely when no queue was ever saved.
#[derive(Debug, serde::Deserialize)]
struct PlayQueueByIndexInner {
    #[serde(rename = "playQueueByIndex")]
    play_queue_by_index: Option<PlayQueueByIndex>,
}

/// A saved server-side play queue, as returned by `getPlayQueueByIndex`.
///
/// `entry` deserializes straight into [`Song`] (Subsonic `Child` objects —
/// same shape `getSimilarSongs2` returns). Navidrome silently drops ids no
/// longer in the library on read, so `current_index` can point past the
/// surviving entries — callers MUST clamp via [`clamp_pulled_index`] before
/// using it.
#[derive(Debug, serde::Deserialize)]
pub struct PlayQueueByIndex {
    /// Saved songs in stored (physical) order; empty when the queue is
    /// present-but-empty (e.g. after an empty save cleared it).
    #[serde(rename = "entry", default)]
    pub entry: Vec<Song>,
    /// 0-based index of the current track into `entry` as STORED — not
    /// re-mapped after the server drops missing ids; may be absent.
    #[serde(rename = "currentIndex")]
    pub current_index: Option<i32>,
    /// Playback offset into the current track, in milliseconds.
    #[serde(rename = "position", default)]
    pub position: i64,
}

/// Clamp a pulled `currentIndex` against the actually-returned entry count.
///
/// The single guard against server-dropped ids / out-of-range indices:
/// `set_queue` does NOT prune, so the caller must clamp before anchoring the
/// cursor. Absent-or-negative on a non-empty queue anchors to the start.
pub fn clamp_pulled_index(raw: Option<i32>, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match raw {
        Some(i) if i >= 0 => Some((i as usize).min(len - 1)),
        _ => Some(0),
    }
}

/// `currentIndex` value to send with a save.
///
/// A non-empty queue MUST carry a valid `currentIndex` — Navidrome
/// range-checks `0..len-1` and rejects a missing one with
/// `ErrorMissingParameter` — so a `None` cursor (reachable: songs added
/// without ever playing) coerces to 0. An empty queue sends none (the
/// validation is skipped server-side and the save clears the stored queue).
fn coerced_current_index(ids_len: usize, current_index: Option<usize>) -> Option<usize> {
    (ids_len > 0).then(|| current_index.unwrap_or(0))
}

#[derive(Clone)]
pub struct PlayQueueApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl PlayQueueApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Save the queue to the server via `savePlayQueueByIndex` (full
    /// replace — one queue per user, each save overwrites the previous).
    ///
    /// `ids` is the physical queue order; duplicates are legal and
    /// preserved. `position_ms` is the offset into the current track in
    /// milliseconds. Saving an empty `ids` list clears the stored queue.
    pub async fn save_play_queue_by_index(
        &self,
        ids: &[&str],
        current_index: Option<usize>,
        position_ms: i64,
    ) -> Result<()> {
        // Owned locals outlive the &str borrows below (the rating.rs shape).
        let pos_str = position_ms.to_string();
        let ci_str = coerced_current_index(ids.len(), current_index).map(|ci| ci.to_string());
        let mut params: Vec<(&str, &str)> = Vec::with_capacity(ids.len() + 2);
        for id in ids {
            params.push(("id", id));
        }
        if let Some(ci) = ci_str.as_deref() {
            params.push(("currentIndex", ci));
        }
        params.push(("position", &pos_str));
        crate::services::api::subsonic::subsonic_post_ok(
            &self.client.http_client(),
            &self.server_url,
            "savePlayQueueByIndex",
            &self.subsonic_credential,
            &params,
            "save play queue",
        )
        .await
    }

    /// Fetch the saved queue via `getPlayQueueByIndex`.
    ///
    /// Returns `None` when no queue was ever saved (the response field is
    /// absent); a present-but-empty queue returns `Some` with an empty
    /// `entry` list.
    ///
    /// Deliberately checks the HTTP + inner Subsonic status BEFORE parsing
    /// (unlike the lenient `subsonic_get_envelope` list pipeline): a
    /// `status:"failed"` envelope inside HTTP 200 also lacks
    /// `playQueueByIndex`, and without the check it would masquerade as the
    /// authoritative "no saved queue on server" instead of surfacing the
    /// real error (and a 401 would never route to session teardown).
    pub async fn get_play_queue_by_index(&self) -> Result<Option<PlayQueueByIndex>> {
        let response = crate::services::api::subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getPlayQueueByIndex",
            &self.subsonic_credential,
            &[],
        )
        .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        crate::services::api::subsonic::check_subsonic_response_status(
            status,
            &body,
            "get play queue",
        )?;
        let envelope: crate::services::api::subsonic::SubsonicEnvelope<PlayQueueByIndexInner> =
            crate::services::api::parse::parse_json_with_preview(&body, "get play queue")?;
        Ok(envelope.response.play_queue_by_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::subsonic::SubsonicEnvelope;

    #[test]
    fn clamp_pulled_index_edges() {
        assert_eq!(clamp_pulled_index(None, 0), None);
        assert_eq!(clamp_pulled_index(Some(2), 0), None);
        assert_eq!(clamp_pulled_index(Some(5), 3), Some(2));
        assert_eq!(clamp_pulled_index(None, 3), Some(0));
        assert_eq!(clamp_pulled_index(Some(-1), 3), Some(0));
        assert_eq!(clamp_pulled_index(Some(1), 3), Some(1));
        assert_eq!(clamp_pulled_index(Some(0), 1), Some(0));
    }

    #[test]
    fn save_coerces_none_cursor_on_non_empty_queue() {
        // Regression guard: a non-empty queue with no playhead must still
        // send currentIndex (Navidrome rejects a missing one), coerced to 0.
        assert_eq!(coerced_current_index(2, None), Some(0));
        assert_eq!(coerced_current_index(2, Some(1)), Some(1));
        // Empty queue sends none — the server-side validation is skipped.
        assert_eq!(coerced_current_index(0, None), None);
        assert_eq!(coerced_current_index(0, Some(3)), None);
    }

    #[test]
    fn deserialize_play_queue_with_duplicates() {
        // The same song twice must survive as two entries, with the
        // playhead as a POSITION (the whole point of the index variant).
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "playQueueByIndex": {
                    "entry": [
                        {"id": "a", "title": "x"},
                        {"id": "a", "title": "x"}
                    ],
                    "currentIndex": 1,
                    "position": 42000,
                    "username": "foogs",
                    "changedBy": "nokkvi"
                }
            }
        }"#;
        let parsed: SubsonicEnvelope<PlayQueueByIndexInner> =
            serde_json::from_str(json).expect("should parse play queue response");
        let pq = parsed.response.play_queue_by_index.unwrap();
        assert_eq!(pq.entry.len(), 2);
        assert_eq!(pq.entry[0].id, "a");
        assert_eq!(pq.entry[1].id, "a");
        assert_eq!(pq.current_index, Some(1));
        assert_eq!(pq.position, 42000);
    }

    #[test]
    fn deserialize_present_but_empty_queue() {
        // The REAL Navidrome empty shape: field present, only username set.
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "playQueueByIndex": {"username": "foogs"}
            }
        }"#;
        let parsed: SubsonicEnvelope<PlayQueueByIndexInner> =
            serde_json::from_str(json).expect("should parse empty play queue");
        let pq = parsed.response.play_queue_by_index.unwrap();
        assert!(pq.entry.is_empty());
        assert_eq!(pq.current_index, None);
        assert_eq!(pq.position, 0);
    }

    #[test]
    fn deserialize_absent_field_is_none() {
        // No queue ever saved: playQueueByIndex absent → None, no error.
        let json = r#"{"subsonic-response": {"status": "ok", "version": "1.16.1"}}"#;
        let parsed: SubsonicEnvelope<PlayQueueByIndexInner> =
            serde_json::from_str(json).expect("should parse response without playQueueByIndex");
        assert!(parsed.response.play_queue_by_index.is_none());
    }
}
