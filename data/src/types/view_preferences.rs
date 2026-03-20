/// All persisted view sort preferences — replaces the opaque 6-positional-parameter tuple
/// returned by `SettingsManager::get_view_preferences()`.
///
/// Moved from `src/app_message.rs` to the data crate so both `SettingsManager`
/// and the UI layer can reference it without circular dependency.
#[derive(Debug, Clone)]
pub struct AllViewPreferences {
    pub albums: crate::types::queue::SortPreferences,
    pub artists: crate::types::queue::SortPreferences,
    pub songs: crate::types::queue::SortPreferences,
    pub genres: crate::types::queue::SortPreferences,
    pub playlists: crate::types::queue::SortPreferences,
    pub queue: crate::types::queue::QueueSortPreferences,
}
