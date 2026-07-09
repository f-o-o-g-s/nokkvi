//! State for the Harbour home view: the discovery shelves and the
//! whole-library search results.
//!
//! Held directly on `Nokkvi` (borrowed into `HarbourViewData` at render time,
//! like every other view). These are one-shot loads, not paged library lists,
//! so they use plain `Vec`s + `loading` flags + generation counters for
//! stale-response rejection rather than `PagedBuffer` — the same shape as
//! [`crate::state::SimilarSongsState`].

use std::collections::HashMap;

use nokkvi_data::{
    backend::{albums::AlbumUIViewData, genres::GenreUIViewData, playlists::PlaylistUIViewData},
    types::{artist::Artist, library_search::LibrarySearchResults, song::Song},
};

/// All Harbour data: shelves + live search.
#[derive(Debug, Clone, Default)]
pub struct HarbourState {
    // --- Shelves (each a fixed top-N, populated by one joined load) ---
    /// "Recently Played" shelf (songs, `/api/song?_sort=recentlyPlayed`) — the
    /// actual tracks the user played, sorted by play date. Song-level rather
    /// than album-level so the shelf reflects individual plays.
    pub recently_played: Vec<Song>,
    /// "Recently Added" shelf (albums, `_sort=recentlyAdded`).
    pub recently_added: Vec<AlbumUIViewData>,
    /// "Random Playlists" shelf (2×2 quad tiles). `artwork_album_ids` are filled
    /// by a follow-up quad-id fan-out after the shelves land.
    pub playlists: Vec<PlaylistUIViewData>,
    /// "Random Genres" shelf (2×2 quad tiles). `artwork_album_ids` are filled
    /// by a follow-up quad-id fan-out after the shelves land.
    pub genres: Vec<GenreUIViewData>,

    // --- "Most Played" shelves (each a fixed top-N by play count) ---
    /// "Most Played Tracks" shelf (songs, `_sort=mostPlayed`).
    pub most_played_songs: Vec<Song>,
    /// "Most Played Albums" shelf (albums, `_sort=mostPlayed`).
    pub most_played_albums: Vec<AlbumUIViewData>,
    /// "Most Played Artists" shelf (artists, `_sort=mostPlayed`). Navidrome's
    /// artist play_count is a scan-time aggregate, so this reflects the last
    /// library scan rather than the most recent listening.
    pub most_played_artists: Vec<Artist>,
    /// "Most Played Genres" shelf — a client-side tally of the top-played songs
    /// by genre (Navidrome can't sort genres by plays). `artwork_album_ids` are
    /// filled by the shared genre quad-id fan-out; `song_count` carries the
    /// number of the user's top tracks in the genre (drives the subtitle).
    pub most_played_genres: Vec<GenreUIViewData>,

    // --- Shelf load lifecycle ---
    /// A shelf load is in flight.
    pub shelves_loading: bool,
    /// Bumped on every shelf (re)load; stale loader results whose captured
    /// generation no longer matches are dropped (Similar-view precedent).
    pub shelves_generation: u64,

    // --- Whole-library search ---
    /// Current header search query (drives shelves-to-results swap).
    pub search_query: String,
    /// Grouped results for the active query. `None` = show shelves (query empty
    /// or below the min-length threshold).
    pub search_results: Option<LibrarySearchResults>,
    /// A search fan-out is in flight.
    pub search_loading: bool,
    /// Bumped on every keystroke; stale search results are dropped.
    pub search_generation: u64,
    /// Resolved album ids for each searched playlist's quad thumbnail, keyed by
    /// playlist id. Filled by a follow-up fan-out after search results land
    /// (search-result playlists are raw and carry no album ids); accumulates
    /// across keystrokes so a re-search never re-resolves a known playlist.
    pub search_playlist_album_ids: HashMap<String, Vec<String>>,
    /// Resolved album ids for each searched genre's quad thumbnail, keyed by
    /// genre name (== id). Genre mirror of [`Self::search_playlist_album_ids`].
    pub search_genre_album_ids: HashMap<String, Vec<String>>,
}

impl HarbourState {
    /// True when no shelf has data yet — used by the switch-view guard to
    /// decide whether to fire an initial load.
    pub fn shelves_empty(&self) -> bool {
        self.recently_played.is_empty()
            && self.recently_added.is_empty()
            && self.playlists.is_empty()
            && self.genres.is_empty()
            && self.most_played_songs.is_empty()
            && self.most_played_albums.is_empty()
            && self.most_played_artists.is_empty()
            && self.most_played_genres.is_empty()
    }

    /// Distinct album ids across the album shelf (Recently Added), in a stable
    /// order — the set whose 80px covers the shelf renderer needs warmed. The
    /// Recently Played shelf is songs now, so its row/panel covers are warmed by
    /// `album_id` through the quad-id warmer instead (see `warm_harbour_artwork`).
    pub fn shelf_album_art_triples(&self) -> Vec<(String, Option<String>, String)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for album in self
            .recently_added
            .iter()
            .chain(self.most_played_albums.iter())
        {
            if seen.insert(album.id.clone()) {
                out.push((
                    album.id.clone(),
                    album.updated_at.clone(),
                    album.artwork_url.clone(),
                ));
            }
        }
        out
    }

    /// Drop all shelf data and bump the generations so any in-flight shelf
    /// load or search fan-out is discarded. Used when the library filter
    /// changes — everything must refetch against the new scope.
    ///
    /// The search **query** survives: the results are scope-stale (dropped
    /// here, along with their quad-id side-maps), but the library-filter
    /// handler / the next Harbour entry re-fires the kept query against the
    /// new scope so the user's search continues seamlessly.
    pub fn invalidate_shelves(&mut self) {
        self.recently_played.clear();
        self.recently_added.clear();
        self.playlists.clear();
        self.genres.clear();
        self.most_played_songs.clear();
        self.most_played_albums.clear();
        self.most_played_artists.clear();
        self.most_played_genres.clear();
        self.shelves_loading = false;
        self.shelves_generation = self.shelves_generation.wrapping_add(1);
        // The active search's results are scope-stale too; a bumped generation
        // drops any in-flight old-scope fan-out when it lands.
        self.search_results = None;
        self.search_loading = false;
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_playlist_album_ids.clear();
        self.search_genre_album_ids.clear();
    }
}
