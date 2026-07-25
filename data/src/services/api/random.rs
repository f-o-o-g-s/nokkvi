//! Subsonic `getRandomSongs` client — the reseed-free random song source.
//!
//! Navidrome maps every "random" browse request onto `Sort: "random"` in the
//! shared `sqlRepository`, and `resetSeededRandom` there re-seeds the server's
//! per-`(table, user)` seeded-random ordering whenever such a query arrives with
//! `Offset == 0`. That covers the native `_sort=random` pages AND the Subsonic
//! `getAlbumList`/`getAlbumList2&type=random` endpoints (`filter.AlbumsByRandom`
//! is literally `Options{Sort: "random"}`). The effect: a one-off random draw
//! silently re-permutes an in-progress "Random"-sort pagination in the browse
//! views — already-scrolled rows reappear, others are skipped. Passing an
//! explicit `seed` does not help; that branch calls `SetSeed` on the same shared
//! key.
//!
//! `getRandomSongs` is the one random endpoint that sidesteps the seeded hasher:
//! `mediaFileRepository.GetRandom` picks rowids with a plain `ORDER BY random()`
//! and never reaches `newSelect` with options, so the shared seed is untouched.
//! Albums and artists have no such escape (albums' random endpoint reseeds;
//! artists have no random endpoint at all), so they draw a single row at a random
//! offset over a STABLE sort instead — see
//! [`crate::services::api::pagination::draw_random_row`].

use anyhow::Result;
use tracing::{debug, warn};

use crate::{
    services::api::{client::ApiClient, parse, subsonic},
    types::song::Song,
};

/// Inner payload of the Subsonic `getRandomSongs` envelope
/// ([`subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct RandomSongsResponseInner {
    #[serde(rename = "randomSongs")]
    random_songs: Option<RandomSongs>,
}

#[derive(Debug, serde::Deserialize)]
struct RandomSongs {
    /// Plain `Vec`, matching the `getSimilarSongs2` / `getTopSongs` parsers:
    /// Navidrome's `responses.Songs.Songs` is a Go slice serialized by
    /// `encoding/json`, so `song` is always a JSON array (and omitted entirely
    /// when empty) — the single-element-collapse quirk
    /// [`subsonic::deserialize_one_or_many`] absorbs elsewhere does not occur on
    /// this field.
    song: Option<Vec<Song>>,
}

/// Navidrome clamps `getRandomSongs` to 500 rows (`min(p.IntOr("size", 10), 500)`
/// in `server/subsonic/album_lists.go`). Asking for more silently truncates, so
/// the client clamps and logs rather than quietly under-delivering. `pub` so a
/// caller's draw-size constant can be `const`-asserted against it.
pub const MAX_RANDOM_SONGS: usize = 500;

#[derive(Clone)]
pub struct RandomApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl RandomApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch up to `size` songs in true random order, optionally restricted to
    /// one genre.
    ///
    /// * `genre` — the genre NAME. Navidrome's `filterByGenre` matches it against
    ///   the tag VALUE stored in each track's `tags` JSON; `Genre::id` from
    ///   `/api/genre` is a separate `tag.id` hash, so pass `name`, not `id`.
    ///   `None` / empty draws from the whole library.
    /// * `library_ids` — sent as repeatable `musicFolderId` params; Navidrome's
    ///   music folders ARE its libraries, so these are the same numeric ids the
    ///   native endpoints take as `library_id`. An empty slice omits the param
    ///   and leaves the server's own per-user library scoping in charge.
    ///
    /// Caveat vs the native `/api/song` shape: Subsonic's `Child` carries no
    /// `compilation` field (only albums expose `isCompilation`), so songs from
    /// this path always deserialize with `compilation: None`. The crossfade
    /// album-continuity policy's compilation escape therefore falls through to
    /// its "Various Artists" album-artist heuristic, which survives because
    /// `Song::album_artist` aliases Subsonic's `displayAlbumArtist`.
    pub async fn get_random_songs(
        &self,
        size: usize,
        genre: Option<&str>,
        library_ids: &[i32],
    ) -> Result<Vec<Song>> {
        let clamped = size.min(MAX_RANDOM_SONGS);
        if clamped < size {
            warn!(
                "getRandomSongs: {size} requested, clamped to the server's {MAX_RANDOM_SONGS}-row cap"
            );
        }

        // Owned strings live alongside `params` so the `&str` borrows inside it
        // outlive the request (same discipline as the native loaders).
        let size_str = clamped.to_string();
        let library_id_strings: Vec<String> = library_ids.iter().map(ToString::to_string).collect();
        let genre = genre.filter(|g| !g.is_empty());
        let params = Self::build_random_songs_params(&size_str, genre, &library_id_strings);

        // Deliberately checks the HTTP + inner Subsonic status BEFORE parsing
        // (unlike the lenient `subsonic_get_envelope` list pipeline, and for the
        // same reason `get_play_queue_by_index` does): Navidrome returns every
        // Subsonic error as HTTP 200 with a `status:"failed"` envelope, which
        // also lacks `randomSongs` — so without the check a stale credential or
        // an inaccessible `musicFolderId` would parse to `Ok(vec![])` and reach
        // the user as "this library has nothing to play".
        let response = subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getRandomSongs",
            &self.subsonic_credential,
            &params,
        )
        .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        subsonic::check_subsonic_response_status(status, &body, "get random songs")?;
        let envelope: subsonic::SubsonicEnvelope<RandomSongsResponseInner> =
            parse::parse_json_with_preview(&body, "getRandomSongs")?;

        let songs = envelope
            .response
            .random_songs
            .and_then(|s| s.song)
            .unwrap_or_default();

        debug!(
            "🎲 getRandomSongs: {} songs (size={clamped}, genre={genre:?})",
            songs.len()
        );

        Ok(songs)
    }

    /// Build the `size` / `genre` / repeatable `musicFolderId` params for a
    /// `getRandomSongs` request. Extracted (mirroring the native loaders'
    /// `build_*_params`) so the wire shape is pinned by tests.
    fn build_random_songs_params<'a>(
        size: &'a str,
        genre: Option<&'a str>,
        library_id_strings: &'a [String],
    ) -> Vec<(&'a str, &'a str)> {
        let mut params: Vec<(&'a str, &'a str)> = vec![("size", size)];
        if let Some(genre) = genre {
            params.push(("genre", genre));
        }
        for id in library_id_strings {
            params.push(("musicFolderId", id.as_str()));
        }
        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::subsonic::SubsonicEnvelope;

    /// The canonical body parses, including the `albumId` every artwork warm
    /// keys on and the `displayAlbumArtist` alias the crossfade policy needs.
    #[test]
    fn parses_a_random_songs_response() {
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "randomSongs": {
                    "song": [
                        {"id": "s1", "title": "One", "artist": "A", "albumId": "al1",
                         "displayAlbumArtist": "Various Artists"},
                        {"id": "s2", "title": "Two", "artist": "B", "albumId": "al2"}
                    ]
                }
            }
        }"#;
        let envelope: SubsonicEnvelope<RandomSongsResponseInner> =
            serde_json::from_str(json).expect("canonical body must parse");
        let songs = envelope
            .response
            .random_songs
            .and_then(|s| s.song)
            .expect("randomSongs.song present");
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].id, "s1");
        assert_eq!(
            songs[0].album_artist.as_deref(),
            Some("Various Artists"),
            "displayAlbumArtist must reach album_artist or the crossfade \
             compilation heuristic silently dies on this path"
        );
        assert_eq!(songs[1].album_id.as_deref(), Some("al2"));
    }

    /// An empty library omits the `song` key entirely (Go `omitempty` on a nil
    /// slice) — an empty draw, not a parse error.
    #[test]
    fn parses_an_empty_random_songs_response() {
        let json = r#"{"subsonic-response":{"status":"ok","randomSongs":{}}}"#;
        let envelope: SubsonicEnvelope<RandomSongsResponseInner> =
            serde_json::from_str(json).expect("empty body must parse");
        assert!(
            envelope
                .response
                .random_songs
                .expect("randomSongs present")
                .song
                .is_none()
        );
    }

    /// A `status:"failed"` envelope inside HTTP 200 — Navidrome's shape for EVERY
    /// Subsonic error — must surface as an error. It also lacks `randomSongs`, so
    /// skipping the status check would report a server failure as an empty
    /// library.
    #[test]
    fn failed_envelope_is_an_error_not_an_empty_draw() {
        let body = r#"{"subsonic-response":{"status":"failed","error":{"code":70,"message":"Library 3 not found or not accessible"}}}"#;
        let err = subsonic::check_subsonic_response_status(
            reqwest::StatusCode::OK,
            body,
            "get random songs",
        )
        .expect_err("a failed envelope must not be treated as success");
        let msg = format!("{err}");
        assert!(msg.contains("get random songs"), "missing label in: {msg}");
        assert!(msg.contains("not accessible"), "missing reason in: {msg}");

        // …and the body really does parse to an absent draw, which is what makes
        // the check load-bearing rather than belt-and-braces.
        let envelope: SubsonicEnvelope<RandomSongsResponseInner> =
            serde_json::from_str(body).expect("failed envelope still parses");
        assert!(envelope.response.random_songs.is_none());
    }

    /// Wire shape: `size` always, `genre` only when non-empty, one repeated
    /// `musicFolderId` per active library.
    #[test]
    fn build_random_songs_params_pins_the_wire_shape() {
        let libs = vec!["1".to_string(), "4".to_string()];

        let bare = RandomApiService::build_random_songs_params("100", None, &[]);
        assert_eq!(bare, vec![("size", "100")]);

        let scoped = RandomApiService::build_random_songs_params("100", Some("Rock"), &libs);
        assert_eq!(
            scoped,
            vec![
                ("size", "100"),
                ("genre", "Rock"),
                ("musicFolderId", "1"),
                ("musicFolderId", "4"),
            ]
        );
    }

    /// The clamp is silent-truncation protection: a draw larger than the server
    /// cap must be reduced client-side so the row subtitle cannot over-promise.
    #[test]
    fn draw_size_clamps_to_the_server_cap() {
        assert_eq!(MAX_RANDOM_SONGS.min(MAX_RANDOM_SONGS + 1), MAX_RANDOM_SONGS);
        assert_eq!(100_usize.min(MAX_RANDOM_SONGS), 100);
    }
}
