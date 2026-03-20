use serde::{Deserialize, Serialize};

use crate::types::{queue_sort_mode::QueueSortMode, sort_mode::SortMode};

/// Sort preferences for a view (sort mode + ascending/descending)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortPreferences {
    pub sort_mode: SortMode,
    pub sort_ascending: bool,
}

impl SortPreferences {
    pub fn new(sort_mode: SortMode, sort_ascending: bool) -> Self {
        Self {
            sort_mode,
            sort_ascending,
        }
    }
}

/// Queue-specific sort preferences (uses QueueSortMode instead of SortMode)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSortPreferences {
    pub sort_mode: QueueSortMode,
    pub sort_ascending: bool,
}

impl QueueSortPreferences {
    pub fn new(sort_mode: QueueSortMode, sort_ascending: bool) -> Self {
        Self {
            sort_mode,
            sort_ascending,
        }
    }
}

/// The playback queue — lightweight ordering and mode state.
///
/// Song data lives in `SongPool`; this struct holds only the ordered list of
/// song IDs, the current playback index, and mode flags.  Serialization cost
/// is proportional to the number of IDs × UUID length (~100 KB at 12k tracks)
/// rather than full `Song` structs (~5 MB).
///
/// The `order` array maps play-order positions to `song_ids` indices.
/// When shuffle is off, `order` is identity `[0, 1, 2, …]`.
/// When shuffle is on, `order` is Fisher-Yates shuffled.
/// `current_order` tracks position within `order`.
/// `queued` holds the order-index of the pre-buffered next song (for gapless/crossfade).
#[derive(Debug, Clone, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode)]
pub struct Queue {
    pub song_ids: Vec<String>,
    pub current_index: Option<usize>,
    /// Position in the `order` array (NOT in `song_ids`).
    #[serde(default)]
    pub current_order: Option<usize>,
    /// Maps play-order → `song_ids` index. Identity when shuffle is off.
    #[serde(default)]
    pub order: Vec<usize>,
    /// Order-index of the pre-buffered next song (gapless/crossfade prep).
    /// Set by `peek_next_song()`, consumed by `transition_to_queued()`.
    #[serde(default)]
    pub queued: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub consume: bool,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    bincode_next::Encode,
    bincode_next::Decode,
)]
pub enum RepeatMode {
    None,
    Track,
    Playlist,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            song_ids: Vec::new(),
            current_index: None,
            current_order: None,
            order: Vec::new(),
            queued: None,
            shuffle: false,
            repeat: RepeatMode::None,
            consume: false,
        }
    }
}
