//! Shared audio format info formatting helper.
//!
//! Produces split format strings from raw codec metadata.
//! Used by the track info strip to place codec info and bitrate on opposite sides.

/// Split format info into left (codec + sample rate) and right (bitrate) parts.
///
/// Used by the track info strip to place codec info on the left
/// and bitrate on the right.
///
/// # Returns
/// `(left, right)` where:
/// - `left`: `"FLAC 44.1kHz"` or `"MP3"` (always present when suffix is non-empty)
/// - `right`: `"1411kbps"` (only present when bitrate > 0)
pub(crate) fn format_audio_info_split(
    suffix: &str,
    sample_rate_khz: f32,
    bitrate_kbps: u32,
) -> Option<(String, Option<String>)> {
    if suffix.is_empty() {
        return None;
    }

    let upper = suffix.to_uppercase();

    let left = if sample_rate_khz > 0.0 {
        format!("{upper} {sample_rate_khz:.1}kHz")
    } else {
        upper
    };

    let right = if bitrate_kbps > 0 {
        Some(format!("{bitrate_kbps}kbps"))
    } else {
        None
    };

    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Combine split parts into a single string (test helper).
    fn format_audio_info(suffix: &str, sample_rate_khz: f32, bitrate_kbps: u32) -> Option<String> {
        let (left, right) = format_audio_info_split(suffix, sample_rate_khz, bitrate_kbps)?;
        Some(match right {
            Some(r) if sample_rate_khz > 0.0 => format!("{left} · {r}"),
            Some(r) => format!("{left} {r}"),
            None => left,
        })
    }

    #[test]
    fn full_info() {
        assert_eq!(
            format_audio_info("flac", 44.1, 1411),
            Some("FLAC 44.1kHz · 1411kbps".to_string())
        );
    }

    #[test]
    fn no_bitrate() {
        assert_eq!(
            format_audio_info("flac", 96.0, 0),
            Some("FLAC 96.0kHz".to_string())
        );
    }

    #[test]
    fn no_sample_rate() {
        assert_eq!(
            format_audio_info("mp3", 0.0, 320),
            Some("MP3 320kbps".to_string())
        );
    }

    #[test]
    fn suffix_only() {
        assert_eq!(format_audio_info("ogg", 0.0, 0), Some("OGG".to_string()));
    }

    #[test]
    fn empty_suffix() {
        assert_eq!(format_audio_info("", 44.1, 1411), None);
    }
}
