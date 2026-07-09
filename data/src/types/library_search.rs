//! Whole-library cross-entity search aggregate — the result shape of
//! [`crate::backend::app_service::AppService::search_library`]'s five-way
//! fan-out.
//!
//! Results stay as **separate typed groups** (never interleaved): the UI
//! renders per-entity sections, and downstream consumers (the future Trawl
//! seed picker) select from a specific group. Group order is the render
//! order: artists · albums · songs · genres · playlists.

use anyhow::Result;

use crate::types::{album::Album, artist::Artist, genre::Genre, playlist::Playlist, song::Song};

/// One whole-library search response: the five entity groups, each already
/// truncated to the fan-out's `per_type_limit`.
#[derive(Debug, Clone, Default)]
pub struct LibrarySearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
    pub genres: Vec<Genre>,
    pub playlists: Vec<Playlist>,
}

impl LibrarySearchResults {
    /// True when every group is empty (renders as "No matches").
    pub fn is_empty(&self) -> bool {
        self.artists.is_empty()
            && self.albums.is_empty()
            && self.songs.is_empty()
            && self.genres.is_empty()
            && self.playlists.is_empty()
    }

    /// Total rows across all five groups.
    pub fn total_len(&self) -> usize {
        self.artists.len()
            + self.albums.len()
            + self.songs.len()
            + self.genres.len()
            + self.playlists.len()
    }

    /// Merge the five per-entity fan-out outcomes into one aggregate.
    ///
    /// Partial-tolerant: a failed entity degrades to an empty group with a
    /// `warn!` (one flaky endpoint must not blank the whole search); `Err`
    /// only when **all five** lookups failed (network down / session gone).
    /// Every group is truncated to `per_type_limit` — albums/artists/songs
    /// are server-limited already, but the genre and playlist loaders have
    /// no limit param, so the cap is enforced uniformly here.
    pub fn from_partial(
        artists: Result<Vec<Artist>>,
        albums: Result<Vec<Album>>,
        songs: Result<Vec<Song>>,
        genres: Result<Vec<Genre>>,
        playlists: Result<Vec<Playlist>>,
        per_type_limit: usize,
    ) -> Result<Self> {
        let mut failures: Vec<String> = Vec::new();
        let artists = take_group("artists", artists, per_type_limit, &mut failures);
        let albums = take_group("albums", albums, per_type_limit, &mut failures);
        let songs = take_group("songs", songs, per_type_limit, &mut failures);
        let genres = take_group("genres", genres, per_type_limit, &mut failures);
        let playlists = take_group("playlists", playlists, per_type_limit, &mut failures);

        if failures.len() == 5 {
            return Err(anyhow::anyhow!(
                "all five library search lookups failed: {}",
                failures.join("; ")
            ));
        }

        Ok(Self {
            artists,
            albums,
            songs,
            genres,
            playlists,
        })
    }
}

/// Shared per-group merge step for [`LibrarySearchResults::from_partial`]:
/// truncate on success, degrade to empty + record the failure on error.
fn take_group<T>(
    label: &str,
    group: Result<Vec<T>>,
    per_type_limit: usize,
    failures: &mut Vec<String>,
) -> Vec<T> {
    match group {
        Ok(mut items) => {
            items.truncate(per_type_limit);
            items
        }
        Err(e) => {
            tracing::warn!("Library search: {label} lookup failed: {e:#}");
            failures.push(format!("{label}: {e:#}"));
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genre(name: &str) -> Genre {
        Genre {
            id: format!("g-{name}"),
            name: name.to_string(),
            album_count: 0,
            song_count: 0,
        }
    }

    #[test]
    fn default_is_empty_with_zero_total() {
        let results = LibrarySearchResults::default();
        assert!(results.is_empty());
        assert_eq!(results.total_len(), 0);
    }

    #[test]
    fn total_len_sums_all_groups_and_is_empty_flips() {
        let results = LibrarySearchResults {
            genres: vec![genre("Ambient"), genre("Doom")],
            ..Default::default()
        };
        assert!(!results.is_empty());
        assert_eq!(results.total_len(), 2);
    }

    #[test]
    fn from_partial_truncates_every_group_to_limit() {
        let results = LibrarySearchResults::from_partial(
            Ok(Vec::new()),
            Ok(Vec::new()),
            Ok(Vec::new()),
            Ok(vec![genre("a"), genre("b"), genre("c"), genre("d")]),
            Ok(Vec::new()),
            2,
        )
        .expect("partial merge with successes must be Ok");
        assert_eq!(results.genres.len(), 2);
        assert_eq!(results.genres[0].name, "a");
    }

    #[test]
    fn from_partial_degrades_failed_entity_to_empty_group() {
        let results = LibrarySearchResults::from_partial(
            Err(anyhow::anyhow!("artists endpoint down")),
            Ok(Vec::new()),
            Ok(Vec::new()),
            Ok(vec![genre("Ambient")]),
            Ok(Vec::new()),
            10,
        )
        .expect("one failed entity must not fail the merge");
        assert!(results.artists.is_empty());
        assert_eq!(results.genres.len(), 1);
    }

    #[test]
    fn from_partial_errs_only_when_all_five_fail() {
        let all_failed = LibrarySearchResults::from_partial(
            Err(anyhow::anyhow!("a")),
            Err(anyhow::anyhow!("b")),
            Err(anyhow::anyhow!("c")),
            Err(anyhow::anyhow!("d")),
            Err(anyhow::anyhow!("e")),
            10,
        );
        assert!(all_failed.is_err());

        let four_failed = LibrarySearchResults::from_partial(
            Err(anyhow::anyhow!("a")),
            Err(anyhow::anyhow!("b")),
            Err(anyhow::anyhow!("c")),
            Err(anyhow::anyhow!("d")),
            Ok(Vec::new()),
            10,
        )
        .expect("a single surviving lookup keeps the merge Ok");
        assert!(four_failed.is_empty());
    }
}
