use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::song::Song;

/// A pool of `Song` structs indexed by their ID for O(1) lookups.
///
/// The queue ordering is maintained separately in `Queue::song_ids`.
/// This struct holds the actual song data, decoupled from position.
///
/// Serialized as a flat `Vec<Song>` so the on-disk format is simple
/// and backward-compatible. Deserialization rebuilds the HashMap.
#[derive(Debug, Clone)]
pub struct SongPool {
    songs: HashMap<String, Song>,
}

impl SongPool {
    pub fn new() -> Self {
        Self {
            songs: HashMap::new(),
        }
    }

    /// Build a pool from a list of songs (used during deserialization and migration).
    pub fn from_songs(songs: Vec<Song>) -> Self {
        let map = songs.into_iter().map(|s| (s.id.clone(), s)).collect();
        Self { songs: map }
    }

    /// Look up a song by ID.
    pub fn get(&self, id: &str) -> Option<&Song> {
        self.songs.get(id)
    }

    /// Look up a song by ID (mutable).
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Song> {
        self.songs.get_mut(id)
    }

    /// Insert a single song into the pool.
    /// If a song with the same ID already exists, it is replaced.
    pub fn insert(&mut self, song: Song) {
        self.songs.insert(song.id.clone(), song);
    }

    /// Insert multiple songs into the pool.
    pub fn insert_many(&mut self, songs: Vec<Song>) {
        self.songs.reserve(songs.len());
        for song in songs {
            self.songs.insert(song.id.clone(), song);
        }
    }

    /// Remove a song from the pool by ID, returning it if present.
    pub fn remove(&mut self, id: &str) -> Option<Song> {
        self.songs.remove(id)
    }

    /// Number of songs in the pool.
    pub fn len(&self) -> usize {
        self.songs.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    /// Reconstruct an ordered `Vec<Song>` matching the given ID order.
    /// IDs not found in the pool are silently skipped.
    pub fn songs_in_order(&self, ids: &[String]) -> Vec<Song> {
        ids.iter()
            .filter_map(|id| self.songs.get(id).cloned())
            .collect()
    }

    /// Clear all songs from the pool.
    pub fn clear(&mut self) {
        self.songs.clear();
    }
}

impl Default for SongPool {
    fn default() -> Self {
        Self::new()
    }
}

// -- Serde: serialize/deserialize as Vec<Song> for persistence --

impl Serialize for SongPool {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as a flat Vec<Song> (order doesn't matter for pool)
        let songs: Vec<&Song> = self.songs.values().collect();
        songs.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SongPool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let songs: Vec<Song> = Vec::deserialize(deserializer)?;
        Ok(Self::from_songs(songs))
    }
}

// -- Bincode: native Encode/Decode (bypasses serde for persistence) --

impl bincode_next::Encode for SongPool {
    fn encode<E: bincode_next::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode_next::error::EncodeError> {
        let songs: Vec<&Song> = self.songs.values().collect();
        songs.encode(encoder)
    }
}

impl bincode_next::Decode<()> for SongPool {
    fn decode<D: bincode_next::de::Decoder<Context = ()>>(
        decoder: &mut D,
    ) -> Result<Self, bincode_next::error::DecodeError> {
        let songs: Vec<Song> = Vec::decode(decoder)?;
        Ok(Self::from_songs(songs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_song(id: &str, title: &str) -> Song {
        Song::test_default(id, title)
    }

    #[test]
    fn insert_and_get() {
        let mut pool = SongPool::new();
        pool.insert(make_song("a", "Song A"));
        assert_eq!(pool.len(), 1);
        assert_eq!(pool.get("a").unwrap().title, "Song A");
    }

    #[test]
    fn remove_returns_song() {
        let mut pool = SongPool::new();
        pool.insert(make_song("a", "Song A"));
        let removed = pool.remove("a");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().title, "Song A");
        assert!(pool.is_empty());
    }

    #[test]
    fn songs_in_order_respects_id_sequence() {
        let mut pool = SongPool::new();
        pool.insert(make_song("c", "Song C"));
        pool.insert(make_song("a", "Song A"));
        pool.insert(make_song("b", "Song B"));

        let ordered = pool.songs_in_order(&["b".to_string(), "a".to_string(), "c".to_string()]);
        let titles: Vec<&str> = ordered.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["Song B", "Song A", "Song C"]);
    }

    #[test]
    fn from_songs_builds_correct_map() {
        let songs = vec![make_song("x", "Song X"), make_song("y", "Song Y")];
        let pool = SongPool::from_songs(songs);
        assert_eq!(pool.len(), 2);
        assert!(pool.get("x").is_some());
        assert!(pool.get("y").is_some());
    }

    #[test]
    fn serde_roundtrip() {
        let mut pool = SongPool::new();
        pool.insert(make_song("a", "Song A"));
        pool.insert(make_song("b", "Song B"));

        let json = serde_json::to_string(&pool).unwrap();
        let deserialized: SongPool = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.len(), 2);
        assert_eq!(deserialized.get("a").unwrap().title, "Song A");
        assert_eq!(deserialized.get("b").unwrap().title, "Song B");
    }
}
