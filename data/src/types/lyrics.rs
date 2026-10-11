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
    /// When the word stops being sung: a TTML cue's `end`, the trailing
    /// `<mm:ss.xx>` stamp an enhanced LRC line may end with, or an end the
    /// server filled in (Navidrome gives every cue one: the next word's
    /// start, and for a line's last word the NEXT LINE's start, which is why
    /// [`LrcLine::word_window`] treats a last word ending at the line's end
    /// as untimed). `None` means the next word's start, or for a last word
    /// the pace fallback.
    pub end_ms: Option<u32>,
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

/// Where the playhead sits among a line's words — the input to the word-level
/// highlight. Words `0..sung` are fully sung; `current` is the one being sung
/// now, with how far through it the playhead is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WordCursor {
    /// Words fully sung, counted from the line's first word.
    pub sung: usize,
    /// The word being sung right now and the playhead's progress through it
    /// (`0.0..1.0`). `None` before the first word, in a gap between two words
    /// (a word with an explicit end that stops short of the next word's
    /// start), and after the last word.
    pub current: Option<(usize, f32)>,
}

impl WordSpan {
    /// The same word `shift_ms` later (earlier when negative), clamped at 0.
    fn shifted(&self, shift_ms: i64) -> WordSpan {
        let shift = |ms: u32| u32::try_from((i64::from(ms) + shift_ms).max(0)).unwrap_or(u32::MAX);
        WordSpan {
            start_ms: shift(self.start_ms),
            end_ms: self.end_ms.map(shift),
            text: self.text.clone(),
        }
    }
}

/// Shortest a line's last word may be guessed to last, in milliseconds, when
/// the source gave it no end: the clamp floor on the line's own word pace.
const LAST_WORD_MIN_MS: u32 = 250;
/// Longest a line's last word may be guessed to last when the source gave it
/// no end — a held note sings out, a sheet's long gap to the next line does not.
const LAST_WORD_MAX_MS: u32 = 1_500;
/// A one-word line with no end stamp sings for this long.
const LAST_WORD_SOLO_MS: u32 = 800;

impl LrcLine {
    /// The window `(start_ms, end_ms)` in which word `index` is sung, with
    /// `end_ms >= start_ms`. A word ends at its explicit end when the source
    /// gave one, cut at the next word's start so windows never overlap, else
    /// at the next word's start. The LAST word ends at its explicit end only
    /// when that end falls SHORT of `line_end_ms` (the next line's start):
    /// Navidrome fills a last cue's missing end with exactly the next line's
    /// start, which would hold the word lit through the whole gap, so an end
    /// at or past the line's end counts as untimed. An untimed last word
    /// ends at the line's own word pace (the mean start-to-start gap, clamped
    /// to [`LAST_WORD_MIN_MS`]..=[`LAST_WORD_MAX_MS`]; [`LAST_WORD_SOLO_MS`]
    /// for a one-word line), never past the line's end when that is known.
    /// `None` when `index` names no word.
    pub fn word_window(&self, index: usize, line_end_ms: Option<u32>) -> Option<(u32, u32)> {
        let word = self.words.get(index)?;
        let start = word.start_ms;
        let end = match self.words.get(index + 1) {
            Some(next) => word.end_ms.map_or(next.start_ms, |e| e.min(next.start_ms)),
            None => {
                let ceiling = line_end_ms.unwrap_or(u32::MAX);
                match word.end_ms {
                    Some(explicit) if explicit < ceiling => explicit,
                    Some(_) | None => {
                        let n = self.words.len() as u32;
                        let pace = if n >= 2 {
                            let first = self.words.first().map_or(start, |w| w.start_ms);
                            (start.saturating_sub(first) / (n - 1))
                                .clamp(LAST_WORD_MIN_MS, LAST_WORD_MAX_MS)
                        } else {
                            LAST_WORD_SOLO_MS
                        };
                        start.saturating_add(pace).min(ceiling)
                    }
                }
            }
        };
        Some((start, end.max(start)))
    }

    /// Where `position_ms` sits among this line's words (pure — a seek is just
    /// a different position). Words are walked in text order, the karaoke
    /// order: a word is sung once the playhead has passed its window's end,
    /// current while inside it, and the walk stops at the first word still
    /// ahead. A line with no word timing yields the default cursor.
    pub fn word_cursor(&self, line_end_ms: Option<u32>, position_ms: u32) -> WordCursor {
        let mut cursor = WordCursor::default();
        for index in 0..self.words.len() {
            let Some((start, end)) = self.word_window(index, line_end_ms) else {
                break;
            };
            if position_ms >= end {
                cursor.sung = index + 1;
                continue;
            }
            if position_ms >= start {
                let fraction = (position_ms - start) as f32 / (end - start) as f32;
                cursor.current = Some((index, fraction));
            }
            break;
        }
        cursor
    }
}

/// A parsed lyrics document. `synced` is false when the source carried no
/// timestamps at all (plain lyrics) — every `time_ms` is then 0 and the sheet
/// drifts with playback progress instead of following a line cursor, so
/// [`LrcDocument::active_line_at`] never names a line of it.
///
/// Plain docs come from the SERVER only: `parse()` still keeps timed lines
/// alone, so the store and cached-LRCLIB channels stay synced-only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LrcDocument {
    pub lines: Vec<LrcLine>,
    pub synced: bool,
}

impl LrcDocument {
    /// Whether this document is worth rendering: it has at least one line.
    /// The single definition of "success" shared by the resolve chain's channel
    /// predicates and the UI's apply path — an empty doc (a header-only file, a
    /// line-less server entry) is a no-match whatever `synced` says, so it
    /// neither ends the chain early nor renders as a blank sheet.
    ///
    /// Timing is a SEPARATE requirement, spelled out by the channels that need
    /// it (`doc.synced && doc.is_renderable()`) rather than folded in here.
    pub fn is_renderable(&self) -> bool {
        !self.lines.is_empty()
    }

    /// Index of the line playing at `position_ms`: the last line whose
    /// timestamp is `<= position_ms`. `None` before the first timestamp
    /// (pre-roll — no line is active yet) and ALWAYS for a plain doc: every
    /// line of one carries `time_ms == 0`, so the search would name its LAST
    /// line from the first tick. O(log n); relies on `parse()` and
    /// `from_structured` having sorted a synced doc's lines by time.
    pub fn active_line_at(&self, position_ms: u32) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let reached = self.lines.partition_point(|l| l.time_ms <= position_ms);
        (reached > 0).then(|| reached - 1)
    }

    /// When line `index` gives way to the next: see [`line_end_ms`].
    pub fn line_end_ms(&self, index: usize) -> Option<u32> {
        line_end_ms(&self.lines, self.synced, index)
    }
}

/// When line `index` of a sheet gives way to the next: the next line's stamp,
/// the ceiling [`LrcLine::word_cursor`] takes for the line's last word. `None`
/// for the last line, and for every line of a plain sheet (`synced == false`,
/// whose stamps are all zero). The one definition, shared by the document and
/// the viewport, which holds the lines without the document.
pub fn line_end_ms(lines: &[LrcLine], synced: bool, index: usize) -> Option<u32> {
    if !synced {
        return None;
    }
    lines.get(index + 1).map(|next| next.time_ms)
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
    // The fraction is optional: `mm:ss`, `mm:ss.xx`, and `mm:ss:xx` are all
    // valid real-world stamps (a fractionless `[03:21]` file must still parse
    // into timed lines, not a synced-but-empty doc).
    let (seconds, fractions) = rest
        .split_once('.')
        .or_else(|| rest.split_once(':'))
        .unwrap_or((rest, ""));

    // The fraction must be ASCII digits. Rejecting non-ASCII here (full-width
    // digits, accented chars) is what keeps the byte-slice below from panicking
    // mid-character on CJK corpora — a bad stamp skips its line, never crashes.
    if !fractions.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fractions = &fractions[..3.min(fractions.len())];
    let minutes: u64 = minutes.parse().ok()?;
    let seconds: u64 = seconds.parse().ok()?;
    let frac_value: u64 = if fractions.is_empty() {
        0
    } else {
        fractions.parse().ok()?
    };

    // Checked arithmetic: an 18-digit minutes value parses fine as u64 but
    // overflows the multiply — a debug-build panic reachable from any store
    // file or LRCLIB response. Overflow = malformed stamp = None.
    let scale = 10u64.pow(3 - u32::try_from(fractions.len()).unwrap_or(3));
    let ms = minutes
        .checked_mul(60_000)?
        .checked_add(seconds.checked_mul(1000)?)?
        .checked_add(frac_value * scale)?;

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

/// Parse a `[length: mm:ss]` value into milliseconds. Checked `u64` math the
/// whole way: a hostile 18-digit minutes value parses as `u64` but would
/// overflow an unchecked multiply (a debug-build panic during the boot index
/// walk). Overflow or an out-of-`u32` total yields `None`.
fn parse_length_ms(value: &str) -> Option<u32> {
    let (minutes, seconds) = value.trim().split_once(':')?;
    let minutes: u64 = minutes.trim().parse().ok()?;
    let seconds: u64 = seconds.trim().parse().ok()?;
    let ms = minutes
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(1000)?;
    u32::try_from(ms).ok()
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

        // A line under several stamps (`[00:10.00][01:10.00]<00:10.10>la…`) is
        // sung once per stamp; its word markers belong to the FIRST stamp, so
        // each later copy carries them shifted by its own distance from it.
        let first_ms = timestamps
            .first()
            .and_then(|ts| parse_timestamp(ts, offset))
            .map(i64::from);
        for ts in &timestamps {
            if let Some(time_ms) = parse_timestamp(ts, offset) {
                let shift = first_ms.map_or(0, |first| i64::from(time_ms) - first);
                lines.push(LrcLine {
                    time_ms,
                    text: text.clone(),
                    words: words.iter().map(|w| w.shifted(shift)).collect(),
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
                        end_ms: None,
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
    // A bare stamp closing the line (`... <00:01.60>world <00:02.00>`) is the
    // moment the last word stops, not an empty word: it becomes that word's
    // end. Any other empty marker (two stamps back to back) is dropped below.
    if words.len() >= 2
        && words.last().is_some_and(|w| w.text.is_empty())
        && let Some(closing) = words.pop()
        && let Some(last) = words.last_mut()
    {
        last.end_ms = Some(closing.start_ms);
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
    /// The cue's `end`. Navidrome fills it for every cue (the next cue's
    /// start; the next line's start for a line's last cue) once any cue of
    /// the line carries one, so it is rarely the source's own.
    pub end_ms: Option<u32>,
    pub value: String,
}

/// Choose one entry from the server's `structuredLyrics` list.
///
/// Navidrome never de-duplicates: its own end-to-end test asserts that a
/// single MP3 yields FOUR entries (a `USLT` and a `SYLT` frame, each under two
/// language tags). So this ranks rather than takes the first.
/// Rank rather than take the first: main+synced, then any synced, then
/// main+unsynced, then any unsynced. Entries with no lines are skipped at every
/// tier (picking one would end the resolve chain on a blank sheet), and list
/// order breaks ties within a tier.
pub fn pick_structured(list: &[StructuredLyrics]) -> Option<&StructuredLyrics> {
    let has_lines = |s: &&StructuredLyrics| !s.lines.is_empty();
    let main = |s: &&StructuredLyrics| is_main_kind(s.kind.as_deref());
    list.iter()
        .find(|s| has_lines(s) && s.synced && main(s))
        .or_else(|| list.iter().find(|s| has_lines(s) && s.synced))
        .or_else(|| list.iter().find(|s| has_lines(s) && main(s)))
        .or_else(|| list.iter().find(has_lines))
}

/// Whether an entry's `kind` names the MAIN lyric layer. Blank or missing
/// counts as main: `kind` is only populated with `enhanced=true`, and the other
/// values (`translation`, `pronunciation`) come only from TTML sources.
fn is_main_kind(kind: Option<&str>) -> bool {
    kind.is_none_or(|k| {
        let k = k.trim();
        k.is_empty() || k.eq_ignore_ascii_case("main")
    })
}

impl LrcDocument {
    /// Convert one structured entry into the internal document. Word timings
    /// come from the matching `cueLine` (joined by `.index`); each word's text
    /// is the cue's own `value` (never a byte-slice — the server's byteStart/
    /// End are inclusive, line-relative, and off-by-one against a Rust range).
    /// A cue with no text (Navidrome keeps the space between two stamps as a
    /// cue of its own) is no word: its start becomes the previous word's end
    /// when that word had none, exactly as the parser's trailing stamp.
    pub fn from_structured(s: &StructuredLyrics) -> LrcDocument {
        let offset = s.offset_ms;
        let mut lines: Vec<LrcLine> = s
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                // A plain entry has no timing at all: pin every stamp to 0 (a
                // stray `start` on one line is noise, not a cue) and drop the
                // word layer, which would have nothing to time against.
                let (time_ms, words) = if s.synced {
                    let words = s
                        .cue_lines
                        .iter()
                        .find(|cl| cl.index == i)
                        .map(|cl| {
                            cl.cues
                                .iter()
                                .fold(Vec::new(), |mut words: Vec<WordSpan>, cue| {
                                    let start_ms = apply_offset(cue.start_ms, offset);
                                    if cue.value.trim().is_empty() {
                                        if let Some(last) = words.last_mut()
                                            && last.end_ms.is_none()
                                        {
                                            last.end_ms = Some(start_ms);
                                        }
                                    } else {
                                        words.push(WordSpan {
                                            start_ms,
                                            end_ms: cue.end_ms.map(|end| apply_offset(end, offset)),
                                            text: cue.value.clone(),
                                        });
                                    }
                                    words
                                })
                        })
                        .unwrap_or_default();
                    (apply_offset(line.start_ms.unwrap_or(0), offset), words)
                } else {
                    (0, Vec::new())
                };
                LrcLine {
                    time_ms,
                    text: line.value.clone(),
                    words,
                }
            })
            .collect();

        // A SYNCED entry's line order isn't guaranteed monotonic, and a line
        // with no `start` (optional in the OpenSubsonic schema) defaults to
        // time 0 mid-document — either breaks the sorted invariant
        // `active_line_at`'s binary search relies on. Stable-sort by time,
        // exactly as `parse()`. A plain entry is left alone: the server's order
        // IS the reading order, and every stamp is 0 anyway.
        if s.synced {
            lines.sort_by_key(|l| l.time_ms);
        }
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

    /// Strip a trailing ` - feat. ...` suffix (ported from firmium). The marker
    /// index comes from `to_lowercase()`, whose byte length can differ from the
    /// original (`İ`→`i̇` grows, `ẞ`→`ß` shrinks), so the slice is taken from the
    /// SAME lowercased string — mixing them mis-cuts or panics mid-character.
    /// The Tier-2 key reduces this through `casefold` anyway, so losing the
    /// original case here is harmless.
    fn strip_feat_suffix(title: &str) -> String {
        let lower = title.to_lowercase();
        for marker in [" - feat.", " - feat ", " - featuring"] {
            if let Some(idx) = lower.find(marker) {
                return lower[..idx].trim_end().to_string();
            }
        }
        lower
    }

    /// Keep the primary artist, dropping ` feat`/` ft`/`/` runs (ported). Slices
    /// the lowercased string the index was found in — see `strip_feat_suffix`.
    fn primary_artist(artist: &str) -> String {
        let lower = artist.to_lowercase();
        let mut cut = lower.len();
        for marker in [" feat.", " feat ", " featuring ", " ft.", " ft ", "/"] {
            if let Some(idx) = lower.find(marker) {
                cut = cut.min(idx);
            }
        }
        lower[..cut].trim().to_string()
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

/// Where an indexed `.lrc` came from. The user's own files always outrank
/// LRCLIB downloads, which `cache_to_store` writes into the store's ROOT-level
/// `.cache/` directory — so a user who drops a hand-made sheet next to a
/// cached one gets theirs, whatever `[length:]` either file carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsOrigin {
    /// Anything the user put in the lyrics dir outside the root `.cache/`.
    User,
    /// An LRCLIB download written by `cache_to_store`.
    Cache,
}

/// One indexed `.lrc` file: its path, header metadata, and Tier-1 identity.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub path: PathBuf,
    pub meta: LrcMetadata,
    tier1: (String, String),
}

/// The two matching tiers for ONE origin. Tier 1 is strict (casefold-only);
/// Tier 2 is the self-guarding loose recovery tier.
#[derive(Debug, Default)]
struct TierMaps {
    tier1: HashMap<(String, String), Vec<IndexEntry>>,
    tier2: HashMap<(String, String), Vec<IndexEntry>>,
}

impl TierMaps {
    /// Tier 1: strict. Resolve among the entries under the exact casefolded
    /// key, or `None` when this origin doesn't hold it.
    fn find_strict(
        &self,
        key1: &(String, String),
        album: Option<&str>,
        length_ms: Option<u32>,
    ) -> Option<&IndexEntry> {
        resolve(self.tier1.get(key1)?, album, length_ms)
    }

    /// Whether this origin holds the strict key at all, regardless of whether
    /// `resolve` can pick a winner among its entries.
    fn has_strict(&self, key1: &(String, String)) -> bool {
        self.tier1.contains_key(key1)
    }

    /// Tier 2: the self-guarding loose recovery tier. Accepts iff every entry
    /// under the loose key shares one strict identity; >= 2 distinct
    /// identities refuse rather than guess — a wrong sheet is worse than none.
    fn find_loose(
        &self,
        artist: &str,
        title: &str,
        album: Option<&str>,
        length_ms: Option<u32>,
    ) -> Option<&IndexEntry> {
        let key2 = normalize::tier2_key(artist, title);
        let entries = self.tier2.get(&key2)?;
        let first = &entries.first()?.tier1;
        entries
            .iter()
            .all(|e| &e.tier1 == first)
            .then(|| resolve(entries, album, length_ms))
            .flatten()
    }

    fn insert(&mut self, path: PathBuf, meta: LrcMetadata, artist: &str, title: &str) {
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

    fn len(&self) -> usize {
        self.tier1.values().map(Vec::len).sum()
    }
}

/// In-memory index of the lyrics store, split by [`LyricsOrigin`]. The two
/// origins keep SEPARATE tier maps, because Tier-1's "a present strict key
/// commits here" rule is absolute: one shared bucket let a cached LRCLIB file
/// answer a lookup the user's own file should have won (the `[length:]` rule
/// in `resolve` drops every entry without the tag, and only cache files carry
/// one). Separate maps make that miss impossible by construction.
#[derive(Debug, Default)]
pub struct LyricsIndex {
    user: TierMaps,
    cache: TierMaps,
}

impl LyricsIndex {
    /// Find the best `.lrc` the USER placed in the store (anything outside the
    /// root-level `.cache/`).
    pub fn find_user(
        &self,
        artist: &str,
        title: &str,
        album: Option<&str>,
        length_ms: Option<u32>,
    ) -> Option<&IndexEntry> {
        self.find_in(&self.user, &self.cache, artist, title, album, length_ms)
    }

    /// Find the best previously-downloaded LRCLIB sheet (root `.cache/` only).
    pub fn find_cached(
        &self,
        artist: &str,
        title: &str,
        album: Option<&str>,
        length_ms: Option<u32>,
    ) -> Option<&IndexEntry> {
        self.find_in(&self.cache, &self.user, artist, title, album, length_ms)
    }

    /// Look the track up in `own`, with `other` consulted for ONE thing: does
    /// the exact strict key exist anywhere?
    ///
    /// Tier-1 supremacy is corpus-wide, not per-origin. Splitting the index
    /// halved Tier-2's field of view, and a loose match is only safe while no
    /// exact one exists: with `Song (Live).lrc` in the user's folder and the
    /// studio sheet in `.cache/`, playing the studio track misses the user's
    /// Tier-1 key, and Tier-2 — which strips the `(Live)` qualifier — would
    /// hand back the live sheet for the studio recording. So a strict key held
    /// by the OTHER origin ends this lookup at `None`, exactly as a strict key
    /// held by this one commits it to Tier 1. The channel order then lets the
    /// origin that actually owns the key answer.
    fn find_in<'a>(
        &self,
        own: &'a TierMaps,
        other: &TierMaps,
        artist: &str,
        title: &str,
        album: Option<&str>,
        length_ms: Option<u32>,
    ) -> Option<&'a IndexEntry> {
        let key1 = normalize::tier1_key(artist, title);
        if own.has_strict(&key1) {
            // An ambiguous strict collision refuses rather than loosening.
            return own.find_strict(&key1, album, length_ms);
        }
        if other.has_strict(&key1) {
            return None;
        }
        own.find_loose(artist, title, album, length_ms)
    }

    /// Insert a parsed entry into its origin's tiers. Skips entries with an
    /// empty artist/title header.
    fn insert(&mut self, path: PathBuf, meta: LrcMetadata, origin: LyricsOrigin) {
        let (Some(artist), Some(title)) = (meta.artist.as_deref(), meta.title.as_deref()) else {
            return;
        };
        if artist.is_empty() || title.is_empty() {
            return;
        }
        let (artist, title) = (artist.to_string(), title.to_string());
        match origin {
            LyricsOrigin::User => self.user.insert(path, meta, &artist, &title),
            LyricsOrigin::Cache => self.cache.insert(path, meta, &artist, &title),
        }
    }

    /// Number of indexed files across both origins (counted once, via Tier 1).
    pub fn len(&self) -> usize {
        self.user.len() + self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

/// The directory name `cache_to_store` writes LRCLIB downloads into. Only the
/// store root's own child by this name is the cache — a user folder deeper in
/// the tree that happens to be called `.cache` holds the user's own files.
const CACHE_DIR_NAME: &str = ".cache";

/// Walk `dir` recursively, index every readable `*.lrc` by its internal tags
/// AND its [`LyricsOrigin`], and log the count. Bad files (unreadable,
/// non-UTF-8, header-empty) are skipped so one broken file can't abort the
/// scan. The origin is decided while walking: the root's direct child named
/// `.cache` flips to [`LyricsOrigin::Cache`], and everything below it inherits.
pub async fn build_index(dir: PathBuf) -> LyricsIndex {
    let mut index = LyricsIndex::default();
    // (dir, origin of its files, whether this dir IS the store root)
    let mut stack = vec![(dir, LyricsOrigin::User, true)];

    while let Some((current, origin, is_root)) = stack.pop() {
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
                let is_cache_root =
                    is_root && path.file_name().and_then(|n| n.to_str()) == Some(CACHE_DIR_NAME);
                let child_origin = if is_cache_root {
                    LyricsOrigin::Cache
                } else {
                    origin
                };
                stack.push((path, child_origin, false));
            } else if path.extension().and_then(|e| e.to_str()) == Some("lrc") {
                let Ok(content) = tokio::fs::read_to_string(&path).await else {
                    continue;
                };
                index.insert(path, read_metadata(&content), origin);
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

    fn doc_at(times_ms: &[u32], synced: bool) -> LrcDocument {
        LrcDocument {
            lines: times_ms
                .iter()
                .map(|&time_ms| LrcLine {
                    time_ms,
                    ..Default::default()
                })
                .collect(),
            synced,
        }
    }

    // --- active line ---

    #[test]
    fn active_line_pre_roll_is_none() {
        let doc = doc_at(&[5_000, 10_000], true);
        assert_eq!(doc.active_line_at(0), None);
        assert_eq!(doc.active_line_at(4_999), None);
        assert_eq!(doc.active_line_at(5_000), Some(0));
        assert_eq!(doc.active_line_at(9_999), Some(0));
        assert_eq!(doc.active_line_at(10_000), Some(1));
    }

    #[test]
    fn a_plain_doc_has_no_active_line() {
        // Every stamp is 0, so a time search would name the LAST line.
        let doc = doc_at(&[0, 0, 0], false);
        for position_ms in [0, 1, 60_000, u32::MAX] {
            assert_eq!(doc.active_line_at(position_ms), None);
        }
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
        // No timestamps anywhere → not synced, no lines. This pins the rule
        // that plain lyrics come from the SERVER only: the `.lrc` parser (and
        // so the store and cached-LRCLIB channels) still keeps timed lines
        // alone, so an untimed file in the lyrics dir stays a no-match.
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
    fn is_renderable_means_has_lines() {
        let line = || LrcLine {
            time_ms: 0,
            text: "x".into(),
            words: vec![],
        };
        // Empty is a no-match whatever `synced` says — a synced-but-empty doc
        // (a timestamp-shaped but unparseable stamp yields one) must not end
        // the resolve chain, and neither must a plain doc with nothing in it.
        assert!(
            !LrcDocument {
                lines: vec![],
                synced: true,
            }
            .is_renderable()
        );
        assert!(
            !LrcDocument {
                lines: vec![],
                synced: false,
            }
            .is_renderable()
        );
        // Plain lyrics with text ARE renderable: they drift with playback
        // rather than following a line cursor.
        assert!(
            LrcDocument {
                lines: vec![line()],
                synced: false,
            }
            .is_renderable()
        );
        assert!(
            LrcDocument {
                lines: vec![line()],
                synced: true,
            }
            .is_renderable()
        );
    }

    #[test]
    fn fractionless_timestamp_parses() {
        // A real-world `[mm:ss]` file (no centisecond fraction) must yield
        // timed lines, not a synced-but-empty doc that ends the resolve chain.
        let doc = parse("[ar:A]\n[ti:T]\n[03:21]line one\n[03:25]line two");
        assert!(doc.synced);
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0].time_ms, 201_000);
        assert_eq!(doc.lines[1].time_ms, 205_000);
    }

    #[test]
    fn multibyte_fraction_skips_line_without_panic() {
        // Full-width digits in the fraction (common in CJK corpora) must not
        // byte-slice mid-character; the bad line is skipped, valid ones remain.
        let doc = parse("[00:10.１２]bad\n[00:20.50]good");
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].time_ms, 20_500);
    }

    #[test]
    fn length_header_overflow_is_none_not_panic() {
        // A hostile 5-digit minutes value overflows u32 mid-multiply if the
        // math isn't done in u64; read_metadata must not panic at index time.
        let meta = read_metadata("[length:9999999:00]\n[00:01.00]a");
        assert_eq!(meta.length_ms, None);
        let ok = read_metadata("[length:03:21]\n[00:01.00]a");
        assert_eq!(ok.length_ms, Some(201_000));
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
                end_ms: None,
                text: "Hello".into()
            }
        );
        assert_eq!(
            doc.lines[0].words[1],
            WordSpan {
                start_ms: 1_600,
                end_ms: None,
                text: "world".into()
            }
        );
    }

    #[test]
    fn trailing_word_stamp_ends_the_last_word() {
        // Enhanced LRC may close a line with a bare `<mm:ss.xx>`: the moment
        // the last word stops. It is an END, never an empty word, and never
        // reaches the display text.
        let doc = parse("[00:01.00]<00:01.10>Hello <00:01.60>world <00:02.00>");
        assert_eq!(doc.lines[0].text, "Hello world");
        assert_eq!(doc.lines[0].words.len(), 2);
        assert_eq!(doc.lines[0].words[0].end_ms, None);
        assert_eq!(doc.lines[0].words[1].end_ms, Some(2_000));

        // The one-word form, and a trailing stamp under an [offset:].
        let solo = parse("[00:01.00]<00:01.10>Hello<00:01.60>");
        assert_eq!(solo.lines[0].words.len(), 1);
        assert_eq!(solo.lines[0].words[0].text, "Hello");
        assert_eq!(solo.lines[0].words[0].end_ms, Some(1_600));
        let shifted = parse("[offset:+100]\n[00:01.00]<00:01.10>Hello <00:01.60>");
        assert_eq!(shifted.lines[0].words[0].start_ms, 1_000);
        assert_eq!(shifted.lines[0].words[0].end_ms, Some(1_500));

        // An empty marker that is NOT last (two stamps back to back) is still
        // dropped: only the closing stamp means an end.
        let doubled = parse("[00:01.00]<00:01.10><00:01.20>Hello <00:01.60>world");
        assert_eq!(doubled.lines[0].words.len(), 2);
        assert_eq!(doubled.lines[0].words[0].start_ms, 1_200);
        assert_eq!(doubled.lines[0].words[1].end_ms, None);
    }

    // --- word_window / word_cursor ---

    fn worded(starts: &[u32]) -> LrcLine {
        LrcLine {
            time_ms: starts.first().copied().unwrap_or(0),
            text: starts
                .iter()
                .map(|s| format!("w{s}"))
                .collect::<Vec<_>>()
                .join(" "),
            words: starts
                .iter()
                .map(|&start_ms| WordSpan {
                    start_ms,
                    end_ms: None,
                    text: format!("w{start_ms}"),
                })
                .collect(),
        }
    }

    #[test]
    fn word_window_ends_at_the_next_word_or_the_explicit_end() {
        let mut line = worded(&[1_000, 1_500, 2_000]);
        assert_eq!(line.word_window(0, None), Some((1_000, 1_500)));
        assert_eq!(line.word_window(1, None), Some((1_500, 2_000)));
        assert_eq!(line.word_window(3, None), None, "no fourth word");

        // An explicit end that stops short of the next word leaves a gap; one
        // that runs past it is cut at the next word's start (no overlap).
        line.words[0].end_ms = Some(1_300);
        assert_eq!(line.word_window(0, None), Some((1_000, 1_300)));
        line.words[0].end_ms = Some(1_900);
        assert_eq!(line.word_window(0, None), Some((1_000, 1_500)));
        // An end before its own start collapses to an instant, never inverts.
        line.words[0].end_ms = Some(900);
        assert_eq!(line.word_window(0, None), Some((1_000, 1_000)));
    }

    #[test]
    fn last_word_window_follows_the_line_pace_under_the_line_end() {
        // Pace 500 ms (two gaps of 500) → the last word sings 2000..2500.
        let line = worded(&[1_000, 1_500, 2_000]);
        assert_eq!(line.word_window(2, None), Some((2_000, 2_500)));
        assert_eq!(line.word_window(2, Some(9_000)), Some((2_000, 2_500)));
        // The next line arriving sooner caps it.
        assert_eq!(line.word_window(2, Some(2_200)), Some((2_000, 2_200)));
        // A next line already past the start makes it an instant.
        assert_eq!(line.word_window(2, Some(1_900)), Some((2_000, 2_000)));

        // The pace is clamped: a crawl doesn't hold the last word for ages
        // and a rapid-fire line still gives it a readable moment.
        let slow = worded(&[0, 5_000, 10_000]);
        assert_eq!(slow.word_window(2, None), Some((10_000, 10_000 + 1_500)));
        let fast = worded(&[0, 50, 100]);
        assert_eq!(fast.word_window(2, None), Some((100, 100 + 250)));

        // A one-word line has no pace to read: the solo length applies.
        let solo = worded(&[3_000]);
        assert_eq!(solo.word_window(0, None), Some((3_000, 3_800)));
        assert_eq!(solo.word_window(0, Some(3_200)), Some((3_000, 3_200)));

        // An explicit end on the last word beats the guess while it falls
        // short of the line's end.
        let mut ended = worded(&[1_000, 1_500, 2_000]);
        ended.words[2].end_ms = Some(3_000);
        assert_eq!(ended.word_window(2, None), Some((2_000, 3_000)));
        assert_eq!(ended.word_window(2, Some(4_000)), Some((2_000, 3_000)));
        // Navidrome fills a last cue's missing end with the NEXT LINE's start:
        // an end at (or past) the line's end is that fill, not a timing, so
        // the pace guess applies instead of holding the word through the gap.
        assert_eq!(ended.word_window(2, Some(3_000)), Some((2_000, 2_500)));
        assert_eq!(ended.word_window(2, Some(2_600)), Some((2_000, 2_500)));
        // The guess itself still never passes the line's end.
        assert_eq!(ended.word_window(2, Some(2_200)), Some((2_000, 2_200)));
    }

    #[test]
    fn repeated_stamps_shift_the_words_with_each_copy() {
        let doc = parse("[00:10.00][01:10.00]<00:10.10>la <00:10.60>la <00:11.00>");
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0].time_ms, 10_000);
        assert_eq!(doc.lines[0].words[0].start_ms, 10_100);
        assert_eq!(doc.lines[0].words[1].end_ms, Some(11_000));
        // The chorus repeat carries its own word times, a minute later.
        assert_eq!(doc.lines[1].time_ms, 70_000);
        assert_eq!(doc.lines[1].words[0].start_ms, 70_100);
        assert_eq!(doc.lines[1].words[1].start_ms, 70_600);
        assert_eq!(doc.lines[1].words[1].end_ms, Some(71_000));
        assert_eq!(doc.lines[1].text, "la la");
    }

    #[test]
    fn from_structured_folds_a_textless_cue_into_the_previous_word_end() {
        // Navidrome keeps the space between `<a>Hello<b> <c>world` as a cue
        // of its own; it is the moment "Hello" ended, never a word.
        let s = StructuredLyrics {
            synced: true,
            offset_ms: 0,
            kind: None,
            lines: vec![StructuredLine {
                start_ms: Some(1_000),
                value: "Hello world".into(),
            }],
            cue_lines: vec![StructuredCueLine {
                index: 0,
                agent_id: None,
                cues: vec![
                    StructuredCue {
                        start_ms: 1_000,
                        end_ms: None,
                        value: "Hello".into(),
                    },
                    StructuredCue {
                        start_ms: 1_400,
                        end_ms: None,
                        value: " ".into(),
                    },
                    StructuredCue {
                        start_ms: 1_600,
                        end_ms: None,
                        value: "world".into(),
                    },
                    StructuredCue {
                        start_ms: 2_000,
                        end_ms: None,
                        value: String::new(),
                    },
                ],
            }],
        };
        let doc = LrcDocument::from_structured(&s);
        let words = &doc.lines[0].words;
        assert_eq!(words.len(), 2);
        assert_eq!(
            (words[0].text.as_str(), words[0].end_ms),
            ("Hello", Some(1_400))
        );
        assert_eq!(
            (words[1].text.as_str(), words[1].end_ms),
            ("world", Some(2_000))
        );
        // A word that already has an end keeps it.
        let mut kept = s.clone();
        kept.cue_lines[0].cues[0].end_ms = Some(1_300);
        let doc = LrcDocument::from_structured(&kept);
        assert_eq!(doc.lines[0].words[0].end_ms, Some(1_300));
    }

    #[test]
    fn word_cursor_walks_the_line() {
        let line = worded(&[1_000, 1_500, 2_000]);
        let at = |pos| line.word_cursor(Some(5_000), pos);

        // Pre-roll: nothing sung, nothing current.
        assert_eq!(at(900), WordCursor::default());
        // Exactly on the first word's start: current at 0.0.
        assert_eq!(
            at(1_000),
            WordCursor {
                sung: 0,
                current: Some((0, 0.0)),
            }
        );
        // A quarter into the first word.
        assert_eq!(
            at(1_125),
            WordCursor {
                sung: 0,
                current: Some((0, 0.25)),
            }
        );
        // Into the second word: the first is sung.
        assert_eq!(
            at(1_750),
            WordCursor {
                sung: 1,
                current: Some((1, 0.5)),
            }
        );
        // Half through the last word (2000..2500 by pace).
        assert_eq!(
            at(2_250),
            WordCursor {
                sung: 2,
                current: Some((2, 0.5)),
            }
        );
        // Past the last word's end: everything sung, nothing current.
        assert_eq!(
            at(2_500),
            WordCursor {
                sung: 3,
                current: None,
            }
        );
        assert_eq!(
            at(4_999),
            WordCursor {
                sung: 3,
                current: None,
            }
        );
        // A seek back is just an earlier position: recomputed from scratch.
        assert_eq!(at(1_125).current, Some((0, 0.25)));
    }

    #[test]
    fn word_cursor_rests_in_a_gap_and_skips_instants() {
        let mut line = worded(&[1_000, 1_500, 2_000]);
        // The first word stops at 1300; 1300..1500 is a gap: sung, not current.
        line.words[0].end_ms = Some(1_300);
        assert_eq!(
            line.word_cursor(None, 1_400),
            WordCursor {
                sung: 1,
                current: None,
            }
        );
        // An instant word (end == start) counts as sung the moment it starts,
        // and never divides by its zero length; the word after it is still
        // ahead until its own start.
        line.words[1].end_ms = Some(1_500);
        assert_eq!(
            line.word_cursor(None, 1_500),
            WordCursor {
                sung: 2,
                current: None,
            }
        );
        assert_eq!(
            line.word_cursor(None, 2_000),
            WordCursor {
                sung: 2,
                current: Some((2, 0.0)),
            }
        );
    }

    #[test]
    fn word_cursor_of_an_unworded_line_is_the_default() {
        let line = LrcLine {
            time_ms: 1_000,
            text: "no word timing".into(),
            words: vec![],
        };
        assert_eq!(line.word_cursor(Some(2_000), 1_500), WordCursor::default());
        assert_eq!(line.word_window(0, None), None);
    }

    #[test]
    fn line_end_is_the_next_stamp_of_a_synced_doc() {
        let doc = parse("[00:01.00]one\n[00:02.00]two");
        assert_eq!(doc.line_end_ms(0), Some(2_000));
        assert_eq!(doc.line_end_ms(1), None, "the last line has no end");
        assert_eq!(doc.line_end_ms(7), None);
        // The free function the viewport calls is the same rule.
        assert_eq!(line_end_ms(&doc.lines, true, 0), Some(2_000));
        assert_eq!(line_end_ms(&doc.lines, false, 0), None);
        // A plain doc's stamps are all zero: no line of it has an end.
        let plain = LrcDocument {
            lines: vec![LrcLine::default(), LrcLine::default()],
            synced: false,
        };
        assert_eq!(plain.line_end_ms(0), None);
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
            index.insert(path, meta, LyricsOrigin::User);
        }
        index
    }

    #[test]
    fn tier1_exact_artist_title_matches() {
        let index = index_of(vec![entry("a.lrc", "Beach House", "Myth", Some("Bloom"))]);
        let hit = index.find_user("beach house", "MYTH", Some("bloom"), None);
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
                .find_user(
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
                .find_user("Air", "La Femme d'Argent", Some("Moon Safari"), None)
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
                .find_user("X", "Song", Some("second album"), None)
                .map(|e| e.path.as_path()),
            Some(PathBuf::from("two.lrc").as_path())
        );
    }

    #[test]
    fn tier2_recovers_mr_dot_drift() {
        let index = index_of(vec![entry("a.lrc", "Artist", "Mr.", Some("Album"))]);
        // Library sends "Mr" (no dot). Tier-1 misses; Tier-2 reduce folds them.
        assert!(
            index
                .find_user("Artist", "Mr", Some("Album"), None)
                .is_some()
        );
    }

    #[test]
    fn tier2_collision_refuses() {
        // Two distinct real titles collapsing to one loose key must refuse.
        let index = index_of(vec![
            entry("a.lrc", "X", "Go!", Some("A")),
            entry("b.lrc", "X", "Go?", Some("B")),
        ]);
        // "Go." reduces to "go" like both; distinct Tier-1 identities → refuse.
        assert!(index.find_user("X", "Go.", Some("C"), None).is_none());
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
        let first = index.find_user("Afsky", "Altid Veltilfreds", Some("Ofte jeg drømmer"), None);
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
                        end_ms: Some(1_400),
                        value: "Hello".into(),
                    },
                    StructuredCue {
                        start_ms: 1_500,
                        end_ms: None,
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
        // A cue's end rides along; a missing one stays unknown.
        assert_eq!(doc.lines[0].words[0].end_ms, Some(1_400));
        assert_eq!(doc.lines[0].words[1].end_ms, None);
    }

    #[test]
    fn from_structured_shifts_cue_ends_by_the_offset() {
        let s = StructuredLyrics {
            synced: true,
            offset_ms: 200,
            kind: None,
            lines: vec![StructuredLine {
                start_ms: Some(1_000),
                value: "Hello".into(),
            }],
            cue_lines: vec![StructuredCueLine {
                index: 0,
                agent_id: None,
                cues: vec![StructuredCue {
                    start_ms: 1_000,
                    end_ms: Some(1_400),
                    value: "Hello".into(),
                }],
            }],
        };
        let doc = LrcDocument::from_structured(&s);
        // Positive offset shifts earlier, exactly as the line and word starts.
        assert_eq!(doc.lines[0].words[0].start_ms, 800);
        assert_eq!(doc.lines[0].words[0].end_ms, Some(1_200));
    }

    #[test]
    fn from_structured_sorts_unsorted_lines() {
        // An out-of-order structured payload (or a line missing `start` → 0)
        // must be sorted so `active_line_at`'s binary search stays valid.
        let s = StructuredLyrics {
            synced: true,
            offset_ms: 0,
            kind: Some("main".into()),
            lines: vec![
                StructuredLine {
                    start_ms: Some(3_000),
                    value: "third".into(),
                },
                StructuredLine {
                    start_ms: Some(1_000),
                    value: "first".into(),
                },
                StructuredLine {
                    start_ms: Some(2_000),
                    value: "second".into(),
                },
            ],
            cue_lines: vec![],
        };
        let doc = LrcDocument::from_structured(&s);
        let times: Vec<u32> = doc.lines.iter().map(|l| l.time_ms).collect();
        assert_eq!(times, [1_000, 2_000, 3_000]);
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn from_structured_keeps_plain_lyrics_in_server_order() {
        // Navidrome hands plain (USLT / `unsyncedlyrics`) text over as
        // `synced: false` with no `start` on any line. The server's order IS
        // the reading order, and every stamp stays 0 so nothing can mistake a
        // plain sheet for a timed one.
        let s = StructuredLyrics {
            synced: false,
            offset_ms: 0,
            kind: Some("main".into()),
            lines: vec![
                StructuredLine {
                    start_ms: None,
                    value: "[Verse 1]".into(),
                },
                StructuredLine {
                    start_ms: None,
                    value: "first".into(),
                },
                StructuredLine {
                    start_ms: None,
                    value: "second".into(),
                },
            ],
            cue_lines: vec![],
        };
        let doc = LrcDocument::from_structured(&s);
        assert!(!doc.synced);
        let texts: Vec<&str> = doc.lines.iter().map(|l| l.text.as_str()).collect();
        // Bracketed lines are lyrics here ([Chorus], [Verse 1]) — rendered
        // verbatim, never stripped.
        assert_eq!(texts, ["[Verse 1]", "first", "second"]);
        assert!(doc.lines.iter().all(|l| l.time_ms == 0));
        assert!(doc.lines.iter().all(|l| l.words.is_empty()));
    }

    #[test]
    fn from_structured_does_not_sort_a_plain_entry() {
        // Stray `start` values on an unsynced entry must not reorder it — a
        // sort would scramble the reading order for no gain (the sheet never
        // reaches `active_line_at`).
        let s = StructuredLyrics {
            synced: false,
            lines: vec![
                StructuredLine {
                    start_ms: Some(9_000),
                    value: "first".into(),
                },
                StructuredLine {
                    start_ms: None,
                    value: "second".into(),
                },
                StructuredLine {
                    start_ms: Some(1_000),
                    value: "third".into(),
                },
            ],
            ..Default::default()
        };
        let doc = LrcDocument::from_structured(&s);
        let texts: Vec<&str> = doc.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["first", "second", "third"]);
        assert!(doc.lines.iter().all(|l| l.time_ms == 0));
    }

    // --- pick_structured ---

    fn entry_of(synced: bool, kind: Option<&str>, lines: &[&str]) -> StructuredLyrics {
        StructuredLyrics {
            synced,
            offset_ms: 0,
            kind: kind.map(Into::into),
            lines: lines
                .iter()
                .map(|v| StructuredLine {
                    start_ms: synced.then_some(1_000),
                    value: (*v).to_string(),
                })
                .collect(),
            cue_lines: vec![],
        }
    }

    #[test]
    fn pick_prefers_synced_over_plain() {
        // The USLT + SYLT case: one MP3, two frames, both `main`. The timed
        // one wins whichever order the server lists them in.
        let list = vec![
            entry_of(false, Some("main"), &["plain"]),
            entry_of(true, Some("main"), &["synced"]),
        ];
        assert_eq!(
            pick_structured(&list).map(|s| s.lines[0].value.as_str()),
            Some("synced")
        );
    }

    #[test]
    fn pick_takes_any_synced_over_a_main_plain() {
        // A synced translation layer still beats plain text: timing is the
        // stronger signal, and the plain tier is the fallback.
        let list = vec![
            entry_of(false, Some("main"), &["plain main"]),
            entry_of(true, Some("translation"), &["synced translation"]),
        ];
        assert_eq!(
            pick_structured(&list).map(|s| s.lines[0].value.as_str()),
            Some("synced translation")
        );
    }

    #[test]
    fn pick_falls_back_to_plain_in_list_order() {
        // Two plain entries under different language tags: list order breaks
        // the tie (Navidrome emits them `eng`, `xxx`, in mapping order).
        let list = vec![
            entry_of(false, None, &["xxx"]),
            entry_of(false, None, &["eng"]),
        ];
        assert_eq!(
            pick_structured(&list).map(|s| s.lines[0].value.as_str()),
            Some("xxx")
        );
    }

    #[test]
    fn pick_prefers_a_main_plain_over_a_non_main_plain() {
        let list = vec![
            entry_of(false, Some("translation"), &["translated"]),
            entry_of(false, Some("main"), &["main"]),
        ];
        assert_eq!(
            pick_structured(&list).map(|s| s.lines[0].value.as_str()),
            Some("main")
        );
    }

    #[test]
    fn pick_skips_entries_with_no_lines_at_every_tier() {
        // A line-less entry is not a hit: picking it would end the resolve
        // chain on a blank sheet.
        let list = vec![
            entry_of(true, Some("main"), &[]),
            entry_of(true, None, &[]),
            entry_of(false, Some("main"), &[]),
            entry_of(false, None, &["the only real one"]),
        ];
        assert_eq!(
            pick_structured(&list).map(|s| s.lines[0].value.as_str()),
            Some("the only real one")
        );

        assert!(
            pick_structured(&[]).is_none(),
            "an empty list picks nothing"
        );
        let all_empty = vec![
            entry_of(true, Some("main"), &[]),
            entry_of(false, None, &[]),
        ];
        assert!(
            pick_structured(&all_empty).is_none(),
            "an all-empty list picks nothing"
        );
    }

    #[test]
    fn pick_treats_a_blank_or_missing_kind_as_main() {
        for kind in [None, Some(""), Some("  "), Some("MAIN")] {
            let list = vec![
                entry_of(true, Some("translation"), &["translation"]),
                entry_of(true, kind, &["main"]),
            ];
            assert_eq!(
                pick_structured(&list).map(|s| s.lines[0].value.as_str()),
                Some("main"),
                "kind {kind:?} must count as main"
            );
        }
    }

    #[test]
    fn tier2_multibyte_feat_does_not_panic() {
        // Case-folding changes byte length; the feat-strip must not mis-slice.
        let (artist, _) = normalize::tier2_key("İrem feat. Guest", "Song");
        assert!(!artist.is_empty());
        let (_, title) = normalize::tier2_key("A", "Track - feat. İX");
        assert!(!title.is_empty());
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
    async fn build_index_splits_user_files_from_the_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let artist_dir = dir.path().join("Artist");
        std::fs::create_dir_all(&artist_dir).expect("mkdir");
        std::fs::write(
            artist_dir.join("song.lrc"),
            "[ar:A]\n[ti:Mine]\n[00:01.00]hand made",
        )
        .expect("write");
        let cache_dir = dir.path().join(".cache");
        std::fs::create_dir_all(&cache_dir).expect("mkdir");
        std::fs::write(
            cache_dir.join("other.lrc"),
            "[ar:B]\n[ti:Downloaded]\n[00:01.00]from lrclib",
        )
        .expect("write");

        let index = build_index(dir.path().to_path_buf()).await;
        assert_eq!(index.len(), 2, "both files are indexed");
        // Each lookup sees only its own origin.
        assert!(index.find_user("A", "Mine", None, None).is_some());
        assert!(index.find_cached("A", "Mine", None, None).is_none());
        assert!(index.find_cached("B", "Downloaded", None, None).is_some());
        assert!(index.find_user("B", "Downloaded", None, None).is_none());
    }

    #[tokio::test]
    async fn a_user_file_beats_a_cached_copy_carrying_length() {
        // The bug this split fixes: one shared bucket put both files under the
        // same Tier-1 key, and `resolve`'s `[length:]` rule dropped every entry
        // WITHOUT the tag — which only hand-made files lack. The user's own
        // sheet lost to the LRCLIB download it was meant to replace.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("mine.lrc"),
            "[ar:Beach House]\n[ti:Myth]\n[al:Bloom]\n[00:45.47]my own words",
        )
        .expect("write");
        let cache_dir = dir.path().join(".cache");
        std::fs::create_dir_all(&cache_dir).expect("mkdir");
        std::fs::write(
            cache_dir.join("downloaded.lrc"),
            "[ar:Beach House]\n[ti:Myth]\n[al:Bloom]\n[length:04:35]\n[00:45.47]lrclib words",
        )
        .expect("write");

        let index = build_index(dir.path().to_path_buf()).await;
        let hit = index
            .find_user("Beach House", "Myth", Some("Bloom"), Some(275_000))
            .expect("the user's own file must answer the user lookup");
        assert_eq!(
            hit.path.file_name().and_then(|n| n.to_str()),
            Some("mine.lrc")
        );
    }

    #[tokio::test]
    async fn a_strict_key_in_the_cache_blocks_a_loose_user_match() {
        // Splitting the index by origin halved Tier-2's field of view. The
        // user owns a LIVE sheet; the studio sheet is an LRCLIB download. The
        // studio track misses the user's Tier-1 key, and Tier-2 strips the
        // `(live)` qualifier — so without the cross-origin strict check the
        // user lookup would hand back the live sheet for the studio take.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("live.lrc"),
            "[ar:Emperor]\n[ti:The Loss and Curse of Reverence (live)]\n[00:10.00]live take",
        )
        .expect("write");
        let cache_dir = dir.path().join(".cache");
        std::fs::create_dir_all(&cache_dir).expect("mkdir");
        std::fs::write(
            cache_dir.join("studio.lrc"),
            "[ar:Emperor]\n[ti:The Loss and Curse of Reverence]\n[al:Anthems]\n[length:06:26]\n[00:10.00]studio take",
        )
        .expect("write");

        let index = build_index(dir.path().to_path_buf()).await;
        assert!(
            index
                .find_user(
                    "Emperor",
                    "The Loss and Curse of Reverence",
                    Some("Anthems"),
                    None
                )
                .is_none(),
            "a loose user match must not outrank an exact sheet the cache holds"
        );
        // The cached channel, next in the chain, owns the strict key and answers.
        let hit = index
            .find_cached(
                "Emperor",
                "The Loss and Curse of Reverence",
                Some("Anthems"),
                None,
            )
            .expect("the cache holds the exact sheet");
        assert_eq!(
            hit.path.file_name().and_then(|n| n.to_str()),
            Some("studio.lrc")
        );

        // The live track still resolves to the user's own sheet: its strict
        // key lives in the user index, so Tier 1 commits there.
        assert!(
            index
                .find_user(
                    "Emperor",
                    "The Loss and Curse of Reverence (live)",
                    Some("Anthems"),
                    None
                )
                .is_some()
        );
    }

    #[tokio::test]
    async fn only_the_root_cache_dir_is_cache() {
        // A user folder that happens to be named `.cache` deeper in the tree
        // holds the user's own files.
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("Artist").join(".cache");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(nested.join("x.lrc"), "[ar:A]\n[ti:T]\n[00:01.00]x").expect("write");

        let index = build_index(dir.path().to_path_buf()).await;
        assert!(
            index.find_user("A", "T", None, None).is_some(),
            "a nested .cache folder is still the user's"
        );
        assert!(index.find_cached("A", "T", None, None).is_none());
    }

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
                .find_user("Beach House", "Myth", Some("Bloom"), None)
                .is_some()
        );
        assert!(index.find_user("Nonexistent", "Song", None, None).is_none());
    }
}
