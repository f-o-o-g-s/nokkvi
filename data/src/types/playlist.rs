//! Playlist model from Navidrome API

use serde::{Deserialize, Serialize};

/// Playlist model from Navidrome API
/// Data from Native API (/api/playlist)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "comment", default)]
    pub comment: String,
    #[serde(rename = "duration", default)]
    pub duration: f32,
    #[serde(rename = "size", default)]
    pub size: i64,
    #[serde(rename = "songCount", default)]
    pub song_count: u32,
    #[serde(rename = "ownerName", default)]
    pub owner_name: String,
    #[serde(rename = "ownerId", default)]
    pub owner_id: String,
    #[serde(rename = "public", default)]
    pub public: bool,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
    /// Reference to a user-uploaded custom cover image, set by
    /// `POST /api/playlist/:id/image` and cleared by the matching DELETE.
    /// Navidrome always emits the key (`""` when none set); pre-feature
    /// servers omit it entirely (→ `None`). Gate on [`Self::custom_image`],
    /// never on `is_some()` — the empty-string form must not count.
    #[serde(rename = "uploadedImage", default)]
    pub uploaded_image: Option<String>,
    /// External image URL (M3U `#EXTIMG` import / plugin-managed). Emitted
    /// with `omitempty`, so it is absent when unset. Parsed for completeness
    /// but NOT treated as "has custom art": the image DELETE endpoint does
    /// not clear it, so keying Set/Reset on it would break reset symmetry
    /// (nokkvi could never remove it). v1 custom-artwork gating keys on
    /// [`Self::custom_image`] only.
    #[serde(rename = "externalImageUrl", default)]
    pub external_image_url: Option<String>,
    /// Raw smart-playlist criteria. Present ⇔ smart playlist. Kept as raw
    /// JSON — the typed view lives in `smart_criteria.rs`; this field is the
    /// byte-faithful substrate for lossless round-trip. Gate on
    /// [`Self::is_smart`], never on `is_some()` — a `"rules": null` emission
    /// must not count as smart.
    #[serde(rename = "rules", default)]
    pub rules: Option<serde_json::Value>,
    /// When the server last evaluated this smart playlist's rules
    /// (owner-only lazy re-evaluation — see the §3.5 server-contract table
    /// in the smart-playlist plan). Absent on regular playlists and on
    /// never-evaluated smart playlists.
    #[serde(rename = "evaluatedAt", default)]
    pub evaluated_at: Option<String>,
    /// File-backed (.nsp/.m3u) playlist path; non-empty ⇒ a scanner-synced
    /// file exists on the server.
    #[serde(rename = "path", default)]
    pub path: String,
    /// Whether the file at `path` re-syncs its rules over API edits on every
    /// scan. `PUT {"sync": false}` detaches (only meaningful when
    /// [`Self::is_file_backed`]).
    #[serde(rename = "sync", default)]
    pub sync: bool,
}

/// The reserved comment prefix that marks a nokkvi draft workspace playlist
/// (the smart-playlist preview engine's scratch object). The full marker
/// grammar is `nokkvi-draft/<version> pid=<u32> ts=<u64>` — see
/// [`DraftMarker::parse`]. The two comment-editing inputs strip a typed
/// leading `nokkvi-draft/` so a well-formed marker can never be entered by
/// hand.
pub const DRAFT_MARKER_PREFIX: &str = "nokkvi-draft/";

/// A strict full parse of a draft-workspace marker comment.
///
/// This is the SINGLE symbol the central list filter
/// (`filter_draft_rows`), the startup orphan sweep, and their tests all
/// share. Strictness is load-bearing: a user-typed comment that merely
/// *begins* with `nokkvi-draft/` must be neither filtered from any surface
/// nor sweep-eligible — a bare prefix test would make a real playlist
/// vanish from every projection at once (Playlists view, Harbour shelves,
/// whole-library search, Trawl seeds, all pickers) and mark it for
/// deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftMarker {
    /// Marker grammar version (`1` today).
    pub version: u32,
    /// The nokkvi process that minted the marker — lets the orphan sweep
    /// skip drafts owned by a live same-box parallel session.
    pub pid: u32,
    /// Unix seconds at the last draft write. Every draft write mints a
    /// fresh `ts`, so an actively-previewing session never ages out.
    pub ts: u64,
}

impl DraftMarker {
    /// Strict full parse: the entire comment must be exactly
    /// `nokkvi-draft/<version> pid=<u32> ts=<u64>` — version segment and
    /// both fields present, well-formed, in order, with single-space
    /// separators and nothing before or after. Anything else → `None`.
    pub fn parse(comment: &str) -> Option<Self> {
        let rest = comment.strip_prefix(DRAFT_MARKER_PREFIX)?;
        let mut parts = rest.split(' ');
        let version: u32 = parts.next()?.parse().ok()?;
        let pid: u32 = parts.next()?.strip_prefix("pid=")?.parse().ok()?;
        let ts: u64 = parts.next()?.strip_prefix("ts=")?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self { version, pid, ts })
    }

    /// Mint the canonical marker comment for this process. Round-trips
    /// through [`Self::parse`] by construction (test-pinned).
    pub fn format(version: u32, pid: u32, ts: u64) -> String {
        format!("{DRAFT_MARKER_PREFIX}{version} pid={pid} ts={ts}")
    }
}

impl Playlist {
    /// Get display name for the playlist
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Whether this is a server-side smart playlist (criteria-driven).
    /// Keys on a real `rules` object — a `"rules": null` emission does not
    /// count (see the field doc).
    pub fn is_smart(&self) -> bool {
        self.rules.as_ref().is_some_and(|r| !r.is_null())
    }

    /// Whether a scanner-synced file (.nsp/.m3u) backs this playlist on the
    /// server. File-backed playlists resurrect after an API delete on the
    /// next scan, and (while [`Self::sync`] is on) their file's rules
    /// overwrite API rule edits every scan.
    pub fn is_file_backed(&self) -> bool {
        !self.path.is_empty()
    }

    /// Whether this row is a nokkvi draft workspace playlist (strict marker
    /// parse of the comment — see [`DraftMarker::parse`]).
    pub fn is_draft(&self) -> bool {
        DraftMarker::parse(&self.comment).is_some()
    }

    /// Get song count
    pub fn get_song_count(&self) -> u32 {
        self.song_count
    }

    /// The user-uploaded custom cover reference, or `None` when this playlist
    /// has no uploaded image. Collapses the `absent → None` and
    /// `present-but-empty → Some("")` cases Navidrome emits (see
    /// [`Self::uploaded_image`]) into a single "is there a custom cover?"
    /// check — mirrors `RadioStation::logo_cover_art`.
    pub fn custom_image(&self) -> Option<&str> {
        self.uploaded_image.as_deref().filter(|s| !s.is_empty())
    }
}

impl std::fmt::Display for Playlist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({} songs)", self.name, self.song_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the `radio_station.rs` test trio: present / empty-string /
    /// absent forms of the wire field, plus the collapsing accessor.
    #[test]
    fn uploaded_image_present_yields_custom_image() {
        let json = r#"{
            "id": "p1",
            "name": "Mix",
            "uploadedImage": "al-p1-cover-ref",
            "externalImageUrl": "https://example/img.png"
        }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert_eq!(p.uploaded_image.as_deref(), Some("al-p1-cover-ref"));
        assert_eq!(p.custom_image(), Some("al-p1-cover-ref"));
        assert_eq!(
            p.external_image_url.as_deref(),
            Some("https://example/img.png")
        );
    }

    /// Navidrome always emits `uploadedImage` (json, no omitempty) — `""`
    /// when no custom image is set. That must NOT count as custom art.
    #[test]
    fn uploaded_image_empty_string_is_not_custom() {
        let json = r#"{ "id": "p2", "name": "Mix", "uploadedImage": "" }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert_eq!(p.uploaded_image.as_deref(), Some(""));
        assert_eq!(p.custom_image(), None, "empty uploadedImage must gate out");
        assert_eq!(p.external_image_url, None);
    }

    /// Pre-feature servers omit both keys entirely; the `Option` fields must
    /// deserialize to `None`, not error — additive wire-safety.
    #[test]
    fn uploaded_image_absent_yields_none() {
        let json = r#"{ "id": "p3", "name": "Mix" }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert_eq!(p.uploaded_image, None);
        assert_eq!(p.custom_image(), None);
        assert_eq!(p.external_image_url, None);
    }

    /// Smart-playlist serde trio, mirroring the `uploaded_image` tests:
    /// rules present / rules absent / evaluatedAt absent. Additive JSON —
    /// pre-smart parses must keep working unchanged.
    #[test]
    fn rules_present_yields_smart() {
        let json = r#"{
            "id": "sp1",
            "name": "Never Played",
            "rules": { "all": [ { "is": { "playcount": 0 } } ] },
            "evaluatedAt": "2026-07-01T10:00:00Z",
            "path": "/music/Library/never_played.nsp",
            "sync": true
        }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert!(p.is_smart());
        assert!(p.is_file_backed());
        assert!(p.sync);
        assert_eq!(p.evaluated_at.as_deref(), Some("2026-07-01T10:00:00Z"));
        assert!(p.rules.as_ref().is_some_and(|r| r.get("all").is_some()));
    }

    #[test]
    fn rules_absent_is_not_smart() {
        let json = r#"{ "id": "p4", "name": "Mix" }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert!(!p.is_smart());
        assert!(!p.is_file_backed());
        assert!(!p.sync);
        assert_eq!(p.evaluated_at, None);
    }

    /// A literal `"rules": null` emission must not count as smart — the
    /// `Option<Value>` would hold `Some(Null)`, so `is_smart` guards on it.
    #[test]
    fn rules_null_is_not_smart() {
        let json = r#"{ "id": "p5", "name": "Mix", "rules": null }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert!(!p.is_smart());
    }

    /// A smart playlist the server has never evaluated: rules present,
    /// evaluatedAt absent.
    #[test]
    fn evaluated_at_absent_on_fresh_smart_playlist() {
        let json = r#"{ "id": "sp2", "name": "Fresh", "rules": { "all": [] } }"#;
        let p: Playlist = serde_json::from_str(json).expect("deserialize");
        assert!(p.is_smart());
        assert_eq!(p.evaluated_at, None);
    }

    /// The strict-parse accept/reject matrix for the draft marker grammar.
    /// Rejections are as load-bearing as the accept: a prefix-only or
    /// malformed comment must NOT parse (it would otherwise vanish a real
    /// playlist from every surface and mark it sweep-eligible).
    #[test]
    fn draft_marker_parse_accept_reject_matrix() {
        // Canonical accept.
        let m = DraftMarker::parse("nokkvi-draft/1 pid=4242 ts=1752800000")
            .expect("canonical marker must parse");
        assert_eq!(
            m,
            DraftMarker {
                version: 1,
                pid: 4242,
                ts: 1_752_800_000
            }
        );

        // format() round-trips through parse() by construction.
        let minted = DraftMarker::format(1, 999, 123_456);
        assert_eq!(
            DraftMarker::parse(&minted),
            Some(DraftMarker {
                version: 1,
                pid: 999,
                ts: 123_456
            })
        );

        // Reject: every malformed variant.
        for bad in [
            "",                                           // empty
            "nokkvi-draft/",                              // bare prefix
            "nokkvi-draft/1",                             // no fields
            "nokkvi-draft/1 pid=42",                      // missing ts
            "nokkvi-draft/1 ts=5 pid=42",                 // wrong field order
            "nokkvi-draft/1 pid=42 ts=5 extra",           // trailing token
            "nokkvi-draft/1 pid=42 ts=5 ",                // trailing space
            "nokkvi-draft/x pid=42 ts=5",                 // non-numeric version
            "nokkvi-draft/1 pid=abc ts=5",                // non-numeric pid
            "nokkvi-draft/1 pid=42 ts=abc",               // non-numeric ts
            "nokkvi-draft/1  pid=42 ts=5",                // double space
            " nokkvi-draft/1 pid=42 ts=5",                // leading junk
            "my playlist nokkvi-draft/1 pid=42 ts=5",     // embedded, not prefix
            "nokkvi-draft/1 pid=-2 ts=5",                 // negative pid
            "NOKKVI-DRAFT/1 pid=42 ts=5",                 // case matters
            "a comment that mentions nokkvi-draft/ only", // prose mention
        ] {
            assert_eq!(DraftMarker::parse(bad), None, "must reject: {bad:?}");
        }
    }

    /// `is_draft()` keys on the strict parse — a user comment that merely
    /// starts with the reserved prefix is NOT a draft.
    #[test]
    fn is_draft_requires_full_marker_grammar() {
        let mut p: Playlist =
            serde_json::from_str(r#"{ "id": "p6", "name": "Mix" }"#).expect("deserialize");
        assert!(!p.is_draft());

        p.comment = "nokkvi-draft/1 pid=4242 ts=1752800000".to_owned();
        assert!(p.is_draft());

        p.comment = "nokkvi-draft/my notes".to_owned();
        assert!(!p.is_draft(), "prefix-only comment must not count as draft");
    }
}
