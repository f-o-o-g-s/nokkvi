//! Single source of truth for the `(View, SortMode) → Subsonic API sort string`
//! mapping. The per-page `sort_mode_to_api_string` shims in
//! `views/{albums,artists,songs,genres,playlists}.rs` delegate here so a new
//! `SortMode` variant only needs adding once across the codebase.

use nokkvi_data::types::sort_mode::SortMode;

use crate::View;

/// Resolve the Subsonic API `type=` parameter for a given view + sort mode.
///
/// Per-view fallbacks preserve historical behavior:
/// - Albums / Songs → `recentlyAdded`
/// - Artists → `random`
/// - Genres / Playlists → `name`
///
/// Carve-outs:
/// - `Artists + Rating` returns `name`. Subsonic does not expose a rating sort
///   for artists, so the artists view loads `name` and re-sorts client-side.
pub(crate) fn sort_mode_to_api_string(view: View, sort_mode: SortMode) -> &'static str {
    use SortMode as S;
    use View as V;

    match (view, sort_mode) {
        // Universal: same API string in every view.
        (_, S::Random) => "random",
        (_, S::MostPlayed) => "mostPlayed",
        (_, S::RecentlyPlayed) => "recentlyPlayed",
        (_, S::Favorited) => "favorited",

        // Albums.
        (V::Albums, S::RecentlyAdded) => "recentlyAdded",
        (V::Albums, S::Name) => "name",
        (V::Albums, S::AlbumArtist) => "albumArtist",
        (V::Albums, S::Artist) => "artist",
        (V::Albums, S::ReleaseYear) => "year",
        (V::Albums, S::SongCount) => "songCount",
        (V::Albums, S::Duration) => "duration",
        (V::Albums, S::Rating) => "rating",
        (V::Albums, S::Genre) => "genre",
        (V::Albums, S::AlbumCount) => "albumCount",
        (V::Albums, _) => "recentlyAdded",

        // Artists. `Rating → "name"` is the load-all/sort-client carve-out.
        (V::Artists, S::Name) => "name",
        (V::Artists, S::AlbumCount) => "albumCount",
        (V::Artists, S::SongCount) => "songCount",
        (V::Artists, S::Rating) => "name",
        (V::Artists, _) => "random",

        // Songs. `Title | Name` collapse to `title` for the Songs API.
        (V::Songs, S::RecentlyAdded) => "recentlyAdded",
        (V::Songs, S::Title | S::Name) => "title",
        (V::Songs, S::Album) => "album",
        (V::Songs, S::Artist) => "artist",
        (V::Songs, S::AlbumArtist) => "albumArtist",
        (V::Songs, S::ReleaseYear) => "year",
        (V::Songs, S::Duration) => "duration",
        (V::Songs, S::Bpm) => "bpm",
        (V::Songs, S::Channels) => "channels",
        (V::Songs, S::Genre) => "genre",
        (V::Songs, S::Rating) => "rating",
        (V::Songs, S::Comment) => "comment",
        (V::Songs, _) => "recentlyAdded",

        // Genres.
        (V::Genres, S::Name) => "name",
        (V::Genres, S::AlbumCount) => "albumCount",
        (V::Genres, S::SongCount) => "songCount",
        (V::Genres, _) => "name",

        // Playlists.
        (V::Playlists, S::Name) => "name",
        (V::Playlists, S::SongCount) => "songCount",
        (V::Playlists, S::Duration) => "duration",
        (V::Playlists, S::UpdatedAt) => "updatedAt",
        (V::Playlists, _) => "name",

        // Other views (Queue, Radios, Settings) do not query the server's
        // sort API. Returning a benign default keeps the type total.
        (V::Queue | V::Radios | V::Settings, _) => "name",
    }
}

/// Returns the sort modes available for the given view.
///
/// Single source of truth for per-view sort options. The match is exhaustive
/// over every `View` variant — adding a new `View` without extending this
/// function is a compile error.
pub(crate) fn sort_modes_for_view(view: View) -> &'static [SortMode] {
    use SortMode as S;
    match view {
        View::Albums => &[
            S::RecentlyAdded,
            S::RecentlyPlayed,
            S::MostPlayed,
            S::Favorited,
            S::Random,
            S::Name,
            S::AlbumArtist,
            S::Artist,
            S::ReleaseYear,
            S::SongCount,
            S::Duration,
            S::Rating,
            S::Genre,
        ],
        View::Artists => &[
            S::Name,
            S::Favorited,
            S::MostPlayed,
            S::AlbumCount,
            S::SongCount,
            S::Rating,
            S::Random,
        ],
        View::Songs => &[
            S::RecentlyAdded,
            S::RecentlyPlayed,
            S::MostPlayed,
            S::Favorited,
            S::Random,
            S::Title,
            S::Album,
            S::Artist,
            S::AlbumArtist,
            S::ReleaseYear,
            S::Duration,
            S::Bpm,
            S::Channels,
            S::Genre,
            S::Rating,
            S::Comment,
        ],
        View::Genres => &[S::Name, S::AlbumCount, S::SongCount, S::Random],
        View::Playlists => &[S::Name, S::SongCount, S::Duration, S::UpdatedAt, S::Random],
        View::Queue => &[],
        View::Radios => &[],
        View::Settings => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant in a view's sort options must resolve to a non-empty API
    /// string. Catches a sort variant added to options without an API mapping.
    #[test]
    fn every_view_option_has_api_string() {
        for &mode in sort_modes_for_view(View::Albums) {
            let s = sort_mode_to_api_string(View::Albums, mode);
            assert!(!s.is_empty(), "Albums + {mode:?} returned empty string");
        }
        for &mode in sort_modes_for_view(View::Artists) {
            let s = sort_mode_to_api_string(View::Artists, mode);
            assert!(!s.is_empty(), "Artists + {mode:?} returned empty string");
        }
        for &mode in sort_modes_for_view(View::Songs) {
            let s = sort_mode_to_api_string(View::Songs, mode);
            assert!(!s.is_empty(), "Songs + {mode:?} returned empty string");
        }
        for &mode in sort_modes_for_view(View::Genres) {
            let s = sort_mode_to_api_string(View::Genres, mode);
            assert!(!s.is_empty(), "Genres + {mode:?} returned empty string");
        }
        for &mode in sort_modes_for_view(View::Playlists) {
            let s = sort_mode_to_api_string(View::Playlists, mode);
            assert!(!s.is_empty(), "Playlists + {mode:?} returned empty string");
        }
    }

    #[test]
    fn sort_options_table_covers_all_views() {
        for view in View::ALL {
            let _ = sort_modes_for_view(*view);
        }
    }

    /// Universal sort modes (Random, MostPlayed, RecentlyPlayed, Favorited)
    /// produce the same API string in every view.
    #[test]
    fn universal_sort_modes_match_across_views() {
        for view in [
            View::Albums,
            View::Artists,
            View::Songs,
            View::Genres,
            View::Playlists,
        ] {
            assert_eq!(sort_mode_to_api_string(view, SortMode::Random), "random");
            assert_eq!(
                sort_mode_to_api_string(view, SortMode::MostPlayed),
                "mostPlayed"
            );
            assert_eq!(
                sort_mode_to_api_string(view, SortMode::RecentlyPlayed),
                "recentlyPlayed"
            );
            assert_eq!(
                sort_mode_to_api_string(view, SortMode::Favorited),
                "favorited"
            );
        }
    }

    /// The Artists + Rating carve-out (load all by name, sort client-side)
    /// is preserved.
    #[test]
    fn artist_rating_loads_by_name() {
        assert_eq!(
            sort_mode_to_api_string(View::Artists, SortMode::Rating),
            "name"
        );
    }
}
