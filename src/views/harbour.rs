//! Harbour — the home / landing view.
//!
//! A proper slot-list [`super::ViewPage`] (like Similar): a single flattened,
//! *heterogeneous* row list rendered through the shared slot-list machinery,
//! with a large horizontal artwork column and themed row chrome.
//!
//! Harbour owns its own row flattening ([`build_harbour_rows`]) because the
//! app's generic expansion machinery is homogeneous and single-open, and
//! Harbour needs many independently-collapsible sections mixing albums,
//! playlists, genres, and search results under one list.
//!
//! Two modes under a stable root:
//! - **Shelves** (empty search): four collapsible discovery sections
//!   (Recently Played, Recently Added, Random Playlists, Random Genres), each
//!   capped at [`HOT_PICKS_PER_SECTION`] hot picks. All four start collapsed —
//!   the home reads as a compact index; centering a header previews its section
//!   in the large artwork column ([`section_preview_panel`]).
//! - **Search** (non-empty header search): the whole-library search grouped
//!   into expandable per-entity sections, each defaulting to expanded.

use std::collections::{HashMap, HashSet};

use iced::{
    Alignment, Element, Length,
    widget::{container, image, mouse_area, text},
};
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, genres::GenreUIViewData, playlists::PlaylistUIViewData},
    types::{artist::Artist, batch::BatchItem, song::Song},
    utils::formatters::{format_duration_short, format_relative_time},
};

use crate::{
    app_message::Message,
    theme,
    widgets::{
        self, SlotListPageMessage, SlotListPageState,
        view_header::{SortMode, ViewHeaderConfig},
    },
};

/// Hot picks per shelf — every discovery section shows at most this many items.
/// Doubles as the loader's per-section fetch/truncate limit (referenced by
/// `update::harbour`) so the fetch cap and the render cap never drift.
pub(crate) const HOT_PICKS_PER_SECTION: usize = 4;

/// Minimum query length before the header search fires network work and before
/// the view drops the "keep typing" hint for the "searching / results" states.
/// Shared by the render gate (this module, [`build_harbour_rows`]) and the
/// fetch gate (`update::harbour::handle_harbour_search`) so the two can never
/// diverge — a mismatch would strand the user on a fake spinner at the boundary
/// length (fetch short-circuits while the view already left the hint state).
pub(crate) const SEARCH_MIN_CHARS: usize = 2;

/// Left inset for Harbour's rows. Harbour rows LEAD with artwork (no index
/// column), so the full-bleed covers form the view's leftmost content — at
/// the index-led views' 8px content inset they read cramped against the
/// window edge. Art-leading surfaces in the app sit a step deeper: the
/// queue's playlist strip pads 11, the modal family's rows pad 12. 12 keeps
/// Harbour on the modal-family rail the owner's eye signed off on.
const HARBOUR_ROW_INSET: f32 = 12.0;

/// Per-type result cap for whole-library search fan-outs (Harbour's grouped
/// preview AND the Trawl modal share it — refine the query rather than page).
pub(crate) const SEARCH_PREVIEW_LIMIT: usize = 8;

/// Stable identity for every collapsible Harbour section (shelf + search
/// group). Membership in the page's `collapsed` set is keyed on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HarbourSectionId {
    RecentlyPlayed,
    RecentlyAdded,
    MostPlayedTracks,
    MostPlayedAlbums,
    MostPlayedArtists,
    MostPlayedGenres,
    Playlists,
    Genres,
    SearchArtists,
    SearchAlbums,
    SearchSongs,
    SearchGenres,
    SearchPlaylists,
}

/// The five entity groups of a whole-library search — the `See all` target and
/// the section identity in the grouped results.
#[derive(Debug, Clone, Copy)]
pub enum HarbourSection {
    Artists,
    Albums,
    Songs,
    Genres,
    Playlists,
}

impl HarbourSection {
    /// The full library view a "See all" on this group routes to.
    pub(crate) fn target_view(self) -> crate::View {
        match self {
            Self::Artists => crate::View::Artists,
            Self::Albums => crate::View::Albums,
            Self::Songs => crate::View::Songs,
            Self::Genres => crate::View::Genres,
            Self::Playlists => crate::View::Playlists,
        }
    }
}

/// What playing an item row does. Most items map to a batch item; genres play
/// ~100 server-random songs of the genre instead of enqueuing the whole thing.
#[derive(Debug, Clone)]
pub(crate) enum PlayTarget {
    Item(BatchItem),
    GenreRandom(String),
}

/// Join a set of optional metadata facets into a single ` • `-separated line,
/// dropping the empty ones (a missing field shortens the line, no placeholder).
/// ` • ` matches the app's shipped metadata separator (`metadata_pill`), so the
/// subtitles and the preview-panel pill read as one system.
fn join_facts(parts: Vec<Option<String>>) -> String {
    parts.into_iter().flatten().collect::<Vec<_>>().join(" • ")
}

/// The section identity glyph — one mapping consumed by BOTH the header's
/// expanded/empty art square and [`section_preview_panel`]'s pill eyebrow, so
/// the two can never drift. Search groups share their shelf sibling's glyph.
pub(crate) fn section_icon(id: HarbourSectionId) -> &'static str {
    match id {
        HarbourSectionId::RecentlyPlayed => "assets/icons/clock.svg",
        HarbourSectionId::RecentlyAdded => "assets/icons/calendar.svg",
        HarbourSectionId::MostPlayedTracks | HarbourSectionId::SearchSongs => {
            "assets/icons/music-2.svg"
        }
        HarbourSectionId::MostPlayedAlbums | HarbourSectionId::SearchAlbums => {
            "assets/icons/disc-3.svg"
        }
        HarbourSectionId::MostPlayedArtists | HarbourSectionId::SearchArtists => {
            "assets/icons/mic.svg"
        }
        HarbourSectionId::MostPlayedGenres
        | HarbourSectionId::Genres
        | HarbourSectionId::SearchGenres => "assets/icons/tags.svg",
        HarbourSectionId::Playlists | HarbourSectionId::SearchPlaylists => {
            "assets/icons/list-music.svg"
        }
    }
}

/// A collapsed section header's first-pick teaser: the concrete subtitle line
/// plus the art keys the header square resolves through the same
/// custom→quad→single ladder as item rows ([`harbour_art_element`]).
#[derive(Debug)]
pub(crate) struct SectionTeaser {
    /// The first pick's ` • `-joined line ("Karma Police • Radiohead •
    /// Played 2 hours ago"). Missing facts drop — never a placeholder.
    pub subtitle: String,
    /// Single-mini key (album id, or artist id for artist sections).
    pub art_album_id: Option<String>,
    /// Album ids feeding the 2×2 quad (fewer than 2 = single mini).
    pub art_album_ids: Vec<String>,
    /// Custom playlist cover key — wins outright when its 80px mini is cached.
    pub custom_playlist_id: Option<String>,
}

/// The section header's subtitle line, resolved from its state:
/// - an empty shelf reads "Nothing here yet" in BOTH states (never a fake
///   pick, never a permanently-blank line);
/// - an expanded header quantifies its contents — "{count} picks" for shelves,
///   "{n} matches" for search groups below [`SEARCH_PREVIEW_LIMIT`] (the
///   fan-out returned under its cap, so it IS the true total) and
///   "8+ matches" at the cap, an honest floor;
/// - a collapsed header shows its first pick's concrete teaser line.
pub(crate) fn section_header_subtitle(
    teaser: Option<&SectionTeaser>,
    expanded: bool,
    count: usize,
    is_search: bool,
) -> String {
    let Some(teaser) = teaser else {
        return "Nothing here yet".to_string();
    };
    if !expanded {
        return teaser.subtitle.clone();
    }
    if is_search {
        if count >= SEARCH_PREVIEW_LIMIT {
            format!("{SEARCH_PREVIEW_LIMIT}+ matches")
        } else if count == 1 {
            "1 match".to_string()
        } else {
            format!("{count} matches")
        }
    } else if count == 1 {
        "1 pick".to_string()
    } else {
        format!("{count} picks")
    }
}

/// One flattened Harbour row. The single source of row order is
/// [`build_harbour_rows`], consumed by both the view (render) and the update
/// handler (which resolves the centered row into an action).
#[derive(Debug)]
pub(crate) enum HarbourRow {
    /// The Trawl mix-builder door — a fixed action row (first slot in
    /// shelves mode; absent during search). Activation opens the modal
    /// instead of expanding/playing; deliberately NOT a Section so it never
    /// joins the collapse machinery.
    Trawl {
        /// Seed count for the live subtitle ("3 seeds • Interleave").
        seeds: usize,
        /// Active blend, the subtitle's second fact.
        blend: nokkvi_data::types::trawl::TrawlBlend,
    },
    /// A collapsible section header. `see_all` is `Some` for search groups
    /// (routes to the full library view) and `None` for shelves.
    Section {
        id: HarbourSectionId,
        title: String,
        /// Items under this header (capped at the hot-picks / preview limit).
        /// Not rendered inline anymore — it feeds the view-header "N items"
        /// sum and the expanded "{count} picks" / "{n} matches" subtitle.
        count: usize,
        expanded: bool,
        see_all: Option<HarbourSection>,
        /// Section identity glyph ([`section_icon`]) — the art square of an
        /// expanded or empty header.
        glyph: &'static str,
        /// First-pick teaser (cover + concrete subtitle) a collapsed header
        /// shows; `None` for an empty shelf.
        teaser: Option<SectionTeaser>,
    },
    /// A playable item under an expanded section.
    Item {
        title: String,
        subtitle: String,
        /// Single-mini fallback + the large side-panel cover key.
        art_album_id: Option<String>,
        /// Album ids feeding the 2x2 quad thumbnail (empty = single mini).
        art_album_ids: Vec<String>,
        play: PlayTarget,
    },
    /// A non-interactive centered hint (search prompts / empty states).
    Hint(String),
}

/// The Trawl row's live subtitle: an invitation when the crate is empty,
/// otherwise the crate's two facts joined app-style ("3 seeds • Interleave").
pub(crate) fn trawl_row_subtitle(
    seeds: usize,
    blend: nokkvi_data::types::trawl::TrawlBlend,
) -> String {
    if seeds == 0 {
        "Build a mix from anything in the library".to_string()
    } else {
        let noun = if seeds == 1 { "seed" } else { "seeds" };
        format!("{seeds} {noun} • {}", blend.label())
    }
}

/// Build the flattened Harbour row list. The ONE source of row order —
/// [`HarbourPage::view`] renders it and `handle_harbour` re-derives it to
/// resolve the centered row into an action, so the two never drift.
pub(crate) fn build_harbour_rows(
    harbour: &crate::state::HarbourState,
    collapsed: &HashSet<HarbourSectionId>,
    trawl: &nokkvi_data::types::trawl::TrawlCrate,
) -> Vec<HarbourRow> {
    let mut rows = Vec::new();
    let query = harbour.search_query.trim();

    if query.is_empty() {
        // ── Shelves mode ──────────────────────────────────────────────────
        // The Trawl door leads the index — a stable first slot (the picker's
        // pinned "Clear default" precedent), hidden during search.
        rows.push(HarbourRow::Trawl {
            seeds: trawl.len(),
            blend: trawl.blend,
        });
        push_song_section(
            &mut rows,
            HarbourSectionId::RecentlyPlayed,
            "Recently Played",
            &harbour.recently_played,
            collapsed,
            false,
        );
        push_album_section(
            &mut rows,
            HarbourSectionId::RecentlyAdded,
            "Recently Added",
            &harbour.recently_added,
            collapsed,
            false,
        );
        // "Most Played" shelves. Each is hidden when its top item has no plays
        // (a fresh/near-empty library), so the shelf never shows arbitrary
        // zero-play rows. Genres derive from the tracks pool, so they share the
        // tracks' has-plays gate.
        let tracks_played = harbour
            .most_played_songs
            .first()
            .is_some_and(|s| s.play_count.unwrap_or(0) > 0);
        if tracks_played {
            push_song_section(
                &mut rows,
                HarbourSectionId::MostPlayedTracks,
                "Most Played Tracks",
                &harbour.most_played_songs,
                collapsed,
                true,
            );
        }
        if harbour
            .most_played_albums
            .first()
            .is_some_and(|a| a.play_count.unwrap_or(0) > 0)
        {
            push_album_section(
                &mut rows,
                HarbourSectionId::MostPlayedAlbums,
                "Most Played Albums",
                &harbour.most_played_albums,
                collapsed,
                true,
            );
        }
        if harbour
            .most_played_artists
            .first()
            .is_some_and(|a| a.play_count.unwrap_or(0) > 0)
        {
            push_artist_section(
                &mut rows,
                HarbourSectionId::MostPlayedArtists,
                "Most Played Artists",
                &harbour.most_played_artists,
                collapsed,
            );
        }
        // Genres derive from the tracks pool, so they need plays (tracks_played)
        // AND at least one tagged genre — otherwise the header would render an
        // empty "(0)" section when the top tracks carry no genre.
        if tracks_played && !harbour.most_played_genres.is_empty() {
            push_genre_section(
                &mut rows,
                HarbourSectionId::MostPlayedGenres,
                "Most Played Genres",
                &harbour.most_played_genres,
                collapsed,
                true,
            );
        }
        push_playlist_section(
            &mut rows,
            HarbourSectionId::Playlists,
            "Random Playlists",
            &harbour.playlists,
            collapsed,
        );
        push_genre_section(
            &mut rows,
            HarbourSectionId::Genres,
            "Random Genres",
            &harbour.genres,
            collapsed,
            false,
        );
        return rows;
    }

    // ── Search mode ───────────────────────────────────────────────────────
    if query.chars().count() < SEARCH_MIN_CHARS {
        rows.push(HarbourRow::Hint(
            "Keep typing to search your library…".to_string(),
        ));
        return rows;
    }
    let Some(results) = &harbour.search_results else {
        // No results yet: a live fan-out shows the searching hint; the
        // no-fan-out case is reachable only after a failed search (the error
        // arm clears the stale results), since every keystroke at or past the
        // threshold arms `search_loading` before the next render.
        rows.push(HarbourRow::Hint(if harbour.search_loading {
            "Searching…".to_string()
        } else {
            "Search failed — edit the query to retry.".to_string()
        }));
        return rows;
    };
    if results.is_empty() {
        rows.push(HarbourRow::Hint("No matches.".to_string()));
        return rows;
    }

    // Search sections default EXPANDED: they start OUT of the collapsed set,
    // so `!collapsed.contains(id)` is `true` until the user collapses one.
    if !results.artists.is_empty() {
        // Teaser: the bare name — the item rows' constant "Artist" fact would
        // be redundant beside the "Artists" section title.
        let teaser = results.artists.first().map(|a| SectionTeaser {
            subtitle: a.name.clone(),
            art_album_id: Some(a.id.clone()),
            art_album_ids: Vec::new(),
            custom_playlist_id: None,
        });
        let expanded = push_search_header(
            &mut rows,
            HarbourSectionId::SearchArtists,
            "Artists",
            HarbourSection::Artists,
            results.artists.len(),
            collapsed,
            teaser,
        );
        if expanded {
            for a in &results.artists {
                rows.push(HarbourRow::Item {
                    title: a.name.clone(),
                    subtitle: "Artist".to_string(),
                    // Artist images live in `album_art` keyed by the artist id
                    // (warmed via the `ar-{id}` endpoint) — the same single-mini
                    // path the standalone Artists view uses.
                    art_album_id: Some(a.id.clone()),
                    art_album_ids: Vec::new(),
                    play: PlayTarget::Item(BatchItem::Artist(a.id.clone())),
                });
            }
        }
    }
    if !results.albums.is_empty() {
        let teaser = results.albums.first().map(|a| SectionTeaser {
            subtitle: join_facts(vec![
                Some(a.name.clone()),
                a.artist
                    .clone()
                    .or_else(|| a.album_artist.clone())
                    .filter(|s| !s.is_empty()),
            ]),
            art_album_id: Some(a.id.clone()),
            art_album_ids: vec![a.id.clone()],
            custom_playlist_id: None,
        });
        let expanded = push_search_header(
            &mut rows,
            HarbourSectionId::SearchAlbums,
            "Albums",
            HarbourSection::Albums,
            results.albums.len(),
            collapsed,
            teaser,
        );
        if expanded {
            for a in &results.albums {
                let subtitle = a
                    .artist
                    .clone()
                    .or_else(|| a.album_artist.clone())
                    .unwrap_or_default();
                rows.push(HarbourRow::Item {
                    title: a.name.clone(),
                    subtitle,
                    art_album_id: Some(a.id.clone()),
                    art_album_ids: vec![a.id.clone()],
                    play: PlayTarget::Item(BatchItem::Album(a.id.clone())),
                });
            }
        }
    }
    if !results.songs.is_empty() {
        let teaser = results.songs.first().map(|s| SectionTeaser {
            subtitle: join_facts(vec![
                Some(s.title.clone()),
                (!s.artist.is_empty()).then(|| s.artist.clone()),
            ]),
            art_album_id: s.album_id.clone(),
            art_album_ids: s.album_id.clone().into_iter().collect(),
            custom_playlist_id: None,
        });
        let expanded = push_search_header(
            &mut rows,
            HarbourSectionId::SearchSongs,
            "Songs",
            HarbourSection::Songs,
            results.songs.len(),
            collapsed,
            teaser,
        );
        if expanded {
            for s in &results.songs {
                rows.push(HarbourRow::Item {
                    title: s.title.clone(),
                    subtitle: s.artist.clone(),
                    art_album_id: s.album_id.clone(),
                    art_album_ids: s.album_id.clone().into_iter().collect(),
                    play: PlayTarget::Item(BatchItem::Song(Box::new(s.clone()))),
                });
            }
        }
    }
    if !results.genres.is_empty() {
        // Teaser quad from the same resolved side-map the item rows read
        // (blank chassis until the fan-out lands, then atomic upgrade).
        let teaser = results.genres.first().map(|g| {
            let album_ids = harbour
                .search_genre_album_ids
                .get(&g.name)
                .cloned()
                .unwrap_or_default();
            SectionTeaser {
                subtitle: format!("{} • {} albums", g.name, g.album_count),
                art_album_id: album_ids.first().cloned(),
                art_album_ids: album_ids,
                custom_playlist_id: None,
            }
        });
        let expanded = push_search_header(
            &mut rows,
            HarbourSectionId::SearchGenres,
            "Genres",
            HarbourSection::Genres,
            results.genres.len(),
            collapsed,
            teaser,
        );
        if expanded {
            for g in &results.genres {
                // Quad thumbnail from the genre's resolved album ids (filled by
                // a follow-up fan-out; empty until it lands → blank, like the
                // shelf genres before their quad ids arrive).
                let album_ids = harbour
                    .search_genre_album_ids
                    .get(&g.name)
                    .cloned()
                    .unwrap_or_default();
                rows.push(HarbourRow::Item {
                    title: g.name.clone(),
                    subtitle: format!("{} albums", g.album_count),
                    art_album_id: album_ids.first().cloned(),
                    art_album_ids: album_ids,
                    play: PlayTarget::GenreRandom(g.name.clone()),
                });
            }
        }
    }
    if !results.playlists.is_empty() {
        let teaser = results.playlists.first().map(|p| {
            let album_ids = harbour
                .search_playlist_album_ids
                .get(&p.id)
                .cloned()
                .unwrap_or_default();
            SectionTeaser {
                subtitle: format!("{} • {} songs", p.name, p.song_count),
                art_album_id: album_ids.first().cloned(),
                art_album_ids: album_ids,
                custom_playlist_id: Some(p.id.clone()),
            }
        });
        let expanded = push_search_header(
            &mut rows,
            HarbourSectionId::SearchPlaylists,
            "Playlists",
            HarbourSection::Playlists,
            results.playlists.len(),
            collapsed,
            teaser,
        );
        if expanded {
            for p in &results.playlists {
                // Quad thumbnail from the playlist's resolved album ids. A custom
                // uploaded cover wins in render_row IF its 80px mini is already
                // cached (from a Playlists-view visit) — Harbour doesn't warm the
                // custom mini for its own rows, same as the Random Playlists shelf.
                let album_ids = harbour
                    .search_playlist_album_ids
                    .get(&p.id)
                    .cloned()
                    .unwrap_or_default();
                rows.push(HarbourRow::Item {
                    title: p.name.clone(),
                    subtitle: format!("{} songs", p.song_count),
                    art_album_id: album_ids.first().cloned(),
                    art_album_ids: album_ids,
                    play: PlayTarget::Item(BatchItem::Playlist(p.id.clone())),
                });
            }
        }
    }

    rows
}

/// A `"N plays"` fact for the Most Played shelves (drops to `""`-suppressed via
/// the caller when zero). Pluralises for readability.
fn plays_label(n: u32) -> String {
    format!("{n} {}", if n == 1 { "play" } else { "plays" })
}

/// Push a song shelf. `most_played` swaps the recency fact ("Played 3 days ago")
/// for a play-count fact ("42 plays") so a Most Played shelf never reads as if it
/// were sorted by recency.
fn push_song_section(
    rows: &mut Vec<HarbourRow>,
    id: HarbourSectionId,
    title: &str,
    songs: &[Song],
    collapsed: &HashSet<HarbourSectionId>,
    most_played: bool,
) {
    let expanded = !collapsed.contains(&id);
    // One fact builder for the teaser AND the item rows, so the header's
    // first-pick line can't drift from the row it previews.
    let song_fact = |s: &Song| {
        if most_played {
            Some(plays_label(s.play_count.unwrap_or(0)))
        } else {
            s.play_date
                .as_deref()
                .map(|d| format!("Played {}", format_relative_time(d)))
        }
    };
    let teaser = songs.first().map(|s| SectionTeaser {
        subtitle: join_facts(vec![
            Some(s.title.clone()),
            (!s.artist.is_empty()).then(|| s.artist.clone()),
            song_fact(s),
        ]),
        art_album_id: s.album_id.clone(),
        art_album_ids: s.album_id.clone().into_iter().collect(),
        custom_playlist_id: None,
    });
    rows.push(HarbourRow::Section {
        id,
        title: title.to_string(),
        count: songs.len().min(HOT_PICKS_PER_SECTION),
        expanded,
        see_all: None,
        glyph: section_icon(id),
        teaser,
    });
    if expanded {
        for s in songs.iter().take(HOT_PICKS_PER_SECTION) {
            rows.push(HarbourRow::Item {
                title: s.title.clone(),
                subtitle: join_facts(vec![
                    (!s.artist.is_empty()).then(|| s.artist.clone()),
                    song_fact(s),
                ]),
                art_album_id: s.album_id.clone(),
                art_album_ids: s.album_id.clone().into_iter().collect(),
                play: PlayTarget::Item(BatchItem::Song(Box::new(s.clone()))),
            });
        }
    }
}

/// Push an album shelf. `most_played` swaps the "Added 3 days ago" fact for a
/// play-count fact, so a Most Played shelf doesn't read as recency-sorted.
fn push_album_section(
    rows: &mut Vec<HarbourRow>,
    id: HarbourSectionId,
    title: &str,
    albums: &[AlbumUIViewData],
    collapsed: &HashSet<HarbourSectionId>,
    most_played: bool,
) {
    let expanded = !collapsed.contains(&id);
    // Shared fact builder (teaser + most-played item rows). The teaser drops
    // the item rows' release-year fact — the pick's title occupies the line.
    let album_fact = |a: &AlbumUIViewData| {
        if most_played {
            Some(plays_label(a.play_count.unwrap_or(0)))
        } else {
            a.created_at
                .as_deref()
                .map(|d| format!("Added {}", format_relative_time(d)))
        }
    };
    let teaser = albums.first().map(|a| SectionTeaser {
        subtitle: join_facts(vec![
            Some(a.name.clone()),
            (!a.artist.is_empty()).then(|| a.artist.clone()),
            album_fact(a),
        ]),
        art_album_id: Some(a.id.clone()),
        art_album_ids: vec![a.id.clone()],
        custom_playlist_id: None,
    });
    rows.push(HarbourRow::Section {
        id,
        title: title.to_string(),
        count: albums.len().min(HOT_PICKS_PER_SECTION),
        expanded,
        see_all: None,
        glyph: section_icon(id),
        teaser,
    });
    if expanded {
        for a in albums.iter().take(HOT_PICKS_PER_SECTION) {
            let subtitle = if most_played {
                join_facts(vec![
                    (!a.artist.is_empty()).then(|| a.artist.clone()),
                    album_fact(a),
                ])
            } else {
                album_item_subtitle(a)
            };
            rows.push(HarbourRow::Item {
                title: a.name.clone(),
                subtitle,
                art_album_id: Some(a.id.clone()),
                art_album_ids: vec![a.id.clone()],
                play: PlayTarget::Item(BatchItem::Album(a.id.clone())),
            });
        }
    }
}

/// Push the Most Played Artists shelf. Artist images live in `album_art` keyed by
/// the artist id (warmed via the `ar-{id}` endpoint — see the search-artist
/// path), so the row keys its thumbnail on the artist id and shows a play-count
/// subtitle. Playing an artist enqueues their whole catalogue.
fn push_artist_section(
    rows: &mut Vec<HarbourRow>,
    id: HarbourSectionId,
    title: &str,
    artists: &[Artist],
    collapsed: &HashSet<HarbourSectionId>,
) {
    let expanded = !collapsed.contains(&id);
    let teaser = artists.first().map(|a| SectionTeaser {
        subtitle: join_facts(vec![
            Some(a.name.clone()),
            Some(plays_label(a.play_count.unwrap_or(0))),
        ]),
        art_album_id: Some(a.id.clone()),
        art_album_ids: Vec::new(),
        custom_playlist_id: None,
    });
    rows.push(HarbourRow::Section {
        id,
        title: title.to_string(),
        count: artists.len().min(HOT_PICKS_PER_SECTION),
        expanded,
        see_all: None,
        glyph: section_icon(id),
        teaser,
    });
    if expanded {
        for a in artists.iter().take(HOT_PICKS_PER_SECTION) {
            rows.push(HarbourRow::Item {
                title: a.name.clone(),
                subtitle: plays_label(a.play_count.unwrap_or(0)),
                art_album_id: Some(a.id.clone()),
                art_album_ids: Vec::new(),
                play: PlayTarget::Item(BatchItem::Artist(a.id.clone())),
            });
        }
    }
}

/// The ` • `-joined item subtitle for a "Recently Added" shelf row: artist,
/// "Added <relative>" (`created_at`), plus the release year. A blank artist /
/// missing date simply shortens the line.
fn album_item_subtitle(a: &AlbumUIViewData) -> String {
    join_facts(vec![
        (!a.artist.is_empty()).then(|| a.artist.clone()),
        a.created_at
            .as_deref()
            .map(|d| format!("Added {}", format_relative_time(d))),
        a.year.map(|y| y.to_string()),
    ])
}

fn push_playlist_section(
    rows: &mut Vec<HarbourRow>,
    id: HarbourSectionId,
    title: &str,
    playlists: &[PlaylistUIViewData],
    collapsed: &HashSet<HarbourSectionId>,
) {
    let expanded = !collapsed.contains(&id);
    // Shared facts line (teaser + item rows): the pick's OWN counts, not the
    // preview panel's section-wide sums.
    let playlist_facts = |p: &PlaylistUIViewData| {
        format!(
            "{} songs • {}",
            p.song_count,
            format_duration_short(p.duration as f64)
        )
    };
    let teaser = playlists.first().map(|p| SectionTeaser {
        // Bare pick name — no "Featuring" prefix in headers (denser; the
        // preview-panel pill keeps its "Featuring").
        subtitle: format!("{} • {}", p.name, playlist_facts(p)),
        art_album_id: p.artwork_album_ids.first().cloned(),
        art_album_ids: p.artwork_album_ids.clone(),
        custom_playlist_id: Some(p.id.clone()),
    });
    rows.push(HarbourRow::Section {
        id,
        title: title.to_string(),
        count: playlists.len().min(HOT_PICKS_PER_SECTION),
        expanded,
        see_all: None,
        glyph: section_icon(id),
        teaser,
    });
    if expanded {
        for p in playlists.iter().take(HOT_PICKS_PER_SECTION) {
            rows.push(HarbourRow::Item {
                title: p.name.clone(),
                subtitle: playlist_facts(p),
                art_album_id: p.artwork_album_ids.first().cloned(),
                art_album_ids: p.artwork_album_ids.clone(),
                play: PlayTarget::Item(BatchItem::Playlist(p.id.clone())),
            });
        }
    }
}

/// Push a genre shelf. `most_played` genres come from the play tally and carry
/// no real album/song counts — only their track share (`song_count`), shown as
/// "N of your top tracks"; the Random Genres shelf shows the full library counts.
fn push_genre_section(
    rows: &mut Vec<HarbourRow>,
    id: HarbourSectionId,
    title: &str,
    genres: &[GenreUIViewData],
    collapsed: &HashSet<HarbourSectionId>,
    most_played: bool,
) {
    let expanded = !collapsed.contains(&id);
    // Shared facts line (teaser + item rows) — the tally copy for Most Played,
    // library counts for Random Genres.
    let genre_facts = |g: &GenreUIViewData| {
        if most_played {
            let n = g.song_count;
            format!(
                "{n} of your top {}",
                if n == 1 { "track" } else { "tracks" }
            )
        } else {
            format!("{} albums • {} songs", g.album_count, g.song_count)
        }
    };
    let teaser = genres.first().map(|g| SectionTeaser {
        subtitle: format!("{} • {}", g.name, genre_facts(g)),
        art_album_id: g.artwork_album_ids.first().cloned(),
        art_album_ids: g.artwork_album_ids.clone(),
        custom_playlist_id: None,
    });
    rows.push(HarbourRow::Section {
        id,
        title: title.to_string(),
        count: genres.len().min(HOT_PICKS_PER_SECTION),
        expanded,
        see_all: None,
        glyph: section_icon(id),
        teaser,
    });
    if expanded {
        for g in genres.iter().take(HOT_PICKS_PER_SECTION) {
            rows.push(HarbourRow::Item {
                title: g.name.clone(),
                subtitle: genre_facts(g),
                art_album_id: g.artwork_album_ids.first().cloned(),
                art_album_ids: g.artwork_album_ids.clone(),
                play: PlayTarget::GenreRandom(g.name.clone()),
            });
        }
    }
}

/// Push a search-group `Section` header and return whether it is expanded.
/// `teaser` carries the group's first match (built at the call site, which
/// knows the entity type) for the collapsed-header teaser.
fn push_search_header(
    rows: &mut Vec<HarbourRow>,
    id: HarbourSectionId,
    title: &str,
    section: HarbourSection,
    count: usize,
    collapsed: &HashSet<HarbourSectionId>,
    teaser: Option<SectionTeaser>,
) -> bool {
    let expanded = !collapsed.contains(&id);
    rows.push(HarbourRow::Section {
        id,
        title: title.to_string(),
        count,
        expanded,
        see_all: Some(section),
        glyph: section_icon(id),
        teaser,
    });
    expanded
}

/// Harbour page. Owns the shared slot-list state plus the set of *collapsed*
/// sections (default: the two random shelves). The two album shelves and every
/// search section stay out of the set so they default expanded.
#[derive(Debug)]
pub struct HarbourPage {
    pub common: SlotListPageState,
    pub collapsed: HashSet<HarbourSectionId>,
}

impl Default for HarbourPage {
    fn default() -> Self {
        // Every shelf section starts COLLAPSED — the home reads as a compact
        // index of sections; centering one previews it in the artwork column,
        // expanding reveals its hot picks. (Search groups stay expanded — see
        // build_harbour_rows — so results are visible the moment you type.)
        let collapsed = HashSet::from([
            HarbourSectionId::RecentlyPlayed,
            HarbourSectionId::RecentlyAdded,
            HarbourSectionId::MostPlayedTracks,
            HarbourSectionId::MostPlayedAlbums,
            HarbourSectionId::MostPlayedArtists,
            HarbourSectionId::MostPlayedGenres,
            HarbourSectionId::Playlists,
            HarbourSectionId::Genres,
        ]);
        Self {
            common: SlotListPageState::new_without_sort_mode(),
            collapsed,
        }
    }
}

impl HarbourPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle a section's collapsed membership.
    pub(crate) fn toggle_section(&mut self, id: HarbourSectionId) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
    }
}

/// Messages emitted by the Harbour view.
#[derive(Debug, Clone)]
pub enum HarbourMessage {
    /// Unified slot-list carrier.
    SlotList(SlotListPageMessage),
    /// Context-menu open/close request — intercepted by the chrome prologue.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Always-Vertical artwork drag handle event — intercepted at root.
    ArtworkColumnVerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Header search input changed (immediate; gated on min-length in handler).
    SearchChanged(String),
    /// "See all" on a search-result group — route to that view with the query.
    SeeAll(HarbourSection),
    /// Expand/collapse a section (chevron / row click).
    ToggleSection(HarbourSectionId),
    /// Shift+Enter: toggle the centered section (the expand-center hotkey).
    ExpandCenter,
    NoOp,
}

/// Borrowed app state the Harbour view renders from.
pub(crate) struct HarbourViewData<'a> {
    pub harbour: &'a crate::state::HarbourState,
    /// The Trawl crate — seed count + blend feed the Trawl row's subtitle.
    pub trawl_crate: &'a nokkvi_data::types::trawl::TrawlCrate,
    /// 80px album-cover cache (row thumbnails).
    pub album_art: &'a HashMap<String, image::Handle>,
    /// Large-cover cache (side artwork panel).
    pub large_artwork: &'a HashMap<String, image::Handle>,
    /// User-uploaded playlist covers, 80px (row thumbnails).
    pub playlist_custom_art: &'a HashMap<String, image::Handle>,
    /// User-uploaded playlist covers, resolution-sized (the large artwork
    /// column) — so a centered custom-cover playlist stays crisp instead of
    /// upscaling its 80px mini.
    pub playlist_custom_large_art: &'a HashMap<String, image::Handle>,
    /// 300px collage-tile caches (keyed by playlist / genre id) feeding the
    /// large artwork column's mosaic for a centered collection or its section
    /// header — the same caches the real Playlists/Genres views render from.
    pub playlist_collage: &'a HashMap<String, Vec<image::Handle>>,
    pub genre_collage: &'a HashMap<String, Vec<image::Handle>>,
    pub window_width: f32,
    pub window_height: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub elevated: bool,
    pub stable_viewport: bool,
}

impl HarbourPage {
    /// Render the Harbour view — mirrors Similar's slot-list layout.
    pub(crate) fn view<'a>(&'a self, data: HarbourViewData<'a>) -> Element<'a, HarbourMessage> {
        let rows = build_harbour_rows(data.harbour, &self.collapsed, data.trawl_crate);
        // Count items across every section (expanded or not) via each Section's
        // `count`, so the header total reflects the populated home rather than
        // dropping to "0 items" when all sections are collapsed — the default
        // landing state. Section `count` already caps at HOT_PICKS / carries the
        // search group size; Item and Hint rows add nothing (a section's items
        // are tallied through its header).
        let item_count: usize = rows
            .iter()
            .map(|r| match r {
                HarbourRow::Section { count, .. } => *count,
                _ => 0,
            })
            .sum();

        // No sort dropdown — pass an empty slice so the header hides it.
        let empty_options: &[String] = &[];
        let header = widgets::view_header::view_header(ViewHeaderConfig {
            current_view: "Harbour".to_string(),
            view_options: empty_options,
            sort_ascending: true,
            search_query: &data.harbour.search_query,
            filtered_count: item_count,
            total_count: item_count,
            item_type: "items",
            search_input_id: crate::views::HARBOUR_SEARCH_ID,
            on_view_selected: Box::new(|_| HarbourMessage::NoOp),
            show_search: true,
            on_search_change: Box::new(HarbourMessage::SearchChanged),
            buttons: vec![],
            on_roulette: None,
            collapsed: false,
            on_hover_enter: None,
            on_hover_exit: None,
            on_dropdown_open: None,
            on_dropdown_close: None,
            total_duration_secs: None,
            sort_placeholder: None,
        });

        use crate::widgets::{
            base_slot_list_layout::BaseSlotListLayoutConfig,
            slot_list::{
                SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
            },
        };

        let slot_list_chrome = chrome_height_with_select_header(false, false);
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
            slot_list_chrome,
            elevated: data.elevated,
        };

        // While the very first shelf load is still in flight (no data yet, no
        // active search), show a loading state through the shared layout so the
        // header text_input keeps focus across the swap.
        if data.harbour.search_query.trim().is_empty()
            && data.harbour.shelves_loading
            && data.harbour.shelves_empty()
        {
            return widgets::base_slot_list_empty_state(
                header,
                "Loading your library…",
                &layout_config,
            );
        }
        if rows.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "Nothing here yet.",
                &layout_config,
            );
        }

        let vertical_artwork_chrome =
            crate::widgets::base_slot_list_layout::vertical_artwork_chrome(&layout_config);
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            slot_list_chrome + vertical_artwork_chrome,
        )
        .with_modifiers(data.modifiers);

        // EFFECTIVE center (honors a click-to-focus `selected_offset`), not the
        // raw viewport center — the update handlers resolve the center through
        // the same accessor, so the artwork panel, the warm path, and Enter all
        // agree on which row is "centered" after a click (Radios precedent).
        let center_index = self.common.get_center_item_index(rows.len());

        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &rows,
            &config,
            HarbourMessage::SlotList(SlotListPageMessage::NavigateUp),
            HarbourMessage::SlotList(SlotListPageMessage::NavigateDown),
            crate::views::scroll_seek_msg(rows.len(), HarbourMessage::SlotList),
            Some(crate::widgets::slot_list::SlotHoverCallback::for_slot_list(
                HarbourMessage::SlotList,
            )),
            |row, ctx| render_row(row, ctx, &data),
        );

        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{
            collage_artwork_panel, single_artwork_panel_with_menu,
        };
        // A blank menu-less single panel — the art-less fallback shared by
        // hints, search-group headers, and items whose covers have not warmed.
        let blank_panel = || {
            single_artwork_panel_with_menu::<HarbourMessage>(None, Vec::new(), false, None, |_| {
                HarbourMessage::NoOp
            })
        };
        // Side panel content, keyed on the centered row:
        // - a centered *collection* Item (playlist/genre) shows its 3×3 collage
        //   of 300px album tiles — the large-column mosaic the real
        //   Playlists/Genres views render, warmed on center by the handler;
        // - a centered single-cover Item (album/song, or a custom-art playlist),
        //   or a collection whose collage has not warmed, shows its crisp large
        //   cover;
        // - a centered *shelf* Section header (see_all == None) shows a preview
        //   panel summarising that section (cover/mosaic sample + a 3-line pill);
        // - search-group headers, hints, and art-less items leave the panel blank.
        let center_row = center_index.and_then(|idx| rows.get(idx));
        let artwork_content = Some(match center_row {
            Some(HarbourRow::Item {
                art_album_id, play, ..
            }) => {
                // A user-uploaded playlist cover wins outright (mirrors the row's
                // custom-art precedence), at large-column resolution — the 80px
                // `playlist_custom_art` mini is never upscaled here. It suppresses
                // the collage mosaic.
                let custom_large = match play {
                    PlayTarget::Item(BatchItem::Playlist(pid)) => {
                        data.playlist_custom_large_art.get(pid)
                    }
                    _ => None,
                };
                let collage = collection_collage(play, &data).filter(|v| !v.is_empty());
                if let Some(handle) = custom_large {
                    single_artwork_panel_with_menu::<HarbourMessage>(
                        Some(handle),
                        Vec::new(),
                        false,
                        None,
                        |_| HarbourMessage::NoOp,
                    )
                } else if let Some(handles) = collage {
                    collage_artwork_panel::<HarbourMessage>(handles)
                } else {
                    // Single-cover item — the crisp large cover warmed on center.
                    single_artwork_panel_with_menu::<HarbourMessage>(
                        art_album_id
                            .as_ref()
                            .and_then(|id| data.large_artwork.get(id)),
                        Vec::new(),
                        false,
                        None,
                        |_| HarbourMessage::NoOp,
                    )
                }
            }
            Some(HarbourRow::Section {
                id, see_all: None, ..
            }) => section_preview_panel(*id, &data),
            // The centered Trawl door: anchor placeholder + a quiet pill
            // naming the crate's state (kept fg2-level — Harbour opens
            // centered on this row, so the landing view must not shout).
            Some(HarbourRow::Trawl { seeds, blend }) => {
                let panel = crate::widgets::base_slot_list_layout::
                    single_artwork_panel_with_visualizer_and_menu::<HarbourMessage>(
                    None,
                    None,
                    None,
                    crate::widgets::base_slot_list_layout::ArtworkPlaceholder::Anchor,
                    Vec::new(),
                    false,
                    None,
                    |_| HarbourMessage::NoOp,
                );
                let meta = if *seeds == 0 {
                    "The crate is empty".to_string()
                } else {
                    trawl_row_subtitle(*seeds, *blend)
                };
                crate::widgets::base_slot_list_layout::wrap_with_pill_overlay(
                    panel,
                    section_pill(
                        "assets/icons/anchor.svg",
                        "TRAWL",
                        "Build a mix".to_string(),
                        meta,
                    ),
                )
            }
            _ => blank_panel(),
        });

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(HarbourMessage::ArtworkColumnDrag),
            Some(HarbourMessage::ArtworkColumnVerticalDrag),
        )
    }
}

/// Render one flattened Harbour row.
fn render_row<'a>(
    row: &HarbourRow,
    ctx: crate::widgets::slot_list::SlotListRowContext,
    data: &HarbourViewData<'a>,
) -> Element<'a, HarbourMessage> {
    use crate::widgets::slot_list::{
        child_slot_button, slot_list_static_icon_color, slot_list_text_column,
    };

    match row {
        HarbourRow::Section {
            id,
            title,
            count,
            expanded,
            see_all,
            glyph,
            teaser,
        } => {
            let m = ctx.metrics;
            // Parent style from the shared state machine (mirrors
            // render_genre_row): an expanded header wears the loud highlight
            // fill + forced-legible text; a collapsed/centered header gets the
            // accent selection ring — both fall out of `to_container_style`.
            let style = ctx.slot_style(*expanded, false, 0);

            // Header art square: a collapsed populated header teases its first
            // pick's cover through the same ladder as item rows; an expanded or
            // empty header shows the section identity glyph on the quiet fg2
            // channel (accent stays exclusive to the Trawl CTA and See-all —
            // the loud expanded fill forces it legible anyway). The cover→glyph
            // swap co-occurs with the fill flip so the expanded header never
            // stacks an identical cover directly above child row 1.
            let art_el: Element<'a, HarbourMessage> = match teaser {
                Some(t) if !*expanded => harbour_art_element(
                    t.custom_playlist_id.as_deref(),
                    t.art_album_id.as_ref(),
                    &t.art_album_ids,
                    data,
                    m.artwork_size,
                    ctx.is_center,
                    ctx.opacity,
                ),
                _ => glyph_art_square(glyph, m.artwork_size, style, ctx.opacity, theme::fg2()),
            };

            let subtitle =
                section_header_subtitle(teaser.as_ref(), *expanded, *count, see_all.is_some());

            // Two-line text column in BOTH states so the title baseline never
            // jitters on toggle. Title keeps `m.title_size` bold — a step below
            // the items' `title_size_lg`, the retained parent/child cue.
            use iced::widget::text::{Ellipsis, Wrapping};
            let text_col = container(
                iced::widget::Column::new()
                    .push(
                        text(title.clone())
                            .size(m.title_size)
                            .font(theme::weighted_ui_font(iced::font::Weight::Bold))
                            .color(style.text_color)
                            .wrapping(Wrapping::None)
                            .ellipsis(Ellipsis::End),
                    )
                    .push(
                        text(subtitle)
                            .size(m.subtitle_size)
                            .color(slot_list_static_icon_color(
                                style,
                                theme::fg3(),
                                ctx.opacity,
                            ))
                            .wrapping(Wrapping::None)
                            .ellipsis(Ellipsis::End),
                    ),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center);

            // Item-mirroring geometry — [pad 8][art][6][text⋯][See-all][caret]
            // — so the header square shares one left rail with the Trawl
            // anchor and every item cover; the caret moves to the RIGHT edge.
            let mut header_row = iced::widget::Row::new()
                .spacing(6.0)
                .align_y(Alignment::Center)
                .height(Length::Fill)
                // The inset is row PADDING, not a Space child — a leading
                // spacer inside a spaced row compounds with .spacing() and
                // drifts the rail. See HARBOUR_ROW_INSET for the 12px choice.
                .padding(iced::Padding::new(0.0).left(HARBOUR_ROW_INSET))
                .push(art_el)
                .push(text_col);

            if let Some(section) = see_all {
                let section = *section;
                // On a forced-legible filled (expanded) header this resolves to
                // style.text_color; on a plain/collapsed header it stays
                // accent_bright at ctx.opacity — mirrors the caret.
                let see_all_color =
                    slot_list_static_icon_color(style, theme::accent_bright(), ctx.opacity);
                // "See all" label + a chevron-right SVG (no unicode arrow).
                let see_all_label = iced::widget::Row::new()
                    .spacing(3.0)
                    .align_y(Alignment::Center)
                    .push(text("See all").size(m.metadata_size).color(see_all_color))
                    .push(
                        crate::embedded_svg::svg_widget("assets/icons/chevron-right.svg")
                            .width(Length::Fixed(12.0))
                            .height(Length::Fixed(12.0))
                            .style(move |_theme, _status| iced::widget::svg::Style {
                                color: Some(see_all_color),
                            }),
                    );
                header_row = header_row.push(
                    mouse_area(container(see_all_label).padding(iced::Padding {
                        left: 6.0,
                        right: 6.0,
                        top: 2.0,
                        bottom: 2.0,
                    }))
                    .on_press(HarbourMessage::SeeAll(section))
                    .interaction(iced::mouse::Interaction::Pointer),
                );
            }
            header_row = header_row.push(caret_icon(
                *expanded,
                slot_list_static_icon_color(style, theme::fg2(), ctx.opacity),
            ));

            let styled = container(header_row.padding(iced::Padding {
                left: 0.0,
                right: 8.0,
                top: 4.0,
                bottom: 4.0,
            }))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::Center)
            .style(move |_: &iced::Theme| style.to_container_style());

            let id = *id;
            mouse_area(styled)
                .on_press(HarbourMessage::ToggleSection(id))
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
        }
        HarbourRow::Trawl { seeds, blend } => {
            // The mix-builder door: anchor glyph (the feature's CTA mark) in
            // the art square, live crate subtitle, standard slot-button
            // plumbing — activation opens the modal (update/harbour.rs).
            // Accent tint (the CTA's exclusive channel) through the shared
            // chassis — section headers use the same square with a quiet fg2.
            let m = ctx.metrics;
            let style = ctx.slot_style(false, false, 0);
            let anchor = glyph_art_square(
                "assets/icons/anchor.svg",
                m.artwork_size,
                style,
                ctx.opacity,
                theme::accent(),
            );

            let content_row = iced::widget::Row::new()
                .spacing(6.0)
                .align_y(Alignment::Center)
                .height(Length::Fill)
                // The inset is row PADDING, not a Space child — a leading
                // spacer inside a spaced row compounds with .spacing() and
                // drifts the rail. See HARBOUR_ROW_INSET for the 12px choice.
                .padding(iced::Padding::new(0.0).left(HARBOUR_ROW_INSET))
                .push(anchor)
                .push(slot_list_text_column(
                    "Trawl".to_string(),
                    None,
                    trawl_row_subtitle(*seeds, *blend),
                    None,
                    m.title_size_lg,
                    m.subtitle_size,
                    style,
                    ctx.is_center,
                    100,
                ));

            child_slot_button(
                content_row,
                &ctx,
                style,
                data.stable_viewport,
                HarbourMessage::SlotList,
            )
        }
        HarbourRow::Item {
            title,
            subtitle,
            art_album_id,
            art_album_ids,
            play,
        } => {
            let m = ctx.metrics;
            let style = ctx.slot_style(false, false, 1);

            // Cover via the shared custom→quad→single ladder (also the
            // collapsed section headers' teaser square).
            let custom_playlist_id = match play {
                PlayTarget::Item(BatchItem::Playlist(pid)) => Some(pid.as_str()),
                _ => None,
            };
            let art_el: Element<'a, HarbourMessage> = harbour_art_element(
                custom_playlist_id,
                art_album_id.as_ref(),
                art_album_ids,
                data,
                m.artwork_size,
                ctx.is_center,
                ctx.opacity,
            );

            let content_row = iced::widget::Row::new()
                .spacing(6.0)
                .align_y(Alignment::Center)
                .height(Length::Fill)
                // The inset is row PADDING, not a Space child — a leading
                // spacer inside a spaced row compounds with .spacing() and
                // drifts the rail. See HARBOUR_ROW_INSET for the 12px choice.
                .padding(iced::Padding::new(0.0).left(HARBOUR_ROW_INSET))
                .push(art_el)
                .push(slot_list_text_column(
                    title.clone(),
                    None,
                    subtitle.clone(),
                    None,
                    m.title_size_lg,
                    m.subtitle_size,
                    style,
                    ctx.is_center,
                    100,
                ));

            child_slot_button(
                content_row,
                &ctx,
                style,
                data.stable_viewport,
                HarbourMessage::SlotList,
            )
        }
        HarbourRow::Hint(msg) => container(
            text(msg.clone())
                .size(ctx.metrics.subtitle_size)
                .color(theme::fg4()),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into(),
    }
}

/// Resolve a Harbour row's 80px art square through the shared ladder:
/// user-uploaded playlist cover (wins outright, suppresses the quad) → 2×2
/// quad from the row's own album ids (atomic upgrade once all tiles land) →
/// single mini → blank `bg2` chassis until a cover warms. ONE ladder for Item
/// rows AND collapsed teaser headers, so the two can't drift.
#[allow(clippy::too_many_arguments)]
fn harbour_art_element<'a, M: 'a>(
    custom_playlist_id: Option<&str>,
    art_album_id: Option<&String>,
    art_album_ids: &[String],
    data: &HarbourViewData<'a>,
    art_size: f32,
    is_center: bool,
    opacity: f32,
) -> Element<'a, M> {
    use crate::widgets::slot_list::{slot_list_artwork_column, slot_list_artwork_quad_column};

    let custom_art = custom_playlist_id.and_then(|pid| data.playlist_custom_art.get(pid));
    let art_handle = custom_art.or_else(|| art_album_id.and_then(|id| data.album_art.get(id)));
    let quad = if custom_art.is_some() {
        None
    } else {
        crate::services::collage_artwork::resolve_quad_handles(art_album_ids, data.album_art)
    };
    match quad {
        Some(tiles) => slot_list_artwork_quad_column(&tiles, art_size, is_center, false, opacity),
        None => slot_list_artwork_column(art_handle, art_size, is_center, false, opacity),
    }
}

/// A centered SVG glyph in a `bg2` chassis square — the Trawl anchor block,
/// shared with the expanded/empty section headers so the two squares stay
/// pixel-identical. `tint` is the glyph's fallback color (accent for the Trawl
/// CTA, quiet `fg2` for headers), routed through [`slot_list_static_icon_color`]
/// so loud-fill (forced-legible) rows swap it for the row's legible text color.
///
/// [`slot_list_static_icon_color`]: crate::widgets::slot_list::slot_list_static_icon_color
fn glyph_art_square<'a, M: 'a>(
    icon: &'static str,
    art_size: f32,
    style: crate::widgets::slot_list::SlotListSlotStyle,
    opacity: f32,
    tint: iced::Color,
) -> Element<'a, M> {
    let tint = crate::widgets::slot_list::slot_list_static_icon_color(style, tint, opacity);
    container(
        crate::embedded_svg::svg_widget(icon)
            .width(Length::Fixed(art_size * 0.55))
            .height(Length::Fixed(art_size * 0.55))
            .style(move |_theme, _status| iced::widget::svg::Style { color: Some(tint) }),
    )
    .width(Length::Fixed(art_size))
    .height(Length::Fixed(art_size))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_theme: &iced::Theme| container::Style {
        background: Some(
            iced::Color {
                a: opacity,
                ..theme::bg2()
            }
            .into(),
        ),
        ..Default::default()
    })
    .into()
}

/// The section-header caret: chevron-down when expanded, chevron-right when
/// collapsed. An SVG (never a unicode glyph) tinted to the secondary text color.
fn caret_icon<'a>(expanded: bool, color: iced::Color) -> Element<'a, HarbourMessage> {
    let path = if expanded {
        "assets/icons/chevron-down.svg"
    } else {
        "assets/icons/chevron-right.svg"
    };
    crate::embedded_svg::svg_widget(path)
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .style(move |_theme, _status| iced::widget::svg::Style { color: Some(color) })
        .into()
}

/// The side-panel preview for a centered *shelf* section header. Fills what was
/// a blank panel with a representative cover behind a 3-line banded pill
/// (eyebrow + title + meta) summarising the section — the same banded-pill frame
/// the Queue "Playing From" and Playlists/Genres panels ship, so it reads as one
/// system. Album shelves surface their top album; the collection shelves surface
/// a representative cover from the first resolvable album across their picks.
/// An empty section returns a blank artwork square so the header keeps a valid
/// panel (preserving `text_input` focus).
fn section_preview_panel<'a>(
    id: HarbourSectionId,
    data: &HarbourViewData<'a>,
) -> Element<'a, HarbourMessage> {
    use crate::widgets::base_slot_list_layout::{
        collage_artwork_panel_with_pill, single_artwork_panel, single_artwork_panel_with_pill,
    };

    let harbour = data.harbour;

    // The representative cover: prefer the crisp large cover (warmed on center
    // by the handler, see `section_cover_album_id`), fall back to the 80px mini
    // until it lands. Panel and warm-path resolve the SAME id so they agree.
    let cover_id = section_cover_album_id(harbour, id);
    let cover = cover_id
        .as_ref()
        .and_then(|a| data.large_artwork.get(a).or_else(|| data.album_art.get(a)));

    // Resolve (eyebrow label, title, meta line) for the section, or bail to a
    // blank square when the section has no data. The eyebrow icon comes from
    // the shared [`section_icon`] mapping (also the header glyph — no drift).
    let resolved: Option<(&'static str, String, String)> = match id {
        HarbourSectionId::RecentlyPlayed => harbour.recently_played.first().map(|s| {
            let meta = join_facts(vec![
                (!s.artist.is_empty()).then(|| s.artist.clone()),
                s.play_date
                    .as_deref()
                    .map(|d| format!("Played {}", format_relative_time(d))),
            ]);
            ("Recently Played", s.title.clone(), meta)
        }),
        HarbourSectionId::RecentlyAdded => harbour.recently_added.first().map(|a| {
            let meta = join_facts(vec![
                (!a.artist.is_empty()).then(|| a.artist.clone()),
                a.created_at
                    .as_deref()
                    .map(|d| format!("Added {}", format_relative_time(d))),
                a.year.map(|y| y.to_string()),
            ]);
            ("Recently Added", a.name.clone(), meta)
        }),
        HarbourSectionId::Playlists => harbour.playlists.first().map(|first| {
            let songs: u32 = harbour.playlists.iter().map(|p| p.song_count).sum();
            let duration: f64 = harbour.playlists.iter().map(|p| p.duration as f64).sum();
            let meta = join_facts(vec![
                Some(format!("{songs} songs")),
                Some(format_duration_short(duration)),
            ]);
            (
                "Random Playlists",
                format!("Featuring {}", first.name),
                meta,
            )
        }),
        HarbourSectionId::Genres => harbour.genres.first().map(|first| {
            let albums: u32 = harbour.genres.iter().map(|g| g.album_count).sum();
            let songs: u32 = harbour.genres.iter().map(|g| g.song_count).sum();
            let meta = join_facts(vec![
                Some(format!("{albums} albums")),
                Some(format!("{songs} songs")),
            ]);
            ("Random Genres", format!("Featuring {}", first.name), meta)
        }),
        HarbourSectionId::MostPlayedTracks => harbour.most_played_songs.first().map(|s| {
            let meta = join_facts(vec![
                (!s.artist.is_empty()).then(|| s.artist.clone()),
                Some(plays_label(s.play_count.unwrap_or(0))),
            ]);
            ("Most Played Tracks", s.title.clone(), meta)
        }),
        HarbourSectionId::MostPlayedAlbums => harbour.most_played_albums.first().map(|a| {
            let meta = join_facts(vec![
                (!a.artist.is_empty()).then(|| a.artist.clone()),
                Some(plays_label(a.play_count.unwrap_or(0))),
            ]);
            ("Most Played Albums", a.name.clone(), meta)
        }),
        HarbourSectionId::MostPlayedArtists => harbour.most_played_artists.first().map(|a| {
            (
                "Most Played Artists",
                a.name.clone(),
                plays_label(a.play_count.unwrap_or(0)),
            )
        }),
        HarbourSectionId::MostPlayedGenres => harbour.most_played_genres.first().map(|first| {
            let n = first.song_count;
            let meta = format!(
                "{n} of your top {}",
                if n == 1 { "track" } else { "tracks" }
            );
            (
                "Most Played Genres",
                format!("Featuring {}", first.name),
                meta,
            )
        }),
        // Search-group headers never reach here (see_all is Some); guard anyway.
        _ => None,
    };

    // The collection shelves (Random Playlists / Genres) preview their first
    // pick's 3×3 collage of 300px tiles behind the pill — the same crisp mosaic
    // the real Playlists/Genres views render, contextualised by the pill's
    // "Featuring {first}" line — falling back to the single representative cover
    // until that collage warms. Album shelves stay single-cover.
    let collage = section_first_collage(id, data).filter(|v| !v.is_empty());

    match resolved {
        Some((label, title, meta)) => {
            let pill = Some(section_pill(section_icon(id), label, title, meta));
            match collage {
                Some(handles) => collage_artwork_panel_with_pill::<HarbourMessage>(
                    handles,
                    pill,
                    Vec::new(),
                    false,
                    None,
                    |_| HarbourMessage::NoOp,
                ),
                None => single_artwork_panel_with_pill::<HarbourMessage>(
                    cover,
                    pill,
                    Vec::new(),
                    false,
                    None,
                    |_| HarbourMessage::NoOp,
                ),
            }
        }
        None => single_artwork_panel::<HarbourMessage>(None),
    }
}

/// The 300px collage tiles for a centered collection Item (playlist / genre),
/// borrowed from the shared collage cache. `None` for albums/songs (no play
/// target resolves to a collage) and until the handler warms the collage on
/// center — the panel then shows the item's single large cover instead.
fn collection_collage<'a>(
    play: &PlayTarget,
    data: &HarbourViewData<'a>,
) -> Option<&'a Vec<image::Handle>> {
    match play {
        PlayTarget::Item(BatchItem::Playlist(pid)) => data.playlist_collage.get(pid),
        PlayTarget::GenreRandom(gid) => data.genre_collage.get(gid),
        PlayTarget::Item(_) => None,
    }
}

/// The 300px collage tiles a collection *section* header previews — its first
/// pick's collage (the pill names that pick via "Featuring {first}"). `None`
/// for album shelves and until the first pick's collage warms. The set of
/// collage-previewing sections comes from the shared [`section_collage_source`]
/// so it can't drift from the handler's on-center warm.
fn section_first_collage<'a>(
    id: HarbourSectionId,
    data: &HarbourViewData<'a>,
) -> Option<&'a Vec<image::Handle>> {
    use crate::app_message::CollageTarget;
    let (target, entity_id, _album_ids) = section_collage_source(data.harbour, id)?;
    match target {
        CollageTarget::Playlist => data.playlist_collage.get(entity_id),
        CollageTarget::Genre => data.genre_collage.get(entity_id),
    }
}

/// For a collection *section* header, the first pick's collage source as
/// `(target, entity_id, album_ids)`, borrowed from `harbour` state. `None` for
/// album shelves and every non-collection section. The single enumeration of
/// "which section headers preview a collage", shared by the view's section
/// preview ([`section_first_collage`]) and the handler's on-center collage warm
/// (`harbour_center_collage_target`) so the two sets can't diverge.
pub(crate) fn section_collage_source(
    harbour: &crate::state::HarbourState,
    id: HarbourSectionId,
) -> Option<(crate::app_message::CollageTarget, &str, &[String])> {
    use crate::app_message::CollageTarget;
    match id {
        HarbourSectionId::Playlists => harbour.playlists.first().map(|p| {
            (
                CollageTarget::Playlist,
                p.id.as_str(),
                p.artwork_album_ids.as_slice(),
            )
        }),
        HarbourSectionId::Genres => harbour.genres.first().map(|g| {
            (
                CollageTarget::Genre,
                g.id.as_str(),
                g.artwork_album_ids.as_slice(),
            )
        }),
        HarbourSectionId::MostPlayedGenres => harbour.most_played_genres.first().map(|g| {
            (
                CollageTarget::Genre,
                g.id.as_str(),
                g.artwork_album_ids.as_slice(),
            )
        }),
        _ => None,
    }
}

/// The representative album id for a section — the cover the preview panel
/// shows. Shared by the panel and the artwork-warm path (handler) so both
/// resolve to the SAME cover: album shelves use their top album; playlist/genre
/// shelves use the first album id across their items' collage ids.
pub(crate) fn section_cover_album_id(
    harbour: &crate::state::HarbourState,
    id: HarbourSectionId,
) -> Option<String> {
    match id {
        HarbourSectionId::RecentlyPlayed => harbour
            .recently_played
            .first()
            .and_then(|s| s.album_id.clone()),
        HarbourSectionId::RecentlyAdded => harbour.recently_added.first().map(|a| a.id.clone()),
        HarbourSectionId::MostPlayedTracks => harbour
            .most_played_songs
            .first()
            .and_then(|s| s.album_id.clone()),
        HarbourSectionId::MostPlayedAlbums => {
            harbour.most_played_albums.first().map(|a| a.id.clone())
        }
        // Artist images key on the artist id (album_art / large_artwork by id);
        // the section warm routes this through the artist large-art loader.
        HarbourSectionId::MostPlayedArtists => {
            harbour.most_played_artists.first().map(|a| a.id.clone())
        }
        HarbourSectionId::Playlists => harbour
            .playlists
            .iter()
            .flat_map(|p| p.artwork_album_ids.iter())
            .next()
            .cloned(),
        HarbourSectionId::Genres => harbour
            .genres
            .iter()
            .flat_map(|g| g.artwork_album_ids.iter())
            .next()
            .cloned(),
        HarbourSectionId::MostPlayedGenres => harbour
            .most_played_genres
            .iter()
            .flat_map(|g| g.artwork_album_ids.iter())
            .next()
            .cloned(),
        _ => None,
    }
}

/// The 3-line summary pill used in every section preview panel: an accent
/// eyebrow (icon + section label), a bold title, and a secondary meta line.
/// Identical shape for all four sections so the panel reads as one system.
fn section_pill<'a>(
    icon: &'static str,
    label: &'static str,
    title: String,
    meta: String,
) -> Element<'a, HarbourMessage> {
    let eyebrow = iced::widget::Row::new()
        .spacing(6.0)
        .align_y(Alignment::Center)
        .push(
            crate::embedded_svg::svg_widget(icon)
                .width(Length::Fixed(12.0))
                .height(Length::Fixed(12.0))
                .style(|_theme, _status| iced::widget::svg::Style {
                    color: Some(theme::accent()),
                }),
        )
        .push(text(label).size(11).color(theme::accent()));

    iced::widget::Column::new()
        .spacing(6.0)
        .width(Length::Fill)
        .push(eyebrow)
        .push(
            text(title)
                .size(15)
                .font(theme::weighted_ui_font(iced::font::Weight::Bold))
                .color(theme::fg0())
                .width(Length::Fill),
        )
        .push(text(meta).size(12).color(theme::fg2()).width(Length::Fill))
        .into()
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for HarbourPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn search_input_id(&self) -> &'static str {
        super::HARBOUR_SEARCH_ID
    }

    fn expand_center_message(&self) -> Option<Message> {
        Some(Message::Harbour(HarbourMessage::ExpandCenter))
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        None
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::NoOp
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Harbour(HarbourMessage::SlotList(
            SlotListPageMessage::AddCenterToQueue,
        )))
    }

    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadHarbour)
    }

    fn slot_list_message(&self, msg: SlotListPageMessage) -> Message {
        Message::Harbour(HarbourMessage::SlotList(msg))
    }

    fn uses_horizontal_artwork_column(&self) -> bool {
        true
    }
}
