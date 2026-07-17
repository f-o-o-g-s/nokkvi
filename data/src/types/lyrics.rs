//! Iced-free LRC lyrics domain: a millisecond-timed parser, a metadata reader,
//! a two-tier tag matcher over a `.lrc` corpus, and the adapter that turns an
//! OpenSubsonic `getLyricsBySongId` payload into the same internal document.
//!
//! The LRC tokenizer is ported from rmpc (`reference-rmpc/src/shared/lrc/`) and
//! fooyin, adapted to `u32` millisecond timing and optional word-level spans.
//! Timing math is done in `u64` and narrowed to `u32` at the end (a `u32`
//! millisecond span covers ~49 days, far beyond any track).

use std::{collections::HashMap, path::PathBuf};

use crate::utils::search::build_searchable_lower;

/// A single karaoke word span within a line — populated from enhanced LRC
/// (`<mm:ss.xx>` markers) or the server's `cueLine`/`cue` layer. Empty for
/// plain line-level lyrics (the common case).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WordSpan {
    pub start_ms: u32,
    pub text: String,
}

/// One timed line of lyrics. `words` is empty unless the source carried
/// word-level timing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LrcLine {
    pub time_ms: u32,
    pub text: String,
    pub words: Vec<WordSpan>,
}

/// A parsed lyrics document. `synced` is false only when the source carried no
/// timestamps at all (plain lyrics) — the render surface treats an unsynced doc
/// as a no-match, so nothing is faked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LrcDocument {
    pub lines: Vec<LrcLine>,
    pub synced: bool,
}

/// Header metadata read from an LRC file (or synthesized when caching a fetch).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LrcMetadata {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub length_ms: Option<u32>,
    /// `[offset:]` value in milliseconds; positive shifts lines earlier.
    pub offset_ms: i64,
}

// ---------------------------------------------------------------------------
// Tokenizer (ported from rmpc `shared/lrc/lyrics.rs`)
// ---------------------------------------------------------------------------

enum Tag {
    Timestamp(String),
    Meta(String, String),
    Invalid,
}

/// Parse a single `[...]` tag from the start of `line`, tolerating brackets
/// nested inside the tag content (e.g. `[ti:Song [Explicit]]`). Returns the
/// classified tag and the number of bytes consumed.
fn next_tag(line: &str) -> Option<(Tag, usize)> {
    if !line.starts_with('[') {
        return None;
    }

    let mut depth = 0;
    let mut close_pos = None;
    for (i, c) in line[1..].char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    close_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    let close_pos = close_pos?;
    let content = &line[1..=close_pos];
    let consumed = close_pos + 2;

    let tag = if is_timestamp(content) {
        Tag::Timestamp(content.to_string())
    } else if let Some((key, value)) = content.split_once(':') {
        Tag::Meta(key.trim().to_string(), value.trim().to_string())
    } else {
        Tag::Invalid
    };

    Some((tag, consumed))
}

/// A tag is a timestamp if it starts with a digit and contains `:`.
fn is_timestamp(content: &str) -> bool {
    content.chars().next().is_some_and(|c| c.is_ascii_digit()) && content.contains(':')
}

/// Parse `mm:ss.xx` (or `mm:ss:xx`) into milliseconds, applying `offset_ms`.
/// Fractions of a second are truncated to 3 digits and scaled to ms.
fn parse_timestamp(ts: &str, offset_ms: i64) -> Option<u32> {
    let (minutes, rest) = ts.split_once(':')?;
    let (seconds, fractions) = rest.split_once('.').or_else(|| rest.split_once(':'))?;

    let fractions = &fractions[..3.min(fractions.len())];
    let minutes: u64 = minutes.parse().ok()?;
    let seconds: u64 = seconds.parse().ok()?;
    let frac_value: u64 = fractions.parse().ok()?;

    let scale = 10u64.pow(3 - u32::try_from(fractions.len()).unwrap_or(3));
    let ms = minutes * 60_000 + seconds * 1000 + frac_value * scale;

    u32::try_from(apply_offset_u64(ms, offset_ms)).ok()
}

/// Apply an `[offset:]` (positive = earlier) to a millisecond value, clamped
/// at 0, working in `u64`.
fn apply_offset_u64(ms: u64, offset_ms: i64) -> u64 {
    let delta = offset_ms.unsigned_abs();
    if offset_ms > 0 {
        ms.saturating_sub(delta)
    } else if offset_ms < 0 {
        ms.saturating_add(delta)
    } else {
        ms
    }
}

/// Apply an offset to a `u32` millisecond value (used for pre-scaled inputs
/// such as the structured API and word timings).
fn apply_offset(ms: u32, offset_ms: i64) -> u32 {
    u32::try_from(apply_offset_u64(u64::from(ms), offset_ms)).unwrap_or(u32::MAX)
}

/// Read only the header tags, stopping at the first timestamp line. Cheap
/// enough to run over the whole corpus for indexing.
pub fn read_metadata(content: &str) -> LrcMetadata {
    let mut meta = LrcMetadata::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.starts_with('[') {
            continue;
        }

        let mut remaining = line;
        let mut found_timestamp = false;
        while let Some((tag, consumed)) = next_tag(remaining) {
            match tag {
                Tag::Timestamp(_) => {
                    found_timestamp = true;
                    break;
                }
                Tag::Meta(key, value) => match key.as_str() {
                    "ti" => meta.title = Some(value),
                    "ar" => meta.artist = Some(value),
                    "al" => meta.album = Some(value),
                    "length" => meta.length_ms = parse_length_ms(&value),
                    "offset" => {
                        if let Ok(offset) = value.parse::<i64>() {
                            meta.offset_ms = offset;
                        }
                    }
                    _ => {}
                },
                Tag::Invalid => {}
            }

            remaining = &remaining[consumed..];
            if !remaining.starts_with('[') {
                break;
            }
        }

        if found_timestamp {
            break;
        }
    }

    meta
}

/// Parse a `[length: mm:ss]` value into milliseconds.
fn parse_length_ms(value: &str) -> Option<u32> {
    let (minutes, seconds) = value.trim().split_once(':')?;
    let minutes: u32 = minutes.trim().parse().ok()?;
    let seconds: u32 = seconds.trim().parse().ok()?;
    Some((minutes * 60 + seconds) * 1000)
}

/// Parse a complete LRC document. `\r` is handled by `str::lines()`. Blank
/// timed lines are preserved as spacer rows; malformed timestamps are skipped
/// gracefully. `synced` is true iff at least one `[mm:ss.xx]` timestamp parsed.
pub fn parse(content: &str) -> LrcDocument {
    let meta = read_metadata(content);
    let offset = meta.offset_ms;
    let mut lines = Vec::new();
    let mut had_timestamp = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.starts_with('[') {
            continue;
        }

        let mut timestamps = Vec::new();
        let mut remaining = line;
        let mut consumed_total = 0;
        while let Some((tag, consumed)) = next_tag(remaining) {
            match tag {
                Tag::Timestamp(ts) => {
                    timestamps.push(ts);
                    consumed_total += consumed;
                    remaining = &remaining[consumed..];
                    if !remaining.starts_with('[') {
                        break;
                    }
                }
                // A non-timestamp tag ends the timestamp run; everything from
                // here is literal lyric text (handles `[00:10][Intro] Welcome`).
                Tag::Meta(..) | Tag::Invalid => break,
            }
        }

        if timestamps.is_empty() {
            continue;
        }
        had_timestamp = true;

        let raw_text = if consumed_total < line.len() {
            &line[consumed_total..]
        } else {
            remaining
        };
        let (text, words) = extract_words(raw_text.trim(), offset);

        for ts in &timestamps {
            if let Some(time_ms) = parse_timestamp(ts, offset) {
                lines.push(LrcLine {
                    time_ms,
                    text: text.clone(),
                    words: words.clone(),
                });
            }
        }
    }

    // Stable-sort by time (real-world files are usually monotonic, but some
    // aren't; a stable sort lets the active-line search be a simple scan).
    lines.sort_by_key(|l| l.time_ms);
    LrcDocument {
        lines,
        synced: had_timestamp,
    }
}

/// Split an enhanced-LRC line into `(display_text, word_spans)`. Plain lines
/// (no `<mm:ss.xx>` markers) return the text unchanged with no words. Each
/// word accumulates the text following its marker up to the next marker.
fn extract_words(text: &str, offset_ms: i64) -> (String, Vec<WordSpan>) {
    if !text.contains('<') {
        return (text.to_string(), Vec::new());
    }

    let mut display = String::with_capacity(text.len());
    let mut words: Vec<WordSpan> = Vec::new();
    let mut rest = text;

    while let Some(lt) = rest.find('<') {
        let before = &rest[..lt];
        display.push_str(before);
        if let Some(word) = words.last_mut() {
            word.text.push_str(before);
        }

        let after = &rest[lt + 1..];
        match after.find('>') {
            Some(gt) if is_timestamp(&after[..gt]) => {
                if let Some(start_ms) = parse_timestamp(&after[..gt], offset_ms) {
                    words.push(WordSpan {
                        start_ms,
                        text: String::new(),
                    });
                }
                rest = &after[gt + 1..];
            }
            // Not a valid word timestamp: keep the '<' literally.
            _ => {
                display.push('<');
                if let Some(word) = words.last_mut() {
                    word.text.push('<');
                }
                rest = after;
            }
        }
    }

    display.push_str(rest);
    if let Some(word) = words.last_mut() {
        word.text.push_str(rest);
    }

    for word in &mut words {
        word.text = word.text.trim().to_string();
    }
    words.retain(|w| !w.text.is_empty());
    (display.trim().to_string(), words)
}

// ---------------------------------------------------------------------------
// Structured-lyrics adapter (getLyricsBySongId → LrcDocument)
// ---------------------------------------------------------------------------

/// Domain form of one OpenSubsonic `structuredLyrics` entry (the wire structs
/// live next to the API service). Kind-selection among multiple entries is the
/// resolve chain's job; this represents a single chosen entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructuredLyrics {
    pub synced: bool,
    pub offset_ms: i64,
    pub kind: Option<String>,
    pub lines: Vec<StructuredLine>,
    pub cue_lines: Vec<StructuredCueLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructuredLine {
    pub start_ms: Option<u32>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructuredCueLine {
    /// Index of the `line[]` this cue-line annotates.
    pub index: usize,
    pub agent_id: Option<String>,
    pub cues: Vec<StructuredCue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructuredCue {
    pub start_ms: u32,
    pub value: String,
}

impl LrcDocument {
    /// Convert one structured entry into the internal document. Word timings
    /// come from the matching `cueLine` (joined by `.index`); each word's text
    /// is the cue's own `value` (never a byte-slice — the server's byteStart/
    /// End are inclusive, line-relative, and off-by-one against a Rust range).
    pub fn from_structured(s: &StructuredLyrics) -> LrcDocument {
        let offset = s.offset_ms;
        let lines = s
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let time_ms = apply_offset(line.start_ms.unwrap_or(0), offset);
                let words = s
                    .cue_lines
                    .iter()
                    .find(|cl| cl.index == i)
                    .map(|cl| {
                        cl.cues
                            .iter()
                            .map(|cue| WordSpan {
                                start_ms: apply_offset(cue.start_ms, offset),
                                text: cue.value.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                LrcLine {
                    time_ms,
                    text: line.value.clone(),
                    words,
                }
            })
            .collect();

        LrcDocument {
            lines,
            synced: s.synced,
        }
    }
}

// ---------------------------------------------------------------------------
// Normalization (matching keys)
// ---------------------------------------------------------------------------

mod normalize {
    use super::build_searchable_lower;

    /// Trailing qualifier words that mark a parenthetical as a variant tag.
    const QUALIFIERS: &[&str] = &[
        "remix",
        "live",
        "extended",
        "acoustic",
        "instrumental",
        "remaster",
        "cover",
        "edit",
        "version",
        "feat.",
        "feat",
        "featuring",
        "deluxe",
    ];

    /// Unicode-aware case fold, reusing the shared search helper.
    pub fn casefold(s: &str) -> String {
        build_searchable_lower(&[s])
    }

    /// Strip a trailing run of qualifier parentheticals: `Song (Live)` -> `Song`
    /// (ported from firmium). Recording-distinguishing, so used only in Tier 2.
    fn strip_qualifier_suffix(title: &str) -> String {
        let mut result = title.to_string();
        loop {
            let trimmed = result.trim_end();
            let open = if trimmed.ends_with(')') {
                '('
            } else if trimmed.ends_with(']') {
                '['
            } else {
                break;
            };
            let Some(start) = trimmed.rfind(open) else {
                break;
            };
            let inner = &trimmed[start + 1..trimmed.len() - 1];
            let first_word = inner
                .split(char::is_whitespace)
                .next()
                .unwrap_or("")
                .to_lowercase();
            if QUALIFIERS.iter().any(|q| first_word.starts_with(q)) {
                result = trimmed[..start].trim_end().to_string();
            } else {
                break;
            }
        }
        result
    }

    /// Strip a trailing ` - feat. ...` suffix (ported from firmium).
    fn strip_feat_suffix(title: &str) -> String {
        let lower = title.to_lowercase();
        for marker in [" - feat.", " - feat ", " - featuring"] {
            if let Some(idx) = lower.find(marker) {
                return title[..idx].trim_end().to_string();
            }
        }
        title.to_string()
    }

    /// Keep the primary artist, dropping ` feat`/` ft`/`/` runs (ported).
    fn primary_artist(artist: &str) -> String {
        let lower = artist.to_lowercase();
        let mut cut = artist.len();
        for marker in [" feat.", " feat ", " featuring ", " ft.", " ft ", "/"] {
            if let Some(idx) = lower.find(marker) {
                cut = cut.min(idx);
            }
        }
        artist[..cut].trim().to_string()
    }

    /// Casefold + `&`->`and` + drop punctuation + collapse whitespace. This is
    /// what folds `Mr.` and `Mr` together in Tier 2.
    fn reduce(s: &str) -> String {
        let lowered = casefold(s).replace('&', " and ");
        let mut out = String::with_capacity(lowered.len());
        let mut pending_space = false;
        for c in lowered.chars() {
            if c.is_alphanumeric() {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                out.push(c);
                pending_space = false;
            } else {
                pending_space = true;
            }
        }
        out
    }

    /// Tier-1 key: **casefold only** — never stripped, so genuinely distinct
    /// recordings (`Song` vs `Song (Live)`) keep distinct strict keys.
    pub fn tier1_key(artist: &str, title: &str) -> (String, String) {
        (casefold(artist), casefold(title))
    }

    /// Tier-2 key: firmium strips + punctuation/whitespace reduction. Loose
    /// enough to recover tag drift; the self-guard in `find` keeps it safe.
    pub fn tier2_key(artist: &str, title: &str) -> (String, String) {
        let artist = primary_artist(artist);
        let title = strip_feat_suffix(&strip_qualifier_suffix(title));
        (reduce(&artist), reduce(&title))
    }
}

// ---------------------------------------------------------------------------
// Two-tier matcher over the store
// ---------------------------------------------------------------------------

/// One indexed `.lrc` file: its path, header metadata, and Tier-1 identity.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub path: PathBuf,
    pub meta: LrcMetadata,
    tier1: (String, String),
}

/// In-memory two-tier index of the lyrics store. Tier 1 is strict
/// (casefold-only); Tier 2 is the self-guarding loose recovery tier.
#[derive(Debug, Default)]
pub struct LyricsIndex {
    tier1: HashMap<(String, String), Vec<IndexEntry>>,
    tier2: HashMap<(String, String), Vec<IndexEntry>>,
}

impl LyricsIndex {
    /// Find the best `.lrc` for a track. Returns `None` rather than guess when
    /// a match is ambiguous — a wrong synced sheet is worse than none.
    pub fn find(
        &self,
        artist: &str,
        title: &str,
        album: Option<&str>,
        length_ms: Option<u32>,
    ) -> Option<&IndexEntry> {
        // Tier 1: strict. If the key is present, resolve here and commit to the
        // result (an ambiguous strict collision refuses rather than loosening).
        let key1 = normalize::tier1_key(artist, title);
        if let Some(entries) = self.tier1.get(&key1) {
            return resolve(entries, album, length_ms);
        }

        // Tier 2 (only on a Tier-1 key miss): accept iff every entry under the
        // loose key shares one strict identity; >= 2 distinct identities refuse.
        let key2 = normalize::tier2_key(artist, title);
        if let Some(entries) = self.tier2.get(&key2) {
            let first = &entries.first()?.tier1;
            if entries.iter().all(|e| &e.tier1 == first) {
                return resolve(entries, album, length_ms);
            }
        }

        None
    }

    /// Insert a parsed entry (both tiers). Skips entries with an empty
    /// artist/title header.
    fn insert(&mut self, path: PathBuf, meta: LrcMetadata) {
        let (Some(artist), Some(title)) = (meta.artist.as_deref(), meta.title.as_deref()) else {
            return;
        };
        if artist.is_empty() || title.is_empty() {
            return;
        }
        let tier1 = normalize::tier1_key(artist, title);
        let key2 = normalize::tier2_key(artist, title);
        let entry = IndexEntry {
            path,
            meta,
            tier1: tier1.clone(),
        };
        self.tier1.entry(tier1).or_default().push(entry.clone());
        self.tier2.entry(key2).or_default().push(entry);
    }

    /// Number of indexed files (counted once, via Tier 1).
    pub fn len(&self) -> usize {
        self.tier1.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.tier1.is_empty()
    }
}

/// Disambiguate among candidates already matched by artist+title. Album is a
/// SOFT signal (a missing album never removes a candidate); it only picks among
/// ties. Falls back to a `[length:]` window, then a deterministic pick for
/// byte-identical tags (translation pairs), else refuses.
fn resolve<'a>(
    entries: &'a [IndexEntry],
    song_album: Option<&str>,
    song_length_ms: Option<u32>,
) -> Option<&'a IndexEntry> {
    match entries {
        [] => None,
        [only] => Some(only),
        many => {
            // 1. Exactly one casefolded-[al:] match wins.
            if let Some(album) = song_album.filter(|a| !a.is_empty()) {
                let album = normalize::casefold(album);
                let mut exact = many.iter().filter(|e| {
                    e.meta
                        .album
                        .as_deref()
                        .is_some_and(|a| normalize::casefold(a) == album)
                });
                if let Some(first) = exact.next()
                    && exact.next().is_none()
                {
                    return Some(first);
                }
            }

            // 2. Nearest [length:] within 5s (inert on the current corpus —
            //    no file carries [length:] — but real armor for growth).
            if let Some(length) = song_length_ms
                && let Some((entry, _)) = many
                    .iter()
                    .filter_map(|e| e.meta.length_ms.map(|l| (e, l.abs_diff(length))))
                    .filter(|(_, diff)| *diff <= 5_000)
                    .min_by_key(|(_, diff)| *diff)
            {
                return Some(entry);
            }

            // 3. Byte-identical tags (e.g. translation pairs): pick the
            //    lexicographically-first path so the choice is deterministic.
            let first = many.first()?;
            let all_identical = many.iter().all(|e| {
                e.meta.artist == first.meta.artist
                    && e.meta.title == first.meta.title
                    && e.meta.album == first.meta.album
            });
            if all_identical {
                return many.iter().min_by(|a, b| a.path.cmp(&b.path));
            }

            // 4. Genuinely ambiguous → refuse.
            None
        }
    }
}

/// Walk `dir` recursively, index every readable `*.lrc` by its internal tags,
/// and log the count. Bad files (unreadable, non-UTF-8, header-empty) are
/// skipped so one broken file can't abort the scan.
pub async fn build_index(dir: PathBuf) -> LyricsIndex {
    let mut index = LyricsIndex::default();
    let mut stack = vec![dir];

    while let Some(current) = stack.pop() {
        let mut read_dir = match tokio::fs::read_dir(&current).await {
            Ok(read_dir) => read_dir,
            Err(e) => {
                tracing::warn!(error = %e, dir = %current.display(), "lyrics: read_dir failed");
                continue;
            }
        };

        loop {
            let entry = match read_dir.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "lyrics: dir entry failed");
                    break;
                }
            };

            let path = entry.path();
            let Ok(file_type) = entry.file_type().await else {
                continue;
            };

            if file_type.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("lrc") {
                let Ok(content) = tokio::fs::read_to_string(&path).await else {
                    continue;
                };
                index.insert(path, read_metadata(&content));
            }
        }
    }

    tracing::info!(count = index.len(), "indexed lyrics");
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(line: &LrcLine) -> u32 {
        line.time_ms
    }

    // --- parser conformance ---

    #[test]
    fn parses_standard_metadata_and_lines() {
        let doc = parse(
            "[ar:Beach House]\n[ti:Myth]\n[al:Bloom]\n\n[00:45.47] Drifting\n[00:48.54] You see",
        );
        assert!(doc.synced);
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0].time_ms, 45_470);
        assert_eq!(doc.lines[0].text, "Drifting");
        assert_eq!(doc.lines[1].time_ms, 48_540);
    }

    #[test]
    fn parses_unsynced_text() {
        // No timestamps anywhere → not synced, no lines.
        let doc = parse("just some plain text\nmore plain text");
        assert!(!doc.synced);
        assert!(doc.lines.is_empty());
    }

    #[test]
    fn preserves_blank_lines() {
        let doc = parse("[00:10.00]first\n[00:20.00]\n[00:30.00]third");
        assert_eq!(doc.lines.len(), 3);
        assert_eq!(doc.lines[1].text, "");
    }

    #[test]
    fn explodes_repeated_timestamps() {
        let doc = parse("[00:04.73][00:05.73][00:06.73]chorus");
        assert_eq!(doc.lines.len(), 3);
        assert!(doc.lines.iter().all(|l| l.text == "chorus"));
        assert_eq!(doc.lines[0].time_ms, 4_730);
        assert_eq!(doc.lines[2].time_ms, 6_730);
    }

    #[test]
    fn stable_sorts_by_timestamp() {
        let doc = parse("[00:30.00]c\n[00:10.00]a\n[00:20.00]b");
        let times: Vec<u32> = doc.lines.iter().map(ms).collect();
        assert_eq!(times, [10_000, 20_000, 30_000]);
    }

    #[test]
    fn centi_milli_normalization() {
        let doc = parse("[00:00.8]a\n[00:10.73]b\n[00:20.563]c\n[00:30.2853]d");
        assert_eq!(doc.lines[0].time_ms, 800);
        assert_eq!(doc.lines[1].time_ms, 10_730);
        assert_eq!(doc.lines[2].time_ms, 20_563);
        assert_eq!(doc.lines[3].time_ms, 30_285); // truncated to 3 frac digits
    }

    #[test]
    fn colon_fraction_separator() {
        let doc = parse("[00:04:73]colon frac");
        assert_eq!(doc.lines[0].time_ms, 4_730);
    }

    #[test]
    fn brackets_in_lyrics_and_metadata() {
        let doc = parse("[ti:Song [Explicit]]\n[00:10.00][Intro] Welcome to the [Show]");
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].text, "[Intro] Welcome to the [Show]");
    }

    #[test]
    fn offset_positive_advances() {
        let doc = parse("[offset:+1000]\n[00:01.86]a\n[00:04.73]b");
        assert_eq!(doc.lines[0].time_ms, 860);
        assert_eq!(doc.lines[1].time_ms, 3_730);
    }

    #[test]
    fn offset_negative_delays() {
        let doc = parse("[offset:-1000]\n[00:01.86]a");
        assert_eq!(doc.lines[0].time_ms, 2_860);
    }

    #[test]
    fn offset_clamps_at_zero() {
        let doc = parse("[offset:+5000]\n[00:01.00]a");
        assert_eq!(doc.lines[0].time_ms, 0);
    }

    #[test]
    fn unknown_tag_and_comments_ignored() {
        let doc = parse("[custom:whatever]\n# comment\n[tool:rmpc]\n[00:10.00]lyrics");
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].text, "lyrics");
    }

    #[test]
    fn malformed_lines_skipped() {
        let doc = parse("[unclosed bracket\n[00:10.00]valid\n]orphan\n[00:20.00]also valid");
        assert_eq!(doc.lines.len(), 2);
    }

    #[test]
    fn zero_timestamp_is_synced() {
        // A sheet whose only stamp is [00:00.00] is synced (had_timestamp),
        // not "time_ms > 0".
        let doc = parse("[00:00.00]start");
        assert!(doc.synced);
        assert_eq!(doc.lines[0].time_ms, 0);
    }

    #[test]
    fn parses_crlf_line_endings() {
        let doc = parse("[ar:A]\r\n[ti:T]\r\n[00:10.00]line\r\n");
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].text, "line"); // no stray \r
        assert_eq!(
            read_metadata("[ar:A]\r\n[ti:T]\r\n").artist.as_deref(),
            Some("A")
        );
    }

    #[test]
    fn parses_enhanced_word_spans() {
        let doc = parse("[00:01.00]<00:01.10>Hello <00:01.60>world");
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].text, "Hello world");
        assert_eq!(doc.lines[0].words.len(), 2);
        assert_eq!(
            doc.lines[0].words[0],
            WordSpan {
                start_ms: 1_100,
                text: "Hello".into()
            }
        );
        assert_eq!(
            doc.lines[0].words[1],
            WordSpan {
                start_ms: 1_600,
                text: "world".into()
            }
        );
    }

    // --- read_metadata ---

    #[test]
    fn read_metadata_stops_at_first_timestamp() {
        let meta = read_metadata("[ti:T]\n[ar:A]\n[00:10.00]x\n[al:Ignored]");
        assert_eq!(meta.title.as_deref(), Some("T"));
        assert_eq!(meta.artist.as_deref(), Some("A"));
        assert_eq!(meta.album, None); // after the first timestamp
    }

    #[test]
    fn read_metadata_length_and_offset() {
        let meta = read_metadata("[length:2:23]\n[offset:+500]\n[00:10.00]x");
        assert_eq!(meta.length_ms, Some(143_000));
        assert_eq!(meta.offset_ms, 500);
    }

    // --- matcher ---

    fn entry(path: &str, artist: &str, title: &str, album: Option<&str>) -> (PathBuf, LrcMetadata) {
        (
            PathBuf::from(path),
            LrcMetadata {
                artist: Some(artist.into()),
                title: Some(title.into()),
                album: album.map(Into::into),
                length_ms: None,
                offset_ms: 0,
            },
        )
    }

    fn index_of(entries: Vec<(PathBuf, LrcMetadata)>) -> LyricsIndex {
        let mut index = LyricsIndex::default();
        for (path, meta) in entries {
            index.insert(path, meta);
        }
        index
    }

    #[test]
    fn tier1_exact_artist_title_matches() {
        let index = index_of(vec![entry("a.lrc", "Beach House", "Myth", Some("Bloom"))]);
        let hit = index.find("beach house", "MYTH", Some("bloom"), None);
        assert_eq!(
            hit.map(|e| e.path.as_path()),
            Some(PathBuf::from("a.lrc").as_path())
        );
    }

    #[test]
    fn tier1_does_not_strip_qualifiers() {
        // Studio + live share the album; strips at Tier-1 would merge them into
        // an unresolvable collision. Tier-1 casefold-only keeps them distinct.
        let index = index_of(vec![
            entry(
                "studio.lrc",
                "Emperor",
                "The Loss and Curse of Reverence",
                Some("Anthems"),
            ),
            entry(
                "live.lrc",
                "Emperor",
                "The Loss and Curse of Reverence (live)",
                Some("Anthems"),
            ),
        ]);
        assert_eq!(
            index
                .find(
                    "Emperor",
                    "The Loss and Curse of Reverence",
                    Some("Anthems"),
                    None
                )
                .map(|e| e.path.as_path()),
            Some(PathBuf::from("studio.lrc").as_path())
        );
    }

    #[test]
    fn album_soft_missing_never_blocks() {
        // Single candidate with no album still matches a song that has one.
        let index = index_of(vec![entry("a.lrc", "Air", "La Femme d'Argent", None)]);
        assert!(
            index
                .find("Air", "La Femme d'Argent", Some("Moon Safari"), None)
                .is_some()
        );
    }

    #[test]
    fn casefolded_album_wins_over_tie() {
        let index = index_of(vec![
            entry("one.lrc", "X", "Song", Some("First Album")),
            entry("two.lrc", "X", "Song", Some("Second Album")),
        ]);
        assert_eq!(
            index
                .find("X", "Song", Some("second album"), None)
                .map(|e| e.path.as_path()),
            Some(PathBuf::from("two.lrc").as_path())
        );
    }

    #[test]
    fn tier2_recovers_mr_dot_drift() {
        let index = index_of(vec![entry("a.lrc", "Artist", "Mr.", Some("Album"))]);
        // Library sends "Mr" (no dot). Tier-1 misses; Tier-2 reduce folds them.
        assert!(index.find("Artist", "Mr", Some("Album"), None).is_some());
    }

    #[test]
    fn tier2_collision_refuses() {
        // Two distinct real titles collapsing to one loose key must refuse.
        let index = index_of(vec![
            entry("a.lrc", "X", "Go!", Some("A")),
            entry("b.lrc", "X", "Go?", Some("B")),
        ]);
        // "Go." reduces to "go" like both; distinct Tier-1 identities → refuse.
        assert!(index.find("X", "Go.", Some("C"), None).is_none());
    }

    #[test]
    fn byte_identical_tag_pair_picks_deterministically() {
        let index = index_of(vec![
            entry(
                "z_danish.lrc",
                "Afsky",
                "Altid Veltilfreds",
                Some("Ofte jeg drømmer"),
            ),
            entry(
                "a.lrc",
                "Afsky",
                "Altid Veltilfreds",
                Some("Ofte jeg drømmer"),
            ),
        ]);
        let first = index.find("Afsky", "Altid Veltilfreds", Some("Ofte jeg drømmer"), None);
        // Lexicographically-first path, stable across calls.
        assert_eq!(
            first.map(|e| e.path.as_path()),
            Some(PathBuf::from("a.lrc").as_path())
        );
    }

    // --- from_structured ---

    #[test]
    fn from_structured_maps_lines_and_words() {
        let s = StructuredLyrics {
            synced: true,
            offset_ms: 0,
            kind: Some("main".into()),
            lines: vec![
                StructuredLine {
                    start_ms: Some(1_000),
                    value: "Hello world".into(),
                },
                StructuredLine {
                    start_ms: Some(2_000),
                    value: "second".into(),
                },
            ],
            cue_lines: vec![StructuredCueLine {
                index: 0,
                agent_id: None,
                cues: vec![
                    StructuredCue {
                        start_ms: 1_000,
                        value: "Hello".into(),
                    },
                    StructuredCue {
                        start_ms: 1_500,
                        value: "world".into(),
                    },
                ],
            }],
        };
        let doc = LrcDocument::from_structured(&s);
        assert!(doc.synced);
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0].time_ms, 1_000);
        assert_eq!(doc.lines[0].words.len(), 2);
        assert_eq!(doc.lines[0].words[1].text, "world");
        assert!(doc.lines[1].words.is_empty()); // no cueLine for index 1
    }

    #[test]
    fn from_structured_empty_words_when_no_cueline() {
        let s = StructuredLyrics {
            synced: true,
            lines: vec![StructuredLine {
                start_ms: Some(500),
                value: "line".into(),
            }],
            ..Default::default()
        };
        let doc = LrcDocument::from_structured(&s);
        assert_eq!(doc.lines.len(), 1);
        assert!(doc.lines[0].words.is_empty());
    }

    // --- build_index round-trip ---

    #[tokio::test]
    async fn build_index_finds_by_tags() {
        let dir = tempfile::tempdir().expect("tempdir");
        let artist_dir = dir.path().join("Beach_House").join("Bloom");
        std::fs::create_dir_all(&artist_dir).expect("mkdir");
        std::fs::write(
            artist_dir.join("myth.lrc"),
            "[ar:Beach House]\n[ti:Myth]\n[al:Bloom]\n[00:45.47]Drifting",
        )
        .expect("write");
        // A non-.lrc file and a header-less file must be skipped, not crash.
        std::fs::write(artist_dir.join("notes.txt"), "ignore me").expect("write");

        let index = build_index(dir.path().to_path_buf()).await;
        assert_eq!(index.len(), 1);
        assert!(
            index
                .find("Beach House", "Myth", Some("Bloom"), None)
                .is_some()
        );
        assert!(index.find("Nonexistent", "Song", None, None).is_none());
    }
}
