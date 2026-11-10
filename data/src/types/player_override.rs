//! What nokkvi's own player row on the server forces on its streams.
//!
//! Navidrome keeps a row per player (user + client + user agent, or the
//! player an API key is on) whose transcoding and max bit rate apply to
//! every stream it serves that player (measured on 0.64.2): a forced
//! transcoding replaces the format AND the bit rate nokkvi asks for, and a
//! max bit rate alone makes the server transcode every song above it, even
//! with nokkvi's Streaming Quality on Original. nokkvi reads the row to say
//! so in Settings; it never writes it.

/// A transcoding forced on nokkvi's player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedTranscoding {
    /// The format it sends (`mp3`, `opus`, ...).
    pub format: String,
    /// Its own bit rate (kbps), used when the player sets none.
    pub default_bitrate_kbps: Option<u32>,
}

/// What nokkvi's player row forces, when it forces anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerOverride {
    /// A transcoding the row forces.
    pub forced: Option<ForcedTranscoding>,
    /// The row's max bit rate (kbps).
    pub max_bitrate_kbps: Option<u32>,
}

/// Where the user changes it, said the same way in every case.
const WHERE_TO_CHANGE: &str = "Change it in Navidrome under Settings > Players.";

impl PlayerOverride {
    /// The override a player row with `forced` and `max_bit_rate` makes, or
    /// `None` when it forces nothing (no transcoding, no cap above 0).
    pub fn of(forced: Option<ForcedTranscoding>, max_bit_rate: i64) -> Option<Self> {
        let max_bitrate_kbps = positive_kbps(max_bit_rate);
        (forced.is_some() || max_bitrate_kbps.is_some()).then_some(Self {
            forced,
            max_bitrate_kbps,
        })
    }

    /// The short value the Settings row shows ("MP3 at 192 kbps", "96 kbps
    /// limit").
    pub fn summary(&self) -> String {
        match &self.forced {
            Some(forced) => match self.forced_bitrate_kbps(forced) {
                Some(kbps) => format!("{} at {kbps} kbps", format_label(&forced.format)),
                None => format_label(&forced.format),
            },
            None => match self.max_bitrate_kbps {
                Some(kbps) => format!("{kbps} kbps limit"),
                None => String::new(),
            },
        }
    }

    /// What it means, in plain words, for the Settings row's subtitle.
    pub fn description(&self) -> String {
        match &self.forced {
            Some(forced) => {
                let what = match self.forced_bitrate_kbps(forced) {
                    Some(kbps) => format!("{} at {kbps} kbps", format_label(&forced.format)),
                    None => format_label(&forced.format),
                };
                format!(
                    "Your server forces {what} for this device, so it replaces the choice above. \
                     {WHERE_TO_CHANGE}"
                )
            }
            None => match self.max_bitrate_kbps {
                Some(kbps) => format!(
                    "Your server limits this device to {kbps} kbps, so songs above that always \
                     stream as a smaller copy. {WHERE_TO_CHANGE}"
                ),
                None => String::new(),
            },
        }
    }

    /// The bit rate a forced transcoding sends at: the player's own cap
    /// when it sets one, else the transcoding's default.
    fn forced_bitrate_kbps(&self, forced: &ForcedTranscoding) -> Option<u32> {
        self.max_bitrate_kbps.or(forced.default_bitrate_kbps)
    }
}

/// A bit rate from the server (kbps), `None` when unset (0 or less).
pub fn positive_kbps(kbps: i64) -> Option<u32> {
    u32::try_from(kbps).ok().filter(|&kbps| kbps > 0)
}

/// How a format is named to the user.
fn format_label(format: &str) -> String {
    match format.trim().to_ascii_lowercase().as_str() {
        "mp3" => "MP3".to_owned(),
        "opus" => "Opus".to_owned(),
        "aac" => "AAC".to_owned(),
        "flac" => "FLAC".to_owned(),
        "ogg" | "oga" | "vorbis" => "Ogg Vorbis".to_owned(),
        "" => "a smaller copy".to_owned(),
        other => other.to_ascii_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mp3(default_bitrate_kbps: Option<u32>) -> ForcedTranscoding {
        ForcedTranscoding {
            format: "mp3".to_owned(),
            default_bitrate_kbps,
        }
    }

    /// A row that forces nothing is no override: the Settings row stays
    /// hidden.
    #[test]
    fn a_row_that_forces_nothing_is_no_override() {
        assert_eq!(PlayerOverride::of(None, 0), None);
        assert_eq!(PlayerOverride::of(None, -1), None);
    }

    /// A forced transcoding replaces nokkvi's format and bit rate: at the
    /// player's own cap when it sets one, else at the transcoding's default.
    #[test]
    fn a_forced_transcoding_names_its_format_and_bit_rate() {
        let at_default = PlayerOverride::of(Some(mp3(Some(192))), 0).expect("an override");
        assert_eq!(at_default.summary(), "MP3 at 192 kbps");
        assert_eq!(
            at_default.description(),
            "Your server forces MP3 at 192 kbps for this device, so it replaces the choice \
             above. Change it in Navidrome under Settings > Players."
        );

        let capped = PlayerOverride::of(Some(mp3(Some(192))), 96).expect("an override");
        assert_eq!(capped.summary(), "MP3 at 96 kbps");

        let no_rate = PlayerOverride::of(Some(mp3(None)), 0).expect("an override");
        assert_eq!(
            no_rate.description(),
            "Your server forces MP3 for this device, so it replaces the choice above. Change it \
             in Navidrome under Settings > Players."
        );
    }

    /// A max bit rate alone makes the server send a smaller copy of every
    /// song above it, whatever nokkvi's setting.
    #[test]
    fn a_cap_alone_says_songs_above_it_stream_as_a_smaller_copy() {
        let cap = PlayerOverride::of(None, 96).expect("an override");
        assert_eq!(cap.summary(), "96 kbps limit");
        assert_eq!(
            cap.description(),
            "Your server limits this device to 96 kbps, so songs above that always stream as a \
             smaller copy. Change it in Navidrome under Settings > Players."
        );
    }

    #[test]
    fn formats_are_named_as_people_know_them() {
        assert_eq!(format_label("opus"), "Opus");
        assert_eq!(format_label("AAC"), "AAC");
        assert_eq!(format_label("wma"), "WMA");
    }
}
