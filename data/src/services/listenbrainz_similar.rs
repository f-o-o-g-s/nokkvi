//! Similar artists from ListenBrainz, for the Artists view's panel.
//!
//! ListenBrainz works out which artists its listeners play within the same
//! listening sessions and publishes the result through its labs API, keyed on
//! MusicBrainz artist ids: `GET similar-artists/json?artist_mbids=<id>&
//! algorithm=<name>`, up to a hundred artists with a similarity score, no
//! account or token needed. The algorithm name picks one of the datasets
//! ListenBrainz keeps; [`ALGORITHM`] is the one listenbrainz.org's own artist
//! pages read. The index of the API is <https://labs.api.listenbrainz.org/>.
//!
//! The fetch sends only the artist's MusicBrainz id; nothing names the
//! server, the user or the library.

use std::{sync::OnceLock, time::Duration};

use anyhow::{Context as _, Result, bail};

use crate::{
    backend::albums::{artwork_client_base, external_host_is_blocked},
    types::artist_recs::{ArtistRec, LibraryMatch},
};

const LABS_URL: &str = "https://labs.api.listenbrainz.org/similar-artists/json";

/// The similar-artists dataset listenbrainz.org's artist pages read (sessions
/// over the whole listening history, 300 s apart, a hundred artists, filtered
/// of the trivially popular). ListenBrainz keeps its older datasets alongside,
/// so a retired name answers with a 400 that lists the current ones; the
/// error carries that body for the log.
const ALGORITHM: &str =
    "session_based_days_7500_session_300_contribution_5_threshold_10_limit_100_filter_True_skip_30";

/// The whole fetch, connect to body. The labs API answers in about a second;
/// a stalled one is given up so the panel never waits on it.
const TIMEOUT: Duration = Duration::from_secs(10);

/// How much of an error body the error message keeps: enough for the labs
/// API's one-line validation errors.
const ERROR_BODY_CHARS: usize = 300;

/// One row of the labs answer. ListenBrainz also sends `type`, `gender` and
/// the `reference_mbid` the list was asked for; none is read.
#[derive(Debug, serde::Deserialize)]
struct SimilarArtistWire {
    artist_mbid: String,
    name: String,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    score: u32,
}

/// A single, lazily-built, redirect-guarded client, as the LRCLIB fetch has:
/// a public host must not `302` the fetch onto a private address.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        artwork_client_base()
            .redirect(crate::utils::redirects::public_hosts_only(
                "listenbrainz",
                external_host_is_blocked,
            ))
            .build()
            .expect("Failed to build the ListenBrainz HTTP client")
    })
}

/// The artists ListenBrainz finds similar to the one with MusicBrainz id
/// `mbid`, most similar first, none of them the artist itself, each with its
/// library standing still [`LibraryMatch::Unknown`]. An artist ListenBrainz
/// has no data for is an empty list; a failed or stalled request, or an answer
/// it cannot read, is an error.
pub async fn fetch_similar_artists(mbid: &str) -> Result<Vec<ArtistRec>> {
    // A static check of a constant URL: a sandbox that blocks the host gets
    // an error, not a quiet empty list.
    if external_host_is_blocked(LABS_URL) {
        bail!("the ListenBrainz labs host is blocked");
    }
    fetch_similar_artists_from(LABS_URL, mbid).await
}

/// [`fetch_similar_artists`] against `endpoint`, so tests can stand in for
/// the labs API.
pub(crate) async fn fetch_similar_artists_from(
    endpoint: &str,
    mbid: &str,
) -> Result<Vec<ArtistRec>> {
    let url = reqwest::Url::parse_with_params(
        endpoint,
        [("artist_mbids", mbid), ("algorithm", ALGORITHM)],
    )
    .context("building the ListenBrainz similar-artists URL")?;
    let response = tokio::time::timeout(TIMEOUT, client().get(url).send())
        .await
        .context("ListenBrainz similar-artists request timed out")?
        .map_err(|e| {
            anyhow::anyhow!(
                "ListenBrainz similar-artists request failed: {}",
                e.without_url()
            )
        })?;
    let status = response.status();
    let body = tokio::time::timeout(TIMEOUT, response.text())
        .await
        .context("ListenBrainz similar-artists answer timed out")?
        .context("reading the ListenBrainz similar-artists answer")?;
    if !status.is_success() {
        let snippet: String = body.chars().take(ERROR_BODY_CHARS).collect();
        bail!("ListenBrainz similar-artists answered HTTP {status}: {snippet}");
    }
    let rows: Vec<SimilarArtistWire> =
        serde_json::from_str(&body).context("parsing the ListenBrainz similar-artists answer")?;
    Ok(from_wire(rows, mbid))
}

/// Rows to recommendations: the artist asked about is dropped should
/// ListenBrainz list it among its own neighbours, an empty comment reads as
/// none, and the order is by score, highest first (stable, so ties keep
/// ListenBrainz's order).
fn from_wire(rows: Vec<SimilarArtistWire>, seed_mbid: &str) -> Vec<ArtistRec> {
    let mut recs: Vec<ArtistRec> = rows
        .into_iter()
        .filter(|row| {
            !row.artist_mbid.eq_ignore_ascii_case(seed_mbid) && !row.name.trim().is_empty()
        })
        .map(|row| ArtistRec {
            mbid: row.artist_mbid,
            name: row.name,
            comment: row.comment.filter(|c| !c.trim().is_empty()),
            score: row.score,
            library: LibraryMatch::Unknown,
        })
        .collect();
    recs.sort_by_key(|rec| std::cmp::Reverse(rec.score));
    recs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::subsonic::tests::one_shot_server;

    /// The labs answer for Radiohead, recorded 2026-10-16 and cut to six rows.
    const RECORDED: &str =
        include_str!("../../testdata/listenbrainz_similar_artists_radiohead.json");
    const RADIOHEAD: &str = "a74b1b7f-71a5-4011-9441-d0b5e4122711";

    #[tokio::test]
    async fn reads_a_recorded_labs_answer_most_similar_first() {
        let base = one_shot_server("200 OK", RECORDED).await;
        let recs = fetch_similar_artists_from(&base, RADIOHEAD)
            .await
            .expect("a recorded answer parses");
        let names: Vec<&str> = recs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Nirvana",
                "Red Hot Chili Peppers",
                "Muse",
                "Coldplay",
                "The Smashing Pumpkins",
                "The Beatles"
            ]
        );
        assert_eq!(recs[0].mbid, "5b11f4ce-a62d-471e-81fc-a69a8278c7da");
        assert_eq!(
            recs[0].comment.as_deref(),
            Some("1980s–1990s US grunge band")
        );
        assert_eq!(recs[1].comment, None, "an empty comment reads as none");
        assert!(recs.windows(2).all(|w| w[0].score >= w[1].score));
        assert!(recs.iter().all(|r| r.library == LibraryMatch::Unknown));
    }

    /// An artist ListenBrainz has no data for answers `[]`: an empty list,
    /// not an error, so the panel shows nothing and asks no more.
    #[tokio::test]
    async fn an_unknown_artist_is_an_empty_list() {
        let base = one_shot_server("200 OK", "[]").await;
        let recs = fetch_similar_artists_from(&base, RADIOHEAD)
            .await
            .expect("ok");
        assert!(recs.is_empty());
    }

    /// A retired algorithm name answers 400 with the current names in the
    /// body; the error keeps that body so the log says what to update.
    #[tokio::test]
    async fn a_failed_request_is_an_error_carrying_the_body() {
        let base = one_shot_server(
            "400 Bad Request",
            "<p>1 validation error: algorithm value is not a valid enumeration member</p>",
        )
        .await;
        let err = fetch_similar_artists_from(&base, RADIOHEAD)
            .await
            .expect_err("a 400 is an error");
        let text = format!("{err:#}");
        assert!(text.contains("HTTP 400"), "got: {text}");
        assert!(text.contains("valid enumeration member"), "got: {text}");
    }

    #[tokio::test]
    async fn an_unreadable_answer_is_an_error() {
        let base = one_shot_server("200 OK", "<html>not json</html>").await;
        assert!(fetch_similar_artists_from(&base, RADIOHEAD).await.is_err());
    }

    fn wire(mbid: &str, name: &str, score: u32) -> SimilarArtistWire {
        SimilarArtistWire {
            artist_mbid: mbid.into(),
            name: name.into(),
            comment: None,
            score,
        }
    }

    #[test]
    fn the_seed_artist_and_nameless_rows_are_dropped_and_ties_keep_their_order() {
        let recs = from_wire(
            vec![
                wire("b", "Second", 5),
                wire(RADIOHEAD, "Radiohead", 100),
                wire("c", "   ", 50),
                wire("a", "First", 5),
                wire("d", "Top", 9),
            ],
            &RADIOHEAD.to_uppercase(),
        );
        let names: Vec<&str> = recs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Top", "Second", "First"]);
    }
}
