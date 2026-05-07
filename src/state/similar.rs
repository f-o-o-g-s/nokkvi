//! Ephemeral state for Similar Songs / Top Songs API results.

/// Ephemeral state for Similar Songs / Top Songs API results.
///
/// Populated by `getSimilarSongs2` or `getTopSongs` API calls triggered from
/// context menus. Not persisted — re-triggered via right-click → Find Similar.
#[derive(Debug, Clone)]
pub struct SimilarSongsState {
    /// API result songs (one-shot, not PagedBuffer)
    pub songs: Vec<nokkvi_data::types::song::Song>,
    /// Provenance label: "Similar to: Paranoid Android" / "Top Songs: Radiohead"
    pub label: String,
    /// Whether an API call is currently in flight
    pub loading: bool,
}
