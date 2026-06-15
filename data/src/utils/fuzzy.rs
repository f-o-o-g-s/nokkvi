//! Lightweight, dependency-free fuzzy subsequence matcher for the settings
//! search.
//!
//! Returns a relevance [`FuzzyMatch::score`] (higher is better) plus the
//! matched **byte ranges** in the haystack so the UI can highlight the matched
//! characters (see `render_detail_row` in the UI crate). The settings corpus is
//! ~150 short, fixed, hand-curated strings, so match *quality* — substrings and
//! word-boundary / acronym ranking — matters far more than throughput, and a
//! small scorer tuned to this exact vocabulary beats a generic library's
//! defaults. The field-weighting / global ranking that consumes these scores
//! lives in `views/settings/entries.rs`.
//!
//! The matcher is greedy (it does not search for the globally optimal
//! alignment) but pairs the greedy walk with a substring fast path, so
//! contiguous matches always score at least as well as the scattered
//! alternative the greedy walk might otherwise pick.

/// Per matched character: base reward. Exposed so the search layer can derive
/// its keep-floor relative to the needle length (see [`is_strong_score`]).
pub(crate) const MATCH_BASE: i32 = 16;

/// Extra structure a match must carry, on top of `len * MATCH_BASE`, to count
/// as "strong" — i.e. at least one consecutive run, word-start, or prefix hit.
/// Filters first-keystroke scattered-subsequence noise without hiding real
/// prefix/acronym matches.
pub(crate) const STRUCTURE_MIN: i32 = 10;

/// Bonus when a matched char immediately follows the previous matched char.
/// Rewards contiguous runs so substrings outrank scattered subsequences.
const CONSECUTIVE_BONUS: i32 = 18;
/// Bonus when the matched char starts a word — the first char, a char after a
/// non-alphanumeric separator, or a camelCase boundary. Makes acronym queries
/// like `sv` rank `Stable Viewport` highly.
const WORD_START_BONUS: i32 = 12;
/// Bonus when the match begins at the very start of the haystack.
const PREFIX_BONUS: i32 = 8;
/// Per-char penalty for haystack chars skipped before the first match, capped
/// so a late-but-clean match isn't punished into oblivion.
const LEADING_GAP_PENALTY: i32 = 1;
const LEADING_GAP_CAP: i32 = 6;

/// A successful fuzzy match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// Relevance score; higher is better. Only comparable across candidates for
    /// the *same* needle.
    pub score: i32,
    /// Coalesced, sorted, non-overlapping byte ranges of the matched characters
    /// in the haystack (for highlighting). Never empty on a successful match;
    /// every range lands on UTF-8 char boundaries.
    pub ranges: Vec<(usize, usize)>,
}

/// Pre-decoded haystack character with the metadata the scorer needs.
struct HChar {
    byte: usize,
    byte_end: usize,
    lower: char,
    word_start: bool,
}

fn lower_first(c: char) -> char {
    // First scalar of the lowercase mapping. Sufficient for the ASCII-dominant
    // settings vocabulary; multi-scalar foldings (e.g. 'ß') collapse to their
    // first scalar, which is acceptable for ranking purposes here.
    c.to_lowercase().next().unwrap_or(c)
}

fn haystack_chars(haystack: &str) -> Vec<HChar> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut prev: Option<char> = None;
    for (byte, ch) in haystack.char_indices() {
        let word_start = match prev {
            None => true,
            Some(p) => !p.is_alphanumeric() || (!p.is_uppercase() && ch.is_uppercase()),
        };
        out.push(HChar {
            byte,
            byte_end: byte + ch.len_utf8(),
            lower: lower_first(ch),
            word_start,
        });
        prev = Some(ch);
    }
    out
}

/// Score a contiguous substring occurrence (the best-scoring one).
fn substring_match(hay: &[HChar], needle: &[char]) -> Option<(i32, Vec<(usize, usize)>)> {
    let n = needle.len();
    if n == 0 || n > hay.len() {
        return None;
    }
    let mut best: Option<(i32, usize)> = None;
    'windows: for start in 0..=hay.len() - n {
        for (k, nc) in needle.iter().enumerate() {
            if hay[start + k].lower != *nc {
                continue 'windows;
            }
        }
        let mut s = 0;
        for (k, hc) in hay[start..start + n].iter().enumerate() {
            s += MATCH_BASE;
            if k > 0 {
                s += CONSECUTIVE_BONUS;
            }
            if hc.word_start {
                s += WORD_START_BONUS;
            }
            if start + k == 0 {
                s += PREFIX_BONUS;
            }
        }
        // Penalize a deep mid-word start, but never a word-boundary match: an
        // acronym/word-start hit is intentional wherever it lands.
        if !hay[start].word_start {
            s -= (start as i32).min(LEADING_GAP_CAP) * LEADING_GAP_PENALTY;
        }
        if best.is_none_or(|(bs, _)| s > bs) {
            best = Some((s, start));
        }
    }
    best.map(|(s, start)| {
        let ranges = hay[start..start + n]
            .iter()
            .map(|hc| (hc.byte, hc.byte_end))
            .collect();
        (s, ranges)
    })
}

/// Greedy left-to-right subsequence walk.
fn subsequence_match(hay: &[HChar], needle: &[char]) -> Option<(i32, Vec<(usize, usize)>)> {
    let mut ni = 0;
    let mut score = 0;
    let mut ranges = Vec::with_capacity(needle.len());
    let mut prev_idx: Option<usize> = None;
    let mut first = true;
    for (i, hc) in hay.iter().enumerate() {
        if ni >= needle.len() {
            break;
        }
        if hc.lower == needle[ni] {
            score += MATCH_BASE;
            if prev_idx == Some(i.wrapping_sub(1)) {
                score += CONSECUTIVE_BONUS;
            }
            if hc.word_start {
                score += WORD_START_BONUS;
            }
            if i == 0 {
                score += PREFIX_BONUS;
            }
            if first {
                if !hc.word_start {
                    score -= (i as i32).min(LEADING_GAP_CAP) * LEADING_GAP_PENALTY;
                }
                first = false;
            }
            ranges.push((hc.byte, hc.byte_end));
            prev_idx = Some(i);
            ni += 1;
        }
    }
    (ni == needle.len()).then_some((score, ranges))
}

/// Merge sorted per-char byte ranges into coalesced, non-overlapping ranges.
fn coalesce(mut raw: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    raw.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(raw.len());
    for (s, e) in raw {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Fuzzy-match `needle` against `haystack`.
///
/// `needle` should already be lowercased by the caller (the search hot path
/// lowercases the query once, mirroring `utils::search::filter_items`); the
/// matcher lowercases defensively regardless. Returns `None` when `needle` is
/// empty or is not a subsequence of `haystack`.
pub fn fuzzy_match(haystack: &str, needle: &str) -> Option<FuzzyMatch> {
    let needle_chars: Vec<char> = needle.chars().map(lower_first).collect();
    if needle_chars.is_empty() {
        return None;
    }
    let hay = haystack_chars(haystack);
    if needle_chars.len() > hay.len() {
        return None;
    }

    let best = match (
        substring_match(&hay, &needle_chars),
        subsequence_match(&hay, &needle_chars),
    ) {
        (Some(a), Some(b)) => {
            if a.0 >= b.0 {
                a
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };

    Some(FuzzyMatch {
        score: best.0,
        ranges: coalesce(best.1),
    })
}

/// Score-only convenience for fields that don't need highlight ranges
/// (category, subtitle, synonyms). Same scoring as [`fuzzy_match`].
pub fn fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    fuzzy_match(haystack, needle).map(|m| m.score)
}

/// Whether a raw score clears the structure floor for `needle` — used to drop
/// weak scattered matches while keeping substrings, prefixes, and acronyms.
pub fn is_strong_score(score: i32, needle: &str) -> bool {
    score >= needle.chars().count() as i32 * MATCH_BASE + STRUCTURE_MIN
}

/// Whether a match clears the structure floor (see [`is_strong_score`]).
pub fn is_strong(m: &FuzzyMatch, needle: &str) -> bool {
    is_strong_score(m.score, needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_needle_is_none() {
        assert!(fuzzy_match("Volume Normalization", "").is_none());
    }

    #[test]
    fn needle_longer_than_haystack_is_none() {
        assert!(fuzzy_match("vol", "volume").is_none());
    }

    #[test]
    fn no_match_is_none() {
        assert!(fuzzy_match("Hello World", "xyz").is_none());
    }

    #[test]
    fn exact_substring_prefix_matches_with_single_range() {
        let m = fuzzy_match("Volume Normalization", "volume").expect("should match");
        assert_eq!(
            m.ranges,
            vec![(0, 6)],
            "contiguous match coalesces to one range"
        );
        assert!(is_strong(&m, "volume"));
        // Highlight ranges slice back to the matched text on a real label.
        assert_eq!(
            &"Volume Normalization"[m.ranges[0].0..m.ranges[0].1],
            "Volume"
        );
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_match("ReplayGain Pre-amp", "REPLAY").is_some());
        assert!(fuzzy_match("ReplayGain Pre-amp", "replay").is_some());
    }

    #[test]
    fn substring_beats_scattered_subsequence() {
        // "cross" as a clean prefix substring should outscore the same letters
        // scattered across a longer string.
        let contiguous = fuzzy_score("Crossfade", "cross").unwrap();
        // "Crop dressing" holds c-r-o (in "Crop") then s-s (in "dressing") with
        // a gap, so "cross" matches only as a scattered subsequence.
        let scattered = fuzzy_score("Crop dressing", "cross").unwrap();
        assert!(
            contiguous > scattered,
            "contiguous {contiguous} should beat scattered {scattered}"
        );
    }

    #[test]
    fn prefix_beats_midword() {
        let prefix = fuzzy_score("Crossfade", "cross").unwrap();
        let midword = fuzzy_score("Recrossfade", "cross").unwrap();
        assert!(
            prefix > midword,
            "prefix {prefix} should beat midword {midword}"
        );
    }

    #[test]
    fn acronym_matches_word_starts() {
        // "sv" should match the leading letters of "Stable Viewport".
        let m = fuzzy_match("Stable Viewport", "sv").expect("acronym should match");
        assert!(is_strong(&m, "sv"), "acronym match should clear the floor");
        // Two separate single-char ranges: the 'S' and the 'V'.
        assert_eq!(m.ranges, vec![(0, 1), (7, 8)]);
    }

    #[test]
    fn scattered_two_char_noise_is_weak() {
        // 'al' across "Stable" matches a(2)…l(4) mid-word with no word-start or
        // consecutive structure, so it should fall below the floor.
        let m = fuzzy_match("Stable Viewport", "al").expect("subsequence exists");
        assert!(
            !is_strong(&m, "al"),
            "scattered low-structure match should be weak, scored {}",
            m.score
        );
    }

    #[test]
    fn unicode_ranges_land_on_char_boundaries() {
        // 'é' is two bytes at index 3..5 in "café".
        let m = fuzzy_match("café", "é").expect("should match the accented char");
        assert_eq!(m.ranges, vec![(3, 5)]);
        // Slicing with the returned range must not panic and must isolate 'é'.
        assert_eq!(&"café"[m.ranges[0].0..m.ranges[0].1], "é");
    }

    #[test]
    fn multichar_substring_coalesces_to_one_range() {
        let m = fuzzy_match("abcdef", "bcd").expect("substring");
        assert_eq!(m.ranges, vec![(1, 4)]);
    }

    #[test]
    fn camelcase_boundary_is_word_start() {
        // The 'G' in "ReplayGain" is a camelCase word start, so "g" there is a
        // strong single-char match.
        let m = fuzzy_match("ReplayGain", "g").expect("should match");
        assert_eq!(m.ranges, vec![(6, 7)]);
        assert!(is_strong(&m, "g"));
    }

    #[test]
    fn fuzzy_score_matches_fuzzy_match_score() {
        let s = fuzzy_score("Crossfade Duration", "fade").unwrap();
        let m = fuzzy_match("Crossfade Duration", "fade").unwrap();
        assert_eq!(s, m.score);
    }
}
