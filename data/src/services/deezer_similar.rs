//! Similar artists from Deezer, the second source behind the Artists view's
//! panel and the Similar Artists modal.
//!
//! Deezer's public catalogue API answers without a key or an account:
//! `GET /search/artist?q=<name>` finds the artist's Deezer id and
//! `GET /artist/<id>/related` lists the artists Deezer relates to them, from
//! its own listeners. Deezer's larger listener base reaches artists
//! ListenBrainz knows little about (small, new or non-English acts) and keeps
//! a famous artist's list closer to them. Deezer has no MusicBrainz ids, so
//! the artist is found by name: an exact name, ignoring case, and among the
//! artists sharing it the one with the most fans (Navidrome's Deezer agent
//! picks the same way). A name Deezer has no exact match for gets no list,
//! rather than a neighbour's.
//!
//! Deezer reports a failure as HTTP 200 with an `error` object (code 4 is its
//! rate limit), which reads as an error here. The fetch sends only the
//! artist's name; nothing names the server, the user or the library.

use std::{cmp::Reverse, sync::OnceLock, time::Duration};

use anyhow::{Context as _, Result, bail};

use crate::{
    backend::albums::{artwork_client_base, external_host_is_blocked},
    types::artist_recs::{ArtistRec, LibraryMatch, RecSource, same_artist_name},
};

const API_BASE: &str = "https://api.deezer.com";

/// Artists a name search reads: room for the several artists a common name
/// is shared by.
const SEARCH_LIMIT: &str = "25";

/// Related artists asked for. Deezer's list runs to about twenty; more than
/// the panel shows, so the blend has spares for names ListenBrainz also
/// lists.
const RELATED_LIMIT: &str = "20";

/// Each request, connect to body. Deezer answers in well under a second; a
/// stalled one is given up so the panel never waits on it.
const TIMEOUT: Duration = Duration::from_secs(10);

/// One artist row of a search or related answer. Deezer also sends links,
/// pictures and counts; only these are read.
#[derive(Debug, serde::Deserialize)]
struct ArtistWire {
    id: u64,
    name: String,
    #[serde(default)]
    nb_fan: u64,
}

/// Deezer's answer: a `data` list, or an `error` in its place.
#[derive(Debug, serde::Deserialize)]
struct Envelope {
    data: Option<Vec<ArtistWire>>,
    error: Option<ErrorWire>,
}

#[derive(Debug, serde::Deserialize)]
struct ErrorWire {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: i64,
}

/// A single, lazily-built, redirect-guarded client, as the ListenBrainz and
/// LRCLIB fetches have: a public host must not `302` the fetch onto a private
/// address.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        artwork_client_base()
            .redirect(crate::utils::redirects::public_hosts_only(
                "deezer",
                external_host_is_blocked,
            ))
            .build()
            .expect("Failed to build the Deezer HTTP client")
    })
}

/// The artists Deezer relates to the one named `name`, in Deezer's order,
/// none of them the artist itself, each with its library standing still
/// [`LibraryMatch::Unknown`]. A name Deezer has no artist by is an empty
/// list; a failed or stalled request, an error answer, or one it cannot read,
/// is an error.
pub async fn fetch_similar_artists(name: &str) -> Result<Vec<ArtistRec>> {
    // A static check of a constant URL: a sandbox that blocks the host gets
    // an error, not a quiet empty list.
    if external_host_is_blocked(API_BASE) {
        bail!("the Deezer API host is blocked");
    }
    let Some(id) = search_artist_from(API_BASE, name).await? else {
        return Ok(Vec::new());
    };
    related_artists_from(API_BASE, id, name).await
}

/// The Deezer id of the artist named `name`, searched at `base` (tests stand
/// in for Deezer): the exact name with the most fans, or `None`.
pub(crate) async fn search_artist_from(base: &str, name: &str) -> Result<Option<u64>> {
    let url = reqwest::Url::parse_with_params(
        &format!("{base}/search/artist"),
        [("q", name), ("limit", SEARCH_LIMIT)],
    )
    .context("building the Deezer artist search URL")?;
    let rows = get_artists(url, "artist search").await?;
    Ok(pick_seed(name, &rows))
}

/// The artists Deezer relates to artist `id` (named `seed_name`), asked at
/// `base` (tests stand in for Deezer).
pub(crate) async fn related_artists_from(
    base: &str,
    id: u64,
    seed_name: &str,
) -> Result<Vec<ArtistRec>> {
    let url = reqwest::Url::parse_with_params(
        &format!("{base}/artist/{id}/related"),
        [("limit", RELATED_LIMIT)],
    )
    .context("building the Deezer related-artists URL")?;
    let rows = get_artists(url, "related artists").await?;
    Ok(from_related(rows, seed_name))
}

/// GET `url` and read the artist rows out of Deezer's envelope; `what` names
/// the request in errors.
async fn get_artists(url: reqwest::Url, what: &str) -> Result<Vec<ArtistWire>> {
    let response = tokio::time::timeout(TIMEOUT, client().get(url).send())
        .await
        .with_context(|| format!("Deezer {what} request timed out"))?
        .map_err(|e| anyhow::anyhow!("Deezer {what} request failed: {}", e.without_url()))?;
    let status = response.status();
    if !status.is_success() {
        bail!("Deezer {what} answered HTTP {status}");
    }
    let body = tokio::time::timeout(TIMEOUT, response.text())
        .await
        .with_context(|| format!("Deezer {what} answer timed out"))?
        .with_context(|| format!("reading the Deezer {what} answer"))?;
    let envelope: Envelope =
        serde_json::from_str(&body).with_context(|| format!("parsing the Deezer {what} answer"))?;
    match envelope {
        Envelope {
            error: Some(error), ..
        } => bail!(
            "Deezer {what} answered error {} ({}): {}",
            error.code,
            error.kind,
            error.message
        ),
        Envelope {
            data: Some(rows), ..
        } => Ok(rows),
        Envelope { data: None, .. } => bail!("Deezer {what} answer has no data"),
    }
}

/// The search row that is the artist named `name`: an exact name, ignoring
/// case, and among several the one with the most fans (the first of equals).
fn pick_seed(name: &str, rows: &[ArtistWire]) -> Option<u64> {
    rows.iter()
        .filter(|row| same_artist_name(&row.name, name))
        .min_by_key(|row| Reverse(row.nb_fan))
        .map(|row| row.id)
}

/// Rows to recommendations in Deezer's order, dropping a nameless row and
/// the seed artist should Deezer list them among their own neighbours.
fn from_related(rows: Vec<ArtistWire>, seed_name: &str) -> Vec<ArtistRec> {
    rows.into_iter()
        .filter(|row| !row.name.trim().is_empty() && !same_artist_name(&row.name, seed_name))
        .map(|row| ArtistRec {
            source: RecSource::Deezer { id: row.id },
            name: row.name,
            comment: None,
            library: LibraryMatch::Unknown,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::subsonic::tests::one_shot_server;

    /// Deezer's related artists for Radiohead (id 399), recorded 2026-10-19
    /// with `limit=6`.
    const RELATED: &str = include_str!("../../testdata/deezer_related_artists_radiohead.json");
    /// Deezer's artist search for "Nirvana", recorded 2026-10-19 with
    /// `limit=5`: a near name first, then two artists named exactly Nirvana,
    /// ten million fans and ten.
    const SEARCH: &str = include_str!("../../testdata/deezer_search_artist_nirvana.json");

    #[tokio::test]
    async fn reads_a_recorded_related_answer_in_deezers_order() {
        let base = one_shot_server("200 OK", RELATED).await;
        let recs = related_artists_from(&base, 399, "Radiohead")
            .await
            .expect("a recorded answer parses");
        let names: Vec<&str> = recs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Jeff Buckley",
                "Blur",
                "The Smashing Pumpkins",
                "Thom Yorke",
                "Pixies",
                "Björk"
            ]
        );
        assert_eq!(recs[1].source, RecSource::Deezer { id: 743 });
        assert!(recs.iter().all(|r| r.comment.is_none()));
        assert!(recs.iter().all(|r| r.library == LibraryMatch::Unknown));
    }

    /// The near name ("Nirvana (UK)") is skipped and the exact name with the
    /// most fans wins over the one with ten.
    #[tokio::test]
    async fn the_search_picks_the_exact_name_with_the_most_fans() {
        let base = one_shot_server("200 OK", SEARCH).await;
        let id = search_artist_from(&base, "nirvana")
            .await
            .expect("a recorded answer parses");
        assert_eq!(id, Some(415));
    }

    /// A name Deezer has no exact artist for gets no list, rather than the
    /// nearest name's.
    #[tokio::test]
    async fn a_name_with_no_exact_match_finds_no_artist() {
        let base = one_shot_server("200 OK", SEARCH).await;
        let id = search_artist_from(&base, "Nirvana UK").await.expect("ok");
        assert_eq!(id, None);
        let base = one_shot_server("200 OK", r#"{"data":[],"total":0}"#).await;
        assert_eq!(search_artist_from(&base, "zzqx").await.expect("ok"), None);
    }

    /// Deezer reports its errors (here the rate limit) as HTTP 200 with an
    /// `error` object: an error, carrying what Deezer said.
    #[tokio::test]
    async fn an_error_object_is_an_error() {
        let base = one_shot_server(
            "200 OK",
            r#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#,
        )
        .await;
        let err = related_artists_from(&base, 399, "Radiohead")
            .await
            .expect_err("an error object is an error");
        let text = format!("{err:#}");
        assert!(text.contains("error 4"), "got: {text}");
        assert!(text.contains("Quota limit exceeded"), "got: {text}");
    }

    #[tokio::test]
    async fn a_failed_or_unreadable_answer_is_an_error() {
        let base = one_shot_server("503 Service Unavailable", "").await;
        assert!(related_artists_from(&base, 399, "Radiohead").await.is_err());
        let base = one_shot_server("200 OK", "<html>not json</html>").await;
        assert!(search_artist_from(&base, "Radiohead").await.is_err());
    }

    fn wire(id: u64, name: &str, nb_fan: u64) -> ArtistWire {
        ArtistWire {
            id,
            name: name.into(),
            nb_fan,
        }
    }

    #[test]
    fn equal_fans_keep_the_first_and_case_is_ignored() {
        let rows = [wire(1, "LOW", 5), wire(2, "Low", 5), wire(3, "Lower", 99)];
        assert_eq!(pick_seed("low", &rows), Some(1));
    }

    #[test]
    fn the_seed_artist_and_nameless_rows_are_dropped() {
        let recs = from_related(
            vec![wire(1, "Blur", 1), wire(2, "radiohead", 1), wire(3, " ", 1)],
            "Radiohead",
        );
        let names: Vec<&str> = recs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Blur"]);
    }
}
