//! Playlist editing state — tracks dirty detection for unsaved changes.

/// State for an active playlist editing session.
///
/// Holds the playlist identity and a snapshot of song IDs at last save,
/// enabling dirty detection by comparing against the current queue order.
/// Also tracks the original playlist name for rename detection.
#[derive(Debug, Clone)]
pub struct PlaylistEditState {
    pub playlist_id: String,
    pub playlist_name: String,
    /// Name at session start — compared against `playlist_name` to detect renames.
    original_name: String,
    /// Song IDs in order at last save — compared against current queue to detect changes.
    saved_snapshot: Vec<String>,
}

impl PlaylistEditState {
    /// Create a new edit state with an initial snapshot of song IDs.
    pub fn new(playlist_id: String, playlist_name: String, song_ids: Vec<String>) -> Self {
        let original_name = playlist_name.clone();
        Self {
            playlist_id,
            playlist_name,
            original_name,
            saved_snapshot: song_ids,
        }
    }

    /// Check whether the current queue differs from the saved snapshot (tracks only).
    pub fn is_dirty(&self, current_queue_ids: &[String]) -> bool {
        self.saved_snapshot != current_queue_ids
    }

    /// Check whether the playlist name has been changed from the original.
    pub fn is_name_dirty(&self) -> bool {
        self.playlist_name != self.original_name
    }

    /// Update the current playlist name (called on each keystroke).
    pub fn set_name(&mut self, name: String) {
        self.playlist_name = name;
    }

    /// Replace the saved snapshot with a new set of song IDs (after a successful save).
    /// Also updates the original name to match the current name.
    pub fn update_snapshot(&mut self, song_ids: Vec<String>) {
        self.saved_snapshot = song_ids;
        self.original_name = self.playlist_name.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn playlist_edit_state_not_dirty_initially() {
        let state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1", "s2", "s3"]));
        assert!(!state.is_dirty(&ids(&["s1", "s2", "s3"])));
    }

    #[test]
    fn playlist_edit_state_dirty_after_reorder() {
        let state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1", "s2", "s3"]));
        assert!(state.is_dirty(&ids(&["s2", "s1", "s3"])));
    }

    #[test]
    fn playlist_edit_state_dirty_after_add() {
        let state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1", "s2", "s3"]));
        assert!(state.is_dirty(&ids(&["s1", "s2", "s3", "s4"])));
    }

    #[test]
    fn playlist_edit_state_dirty_after_remove() {
        let state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1", "s2", "s3"]));
        assert!(state.is_dirty(&ids(&["s1", "s3"])));
    }

    #[test]
    fn playlist_edit_update_snapshot() {
        let mut state =
            PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1", "s2", "s3"]));
        let new_ids = ids(&["s2", "s1", "s3", "s4"]);
        state.update_snapshot(new_ids.clone());
        assert!(!state.is_dirty(&new_ids));
    }

    #[test]
    fn playlist_edit_state_name_not_dirty_initially() {
        let state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1"]));
        assert!(!state.is_name_dirty());
    }

    #[test]
    fn playlist_edit_state_name_dirty_after_rename() {
        let mut state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1"]));
        state.set_name("New Name".into());
        assert!(state.is_name_dirty());
    }

    #[test]
    fn playlist_edit_state_name_not_dirty_after_revert() {
        let mut state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1"]));
        state.set_name("New Name".into());
        state.set_name("My Mix".into());
        assert!(!state.is_name_dirty());
    }

    #[test]
    fn playlist_edit_state_name_reset_after_save() {
        let mut state = PlaylistEditState::new("p1".into(), "My Mix".into(), ids(&["s1"]));
        state.set_name("Renamed".into());
        assert!(state.is_name_dirty());
        state.update_snapshot(ids(&["s1"]));
        assert!(!state.is_name_dirty());
    }
}
