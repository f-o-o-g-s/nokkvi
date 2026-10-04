//! The playlist the queue was loaded from, as the settings store keeps it.

use super::settings::LivePlayerSettings;

/// The stored active-playlist context: what `SettingsService::set_active_playlist`
/// persists and session restore reads back.
///
/// On disk these stay the flat `active_playlist_*` rows of
/// `PersistedPlayerSettings` (the golden-bytes tests pin that layout); this
/// value only carries them between the UI and the settings manager, so the
/// three `String` fields (name, comment, updated) travel by name instead of
/// as positional arguments that could trade places.
///
/// `Default` is the cleared context the store writes when no playlist is
/// active: empty strings, 0 duration and count, private.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActivePlaylistRecord {
    pub id: String,
    pub name: String,
    pub comment: String,
    /// Total duration in seconds, or 0.0 when unknown.
    pub duration_secs: f32,
    /// Last-updated timestamp (raw ISO-8601), or empty when unknown.
    pub updated: String,
    pub public: bool,
    /// Song count, or 0 when unknown.
    pub song_count: u32,
}

impl LivePlayerSettings {
    /// The stored active-playlist context, or `None` when no playlist is
    /// active (no stored id).
    pub fn active_playlist(&self) -> Option<ActivePlaylistRecord> {
        let id = self.active_playlist_id.clone()?;
        Some(ActivePlaylistRecord {
            id,
            name: self.active_playlist_name.clone(),
            comment: self.active_playlist_comment.clone(),
            duration_secs: self.active_playlist_duration,
            updated: self.active_playlist_updated.clone(),
            public: self.active_playlist_public,
            song_count: self.active_playlist_song_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ActivePlaylistRecord;
    use crate::services::{settings::SettingsManager, state_storage::StateStorage};

    fn manager() -> (SettingsManager, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = StateStorage::new(tmp.path().join("test_settings.redb")).expect("storage");
        (SettingsManager::for_test(storage), tmp)
    }

    /// The write (record → flat `active_playlist_*` rows) and read (rows →
    /// record) mappings are separate field lists; every field must come back
    /// where it went in. Distinct values per field so a crossed pair fails.
    #[test]
    fn stored_record_reads_back_field_for_field() {
        let (mut mgr, _tmp) = manager();
        let record = ActivePlaylistRecord {
            id: "pl-7".to_string(),
            name: "Night Drive".to_string(),
            comment: "for the long way home".to_string(),
            duration_secs: 1234.5,
            updated: "2026-10-04T06:00:00Z".to_string(),
            public: true,
            song_count: 42,
        };

        mgr.set_active_playlist(Some(record.clone()))
            .expect("set_active_playlist");

        assert_eq!(mgr.get_player_settings().active_playlist(), Some(record));
    }

    /// Clearing writes the cleared defaults, and reads back as no context.
    #[test]
    fn clearing_the_record_reads_back_as_none() {
        let (mut mgr, _tmp) = manager();
        mgr.set_active_playlist(Some(ActivePlaylistRecord {
            id: "pl-7".to_string(),
            name: "Night Drive".to_string(),
            public: true,
            song_count: 42,
            ..Default::default()
        }))
        .expect("set");

        mgr.set_active_playlist(None).expect("clear");

        let live = mgr.get_player_settings();
        assert_eq!(live.active_playlist(), None);
        assert!(live.active_playlist_name.is_empty());
        assert!(!live.active_playlist_public);
        assert_eq!(live.active_playlist_song_count, 0);
    }
}
