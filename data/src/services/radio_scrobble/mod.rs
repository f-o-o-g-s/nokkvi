//! Direct-to-service scrobbling for internet radio.
//!
//! Radio tracks carry only ICY `StreamTitle` metadata (artist + title strings)
//! and have no Navidrome library media-file id, so they cannot be scrobbled
//! through Navidrome's Subsonic `/rest/scrobble` relay (which is strictly
//! id-gated — see the investigation that motivated this module). Instead, radio
//! plays are submitted DIRECTLY to external scrobble services, independent of
//! Navidrome.
//!
//! Two targets ship, dispatched together: ListenBrainz ([`listenbrainz`], a
//! single user token) and Last.fm ([`lastfm`], which additionally needs
//! `api_sig` signing + a browser-auth session key).
//!
//! Iced-free: lives in the data crate and is driven by the UI crate's
//! radio-metadata update path (`src/update/playback.rs`), reusing the existing
//! `ScrobbleState` timing gate.

pub mod lastfm;
pub mod listenbrainz;
pub mod parse;
pub mod source;

/// A radio track to scrobble, normalized from ICY stream metadata.
///
/// Target-agnostic: both the ListenBrainz and Last.fm submitters
/// consume this shape. `artist` and `title` are guaranteed non-empty by the
/// constructors; `album` is usually absent for radio (ICY `StreamTitle` carries
/// only `"Artist - Title"`, never an album) and `station_name` records the
/// originating station for the scrobble's service-name metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrobbleTrack {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub station_name: Option<String>,
}

impl ScrobbleTrack {
    /// Build from already-split ICY fields (as nokkvi's radio-metadata path
    /// produces them at `src/update/playback.rs`), cleaning each field and
    /// rejecting the track when either the artist or the title is missing or
    /// blank after cleanup.
    ///
    /// A scrobble with no artist is low-value and pollutes listen history, so
    /// title-only streams (where the station puts everything in the title with
    /// no `" - "` separator) are intentionally NOT scrobbled — the caller gets
    /// `None` and skips submission. `album` / `station_name` are cleaned and
    /// dropped to `None` when blank.
    pub fn from_icy(
        artist: Option<&str>,
        title: Option<&str>,
        album: Option<&str>,
        station_name: Option<&str>,
    ) -> Option<Self> {
        let artist = parse::clean(artist?);
        let title = parse::clean(title?);
        if artist.is_empty() || title.is_empty() {
            return None;
        }
        Some(Self {
            artist,
            title,
            album: album.map(parse::clean).filter(|s| !s.is_empty()),
            station_name: station_name.map(parse::clean).filter(|s| !s.is_empty()),
        })
    }

    /// Build directly from already-cleaned `artist`/`title` (the timing state
    /// machine stores cleaned values, so action-driven submits skip the
    /// `from_icy` re-clean). `album` is always absent for radio.
    pub fn from_clean(artist: String, title: String, station_name: Option<String>) -> Self {
        Self {
            artist,
            title,
            album: None,
            station_name,
        }
    }
}

/// Which scrobble targets a submit should attempt. A retry narrows to only the
/// targets that haven't yet succeeded, so a target that already accepted the
/// listen is never re-submitted (the listen's fixed timestamp would otherwise
/// drive redundant idempotent re-sends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrobbleTargets {
    pub listenbrainz: bool,
    pub lastfm: bool,
}

impl ScrobbleTargets {
    /// Both targets (the first attempt for a track, and every now-playing).
    pub const ALL: Self = Self {
        listenbrainz: true,
        lastfm: true,
    };

    /// True when at least one target is requested.
    pub fn any(self) -> bool {
        self.listenbrainz || self.lastfm
    }
}

/// Per-target outcome of a submit. `None` = not attempted (the target is
/// unconfigured or wasn't requested this round); `Some(Ok)` = accepted;
/// `Some(Err)` = a retryable failure. Lets the caller latch each target
/// independently instead of collapsing both into one pass/fail.
#[derive(Debug, Clone, Default)]
pub struct RadioSubmitOutcome {
    pub listenbrainz: Option<Result<(), String>>,
    pub lastfm: Option<Result<(), String>>,
}

impl RadioSubmitOutcome {
    /// Collapse to a single best-effort result (for the now-playing path, which
    /// has no per-target retry): `Ok` unless a target errored, joining messages.
    pub fn into_combined(self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Some(Err(e)) = self.listenbrainz {
            errors.push(format!("ListenBrainz: {e}"));
        }
        if let Some(Err(e)) = self.lastfm {
            errors.push(format!("Last.fm: {e}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_icy_builds_full_track() {
        let t = ScrobbleTrack::from_icy(
            Some("Daft Punk"),
            Some("Around the World"),
            Some("Homework"),
            Some("SomaFM Groove Salad"),
        )
        .expect("complete metadata must build a track");
        assert_eq!(t.artist, "Daft Punk");
        assert_eq!(t.title, "Around the World");
        assert_eq!(t.album.as_deref(), Some("Homework"));
        assert_eq!(t.station_name.as_deref(), Some("SomaFM Groove Salad"));
    }

    #[test]
    fn from_icy_rejects_missing_or_blank_artist() {
        assert!(ScrobbleTrack::from_icy(None, Some("Title"), None, None).is_none());
        assert!(ScrobbleTrack::from_icy(Some("   "), Some("Title"), None, None).is_none());
        assert!(ScrobbleTrack::from_icy(Some("\0\0"), Some("Title"), None, None).is_none());
    }

    #[test]
    fn from_icy_rejects_missing_or_blank_title() {
        assert!(ScrobbleTrack::from_icy(Some("Artist"), None, None, None).is_none());
        assert!(ScrobbleTrack::from_icy(Some("Artist"), Some("  "), None, None).is_none());
    }

    #[test]
    fn from_icy_drops_blank_album_and_station_to_none() {
        let t = ScrobbleTrack::from_icy(Some("A"), Some("B"), Some("   "), Some(""))
            .expect("blank optionals must not block a valid track");
        assert_eq!(t.album, None);
        assert_eq!(t.station_name, None);
    }
}
