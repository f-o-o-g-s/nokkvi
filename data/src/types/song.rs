use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// ReplayGain data from the Subsonic API.
#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode,
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

#[derive(Debug, Clone, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode)]
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
    #[serde(rename = "trackNumber")]
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
    #[serde(rename = "channels")]
    pub channels: Option<u32>,
    #[serde(rename = "comment")]
    pub comment: Option<String>,
    #[serde(rename = "rating")]
    pub rating: Option<u32>,
    #[serde(rename = "albumArtist")]
    pub album_artist: Option<String>,
    #[serde(rename = "suffix")]
    pub suffix: Option<String>,
    #[serde(rename = "sampleRate")]
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
    #[serde(rename = "replayGain", default)]
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
    pub fn format_duration(&self) -> String {
        let minutes = self.duration / 60;
        let seconds = self.duration % 60;
        format!("{minutes}:{seconds:02}")
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
