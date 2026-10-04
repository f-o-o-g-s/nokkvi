//! The request shape shared by the songs / albums / artists list loaders.

use super::filter::LibraryFilter;

/// What a library list request asks for, apart from paging and the
/// per-endpoint switches (Artists' `album_artists_only`): sort mode and
/// order, an optional title search or entity filter, and the library scope.
///
/// One borrow replaces five loose arguments on every `load_*` loader in the
/// API and backend layers, so the two `&str` sort fields can't trade places.
/// `Default` is an empty, unscoped query; callers name the fields they set:
///
/// ```ignore
/// LibraryQuery { sort_mode: "mostPlayed", sort_order: "DESC", library_ids: &ids, ..Default::default() }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct LibraryQuery<'a> {
    /// Sort/filter mode wire name: `"recentlyAdded"`, `"random"`,
    /// `"title"`, `"favorited"`, … Each endpoint maps it to its own
    /// `_sort` value.
    pub sort_mode: &'a str,
    /// `"ASC"` or `"DESC"`. Empty falls back to the per-mode default.
    pub sort_order: &'a str,
    /// Title / name substring search. Ignored when `filter` is set.
    pub search_query: Option<&'a str>,
    /// Artist / album / genre / library scope from a navigation surface.
    pub filter: Option<&'a LibraryFilter>,
    /// When non-empty, restrict results to these library (music folder) IDs
    /// via repeatable `library_id` params. Empty omits the param: Navidrome
    /// already limits results to the libraries the user can access.
    pub library_ids: &'a [i32],
}
