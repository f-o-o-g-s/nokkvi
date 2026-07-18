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
    /// Whether the async OpenSubsonic extensions probe has RESPONDED at all.
    /// `songlyrics_ext == false` is only authoritative once this is true —
    /// before the probe lands the server channel is skipped-but-unknown, so a
    /// miss resolved then must not be negative-cached (the miss-cache
    /// completeness gate consumes this; `resolve_from` itself does not).
    pub ext_probe_landed: bool,
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
    content.push_str(&format!("[ar:{}]\n", header_safe(&song.artist)));
    content.push_str(&format!("[ti:{}]\n", header_safe(&song.title)));
    if !song.album.is_empty() {
        content.push_str(&format!("[al:{}]\n", header_safe(&song.album)));
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

/// Make a tag value safe to interpolate into a `[key:value]` header. Newlines
/// always collapse to a space. Brackets are kept VERBATIM when balanced —
/// `next_tag`'s depth counter round-trips nested brackets, so a title like
/// `Song [Live]` re-parses exactly and keeps its Tier-1 identity (mangling it
/// desynced the cached header from the song on BOTH match tiers: the query
/// side strips the `[Live]` qualifier, a de-bracketed header cannot). Only an
/// UNBALANCED sequence — the case that actually breaks parsing — degrades,
/// to parentheses, which the qualifier-stripper treats identically.
fn header_safe(value: &str) -> String {
    let no_newlines: String = value
        .chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect();

    let mut depth: i32 = 0;
    let mut balanced = true;
    for c in no_newlines.chars() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth < 0 {
                    balanced = false;
                    break;
                }
            }
            _ => {}
        }
    }
    if balanced && depth == 0 {
        return no_newlines.trim().to_string();
    }
    no_newlines
        .chars()
        .map(|c| match c {
            '[' => '(',
            ']' => ')',
            other => other,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Reduce a string to a filesystem-safe stem, then append a stable hash of the
/// FULL key. Two songs whose readable stems collide (punctuation-only
/// differences that both map to `_`, or content beyond the cap) still get
/// distinct filenames — without the hash they silently overwrote each other's
/// cached lyrics. The hash is deterministic, so re-caching the same song reuses
/// its file rather than accumulating duplicates.
fn sanitize_filename(s: &str) -> String {
    // Cap the stem by BYTES, not chars: 80 CJK chars are ~240 bytes, and with
    // the hash suffix + ".lrc" that overflows Linux's 255-byte NAME_MAX —
    // tokio::fs::write then fails silently (debug-logged) and the cache never
    // persists. 80 bytes + 21-byte suffix stays comfortably inside.
    let mut stem = String::new();
    for c in s.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }) {
        if stem.len() + c.len_utf8() > 80 {
            break;
        }
        stem.push(c);
    }
    // FNV-1a over the full key — a stable, collision-resistant suffix.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{stem}_{hash:016x}")
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
        ext_probe_landed: true,
        fetch_online: true,
    };

    #[test]
    fn header_safe_roundtrips_balanced_and_degrades_unbalanced() {
        // BALANCED brackets round-trip verbatim — next_tag's depth counter
        // handles nesting, and preserving them keeps the cached header on the
        // song's exact Tier-1 identity (a qualifier like "[Live]" must remain
        // strippable on both sides of the match).
        assert_eq!(header_safe("Song [Live]"), "Song [Live]");
        let content = format!("[ti:{}]\n[00:01.00]x", header_safe("Song [Live]"));
        assert_eq!(
            crate::types::lyrics::read_metadata(&content)
                .title
                .as_deref(),
            Some("Song [Live]")
        );

        // UNBALANCED brackets (the case that actually breaks parsing) degrade
        // to parentheses; the header still parses, and Tier-2's qualifier
        // strip + reduce treat parens exactly like brackets.
        assert_eq!(header_safe("Song [Live"), "Song (Live");
        let content = format!("[ti:{}]\n[00:01.00]x", header_safe("Song [Live"));
        assert_eq!(
            crate::types::lyrics::read_metadata(&content)
                .title
                .as_deref(),
            Some("Song (Live")
        );
        assert_eq!(header_safe("Song] X"), "Song) X");

        // Newlines always collapse to spaces.
        assert_eq!(header_safe("A\r\nB"), "A  B");
    }

    #[test]
    fn sanitize_filename_distinguishes_colliding_stems() {
        // Two songs whose readable stems collapse identically must not share a
        // cache filename (which silently overwrote the first file's lyrics).
        let a = sanitize_filename("AC/DC-T.N.T.-High Voltage");
        let b = sanitize_filename("AC DC-T N T-High Voltage");
        assert_ne!(a, b);
        // Deterministic: the same key always maps to the same file.
        assert_eq!(a, sanitize_filename("AC/DC-T.N.T.-High Voltage"));
    }

    #[test]
    fn sanitize_filename_stays_under_name_max_for_multibyte() {
        // 80 CJK chars are ~240 bytes — the stem must cap by BYTES so the
        // final name (stem + "_<16 hex>.lrc") stays inside Linux's 255-byte
        // NAME_MAX; otherwise the cache write fails silently for CJK tags.
        let key = "東".repeat(120);
        let name = format!("{}.lrc", sanitize_filename(&key));
        assert!(name.len() <= 255, "filename {} bytes", name.len());
        assert!(name.len() >= 80, "stem should still carry real content");
    }

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
            ext_probe_landed: true,
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
            ext_probe_landed: true,
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
