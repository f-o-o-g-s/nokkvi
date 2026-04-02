use crate::types::song::Song;

/// A single item representing a selection source. We use this to maintain the exact
/// visual Top-to-Bottom order that the user originally clicked things in, so the queue
/// will be built in the correct intuitive order.
#[derive(Debug, Clone)]
pub enum BatchItem {
    Song(Box<Song>),
    Album(String),
    Artist(String),
    Genre(String),
    Playlist(String),
}

/// A payload containing a batch of items (usually generated from a multi-selection shift+click).
#[derive(Debug, Clone, Default)]
pub struct BatchPayload {
    pub items: Vec<BatchItem>,
}

impl BatchPayload {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn with_item(mut self, item: BatchItem) -> Self {
        self.items.push(item);
        self
    }
}
