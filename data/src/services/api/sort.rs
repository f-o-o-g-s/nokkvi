//! Per-domain sort-parameter and default-order tables for Navidrome API calls.
//!
//! Centralizes the per-entity match arms that previously lived inside each
//! `*ApiService`, so adding a new sort mode is one table edit. Drift between
//! domains (e.g. `mostPlayed` maps to `"playCount"` for Songs but `"play_count"`
//! for Albums/Artists) is documented per-arm rather than silently distributed
//! across files.

use rand::seq::SliceRandom;

/// Identifies which per-entity sort table to consult.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SortDomain {
    Albums,
    Artists,
    Songs,
    Genres,
    Playlists,
}

/// Map a UI-facing sort mode to the `_sort` query parameter for the given domain.
///
/// Returns `&'static str` — every per-arm value is a static literal, so no
/// allocation is needed at the call site.
pub(crate) fn map_sort_mode(domain: SortDomain, sort_mode: &str) -> &'static str {
    match domain {
        SortDomain::Albums => map_albums(sort_mode),
        SortDomain::Artists => map_artists(sort_mode),
        SortDomain::Songs => map_songs(sort_mode),
        SortDomain::Genres => map_genres(sort_mode),
        SortDomain::Playlists => map_playlists(sort_mode),
    }
}

/// Get the default `_order` (ASC / DESC) for a given sort mode in the given
/// domain. Used when the caller did not provide an explicit order.
pub(crate) fn default_order(domain: SortDomain, sort_mode: &str) -> &'static str {
    match domain {
        SortDomain::Albums => default_order_albums(sort_mode),
        SortDomain::Artists => default_order_artists(sort_mode),
        SortDomain::Songs => default_order_songs(sort_mode),
        SortDomain::Genres => default_order_genres(sort_mode),
        SortDomain::Playlists => default_order_playlists(sort_mode),
    }
}

/// Shuffle a vector of entities in-place (client-side random sort).
///
/// Navidrome does not support `_sort=random` for artists, genres, or playlists,
/// so those domains load by name and shuffle here.
pub(crate) fn apply_random_shuffle<T>(items: &mut [T]) {
    let mut rng = rand::rng();
    items.shuffle(&mut rng);
}

// =============================================================================
// Albums
// =============================================================================

fn map_albums(sort_mode: &str) -> &'static str {
    match sort_mode {
        "recentlyAdded" => "recently_added",
        "recentlyPlayed" => "play_date",
        "mostPlayed" => "play_count",
        "favorited" => "starred_at",
        "random" => "random",
        "name" => "name",
        "albumArtist" => "album_artist",
        "artist" => "artist",
        "year" => "max_year",
        "songCount" => "songCount",
        "duration" => "duration",
        "rating" => "rating",
        "genre" => "genre",
        "all" => "name",
        _ => "recently_added",
    }
}

fn default_order_albums(sort_mode: &str) -> &'static str {
    match sort_mode {
        "recentlyAdded" | "recentlyPlayed" | "mostPlayed" | "favorited" | "year" | "songCount"
        | "duration" | "rating" => "DESC",
        _ => "ASC",
    }
}

// =============================================================================
// Artists
// =============================================================================

fn map_artists(sort_mode: &str) -> &'static str {
    match sort_mode {
        "name" => "name",
        "favorited" => "starred_at",
        "mostPlayed" => "play_count",
        "albumCount" => "album_count",
        "songCount" => "song_count",
        "random" => "name", // Random is handled client-side
        _ => "name",
    }
}

fn default_order_artists(sort_mode: &str) -> &'static str {
    match sort_mode {
        "favorited" | "mostPlayed" | "albumCount" | "songCount" => "DESC",
        _ => "ASC",
    }
}

// =============================================================================
// Songs
// =============================================================================
//
// Note: Songs maps `mostPlayed` to `"playCount"` (camelCase) where Albums/Artists
// use `"play_count"` (snake_case). This drift is preserved as current Navidrome
// server behavior; harmonizing it is out of scope.

fn map_songs(sort_mode: &str) -> &'static str {
    match sort_mode {
        "recentlyAdded" => "createdAt",
        "recentlyPlayed" => "playDate",
        "mostPlayed" => "playCount", // camelCase quirk — see module note
        "favorited" => "starred_at",
        "random" => "random",
        "title" => "title",
        "album" => "album",
        "artist" => "artist",
        "albumArtist" => "order_album_artist_name",
        "year" => "year",
        "duration" => "duration",
        "bpm" => "bpm",
        "channels" => "channels",
        "genre" => "genre",
        "rating" => "rating",
        "comment" => "comment",
        _ => "createdAt",
    }
}

fn default_order_songs(sort_mode: &str) -> &'static str {
    match sort_mode {
        "recentlyAdded" | "recentlyPlayed" | "mostPlayed" | "favorited" | "year" | "duration"
        | "bpm" | "channels" | "rating" => "DESC",
        _ => "ASC",
    }
}

// =============================================================================
// Genres
// =============================================================================
//
// Note: `mostPlayed` is intentionally absent — falls through to the wildcard
// arm and maps to `"name"`. Preserved as current behavior.

fn map_genres(sort_mode: &str) -> &'static str {
    match sort_mode {
        "name" => "name",
        "albumCount" => "album_count",
        "songCount" => "song_count",
        "random" => "name", // Random is handled client-side
        _ => "name",
    }
}

fn default_order_genres(sort_mode: &str) -> &'static str {
    match sort_mode {
        "albumCount" | "songCount" => "DESC",
        _ => "ASC",
    }
}

// =============================================================================
// Playlists
// =============================================================================
//
// Note: `mostPlayed` is intentionally absent — falls through to the wildcard
// arm and maps to `"name"`. Preserved as current behavior.

fn map_playlists(sort_mode: &str) -> &'static str {
    match sort_mode {
        "name" => "name",
        "songCount" => "song_count",
        "duration" => "duration",
        "updatedAt" => "updated_at",
        "random" => "name", // Random is handled client-side
        _ => "name",
    }
}

fn default_order_playlists(sort_mode: &str) -> &'static str {
    match sort_mode {
        "songCount" | "duration" | "updatedAt" => "DESC",
        _ => "ASC",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // `mostPlayed` mapping — pins per-domain behavior including preserved drift.
    // =========================================================================

    #[test]
    fn albums_most_played_maps_to_play_count_snake_case() {
        assert_eq!(
            map_sort_mode(SortDomain::Albums, "mostPlayed"),
            "play_count"
        );
    }

    #[test]
    fn artists_most_played_maps_to_play_count_snake_case() {
        assert_eq!(
            map_sort_mode(SortDomain::Artists, "mostPlayed"),
            "play_count"
        );
    }

    /// Songs uses `"playCount"` (camelCase) where Albums/Artists use snake_case.
    /// This is current Navidrome server behavior — DO NOT "fix" without
    /// coordinating a server-API behavior change across the codebase.
    #[test]
    fn songs_most_played_maps_to_play_count_camel_case() {
        assert_eq!(map_sort_mode(SortDomain::Songs, "mostPlayed"), "playCount");
    }

    /// Genres has no `mostPlayed` arm — falls through to wildcard which is `"name"`.
    #[test]
    fn genres_most_played_falls_through_to_name() {
        assert_eq!(map_sort_mode(SortDomain::Genres, "mostPlayed"), "name");
    }

    /// Playlists has no `mostPlayed` arm — falls through to wildcard which is `"name"`.
    #[test]
    fn playlists_most_played_falls_through_to_name() {
        assert_eq!(map_sort_mode(SortDomain::Playlists, "mostPlayed"), "name");
    }

    // =========================================================================
    // `mostPlayed` default order — explicit DESC for the three domains that
    // have an arm, ASC fallback for genres/playlists (no arm).
    // =========================================================================

    #[test]
    fn albums_most_played_default_order_is_desc() {
        assert_eq!(default_order(SortDomain::Albums, "mostPlayed"), "DESC");
    }

    #[test]
    fn artists_most_played_default_order_is_desc() {
        assert_eq!(default_order(SortDomain::Artists, "mostPlayed"), "DESC");
    }

    #[test]
    fn songs_most_played_default_order_is_desc() {
        assert_eq!(default_order(SortDomain::Songs, "mostPlayed"), "DESC");
    }

    /// Genres has no `mostPlayed` in its default-order arm — falls to ASC.
    #[test]
    fn genres_most_played_default_order_falls_through_to_asc() {
        assert_eq!(default_order(SortDomain::Genres, "mostPlayed"), "ASC");
    }

    /// Playlists has no `mostPlayed` in its default-order arm — falls to ASC.
    #[test]
    fn playlists_most_played_default_order_falls_through_to_asc() {
        assert_eq!(default_order(SortDomain::Playlists, "mostPlayed"), "ASC");
    }

    // =========================================================================
    // Wildcard fallbacks — pin per-domain behavior so future drift breaks a test.
    // =========================================================================

    /// Albums wildcard fallback is `"recently_added"` (NOT `"name"`).
    #[test]
    fn albums_unknown_sort_falls_back_to_recently_added() {
        assert_eq!(
            map_sort_mode(SortDomain::Albums, "definitely-not-a-sort"),
            "recently_added"
        );
    }

    #[test]
    fn artists_unknown_sort_falls_back_to_name() {
        assert_eq!(
            map_sort_mode(SortDomain::Artists, "definitely-not-a-sort"),
            "name"
        );
    }

    /// Songs wildcard fallback is `"createdAt"` (camelCase), NOT `"recently_added"`.
    /// Distinct from Albums by design.
    #[test]
    fn songs_unknown_sort_falls_back_to_created_at_camel_case() {
        assert_eq!(
            map_sort_mode(SortDomain::Songs, "definitely-not-a-sort"),
            "createdAt"
        );
    }

    #[test]
    fn genres_unknown_sort_falls_back_to_name() {
        assert_eq!(
            map_sort_mode(SortDomain::Genres, "definitely-not-a-sort"),
            "name"
        );
    }

    #[test]
    fn playlists_unknown_sort_falls_back_to_name() {
        assert_eq!(
            map_sort_mode(SortDomain::Playlists, "definitely-not-a-sort"),
            "name"
        );
    }

    /// All wildcard default-order paths converge on ASC.
    #[test]
    fn unknown_sort_default_order_is_asc_in_every_domain() {
        for domain in [
            SortDomain::Albums,
            SortDomain::Artists,
            SortDomain::Songs,
            SortDomain::Genres,
            SortDomain::Playlists,
        ] {
            assert_eq!(
                default_order(domain, "definitely-not-a-sort"),
                "ASC",
                "{domain:?} unknown-sort default order must be ASC"
            );
        }
    }

    // =========================================================================
    // Random shuffle — smoke test that the helper actually permutes.
    // Determinism is not asserted (real rng); we just confirm length stability
    // and that every input element is still present.
    // =========================================================================

    #[test]
    fn apply_random_shuffle_preserves_length_and_elements() {
        let mut items: Vec<u32> = (0..32).collect();
        let original: Vec<u32> = items.clone();
        apply_random_shuffle(&mut items);

        assert_eq!(items.len(), original.len(), "length must be preserved");

        let mut sorted_items = items.clone();
        sorted_items.sort_unstable();
        let mut sorted_original = original.clone();
        sorted_original.sort_unstable();
        assert_eq!(
            sorted_items, sorted_original,
            "shuffle must be a permutation"
        );
    }

    #[test]
    fn apply_random_shuffle_on_empty_is_noop() {
        let mut items: Vec<u32> = Vec::new();
        apply_random_shuffle(&mut items);
        assert!(items.is_empty());
    }
}
