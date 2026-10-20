use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::player_settings::TrackArtwork;

/// ReplayGain data from the Subsonic API.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
)]
pub struct ReplayGain {
    #[serde(rename = "albumGain")]
    pub album_gain: Option<f64>,
    #[serde(rename = "trackGain")]
    pub track_gain: Option<f64>,
    #[serde(rename = "albumPeak")]
    pub album_peak: Option<f64>,
    #[serde(rename = "trackPeak")]
    pub track_peak: Option<f64>,
}

#[derive(
    Debug, Clone, Default, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode,
)]
pub struct Song {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "title", default)]
    pub title: String,
    #[serde(rename = "artist", default)]
    pub artist: String,
    #[serde(rename = "artistId")]
    pub artist_id: Option<String>,
    #[serde(rename = "album", default)]
    pub album: String,
    #[serde(rename = "albumId")]
    pub album_id: Option<String>,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
    // Duration can be a float in API, convert to u32 (seconds)
    #[serde(rename = "duration", default)]
    #[serde(deserialize_with = "deserialize_duration")]
    pub duration: u32, // seconds
    #[serde(rename = "trackNumber", alias = "track")]
    pub track: Option<u32>,
    #[serde(rename = "discNumber")]
    pub disc: Option<u32>,
    #[serde(rename = "year")]
    pub year: Option<u32>,
    #[serde(rename = "genre")]
    pub genre: Option<String>,
    #[serde(rename = "path", default)]
    pub path: String,
    #[serde(rename = "size", default)]
    pub size: u64,
    #[serde(rename = "bitRate")]
    pub bitrate: Option<u32>,
    #[serde(rename = "starred", default)]
    #[serde(deserialize_with = "crate::types::deserialize_starred")]
    pub starred: bool,
    #[serde(rename = "playCount")]
    pub play_count: Option<u32>,
    // Additional fields for songs view sorting
    #[serde(rename = "bpm")]
    pub bpm: Option<u32>,
    #[serde(rename = "channels", alias = "channelCount")]
    pub channels: Option<u32>,
    #[serde(rename = "comment")]
    pub comment: Option<String>,
    #[serde(rename = "rating", alias = "userRating")]
    pub rating: Option<u32>,
    // `displayAlbumArtist` is the Subsonic `Child` spelling (Navidrome sets it
    // from `mf.AlbumArtist`); the native `/api/song` sends `albumArtist`. The
    // alias keeps the crossfade policy's "Various Artists" compilation heuristic
    // alive for every Subsonic-sourced batch (getRandomSongs, getSimilarSongs2,
    // getTopSongs), which carry no `compilation` flag at all.
    #[serde(rename = "albumArtist", alias = "displayAlbumArtist")]
    pub album_artist: Option<String>,
    #[serde(rename = "suffix")]
    pub suffix: Option<String>,
    #[serde(rename = "sampleRate", alias = "samplingRate")]
    pub sample_rate: Option<u32>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(rename = "playDate")]
    pub play_date: Option<String>,
    // Fields added for info modal parity with Feishin
    #[serde(rename = "compilation", default)]
    pub compilation: Option<bool>,
    #[serde(rename = "bitDepth")]
    pub bit_depth: Option<u32>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    /// Read from either API shape (see [`deserialize_replay_gain`]); written
    /// back as the Subsonic `replayGain` object. Flattened only so the
    /// deserializer can see the native API's four top-level `rg*` keys; the
    /// field itself (and so the bincode queue layout) is unchanged.
    #[serde(
        flatten,
        deserialize_with = "deserialize_replay_gain",
        serialize_with = "serialize_replay_gain"
    )]
    pub replay_gain: Option<ReplayGain>,
    /// Dynamic metadata tags from Navidrome (barcode, ISRC, etc.)
    #[serde(default)]
    pub tags: Option<HashMap<String, Vec<String>>>,
    /// Role-based participants (composer, lyricist, producer, etc.)
    #[serde(default)]
    pub participants: Option<HashMap<String, Vec<crate::types::album::Participant>>>,
    /// Original position when added to queue (for Queue Order sort restoration).
    /// Only meaningful in queue context; `None` for songs not in a queue.
    #[serde(default)]
    pub original_position: Option<u32>,
}

/// Read a song's ReplayGain from whichever shape the API sent: the Subsonic
/// `replayGain` object (`getPlaylist`, `getSimilarSongs2`, `getRandomSongs`,
/// play queue) or the native `/api/song` flat `rgTrackGain` / `rgAlbumGain` /
/// `rgTrackPeak` / `rgAlbumPeak` fields (all `null` for an untagged file).
fn deserialize_replay_gain<'de, D>(deserializer: D) -> Result<Option<ReplayGain>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Shapes {
        #[serde(rename = "replayGain", default)]
        subsonic: Option<ReplayGain>,
        #[serde(rename = "rgAlbumGain", default)]
        album_gain: Option<f64>,
        #[serde(rename = "rgTrackGain", default)]
        track_gain: Option<f64>,
        #[serde(rename = "rgAlbumPeak", default)]
        album_peak: Option<f64>,
        #[serde(rename = "rgTrackPeak", default)]
        track_peak: Option<f64>,
    }

    // Navidrome before 0.57 (7640c474) sent 0 rather than null for a missing
    // value. A scope whose gain and peak are BOTH exactly 0 is untagged: no
    // real track peaks at 0.0, and a genuine 0 dB gain carries its peak.
    fn scope(gain: Option<f64>, peak: Option<f64>) -> (Option<f64>, Option<f64>) {
        if gain == Some(0.0) && peak == Some(0.0) {
            (None, None)
        } else {
            (gain, peak)
        }
    }

    let shapes = Shapes::deserialize(deserializer)?;
    Ok(shapes.subsonic.or_else(|| {
        let (album_gain, album_peak) = scope(shapes.album_gain, shapes.album_peak);
        let (track_gain, track_peak) = scope(shapes.track_gain, shapes.track_peak);
        let native = ReplayGain {
            album_gain,
            track_gain,
            album_peak,
            track_peak,
        };
        (native != ReplayGain::default()).then_some(native)
    }))
}

/// Write the flattened `replay_gain` back as the `replayGain` key it has
/// always serialized to (`null` when absent).
fn serialize_replay_gain<S>(
    replay_gain: &Option<ReplayGain>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry("replayGain", replay_gain)?;
    map.end()
}

// Helper to deserialize duration (can be f64 or u32)
fn deserialize_duration<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Duration {
        Float(f64),
        Int(u32),
    }

    match Duration::deserialize(deserializer)? {
        Duration::Float(f) => Ok(f.clamp(0.0, u32::MAX as f64) as u32),
        Duration::Int(i) => Ok(i),
    }
}

impl Song {
    /// Whether this song is starred. Song's `starred` field is a plain
    /// `bool` (the JSON parser maps both `false` and the empty-string
    /// timestamp to `false` via `deserialize_starred`), so there's no
    /// `Option` to unwrap here. Provided so callers can use the uniform
    /// `entity.is_starred()` accessor pattern Album / Artist already have.
    pub fn is_starred(&self) -> bool {
        self.starred
    }

    /// Server-reported duration in milliseconds, for sanity-checking the
    /// audio decoder's probed duration (`None` when the server reports no
    /// duration). Navidrome's value comes from a full taglib scan, so it
    /// stays trustworthy even when Symphonia's probe falls back to a
    /// bitrate-extrapolated estimate (see GH pdeljanov/Symphonia#516).
    pub fn expected_duration_ms(&self) -> Option<u64> {
        (self.duration > 0).then(|| u64::from(self.duration) * 1000)
    }

    /// Construct a minimal Song for unit tests. All optional fields default to `None`.
    #[cfg(test)]
    pub fn test_default(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            artist: "Artist".to_string(),
            artist_id: None,
            album: "Album".to_string(),
            album_id: None,
            cover_art: None,
            duration: 180,
            track: None,
            disc: None,
            year: None,
            genre: None,
            path: String::new(),
            size: 0,
            bitrate: None,
            starred: false,
            play_count: None,
            bpm: None,
            channels: None,
            comment: None,
            rating: None,
            album_artist: None,
            suffix: None,
            sample_rate: None,
            created_at: None,
            play_date: None,
            compilation: None,
            bit_depth: None,
            updated_at: None,
            replay_gain: None,
            tags: None,
            participants: None,
            original_position: None,
        }
    }
}

/// A song's cover key: the id its thumbnail and full-size art are fetched
/// and cached under. Every surface that shows a song's cover asks this, so
/// the Track Artwork setting flips them all at once.
pub trait SongCover {
    /// The `mf-<song id>` key of the art embedded in the song's own file;
    /// `None` when it has none or its source didn't say.
    fn own_cover(&self) -> Option<&str>;

    /// The album id the album's cover is keyed under.
    fn album_cover(&self) -> Option<&str>;

    /// The key `mode` shows: under `OwnCover` the song's own art when it has
    /// some, else (and under `AlbumCover`) the album's.
    fn cover_key(&self, mode: TrackArtwork) -> Option<&str> {
        match mode {
            TrackArtwork::AlbumCover => self.album_cover(),
            TrackArtwork::OwnCover => self.own_cover().or_else(|| self.album_cover()),
        }
    }
}

impl SongCover for Song {
    /// `cover_art` names the song's own art only as a mediafile cover id:
    /// Navidrome's Subsonic `coverArt` is `mf-<id>[_<timestamp>]` for a file
    /// with embedded art and an `al-` / `dc-` id otherwise, and the native
    /// parse writes `mf-<id>` from `hasCoverArt`
    /// (`SongsApiService::parse_song_response`). The suffix is dropped (as
    /// Navidrome's own `ParseArtworkID` splits it) so every source keys one
    /// track the same way.
    fn own_cover(&self) -> Option<&str> {
        let cover = self.cover_art.as_deref()?;
        cover
            .starts_with("mf-")
            .then(|| cover.split_once('_').map_or(cover, |(id, _)| id))
    }

    fn album_cover(&self) -> Option<&str> {
        self.album_id.as_deref()
    }
}

impl std::fmt::Display for Song {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} - {}", self.artist, self.title)
    }
}

impl crate::backend::Starable for Song {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_starred(&mut self, starred: bool) {
        self.starred = starred;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

impl crate::backend::Ratable for Song {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_rating(&mut self, rating: Option<u32>) {
        self.rating = rating;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

impl crate::backend::PlayCountable for Song {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn play_count(&self) -> Option<u32> {
        self.play_count
    }
    fn set_play_count(&mut self, count: Option<u32>) {
        self.play_count = count;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song_with(cover_art: Option<&str>, album_id: Option<&str>) -> Song {
        Song {
            cover_art: cover_art.map(str::to_string),
            album_id: album_id.map(str::to_string),
            ..Song::test_default("s1", "Song")
        }
    }

    /// Only a mediafile cover id names art of the song's own; Navidrome sends
    /// `al-` / `dc-` ids for a song without one. The `_<timestamp>` suffix is
    /// dropped so every source keys the same track the same way.
    #[test]
    fn own_cover_is_the_mediafile_cover_id_without_its_suffix() {
        assert_eq!(song_with(Some("mf-s1"), None).own_cover(), Some("mf-s1"));
        assert_eq!(
            song_with(Some("mf-s1_18f0c3a2"), None).own_cover(),
            Some("mf-s1")
        );
        assert_eq!(song_with(Some("al-a1_c12cf44e"), None).own_cover(), None);
        assert_eq!(song_with(Some("dc-a1:2"), None).own_cover(), None);
        assert_eq!(song_with(Some("a1"), None).own_cover(), None);
        assert_eq!(song_with(None, Some("a1")).own_cover(), None);
    }

    #[test]
    fn cover_key_follows_the_track_artwork_mode() {
        let own = song_with(Some("mf-s1"), Some("a1"));
        assert_eq!(own.cover_key(TrackArtwork::AlbumCover), Some("a1"));
        assert_eq!(own.cover_key(TrackArtwork::OwnCover), Some("mf-s1"));

        let bare = song_with(None, Some("a1"));
        assert_eq!(bare.cover_key(TrackArtwork::AlbumCover), Some("a1"));
        assert_eq!(bare.cover_key(TrackArtwork::OwnCover), Some("a1"));
    }

    /// Subsonic endpoints like `getSimilarSongs2` may omit `path` and `size`
    /// (Go's `omitempty` drops zero-value fields). Verify deserialization
    /// succeeds with defaults when these fields are absent.
    #[test]
    fn test_deserialize_song_missing_path_and_size() {
        let json = r#"{
            "id": "s1",
            "title": "Creep",
            "artist": "Radiohead",
            "album": "Pablo Honey",
            "duration": 239,
            "starred": false
        }"#;

        let song: Song = serde_json::from_str(json).expect("should deserialize without path/size");
        assert_eq!(song.id, "s1");
        assert_eq!(song.path, ""); // default empty string
        assert_eq!(song.size, 0); // default zero
    }

    /// Normal deserialization with all fields present should still work.
    #[test]
    fn test_deserialize_song_with_all_fields() {
        let json = r#"{
            "id": "s2",
            "title": "Paranoid Android",
            "artist": "Radiohead",
            "album": "OK Computer",
            "duration": 384,
            "path": "/music/radiohead/paranoid.flac",
            "size": 42000000,
            "starred": true
        }"#;

        let song: Song = serde_json::from_str(json).expect("should deserialize with all fields");
        assert_eq!(song.path, "/music/radiohead/paranoid.flac");
        assert_eq!(song.size, 42000000);
        assert!(song.starred);
    }

    /// Navidrome's native `/api/song` (album, song, genre and Songs-view
    /// loads) sends ReplayGain as four flat fields, not the Subsonic
    /// `replayGain` object. They must reach `replay_gain`, or every native
    /// load plays with no ReplayGain at all.
    #[test]
    fn native_song_json_carries_flat_replay_gain() {
        let json = r#"{
            "id": "n1",
            "title": "Waves",
            "duration": 320.01,
            "libraryId": 1,
            "mbzAlbumId": "x",
            "rgAlbumGain": -6.5,
            "rgAlbumPeak": 0.98,
            "rgTrackGain": -7.25,
            "rgTrackPeak": 0.95,
            "participants": {"artist": [{"id": "a1", "name": "A"}]}
        }"#;

        let song: Song = serde_json::from_str(json).expect("native song deserializes");

        assert_eq!(
            song.replay_gain,
            Some(ReplayGain {
                album_gain: Some(-6.5),
                track_gain: Some(-7.25),
                album_peak: Some(0.98),
                track_peak: Some(0.95),
            })
        );
        assert_eq!(song.duration, 320, "the other fields still parse");
        assert!(song.participants.is_some());
    }

    /// The Subsonic `replayGain` object keeps working, and a native song
    /// whose gains are all `null` (untagged file) carries none.
    #[test]
    fn subsonic_replay_gain_object_and_untagged_native_song() {
        let subsonic = r#"{"id": "s1", "replayGain": {"trackGain": -3.0, "albumGain": -5.0}}"#;
        let song: Song = serde_json::from_str(subsonic).expect("subsonic song deserializes");
        assert_eq!(
            song.replay_gain,
            Some(ReplayGain {
                album_gain: Some(-5.0),
                track_gain: Some(-3.0),
                album_peak: None,
                track_peak: None,
            })
        );

        let untagged = r#"{"id": "n2", "rgAlbumGain": null, "rgAlbumPeak": null,
            "rgTrackGain": null, "rgTrackPeak": null}"#;
        let song: Song = serde_json::from_str(untagged).expect("untagged song deserializes");
        assert_eq!(song.replay_gain, None);
    }

    /// Navidrome before 0.57 sent `0` instead of `null` for a missing gain
    /// or peak. A scope whose gain AND peak are both exactly zero is untagged
    /// (a real track never peaks at 0.0), so the user's fallback settings
    /// still apply; a real 0 dB gain carries a real peak and is kept.
    #[test]
    fn pre_057_navidrome_zeros_read_as_untagged() {
        let untagged = r#"{"id": "o1", "rgAlbumGain": 0, "rgAlbumPeak": 0,
            "rgTrackGain": 0, "rgTrackPeak": 0}"#;
        let song: Song = serde_json::from_str(untagged).expect("old-server song deserializes");
        assert_eq!(song.replay_gain, None, "all zeros = no tags");

        let track_only = r#"{"id": "o2", "rgAlbumGain": 0, "rgAlbumPeak": 0,
            "rgTrackGain": -7.25, "rgTrackPeak": 0.95}"#;
        let song: Song = serde_json::from_str(track_only).expect("old-server song deserializes");
        assert_eq!(
            song.replay_gain,
            Some(ReplayGain {
                album_gain: None,
                track_gain: Some(-7.25),
                album_peak: None,
                track_peak: Some(0.95),
            }),
            "a zeroed album scope must not shadow the track gain in Album mode"
        );

        let real_zero_db = r#"{"id": "n3", "rgTrackGain": 0.0, "rgTrackPeak": 0.8}"#;
        let song: Song = serde_json::from_str(real_zero_db).expect("song deserializes");
        assert_eq!(
            song.replay_gain.and_then(|rg| rg.track_gain),
            Some(0.0),
            "a genuine 0 dB track gain is kept"
        );
    }

    /// A song serializes its ReplayGain as the Subsonic `replayGain` object,
    /// as it always has, and reads it back unchanged.
    #[test]
    fn replay_gain_serializes_as_the_subsonic_object() {
        let mut song = Song::test_default("r1", "Round trip");
        song.replay_gain = Some(ReplayGain {
            album_gain: Some(-5.0),
            track_gain: Some(-3.0),
            album_peak: Some(0.9),
            track_peak: None,
        });

        let value = serde_json::to_value(&song).expect("serializes");
        assert_eq!(value["replayGain"]["trackGain"], -3.0);
        assert!(value.get("rgTrackGain").is_none());

        let back: Song = serde_json::from_value(value).expect("reads back");
        assert_eq!(back.replay_gain, song.replay_gain);

        let none = serde_json::to_value(Song::test_default("r2", "None")).expect("serializes");
        assert!(none["replayGain"].is_null(), "no ReplayGain stays `null`");
    }
}
