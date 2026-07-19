//! Playlist editing state — tracks dirty detection for unsaved changes.

/// State for an active playlist editing session.
///
/// Holds the playlist identity and a snapshot of song IDs at last save,
/// enabling dirty detection by comparing against the current queue order.
/// Also tracks the original playlist name, comment, and public flag for
/// rename/edit detection.
#[derive(Debug, Clone)]
pub struct PlaylistEditState {
    pub playlist_id: String,
    pub playlist_name: String,
    pub playlist_comment: String,
    pub playlist_public: bool,
    /// Name at session start — compared against `playlist_name` to detect renames.
    original_name: String,
    /// Comment at session start — compared against `playlist_comment` to detect edits.
    original_comment: String,
    /// Public flag at session start — compared against `playlist_public` to detect toggles.
    original_public: bool,
    /// Song IDs in order at last save — compared against current queue to detect changes.
    saved_snapshot: Vec<String>,
    /// Server `updatedAt` token captured when the editor session opened.
    ///
    /// Used as an optimistic-concurrency guard: before a destructive
    /// full-overwrite (`replace_playlist_tracks`), the save path re-reads the
    /// playlist's current `updatedAt` and aborts if it changed, rather than
    /// silently destroying a concurrent server-side edit. Empty when the
    /// playlist had no token (e.g. a freshly-created empty playlist), in which
    /// case the staleness compare treats it as "not stale".
    loaded_updated_at: String,
}

impl PlaylistEditState {
    /// Create a new edit state with an initial snapshot of song IDs.
    pub fn new(
        playlist_id: String,
        playlist_name: String,
        playlist_comment: String,
        playlist_public: bool,
        song_ids: Vec<String>,
    ) -> Self {
        let original_name = playlist_name.clone();
        let original_comment = playlist_comment.clone();
        let original_public = playlist_public;
        Self {
            playlist_id,
            playlist_name,
            playlist_comment,
            playlist_public,
            original_name,
            original_comment,
            original_public,
            saved_snapshot: song_ids,
            // Defaulted empty so existing call sites are unchanged; the editor
            // open path seeds it via `set_loaded_updated_at`.
            loaded_updated_at: String::new(),
        }
    }

    /// Seed the server `updatedAt` token captured when the editor opened.
    ///
    /// Kept as a setter (rather than a `new()` parameter) so every existing
    /// `PlaylistEditState::new(...)` call site stays unchanged.
    pub fn set_loaded_updated_at(&mut self, updated_at: String) {
        self.loaded_updated_at = updated_at;
    }

    /// The server `updatedAt` token captured at editor open (empty if none).
    pub fn loaded_updated_at(&self) -> &str {
        &self.loaded_updated_at
    }

    /// Check whether the current queue differs from the saved snapshot (tracks only).
    pub fn is_dirty(&self, current_queue_ids: &[String]) -> bool {
        self.saved_snapshot != current_queue_ids
    }

    /// Check whether the playlist name has been changed from the original.
    pub fn is_name_dirty(&self) -> bool {
        self.playlist_name != self.original_name
    }

    /// Check whether the playlist comment has been changed from the original.
    pub fn is_comment_dirty(&self) -> bool {
        self.playlist_comment != self.original_comment
    }

    /// Check whether the public flag has been changed from the original.
    pub fn is_public_dirty(&self) -> bool {
        self.playlist_public != self.original_public
    }

    /// Whether any metadata field (name, comment, or public) is dirty.
    /// Used by the save handler to decide whether to call `update_playlist`.
    pub fn has_metadata_changes(&self) -> bool {
        self.is_name_dirty() || self.is_comment_dirty() || self.is_public_dirty()
    }

    /// Update the current playlist name (called on each keystroke).
    pub fn set_name(&mut self, name: String) {
        self.playlist_name = name;
    }

    /// Update the current playlist comment (called on each keystroke).
    pub fn set_comment(&mut self, comment: String) {
        self.playlist_comment = comment;
    }

    /// Update the current public flag (called on toggle).
    /// Re-baseline the metadata originals to the current values — called
    /// after a successful rules-session save so the dirty checks read
    /// clean without rebuilding the whole state.
    pub fn mark_metadata_saved(&mut self) {
        self.original_name = self.playlist_name.clone();
        self.original_comment = self.playlist_comment.clone();
        self.original_public = self.playlist_public;
    }

    pub fn set_public(&mut self, value: bool) {
        self.playlist_public = value;
    }

    /// Replace the saved snapshot with a new set of song IDs (after a successful save).
    /// Also updates the original name, comment, and public flag to match the current values.
    pub fn update_snapshot(&mut self, song_ids: Vec<String>) {
        self.saved_snapshot = song_ids;
        self.original_name = self.playlist_name.clone();
        self.original_comment = self.playlist_comment.clone();
        self.original_public = self.playlist_public;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    fn state(name: &str, comment: &str) -> PlaylistEditState {
        PlaylistEditState::new("p1".into(), name.into(), comment.into(), true, ids(&["s1"]))
    }

    #[test]
    fn playlist_edit_state_not_dirty_initially() {
        let state = PlaylistEditState::new(
            "p1".into(),
            "My Mix".into(),
            String::new(),
            true,
            ids(&["s1", "s2", "s3"]),
        );
        assert!(!state.is_dirty(&ids(&["s1", "s2", "s3"])));
    }

    #[test]
    fn playlist_edit_state_dirty_after_reorder() {
        let state = PlaylistEditState::new(
            "p1".into(),
            "My Mix".into(),
            String::new(),
            true,
            ids(&["s1", "s2", "s3"]),
        );
        assert!(state.is_dirty(&ids(&["s2", "s1", "s3"])));
    }

    #[test]
    fn playlist_edit_state_dirty_after_add() {
        let state = PlaylistEditState::new(
            "p1".into(),
            "My Mix".into(),
            String::new(),
            true,
            ids(&["s1", "s2", "s3"]),
        );
        assert!(state.is_dirty(&ids(&["s1", "s2", "s3", "s4"])));
    }

    #[test]
    fn playlist_edit_state_dirty_after_remove() {
        let state = PlaylistEditState::new(
            "p1".into(),
            "My Mix".into(),
            String::new(),
            true,
            ids(&["s1", "s2", "s3"]),
        );
        assert!(state.is_dirty(&ids(&["s1", "s3"])));
    }

    #[test]
    fn playlist_edit_update_snapshot() {
        let mut state = PlaylistEditState::new(
            "p1".into(),
            "My Mix".into(),
            String::new(),
            true,
            ids(&["s1", "s2", "s3"]),
        );
        let new_ids = ids(&["s2", "s1", "s3", "s4"]);
        state.update_snapshot(new_ids.clone());
        assert!(!state.is_dirty(&new_ids));
    }

    #[test]
    fn playlist_edit_state_name_not_dirty_initially() {
        let s = state("My Mix", "");
        assert!(!s.is_name_dirty());
    }

    #[test]
    fn playlist_edit_state_name_dirty_after_rename() {
        let mut s = state("My Mix", "");
        s.set_name("New Name".into());
        assert!(s.is_name_dirty());
    }

    #[test]
    fn playlist_edit_state_name_not_dirty_after_revert() {
        let mut s = state("My Mix", "");
        s.set_name("New Name".into());
        s.set_name("My Mix".into());
        assert!(!s.is_name_dirty());
    }

    #[test]
    fn playlist_edit_state_name_reset_after_save() {
        let mut s = state("My Mix", "");
        s.set_name("Renamed".into());
        assert!(s.is_name_dirty());
        s.update_snapshot(ids(&["s1"]));
        assert!(!s.is_name_dirty());
    }

    #[test]
    fn playlist_edit_state_comment_not_dirty_initially() {
        let s = state("Mix", "Original comment");
        assert!(!s.is_comment_dirty());
    }

    #[test]
    fn playlist_edit_state_comment_dirty_after_change() {
        let mut s = state("Mix", "Original comment");
        s.set_comment("Updated comment".into());
        assert!(s.is_comment_dirty());
    }

    #[test]
    fn playlist_edit_state_comment_not_dirty_after_revert() {
        let mut s = state("Mix", "Original comment");
        s.set_comment("Updated comment".into());
        s.set_comment("Original comment".into());
        assert!(!s.is_comment_dirty());
    }

    #[test]
    fn playlist_edit_state_comment_reset_after_save() {
        let mut s = state("Mix", "Original comment");
        s.set_comment("New comment".into());
        assert!(s.is_comment_dirty());
        s.update_snapshot(ids(&["s1"]));
        assert!(!s.is_comment_dirty());
    }

    // ---- Public flag (T1–T4) ----

    #[test]
    fn playlist_edit_state_public_not_dirty_initially() {
        let s = state("Mix", "");
        assert!(!s.is_public_dirty());
    }

    #[test]
    fn playlist_edit_state_public_dirty_after_toggle() {
        let mut s = state("Mix", "");
        s.set_public(false);
        assert!(s.is_public_dirty());
    }

    #[test]
    fn playlist_edit_state_public_not_dirty_after_revert() {
        let mut s = state("Mix", "");
        s.set_public(false);
        s.set_public(true);
        assert!(!s.is_public_dirty());
    }

    #[test]
    fn playlist_edit_state_public_reset_after_snapshot() {
        let mut s = state("Mix", "");
        s.set_public(false);
        assert!(s.is_public_dirty());
        s.update_snapshot(ids(&["s1"]));
        assert!(!s.is_public_dirty());
    }

    // ---- Optimistic-concurrency staleness guard (N10) ----

    #[test]
    fn save_skips_when_tracks_clean() {
        // Layer 1 gate condition: a loaded session whose buffer matches the
        // snapshot is NOT dirty, so the save path skips the destructive replace.
        let s = PlaylistEditState::new(
            "p1".into(),
            "Mix".into(),
            String::new(),
            true,
            ids(&["s1", "s2", "s3"]),
        );
        assert!(
            !s.is_dirty(&ids(&["s1", "s2", "s3"])),
            "an unchanged track list must report not-dirty (replace is skipped)"
        );
    }

    #[test]
    fn loaded_updated_at_round_trips() {
        let mut s = state("Mix", "");
        assert_eq!(s.loaded_updated_at(), "", "defaults empty");
        s.set_loaded_updated_at("2026-05-30T10:00:00Z".into());
        assert_eq!(s.loaded_updated_at(), "2026-05-30T10:00:00Z");
    }
}
