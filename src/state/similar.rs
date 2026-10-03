//! Ephemeral state for Similar Songs / Top Songs API results.

/// What a Similar-tab result list was fetched for. The header label is
/// formatted here, in one place, and the views match on the variant for the
/// tab name and the loading copy rather than parsing the label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimilarSource {
    /// `getSimilarSongs2` seeded by a song, album or artist; carries the
    /// seed's display name.
    SimilarTo(String),
    /// `getTopSongs` for an artist; carries the artist name.
    TopSongs(String),
}

impl SimilarSource {
    /// Header label: "Similar to: Paranoid Android" / "Top Songs: Radiohead".
    pub fn label(&self) -> String {
        match self {
            Self::SimilarTo(seed) => format!("Similar to: {seed}"),
            Self::TopSongs(artist) => format!("Top Songs: {artist}"),
        }
    }
}

/// Ephemeral state for Similar Songs / Top Songs API results.
///
/// Populated by `getSimilarSongs2` or `getTopSongs` API calls triggered from
/// context menus. Not persisted — re-triggered via right-click → Find Similar.
#[derive(Debug, Clone)]
pub struct SimilarSongsState {
    /// API result songs (one-shot, not PagedBuffer)
    pub songs: Vec<nokkvi_data::types::song::Song>,
    /// What the results were fetched for (drives the header label)
    pub source: SimilarSource,
    /// Whether an API call is currently in flight
    pub loading: bool,
}

#[cfg(test)]
mod tests {
    use super::SimilarSource;

    #[test]
    fn label_formats_each_source() {
        assert_eq!(
            SimilarSource::SimilarTo("Paranoid Android".into()).label(),
            "Similar to: Paranoid Android"
        );
        assert_eq!(
            SimilarSource::TopSongs("Radiohead".into()).label(),
            "Top Songs: Radiohead"
        );
    }
}
