use serde::{Deserialize, Serialize};

/// Queue sort modes — each mode is a one-shot action that physically reorders
/// the queue.  Manual reorder (drag / hotkey) works from any mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueSortMode {
    Album,
    Artist,
    Title,
    Duration,
    Genre,
    Rating,
}

impl QueueSortMode {
    pub fn all() -> Vec<QueueSortMode> {
        vec![
            QueueSortMode::Album,
            QueueSortMode::Artist,
            QueueSortMode::Title,
            QueueSortMode::Duration,
            QueueSortMode::Genre,
            QueueSortMode::Rating,
        ]
    }
}

impl std::fmt::Display for QueueSortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueSortMode::Album => write!(f, "Album"),
            QueueSortMode::Artist => write!(f, "Artist"),
            QueueSortMode::Title => write!(f, "Title"),
            QueueSortMode::Duration => write!(f, "Duration"),
            QueueSortMode::Genre => write!(f, "Genre"),
            QueueSortMode::Rating => write!(f, "Rating"),
        }
    }
}
