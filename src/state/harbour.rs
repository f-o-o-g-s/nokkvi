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
    /// filled by the genre quad-id fan-out; `song_count` carries the number of
    /// the user's top tracks in the genre (drives the subtitle).
    pub most_played_genres: Vec<GenreUIViewData>,

    // --- The Random block's pre-drawn picks (re-rolled every shelves load,
    //     so the Refresh hotkey doubles as the re-roll). Each RandomPlay row
    //     previews its pick (thumbnail + facts + large panel) and activation
    //     plays exactly what is shown. ---
    /// Random Album pick (uniform count+offset draw — `load_random_album`).
    /// Deliberately not `_sort=random`: see `services::api::pagination`.
    pub random_album: Option<AlbumUIViewData>,
    /// Random Artist pick (uniform count+offset draw — `load_random_artist`).
    pub random_artist: Option<Artist>,
    /// Random Songs pre-drawn batch ([`RANDOM_SONGS_DRAW`] songs from Subsonic
    /// `getRandomSongs`); the first song is the row's teaser, activation plays
    /// the batch.
    ///
    /// [`RANDOM_SONGS_DRAW`]: crate::views::harbour::RANDOM_SONGS_DRAW
    pub random_songs: Vec<Song>,
    /// Random Genre pick (client-shuffled genres list; a non-zero `song_count` is
    /// preferred but not required — the count rides an opportunistic enrichment
    /// that can be absent). Its `artwork_album_ids` are filled by the shared
    /// genre quad fan-out.
    pub random_genre: Option<GenreUIViewData>,
    /// Random Playlist pick (client-shuffled playlists list, first with songs —
    /// an empty playlist can't be played). Its `artwork_album_ids` are filled by
    /// the playlist quad fan-out. A user-uploaded cover wins in render, but only
    /// once some other surface has warmed that 80px mini — `warm_harbour_artwork`
    /// does not fetch playlist custom minis, which is why the row's art gate
    /// checks the CACHE rather than the pick's id.
    pub random_playlist: Option<PlaylistUIViewData>,

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
    /// genre NAME — Harbour's genre identity throughout (a `Genre::id` from the
    /// server is a `tag.id` hash, not the name). Genre mirror of
    /// [`Self::search_playlist_album_ids`].
    pub search_genre_album_ids: HashMap<String, Vec<String>>,
}

impl HarbourState {
    /// True when no shelf has data yet — used by the switch-view guard to
    /// decide whether to fire an initial load.
    pub fn shelves_empty(&self) -> bool {
        self.recently_played.is_empty()
            && self.recently_added.is_empty()
            && self.most_played_songs.is_empty()
            && self.most_played_albums.is_empty()
            && self.most_played_artists.is_empty()
            && self.most_played_genres.is_empty()
            && self.random_album.is_none()
            && self.random_artist.is_none()
            && self.random_songs.is_empty()
            && self.random_genre.is_none()
            && self.random_playlist.is_none()
    }

    /// Distinct album ids across the album shelves (Recently Added, Most
    /// Played Albums) plus the Random Album pick, in a stable order — the set
    /// whose 80px covers the shelf renderer needs warmed. The song shelves
    /// warm their covers by `album_id` through the quad-id warmer instead
    /// (see `warm_harbour_artwork`).
    /// The shelf artists whose `ar-{id}` mini needs warming: every Most
    /// Played artist plus the Random Artist pick, minus art the server marked
    /// absent.
    pub fn shelf_artist_ids(&self) -> Vec<String> {
        self.most_played_artists
            .iter()
            .chain(self.random_artist.iter())
            .filter(|a| !a.image.image_absent)
            .map(|a| a.id.clone())
            .collect()
    }

    pub fn shelf_album_art_triples(&self) -> Vec<(String, Option<String>, String)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for album in self
            .recently_added
            .iter()
            .chain(self.most_played_albums.iter())
            .chain(self.random_album.iter())
        {
            // An empty URL is art the server marked absent: nothing to warm.
            if !album.artwork_url.is_empty() && seen.insert(album.id.clone()) {
                // Same entry shape as the Albums view's prefetch
                // (`album_prefetch_entry`): the version matches the URL's.
                out.push((
                    album.id.clone(),
                    nokkvi_data::types::image_info::artwork_version(
                        &album.image,
                        album.updated_at.as_deref(),
                    ),
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
        self.most_played_songs.clear();
        self.most_played_albums.clear();
        self.most_played_artists.clear();
        self.most_played_genres.clear();
        self.random_album = None;
        self.random_artist = None;
        self.random_songs.clear();
        self.random_genre = None;
        self.random_playlist = None;
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

/// The Harbour Trawl panel's procedural scene: the trawling longship and the
/// sea it sails on. Not server data, so it survives logout.
#[derive(Debug)]
pub struct HarbourScene {
    /// The trawling longship — a SEPARATE `BoatState` from the
    /// Lines-visualizer `Nokkvi.boat`, driven by the same per-frame
    /// `Message::BoatTick` but stepped against a procedural sea
    /// (`widgets::harbour_sea::sea_bars`) so it sails with no audio playing.
    /// Ticks only while the Harbour view is showing with an empty search
    /// (`update::boat::step_harbour_scene`); hidden otherwise with position
    /// preserved, mirroring the Lines boat's hide contract.
    pub boat: crate::widgets::boat::BoatState,
    /// Travelling phase of the procedural sea, in `[0, 1)`. Advanced by the
    /// boat tick at `harbour_sea::SEA_DRIFT_HZ`; wrap-safe because every
    /// layer's phase multiplier is an integer (see
    /// `widgets::harbour_sea::sea_bars`).
    pub sea_phase: f32,
    /// Completed phase cycles of the sea — incremented each time
    /// `sea_phase` wraps. Rare scene events (shooting star, leaping fish)
    /// hash THIS to vary their timing and trajectory per ~20 s cycle, which
    /// is what keeps a pure-phase animation from replaying an identical event
    /// loop forever.
    pub sea_cycle: u32,
    /// The sea heights the boat was stepped against this frame — stored so
    /// the view draws the SAME array the physics sampled (the coherence
    /// guarantee that keeps the hull sitting ON the drawn water).
    pub sea_bars: Vec<f64>,
}

impl Default for HarbourScene {
    fn default() -> Self {
        Self {
            boat: crate::widgets::boat::BoatState {
                // Start mid-panel so the first Harbour open doesn't watch the
                // boat surface from a corner; every other field lazily seeds
                // in `boat_physics::step()` (facing, rng, timers).
                x_ratio: 0.5,
                ..Default::default()
            },
            sea_phase: 0.0,
            sea_cycle: 0,
            sea_bars: Vec::new(),
        }
    }
}
