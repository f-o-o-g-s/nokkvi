//! Lyrics source helpers: the direct LRCLIB fetch, the disk cache writer, and
//! the pure store->server->LRCLIB precedence helper. The `resolve_lyrics` chain
//! that wires these to the real channels lives on `AppService`.

use std::{sync::OnceLock, time::Duration};

use crate::{
    backend::albums::{artwork_client_base, external_host_is_blocked},
    types::{lyrics::LrcDocument, song::Song},
};

const LRCLIB_URL: &str = "https://lrclib.net/api/get";
const LRCLIB_TIMEOUT: Duration = Duration::from_secs(5);

/// Options that gate the resolve chain's optional channels.
#[derive(Debug, Clone, Copy)]
pub struct ResolveOpts {
    /// The server advertises the `songLyrics` extension (gates getLyricsBySongId).
    pub songlyrics_ext: bool,
    /// The user allows the direct third-party LRCLIB fetch.
    pub fetch_online: bool,
}

/// A single, lazily-built, redirect-guarded client for the LRCLIB fetch —
/// mirrors `AlbumsService`'s external-image client (per-hop SSRF re-check) so a
/// public host can't `302` the fetch to a private address.
fn lrclib_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        artwork_client_base()
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("lrclib: too many redirects")
                } else if external_host_is_blocked(attempt.url().as_str()) {
                    attempt.error("lrclib: redirect to a private/loopback host")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .expect("Failed to build LRCLIB HTTP client")
    })
}

#[derive(serde::Deserialize)]
struct LrclibResponse {
    #[serde(default)]
    instrumental: bool,
    #[serde(rename = "syncedLyrics", default)]
    synced_lyrics: Option<String>,
}

/// Fetch synced lyrics from LRCLIB's **exact** `/api/get` (never the fuzzy
/// `/api/search`), so a hit is never-wrong by construction. Returns the parsed
/// document **and** the raw `syncedLyrics` text (already valid LRC) so the cache
/// can persist it verbatim — no serializer needed. `None` on miss, instrumental,
/// timeout, or an unsynced result.
pub async fn fetch_lrclib(
    artist: &str,
    title: &str,
    album: &str,
    duration_secs: u32,
) -> Option<(LrcDocument, String)> {
    if external_host_is_blocked(LRCLIB_URL) {
        return None;
    }

    let duration = duration_secs.to_string();
    let params = [
        ("artist_name", artist),
        ("track_name", title),
        ("album_name", album),
        ("duration", duration.as_str()),
    ];
    let mut full_url = String::from(LRCLIB_URL);
    for (i, (key, value)) in params.iter().enumerate() {
        full_url.push(if i == 0 { '?' } else { '&' });
        full_url.push_str(key);
        full_url.push('=');
        full_url
            .push_str(&url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>());
    }

    let request = lrclib_client().get(&full_url);
    let response = match tokio::time::timeout(LRCLIB_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => {
            tracing::debug!(error = %e.without_url(), "lrclib fetch failed");
            return None;
        }
        Err(_) => {
            tracing::debug!("lrclib fetch timed out");
            return None;
        }
    };

    // A 404 is the normal "no match" answer — not an error worth logging.
    if !response.status().is_success() {
        return None;
    }

    let body: LrclibResponse = match response.json().await {
        Ok(body) => body,
        Err(e) => {
            tracing::debug!(error = %e.without_url(), "lrclib response parse failed");
            return None;
        }
    };

    if body.instrumental {
        return None;
    }
    let synced = body.synced_lyrics?;
    let doc = crate::types::lyrics::parse(&synced);
    doc.synced.then_some((doc, synced))
}

/// Persist a fetched LRCLIB result into the store so the next play resolves it
/// offline via the store channel. LRCLIB's `syncedLyrics` is header-less, and
/// the index matches on `[ar:]/[ti:]/[al:]` headers (not paths) — so synthesize
/// authoritative headers from the Song's own tags (which drove the exact match)
/// before the raw text. Best-effort: a write failure is logged, not surfaced.
pub async fn cache_to_store(raw_synced: &str, song: &Song) {
    let Ok(dir) = crate::utils::paths::get_lyrics_dir() else {
        return;
    };
    let cache_dir = dir.join(".cache");
    if tokio::fs::create_dir_all(&cache_dir).await.is_err() {
        return;
    }

    let mut content = String::with_capacity(raw_synced.len() + 128);
    content.push_str(&format!("[ar:{}]\n", song.artist));
    content.push_str(&format!("[ti:{}]\n", song.title));
    if !song.album.is_empty() {
        content.push_str(&format!("[al:{}]\n", song.album));
    }
    content.push_str(&format!(
        "[length:{:02}:{:02}]\n",
        song.duration / 60,
        song.duration % 60
    ));
    content.push_str(raw_synced);

    let name = sanitize_filename(&format!("{}-{}-{}", song.artist, song.title, song.album));
    let path = cache_dir.join(format!("{name}.lrc"));
    if let Err(e) = tokio::fs::write(&path, content).await {
        tracing::debug!(error = %e, "lrclib cache write failed");
    }
}

/// Reduce a string to a filesystem-safe stem (alphanumerics kept, everything
/// else collapsed to `_`, length-capped).
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(120)
        .collect()
}

/// Pure precedence helper — the "Feishin or better" ordering, testable without a
/// live `AppService`: local store, then the server (`songLyrics` ext), then the
/// direct LRCLIB fetch. Probes are lazy so a store hit never touches the network.
pub async fn resolve_from<StoreFut, ApiFut, LrclibFut>(
    store: impl FnOnce() -> StoreFut,
    api: impl FnOnce() -> ApiFut,
    lrclib: impl FnOnce() -> LrclibFut,
    opts: ResolveOpts,
) -> Option<LrcDocument>
where
    StoreFut: std::future::Future<Output = Option<LrcDocument>>,
    ApiFut: std::future::Future<Output = Option<LrcDocument>>,
    LrclibFut: std::future::Future<Output = Option<LrcDocument>>,
{
    if let Some(doc) = store().await {
        return Some(doc);
    }
    if opts.songlyrics_ext
        && let Some(doc) = api().await
    {
        return Some(doc);
    }
    if opts.fetch_online
        && let Some(doc) = lrclib().await
    {
        return Some(doc);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::lyrics::{LrcDocument, LrcLine};

    fn doc(tag: &str) -> LrcDocument {
        LrcDocument {
            lines: vec![LrcLine {
                time_ms: 0,
                text: tag.into(),
                words: vec![],
            }],
            synced: true,
        }
    }

    const BOTH_ON: ResolveOpts = ResolveOpts {
        songlyrics_ext: true,
        fetch_online: true,
    };

    #[tokio::test]
    #[ignore]
    async fn live_lrclib_fetch() {
        // Real network: a widely-available track should return synced lyrics.
        let hit = fetch_lrclib("Radiohead", "Creep", "Pablo Honey", 238).await;
        match hit {
            Some((doc, raw)) => {
                eprintln!(
                    "lrclib hit: {} lines, {} raw bytes",
                    doc.lines.len(),
                    raw.len()
                );
                assert!(doc.synced && !doc.lines.is_empty());
                assert!(raw.contains('['));
            }
            None => eprintln!("lrclib returned no match (acceptable if offline / not in db)"),
        }
    }

    #[tokio::test]
    async fn store_hit_wins() {
        let result = resolve_from(
            || async { Some(doc("store")) },
            || async { Some(doc("api")) },
            || async { Some(doc("lrclib")) },
            BOTH_ON,
        )
        .await;
        assert_eq!(result.unwrap().lines[0].text, "store");
    }

    #[tokio::test]
    async fn api_wins_when_store_empty() {
        let result = resolve_from(
            || async { None },
            || async { Some(doc("api")) },
            || async { Some(doc("lrclib")) },
            BOTH_ON,
        )
        .await;
        assert_eq!(result.unwrap().lines[0].text, "api");
    }

    #[tokio::test]
    async fn lrclib_wins_when_store_and_api_empty() {
        let result = resolve_from(
            || async { None },
            || async { None },
            || async { Some(doc("lrclib")) },
            BOTH_ON,
        )
        .await;
        assert_eq!(result.unwrap().lines[0].text, "lrclib");
    }

    #[tokio::test]
    async fn api_skipped_without_ext() {
        let opts = ResolveOpts {
            songlyrics_ext: false,
            fetch_online: true,
        };
        let result = resolve_from(
            || async { None },
            || async { panic!("api must not run without the songLyrics ext") },
            || async { Some(doc("lrclib")) },
            opts,
        )
        .await;
        assert_eq!(result.unwrap().lines[0].text, "lrclib");
    }

    #[tokio::test]
    async fn lrclib_skipped_without_fetch_online() {
        let opts = ResolveOpts {
            songlyrics_ext: true,
            fetch_online: false,
        };
        let result = resolve_from(
            || async { None },
            || async { None },
            || async { panic!("lrclib must not run when fetch_online is off") },
            opts,
        )
        .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn cache_roundtrips_through_index() {
        // A cached LRCLIB result must be re-indexable by the song's tags.
        let tmp = tempfile::tempdir().expect("tempdir");
        // Point the cache writer at a temp store via a direct write mirroring
        // cache_to_store's header synthesis, then build_index over it.
        let cache_dir = tmp.path().join(".cache");
        std::fs::create_dir_all(&cache_dir).expect("mkdir");
        let song = Song {
            id: "s1".into(),
            title: "Myth".into(),
            artist: "Beach House".into(),
            album: "Bloom".into(),
            duration: 275,
            ..Default::default()
        };
        let raw = "[00:45.47]Drifting in and out";
        let mut content = String::new();
        content.push_str(&format!(
            "[ar:{}]\n[ti:{}]\n[al:{}]\n",
            song.artist, song.title, song.album
        ));
        content.push_str(&format!(
            "[length:{:02}:{:02}]\n",
            song.duration / 60,
            song.duration % 60
        ));
        content.push_str(raw);
        std::fs::write(
            cache_dir.join(format!(
                "{}.lrc",
                sanitize_filename("Beach House-Myth-Bloom")
            )),
            content,
        )
        .expect("write");

        let index = crate::types::lyrics::build_index(tmp.path().to_path_buf()).await;
        assert!(
            index
                .find(&song.artist, &song.title, Some(&song.album), None)
                .is_some(),
            "synthetic headers must make the cache tag-matchable"
        );
    }
}
