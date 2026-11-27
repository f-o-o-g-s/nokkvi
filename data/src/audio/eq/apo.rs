//! Equalizer APO / AutoEq text: read a `ParametricEQ.txt` (or any Equalizer
//! APO filter list) into a band list, and write a band list back as one.
//!
//! Ported from Pawse's `crates/equalizer/src/apo.rs`. Changed here: a filter
//! line is read as keyword / value pairs, so a value that isn't a plain
//! number (a decimal comma, `kHz`) or a stray token refuses the line instead
//! of falling back to a default; parsed bands are clamped; the export rounds
//! the preamp down, caps the list at `EQ_MAX_BANDS`, and keeps the same
//! precision as config.toml.
//!
//! MIT License
//!
//! Copyright (c) 2026 Aleksandr Popov
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use super::{
    band::{EQ_DEFAULT_Q, EQ_MAX_BANDS, EqBand, EqBandKind},
    filter::preamp_db,
    tidied,
};
use crate::utils::noun::Noun;

/// The rate the exported `Preamp:` line is computed at. Below 20 kHz the
/// response barely depends on it.
const EXPORT_RATE: f64 = 48_000.0;

/// Why a text can't be imported. Every case refuses the whole file: half of
/// a correction, or the left channel's filters on both, is worse than none.
/// The messages are written for a toast; line numbers count from 1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApoParseError {
    /// A filter type the equalizer doesn't build (`BP`, `NO`, `AP`, `IIR`, …).
    #[error("line {line}: unsupported filter type {kind}")]
    Unsupported { line: usize, kind: String },
    /// A `Filter…:` line that doesn't read as `ON|OFF <type> <keyword value>…`.
    #[error("line {line}: unreadable filter")]
    Malformed { line: usize },
    /// A `Channel:` line other than `all`: the filters after it are meant for
    /// some channels only.
    #[error("line {line}: filters for separate channels are not supported")]
    Channels { line: usize },
    /// Nothing but empty slots, comments and other commands.
    #[error("no filters found")]
    Empty,
    /// More filters than a band list holds (empty slots don't count).
    #[error("{}, at most {EQ_MAX_BANDS} are supported", filters(.count))]
    TooMany { count: usize },
}

fn filters(count: &usize) -> String {
    Noun::FILTER.count(*count)
}

/// Read the filters of an Equalizer APO text, such as AutoEq's
/// `ParametricEQ.txt` or a Room EQ Wizard export.
///
/// Only `Filter…:` lines are read (a leading byte order mark is dropped):
/// `ON` / `OFF`, a type (`PK` / `PEQ`, `LSC` / `LS`, `HSC` / `HS`, `LP` /
/// `LPQ`, `HP` / `HPQ`), then `Fc`, `Gain` and `Q` or `BW Oct`, keywords in
/// any case, the `Hz` / `dB` units optional. A `None` filter (an unused
/// slot) is skipped. A peak needs a gain and a Q; a shelf needs a gain; a
/// shelf or pass without a Q takes [`EQ_DEFAULT_Q`]. The `Preamp:` line is
/// ignored: the equalizer computes its own. The bands come back clamped.
pub fn parse(text: &str) -> Result<Vec<EqBand>, ApoParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut bands = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let Some((head, body)) = raw.trim().split_once(':') else {
            continue;
        };
        let head = head.trim().to_ascii_lowercase();
        if head == "channel" && !body.trim().eq_ignore_ascii_case("all") {
            return Err(ApoParseError::Channels { line });
        }
        if head.starts_with("filter")
            && let Some(band) = parse_filter(body, line)?
        {
            bands.push(band);
        }
    }
    if bands.is_empty() {
        return Err(ApoParseError::Empty);
    }
    if bands.len() > EQ_MAX_BANDS {
        return Err(ApoParseError::TooMany { count: bands.len() });
    }
    Ok(bands)
}

/// One `Filter…:` line's body. `None` for an unused slot.
fn parse_filter(body: &str, line: usize) -> Result<Option<EqBand>, ApoParseError> {
    let malformed = || ApoParseError::Malformed { line };
    let mut tokens = body.split_whitespace();
    let enabled = match tokens.next() {
        Some(state) if state.eq_ignore_ascii_case("on") => true,
        Some(state) if state.eq_ignore_ascii_case("off") => false,
        _ => return Err(malformed()),
    };
    let code = tokens.next().ok_or_else(malformed)?;
    let kind = match code.to_ascii_uppercase().as_str() {
        "PK" | "PEQ" => EqBandKind::Peak,
        "LSC" | "LS" => EqBandKind::LowShelf,
        "HSC" | "HS" => EqBandKind::HighShelf,
        "LP" | "LPQ" => EqBandKind::LowPass,
        "HP" | "HPQ" => EqBandKind::HighPass,
        "NONE" => return Ok(None),
        _ => {
            return Err(ApoParseError::Unsupported {
                line,
                kind: code.to_string(),
            });
        }
    };
    let values = Values::read(tokens).ok_or_else(malformed)?;
    let freq = values.freq.ok_or_else(malformed)?;
    let gain = match (kind.has_gain(), values.gain) {
        (true, Some(gain)) => gain,
        (true, None) => return Err(malformed()),
        // A pass filter has no gain; a written one is read and ignored.
        (false, _) => 0.0,
    };
    let q = match (values.q, values.octaves) {
        (Some(q), _) => q,
        (None, Some(octaves)) if octaves > 0.0 => q_from_octaves(octaves),
        // A bandwidth of zero or less has no Q.
        (None, Some(_)) => return Err(malformed()),
        (None, None) if kind == EqBandKind::Peak => return Err(malformed()),
        (None, None) => EQ_DEFAULT_Q,
    };
    Ok(Some(
        EqBand {
            enabled,
            ..EqBand::new(kind, freq, gain, q)
        }
        .clamped(),
    ))
}

/// The keyword / value pairs after a filter's type, in any order:
/// `Fc <hz> [Hz]`, `Gain <db> [dB]`, `Q <q>`, `BW Oct <octaves>`.
#[derive(Default)]
struct Values {
    freq: Option<f64>,
    gain: Option<f64>,
    q: Option<f64>,
    octaves: Option<f64>,
}

impl Values {
    /// `None` when a token is none of those, a keyword comes twice, or a value
    /// isn't a finite number with a decimal point. A decimal comma, `kHz` or a
    /// fixed-slope shelf's `12dB` refuses the line instead of reading as a
    /// default.
    fn read<'a>(tokens: impl Iterator<Item = &'a str>) -> Option<Self> {
        let mut values = Self::default();
        let mut tokens = tokens.peekable();
        while let Some(keyword) = tokens.next() {
            let (slot, unit) = if keyword.eq_ignore_ascii_case("fc") {
                (&mut values.freq, Some("hz"))
            } else if keyword.eq_ignore_ascii_case("gain") {
                (&mut values.gain, Some("db"))
            } else if keyword.eq_ignore_ascii_case("q") {
                (&mut values.q, None)
            } else if keyword.eq_ignore_ascii_case("bw")
                && tokens
                    .next_if(|token| token.eq_ignore_ascii_case("oct"))
                    .is_some()
            {
                (&mut values.octaves, None)
            } else {
                return None;
            };
            if slot.is_some() {
                return None;
            }
            let value = tokens.next()?.parse::<f64>().ok()?;
            if !value.is_finite() {
                return None;
            }
            *slot = Some(value);
            if let Some(unit) = unit {
                tokens.next_if(|token| token.eq_ignore_ascii_case(unit));
            }
        }
        Some(values)
    }
}

/// The Q of a peak `octaves` wide (Equalizer APO's `BW Oct`).
fn q_from_octaves(octaves: f64) -> f64 {
    let ratio = 2f64.powf(octaves);
    ratio.sqrt() / (ratio - 1.0)
}

/// Write `bands` as an Equalizer APO text that AutoEq-aware players read: a
/// `Preamp:` line, then one `Filter N:` line per band (pass filters as
/// `LPQ` / `HPQ`, the forms that carry a Q). Bands are clamped, rounded to
/// config.toml's precision and cut at [`EQ_MAX_BANDS`], so every export
/// imports again.
pub fn write(bands: &[EqBand]) -> String {
    let bands: Vec<EqBand> = bands
        .iter()
        .take(EQ_MAX_BANDS)
        .map(|band| tidied(band.clamped()))
        .collect();
    let mut text = format!("Preamp: {:.1} dB\n", export_preamp(&bands));
    for (index, band) in bands.iter().enumerate() {
        text.push_str(&filter_line(index + 1, band));
        text.push('\n');
    }
    text
}

/// [`preamp_db`] rounded down to the 0.1 dB it is written with, so a player
/// that applies the line (Equalizer APO) never boosts either. The hair of
/// tolerance keeps float noise on a cut-only list from printing -0.1.
fn export_preamp(bands: &[EqBand]) -> f64 {
    (preamp_db(bands, EXPORT_RATE) * 10.0 + 1e-6).floor() / 10.0 + 0.0
}

fn filter_line(number: usize, band: &EqBand) -> String {
    let state = if band.enabled { "ON" } else { "OFF" };
    let code = match band.kind {
        EqBandKind::Peak => "PK",
        EqBandKind::LowShelf => "LSC",
        EqBandKind::HighShelf => "HSC",
        EqBandKind::LowPass => "LPQ",
        EqBandKind::HighPass => "HPQ",
    };
    let EqBand { freq, gain, q, .. } = *band;
    if band.kind.has_gain() {
        format!("Filter {number}: {state} {code} Fc {freq} Hz Gain {gain} dB Q {q}")
    } else {
        format!("Filter {number}: {state} {code} Fc {freq} Hz Q {q}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{EQ_DEFAULT_Q, EQ_FREQ_MAX, EQ_Q_MIN, EqBandKind, preamp_db},
        *,
    };

    const RATE: f64 = 48_000.0;

    /// AutoEq's Sennheiser HD 600 profile, with the Windows line endings an
    /// Equalizer APO config is saved with.
    const HD600: &str = "Preamp: -6.3 dB\r
Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70\r
Filter 2: ON PK Fc 125 Hz Gain -2.7 dB Q 0.55\r
Filter 3: ON PK Fc 8445 Hz Gain 3.3 dB Q 1.61\r
Filter 4: ON PK Fc 522 Hz Gain 0.7 dB Q 1.02\r
Filter 5: ON PK Fc 1298 Hz Gain -1.2 dB Q 2.14\r
Filter 6: ON HSC Fc 10000 Hz Gain -3.1 dB Q 0.70\r
Filter 7: ON PK Fc 3158 Hz Gain -1.8 dB Q 3.67\r
Filter 8: ON PK Fc 2166 Hz Gain 0.9 dB Q 3.32\r
Filter 9: ON PK Fc 6639 Hz Gain 2.2 dB Q 5.82\r
Filter 10: ON PK Fc 5433 Hz Gain -1.2 dB Q 5.70\r
";

    /// A current AutoEq `ParametricEQ.txt`, as downloaded (Unix line endings,
    /// no trailing newline).
    const AUTOEQ: &str = "Preamp: -6.2 dB
Filter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70
Filter 2: ON PK Fc 205 Hz Gain -2.4 dB Q 0.88
Filter 3: ON PK Fc 1402 Hz Gain 1.9 dB Q 1.95
Filter 4: ON PK Fc 3036 Hz Gain -3.6 dB Q 3.09
Filter 5: ON PK Fc 5521 Hz Gain 2.9 dB Q 4.03
Filter 6: ON PK Fc 7339 Hz Gain -4.1 dB Q 3.55
Filter 7: ON PK Fc 9886 Hz Gain 2.3 dB Q 2.30
Filter 8: ON HSC Fc 10000 Hz Gain -1.6 dB Q 0.70
Filter 9: ON PK Fc 40 Hz Gain 1.1 dB Q 1.39
Filter 10: ON PK Fc 16015 Hz Gain -1.3 dB Q 0.92";

    /// Room EQ Wizard's Equalizer APO export: a header (some lines with a
    /// colon), padded columns, unused filter slots written as `None`.
    const REW: &str = "Filter Settings file

Room EQ V5.31
Dated: 27 Nov 2026 14:03:11

Notes:

Equaliser: Generic
Averages 2026-11-27
Filter  1: ON  PK       Fc   63.00 Hz  Gain  -5.20 dB  Q  4.000
Filter  2: ON  PK       Fc   118.5 Hz  Gain  -3.10 dB  Q  6.124
Filter  3: ON  None
Filter  4: ON  LS       Fc   80.00 Hz  Gain   2.00 dB
Filter  5: OFF PK       Fc   2400 Hz   Gain   1.50 dB  Q  2.000
Filter  6: ON  None
Filter  7: ON  None
";

    fn band(kind: EqBandKind, freq: f64, gain: f64, q: f64) -> EqBand {
        EqBand::new(kind, freq, gain, q)
    }

    fn off(band: EqBand) -> EqBand {
        EqBand {
            enabled: false,
            ..band
        }
    }

    /// A file of `count` peaks, one per line.
    fn peaks(count: usize) -> String {
        (1..=count)
            .map(|i| format!("Filter {i}: ON PK Fc {} Hz Gain 1 dB Q 1\n", 100 * i))
            .collect()
    }

    // -- Ported from Pawse ------------------------------------------------

    #[test]
    fn parses_an_autoeq_file() {
        let bands = parse(HD600).expect("HD600 parses");
        assert_eq!(bands.len(), 10);
        assert_eq!(bands[0], band(EqBandKind::LowShelf, 105.0, 6.5, 0.7));
        assert_eq!(bands[5], band(EqBandKind::HighShelf, 10_000.0, -3.1, 0.7));
        assert_eq!(bands[9], band(EqBandKind::Peak, 5433.0, -1.2, 5.7));
    }

    #[test]
    fn autoeq_preamp_matches_the_file() {
        let bands = parse(HD600).expect("HD600 parses");
        assert!((preamp_db(&bands, RATE) + 6.3).abs() < 0.15);
    }

    #[test]
    fn parses_apo_variants() {
        let text = "# comment\nPreamp: -3 dB\nFilter: OFF PK Fc 1000 Hz Gain 3 dB BW Oct 1\n\
                    filter 2: on hpq fc 30 hz q 0.5\nFilter 3: ON LP Fc 18000 Hz\n\
                    Include: other.txt\n";
        let bands = parse(text).expect("APO variants parse");
        assert_eq!(bands.len(), 3);
        assert!(!bands[0].enabled);
        assert!((bands[0].q - std::f64::consts::SQRT_2).abs() < 1e-9);
        assert_eq!(bands[1], band(EqBandKind::HighPass, 30.0, 0.0, 0.5));
        assert_eq!(
            bands[2],
            band(EqBandKind::LowPass, 18_000.0, 0.0, EQ_DEFAULT_Q)
        );
    }

    #[test]
    fn rejects_what_it_cannot_play() {
        for (text, error) in [
            (
                "Filter 1: ON NO Fc 1000 Hz\n",
                ApoParseError::Unsupported {
                    line: 1,
                    kind: "NO".into(),
                },
            ),
            (
                "Filter 1: ON PK Fc 1000 Hz Q 1\n",
                ApoParseError::Malformed { line: 1 },
            ),
            (
                "Filter 1: ON PK Fc 1000 Hz Gain 2 dB\n",
                ApoParseError::Malformed { line: 1 },
            ),
            ("GraphicEQ: 20 -1; 30 -2\n", ApoParseError::Empty),
        ] {
            assert_eq!(parse(text), Err(error), "{text:?}");
        }
    }

    #[test]
    fn import_skips_empty_slots_and_a_byte_order_mark() {
        let text = "\u{feff}Filter 1: ON PK Fc 100 Hz Gain 2 dB Q 1\nFilter 2: ON None\n";
        assert_eq!(
            parse(text),
            Ok(vec![band(EqBandKind::Peak, 100.0, 2.0, 1.0)])
        );
    }

    #[test]
    fn import_refuses_per_channel_filters() {
        let text = "Channel: all\nFilter 1: ON PK Fc 100 Hz Gain 2 dB Q 1\nChannel: L\n\
                    Filter 2: ON PK Fc 200 Hz Gain 2 dB Q 1\n";
        assert_eq!(parse(text), Err(ApoParseError::Channels { line: 3 }));
    }

    #[test]
    fn import_stops_at_the_band_limit() {
        assert_eq!(
            parse(&peaks(EQ_MAX_BANDS)).map(|b| b.len()),
            Ok(EQ_MAX_BANDS)
        );
        let with_empty_slot = format!("{}Filter 99: ON None\n", peaks(EQ_MAX_BANDS));
        assert_eq!(
            parse(&with_empty_slot).map(|b| b.len()),
            Ok(EQ_MAX_BANDS),
            "a None slot doesn't count"
        );
        assert_eq!(
            parse(&peaks(EQ_MAX_BANDS + 1)),
            Err(ApoParseError::TooMany {
                count: EQ_MAX_BANDS + 1
            })
        );
    }

    // -- Formats in the wild ----------------------------------------------

    #[test]
    fn parses_a_current_autoeq_download() {
        let bands = parse(AUTOEQ).expect("AutoEq parses");
        assert_eq!(bands.len(), 10);
        assert_eq!(bands[0], band(EqBandKind::LowShelf, 105.0, 5.5, 0.7));
        assert_eq!(bands[7], band(EqBandKind::HighShelf, 10_000.0, -1.6, 0.7));
        assert_eq!(bands[9], band(EqBandKind::Peak, 16_015.0, -1.3, 0.92));
        assert!(bands.iter().all(|b| b.enabled));
    }

    #[test]
    fn parses_a_rew_export_with_empty_slots() {
        let bands = parse(REW).expect("REW parses");
        assert_eq!(
            bands,
            vec![
                band(EqBandKind::Peak, 63.0, -5.2, 4.0),
                band(EqBandKind::Peak, 118.5, -3.1, 6.124),
                band(EqBandKind::LowShelf, 80.0, 2.0, EQ_DEFAULT_Q),
                off(band(EqBandKind::Peak, 2400.0, 1.5, 2.0)),
            ]
        );
    }

    #[test]
    fn windows_and_unix_line_endings_read_the_same() {
        let unix = HD600.replace("\r\n", "\n");
        assert_eq!(parse(HD600), parse(&unix));
        // A lone `\r` at the end of the last line too.
        assert_eq!(
            parse("Filter 1: ON PK Fc 100 Hz Gain 2 dB Q 1\r"),
            Ok(vec![band(EqBandKind::Peak, 100.0, 2.0, 1.0)])
        );
    }

    #[test]
    fn keywords_ignore_case() {
        let text = "FILTER 1: on pk FC 100 HZ GAIN 2 DB q 1\n\
                    filter 2: Off Lsc fc 80 hz gain -3 db q 0.5\n\
                    fIlTeR 3: ON hsc Fc 9000 Hz Gain 1 dB bw oct 2\n\
                    channel: ALL\n";
        let bands = parse(text).expect("mixed case parses");
        assert_eq!(bands[0], band(EqBandKind::Peak, 100.0, 2.0, 1.0));
        assert_eq!(bands[1], off(band(EqBandKind::LowShelf, 80.0, -3.0, 0.5)));
        assert_eq!(bands[2].kind, EqBandKind::HighShelf);
    }

    #[test]
    fn every_kind_code_reads() {
        for (code, kind) in [
            ("PK", EqBandKind::Peak),
            ("PEQ", EqBandKind::Peak),
            ("LSC", EqBandKind::LowShelf),
            ("LS", EqBandKind::LowShelf),
            ("HSC", EqBandKind::HighShelf),
            ("HS", EqBandKind::HighShelf),
            ("LP", EqBandKind::LowPass),
            ("LPQ", EqBandKind::LowPass),
            ("HP", EqBandKind::HighPass),
            ("HPQ", EqBandKind::HighPass),
        ] {
            let text = format!("Filter: ON {code} Fc 500 Hz Gain 2 dB Q 1.5\n");
            let bands = parse(&text).unwrap_or_else(|e| panic!("{code}: {e}"));
            assert_eq!(bands[0].kind, kind, "{code}");
            let gain = if kind.has_gain() { 2.0 } else { 0.0 };
            assert_eq!(bands[0], band(kind, 500.0, gain, 1.5), "{code}");
        }
    }

    #[test]
    fn units_are_optional() {
        let bare = parse("Filter 1: ON PK Fc 100 Gain 2 Q 1\n");
        assert_eq!(bare, Ok(vec![band(EqBandKind::Peak, 100.0, 2.0, 1.0)]));
        let reordered = parse("Filter 1: ON PK Q 1 Gain 2 dB Fc 100 Hz\n");
        assert_eq!(bare, reordered, "the keywords can come in any order");
    }

    #[test]
    fn q_wins_over_a_bandwidth() {
        let bands = parse("Filter 1: ON PK Fc 100 Hz Gain 2 dB BW Oct 1 Q 3\n").expect("parses");
        assert_eq!(bands[0].q, 3.0);
    }

    #[test]
    fn values_out_of_range_are_clamped_not_refused() {
        let bands =
            parse("Filter 1: ON PK Fc 99999 Hz Gain -60 dB Q 0.01\nFilter 2: ON HP Fc 2 Hz Q 50\n")
                .expect("parses");
        assert_eq!(
            bands[0],
            band(EqBandKind::Peak, 99_999.0, -60.0, 0.01).clamped()
        );
        assert_eq!(bands[0].freq, EQ_FREQ_MAX);
        assert_eq!(bands[0].q, EQ_Q_MIN);
        assert_eq!(
            bands[1],
            band(EqBandKind::HighPass, 2.0, 0.0, 50.0).clamped()
        );
    }

    // -- Refusals ---------------------------------------------------------

    #[test]
    fn a_line_it_cannot_read_is_refused_with_its_number() {
        for (body, why) in [
            ("PK Fc 100 Hz Gain 2 dB Q 1", "no ON / OFF"),
            ("ON", "no filter type"),
            ("ON PK Gain 2 dB Q 1", "no Fc"),
            ("ON LS Fc 100 Hz", "a shelf without a gain"),
            ("ON PK Fc 100,5 Hz Gain 2 dB Q 1", "a decimal comma in Fc"),
            ("ON PK Fc 100 Hz Gain 2,5 dB Q 1", "a decimal comma in Gain"),
            (
                "ON LS Fc 100 Hz Gain 2 dB Q 0,7",
                "a decimal comma in a shelf's Q",
            ),
            (
                "ON HP Fc 30 Hz Gain 0,0 dB",
                "a decimal comma in a pass's ignored gain",
            ),
            ("ON PK Fc 1.2 kHz Gain 2 dB Q 1", "a unit it doesn't read"),
            ("ON LS 12dB Fc 100 Hz Gain 2 dB", "a fixed-slope shelf"),
            ("ON PK Fc 100Hz Gain 2 dB Q 1", "a unit glued to its number"),
            ("ON PK Fc Hz Gain 2 dB Q 1", "a keyword without a value"),
            ("ON PK Fc 100 Hz Gain 2 dB Q", "a value cut off at the end"),
            ("ON PK Fc 100 Hz Fc 200 Hz Gain 2 dB Q 1", "a keyword twice"),
            ("ON PK Fc 100 Hz Gain 2 dB Q inf", "an infinite value"),
            ("ON PK Fc 100 Hz Gain nan dB Q 1", "a NaN value"),
            ("ON PK Fc 100 Hz Gain 2 dB BW 1", "BW without Oct"),
            ("ON PK Fc 100 Hz Gain 2 dB BW Oct 0", "a zero bandwidth"),
            ("ON PK Fc 100 Hz Gain 2 dB Q 1 extra", "a stray token"),
        ] {
            let text = format!(
                "Preamp: -1 dB\nFilter 1: ON PK Fc 50 Hz Gain 1 dB Q 1\nFilter 2: {body}\n"
            );
            assert_eq!(
                parse(&text),
                Err(ApoParseError::Malformed { line: 3 }),
                "{why}: {body:?}"
            );
        }
    }

    #[test]
    fn every_other_filter_type_is_refused_by_name() {
        for code in ["BP", "NO", "AP", "IIR", "LSQ", "peaking"] {
            let text =
                format!("Filter 1: ON PK Fc 50 Hz Gain 1 dB Q 1\nFilter 2: ON {code} Fc 100 Hz\n");
            assert_eq!(
                parse(&text),
                Err(ApoParseError::Unsupported {
                    line: 2,
                    kind: code.to_string()
                }),
                "{code}"
            );
        }
    }

    #[test]
    fn only_every_channel_at_once_is_read() {
        let filter = "Filter 1: ON PK Fc 100 Hz Gain 2 dB Q 1\n";
        assert!(parse(&format!("Channel: all\n{filter}")).is_ok());
        for channels in ["L", "R", "L R", "1 2", "C", ""] {
            assert_eq!(
                parse(&format!("{filter}Channel: {channels}\n")),
                Err(ApoParseError::Channels { line: 2 }),
                "Channel: {channels}"
            );
        }
    }

    #[test]
    fn a_file_without_filters_is_refused() {
        for text in [
            "",
            "\u{feff}",
            "Preamp: -3 dB\n",
            "Filter 1: ON None\nFilter 2: OFF None\n",
            "# Filter 1: ON PK Fc 100 Hz Gain 2 dB Q 1\n",
        ] {
            assert_eq!(parse(text), Err(ApoParseError::Empty), "{text:?}");
        }
    }

    #[test]
    fn errors_read_as_toast_text() {
        assert_eq!(
            ApoParseError::Unsupported {
                line: 4,
                kind: "BP".into()
            }
            .to_string(),
            "line 4: unsupported filter type BP"
        );
        assert_eq!(
            ApoParseError::Malformed { line: 7 }.to_string(),
            "line 7: unreadable filter"
        );
        assert_eq!(
            ApoParseError::Channels { line: 2 }.to_string(),
            "line 2: filters for separate channels are not supported"
        );
        assert_eq!(ApoParseError::Empty.to_string(), "no filters found");
        assert_eq!(
            ApoParseError::TooMany { count: 34 }.to_string(),
            "34 filters, at most 32 are supported"
        );
    }

    // -- Write ------------------------------------------------------------

    #[test]
    fn writes_the_autoeq_layout() {
        let bands = [
            band(EqBandKind::LowShelf, 105.0, 5.5, 0.7),
            off(band(EqBandKind::Peak, 31.5, -2.25, 1.234)),
            band(EqBandKind::HighShelf, 10_000.0, -1.6, EQ_DEFAULT_Q),
            band(EqBandKind::HighPass, 25.0, 4.0, 0.5),
            band(EqBandKind::LowPass, 18_000.0, 0.0, EQ_DEFAULT_Q),
        ];
        let text = write(&bands);
        let mut lines = text.lines();
        assert!(
            lines.next().is_some_and(|l| l.starts_with("Preamp: -")),
            "{text}"
        );
        assert_eq!(
            lines.collect::<Vec<_>>(),
            [
                "Filter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.7",
                "Filter 2: OFF PK Fc 31.5 Hz Gain -2.25 dB Q 1.234",
                "Filter 3: ON HSC Fc 10000 Hz Gain -1.6 dB Q 0.707",
                "Filter 4: ON HPQ Fc 25 Hz Q 0.5",
                "Filter 5: ON LPQ Fc 18000 Hz Q 0.707",
            ]
        );
        assert!(text.ends_with('\n'));
    }

    /// The `Preamp:` line is what another player (Equalizer APO itself) runs
    /// the file with, so it is rounded down: written, it never boosts either.
    #[test]
    fn the_preamp_line_rounds_down_and_never_reads_minus_zero() {
        let bands = [band(EqBandKind::Peak, 1000.0, 6.24, 4.0)];
        let exact = preamp_db(&bands, RATE);
        assert!(exact > -6.3 && exact < -6.2, "{exact}");
        assert!(write(&bands).starts_with("Preamp: -6.3 dB\n"));

        let cuts = [band(EqBandKind::Peak, 1000.0, -6.0, 1.0)];
        assert!(
            write(&cuts).starts_with("Preamp: 0.0 dB\n"),
            "{}",
            write(&cuts)
        );
        let flat = [band(EqBandKind::Peak, 1000.0, 0.0, 1.0)];
        assert!(write(&flat).starts_with("Preamp: 0.0 dB\n"));
    }

    #[test]
    fn written_files_read_back() {
        let bands = vec![
            band(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
            off(band(EqBandKind::Peak, 31.5, -2.25, 1.234)),
            band(EqBandKind::HighPass, 25.0, 0.0, 0.5),
            band(EqBandKind::HighShelf, 9876.54, 12.75, 0.707),
            band(EqBandKind::LowPass, 21_000.0, 0.0, 2.5),
        ];
        assert_eq!(parse(&write(&bands)), Ok(bands));
    }

    /// Values finer than the written precision come back rounded to it
    /// (hundredths of a hertz and a decibel, thousandths of Q), and a second
    /// export of the import is the same text.
    #[test]
    fn a_round_trip_keeps_the_written_precision_and_settles() {
        let bands = vec![
            band(EqBandKind::Peak, 1234.5678, -3.21987, 1.63299),
            band(
                EqBandKind::LowShelf,
                63.004,
                2.996,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
        ];
        let text = write(&bands);
        let back = parse(&text).expect("written text parses");
        for (a, b) in bands.iter().zip(&back) {
            assert_eq!(a.kind, b.kind);
            assert!(
                (a.freq - b.freq).abs() <= 0.005 + 1e-9,
                "{} vs {}",
                a.freq,
                b.freq
            );
            assert!(
                (a.gain - b.gain).abs() <= 0.005 + 1e-9,
                "{} vs {}",
                a.gain,
                b.gain
            );
            assert!((a.q - b.q).abs() <= 0.0005 + 1e-9, "{} vs {}", a.q, b.q);
        }
        assert_eq!(
            write(&back),
            text,
            "re-exporting the import changes nothing"
        );
    }

    #[test]
    fn write_clamps_and_caps_the_list() {
        let wild = band(EqBandKind::Peak, 1e9, f64::NAN, 0.0);
        let text = write(&[wild; EQ_MAX_BANDS + 3]);
        let back = parse(&text).expect("an export always imports");
        assert_eq!(back.len(), EQ_MAX_BANDS);
        assert_eq!(back[0], wild.clamped());
        assert!(!text.contains("NaN") && !text.contains("-0 "), "{text}");
    }
}
