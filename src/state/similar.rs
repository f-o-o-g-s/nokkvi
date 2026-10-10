//! Ephemeral state for Similar Songs / Top Songs API results.

/// What a Similar-tab result list was fetched for. The header label is
/// formatted here, in one place, and the views match on the variant for the
/// tab name and the loading copy rather than parsing the label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimilarSource {
    /// `getSimilarSongs2` seeded by a song, album or artist; carries the
    /// seed's display name.
    SimilarTo(String),
    /// `getTopSongs` for an artist: `artist` is the display name the header
    /// shows, `lookup` is what the request sends (the artist id when the
    /// server supports it, else the name).
    TopSongs {
        artist: String,
        lookup: nokkvi_data::services::api::similar::TopSongsArtist,
    },
}

impl SimilarSource {
    /// Header label: "Similar to: Paranoid Android" / "Top Songs: Radiohead".
    pub fn label(&self) -> String {
        match self {
            Self::SimilarTo(seed) => format!("Similar to: {seed}"),
            Self::TopSongs { artist, .. } => format!("Top Songs: {artist}"),
        }
    }

    /// What the result list holds, for the empty and failure toasts.
    pub fn result_noun(&self) -> &'static str {
        match self {
            Self::SimilarTo(_) => "similar songs",
            Self::TopSongs { .. } => "top songs",
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
            SimilarSource::TopSongs {
                artist: "Radiohead".into(),
                lookup: nokkvi_data::services::api::similar::TopSongsArtist::Id {
                    id: "ar-1".into(),
                    name: "Radiohead".into(),
                },
            }
            .label(),
            "Top Songs: Radiohead"
        );
    }

    #[test]
    fn result_noun_names_each_source() {
        assert_eq!(
            SimilarSource::SimilarTo("x".into()).result_noun(),
            "similar songs"
        );
        assert_eq!(
            SimilarSource::TopSongs {
                artist: "x".into(),
                lookup: nokkvi_data::services::api::similar::TopSongsArtist::Name("x".into()),
            }
            .result_noun(),
            "top songs"
        );
    }
}
