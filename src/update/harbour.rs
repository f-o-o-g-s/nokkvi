//! Update handlers for the Harbour home view.
//!
//! Built up across milestones:
//! - M2: message dispatch skeleton — search-query capture + load lifecycle flag.
//! - M3 (here): the joined shelf fetch + generation-gated population +
//!   artwork warm-up (shelf covers, genre quad tiles).
//! - M4: card / genre play actions.
//! - M5: the whole-library search fan-out + grouped results.
//! - Later: the Random block's one-press draws (`play_random_kind`).

use std::collections::{HashMap, HashSet};

use iced::Task;
use nokkvi_data::{
    backend::{
        albums::{AlbumUIViewData, AlbumsService},
        app_service::AppService,
        genres::GenreUIViewData,
    },
    types::{
        batch::{BatchItem, BatchPayload},
        one_shot_shuffle::OneShotShuffle,
    },
};

use crate::{
    Nokkvi, View,
    app_message::{
        ArtworkMessage, CollageTarget, HarbourLoaderMessage, HarbourShelvesData, Message,
        NavigationMessage,
    },
    views::{
        HarbourMessage,
        harbour::{
            HOT_PICKS_PER_SECTION, HarbourRow, HarbourSectionId, PlayTarget, RandomKind,
            SEARCH_MIN_CHARS, SEARCH_PREVIEW_LIMIT, build_harbour_rows,
        },
    },
};

/// Degrade a secondary shelf's fetch to an empty list on failure (warn-logged),
/// so one flaky sort never blanks the whole home view. A free fn rather than a
/// closure because it's called at several entity types.
fn recover_shelf<T>(label: &str, result: anyhow::Result<Vec<T>>) -> Vec<T> {
    result.unwrap_or_else(|e| {
        tracing::warn!("Harbour: {label} shelf failed: {e:#}");
        Vec::new()
    })
}

/// Single-pick twin of [`recover_shelf`] for the Random block's draws — a
/// failed draw degrades to an empty row (action-copy fallback), never a blank
/// home view.
fn recover_pick<T>(label: &str, result: anyhow::Result<Option<T>>) -> Option<T> {
    result.unwrap_or_else(|e| {
        tracing::warn!("Harbour: {label} draw failed: {e:#}");
        None
    })
}

/// How many top-played songs to fetch as the genre-tally sample. The Most Played
/// Tracks shelf shows the top [`HOT_PICKS_PER_SECTION`] of these; the whole pool
/// is tallied by genre for the (server-unsortable) Most Played Genres shelf.
const MOST_PLAYED_TALLY_POOL: usize = 200;

/// Rank genres by the user's play counts across a sample of their most-played
/// songs — the client-side stand-in for a "most played genres" sort Navidrome
/// doesn't offer. Sums each song's `play_count` per primary genre (the ranking
/// key) and counts the tracks (surfaced as the row's `song_count` subtitle),
/// returning the top [`HOT_PICKS_PER_SECTION`] genres. Approximate: only the
/// sampled songs and each song's primary genre count.
pub(crate) fn tally_genres_by_play(
    songs: &[nokkvi_data::types::song::Song],
) -> Vec<GenreUIViewData> {
    let mut plays: HashMap<String, u64> = HashMap::new();
    let mut tracks: HashMap<String, u32> = HashMap::new();
    for s in songs {
        let Some(genre) = s.genre.as_ref().filter(|g| !g.is_empty()) else {
            continue;
        };
        *plays.entry(genre.clone()).or_default() += u64::from(s.play_count.unwrap_or(0));
        *tracks.entry(genre.clone()).or_default() += 1;
    }
    let mut ranked: Vec<(String, u64, u32)> = plays
        .into_iter()
        .map(|(g, p)| {
            let t = tracks.get(&g).copied().unwrap_or(0);
            (g, p, t)
        })
        .collect();
    // Rank by summed plays desc, then track count desc, then name for a stable
    // deterministic order among ties.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    ranked.truncate(HOT_PICKS_PER_SECTION);
    ranked
        .into_iter()
        .map(|(name, _plays, track_count)| {
            GenreUIViewData::from(nokkvi_data::types::genre::Genre {
                // Placeholder until `stamp_tally_genre_ids` sets the tag id.
                id: name.clone(),
                name,
                album_count: 0,
                song_count: track_count,
            })
        })
        .collect()
}

/// Stamp each tally genre with its tag id from the server's `/api/genre` list,
/// so its quad lookup can filter on `genre_id` (which matches only the tag id
/// since Navidrome 0.64). An exact name match wins; otherwise the names are
/// compared lowercased, because Navidrome derives one tag id from the
/// lowercased value while a song's `genre` string can differ in case from the
/// listed name. Only `id` changes: the name stays Harbour's key.
///
/// A miss leaves `id == name`. On 0.64+ that genre's quad stays blank; older
/// servers still match the name.
pub(crate) fn stamp_tally_genre_ids(
    tally: &mut [GenreUIViewData],
    server_genres: &[nokkvi_data::types::genre::Genre],
) {
    for genre in tally {
        let exact = server_genres.iter().find(|g| g.name == genre.name);
        let matched = exact.or_else(|| {
            let lower = genre.name.to_lowercase();
            server_genres
                .iter()
                .find(|g| g.name.to_lowercase() == lower)
        });
        if let Some(server) = matched {
            genre.id.clone_from(&server.id);
        }
    }
}

/// The Random Genre draw from a client-shuffled genre list: the first genre
/// with songs, else the first genre. The playability filter is a PREFERENCE,
/// not a gate: `song_count` rides the opportunistic Subsonic `getGenres`
/// enrichment, which `load_genres_with_libraries` degrades to an empty counts
/// map on failure — gating on it would zero every candidate and deaden the row
/// on any server that only exposes `/api/`. Falling back to the first genre
/// keeps the one-press play working (Navidrome only lists tagged genres, so a
/// genre in the list has songs regardless of the count).
pub(crate) fn pick_random_genre(
    genres: &[nokkvi_data::types::genre::Genre],
) -> Option<nokkvi_data::types::genre::Genre> {
    genres
        .iter()
        .find(|g| g.song_count > 0)
        .or_else(|| genres.first())
        .cloned()
}

/// The `(name, tag id)` pairs the shelf quad fan-out still has to resolve: the
/// Random Genre pick and the Most Played genres whose `artwork_album_ids` are
/// empty, deduped by NAME (Harbour's genre key). The pick is chained first so
/// its `/api/genre` id wins over a same-name tally entry whose stamp may have
/// missed.
pub(crate) fn genres_needing_quad_ids(
    harbour: &crate::state::HarbourState,
) -> Vec<(String, String)> {
    let mut seen_genre_names = HashSet::new();
    harbour
        .random_genre
        .iter()
        .chain(harbour.most_played_genres.iter())
        .filter(|g| g.artwork_album_ids.is_empty())
        .filter(|g| seen_genre_names.insert(g.name.clone()))
        .map(|g| (g.name.clone(), g.id.clone()))
        .collect()
}

/// The album ids whose 80px covers a whole-library search preview needs warmed:
/// one per album row (its own id) and one per song row (its `album_id`, when
/// set). Artist rows carry no cover; genre and playlist search rows have no
/// resolved album ids, so neither contributes. Free fn so the set is
/// unit-testable without a live `app_service`.
pub(crate) fn search_warm_album_ids(
    results: &nokkvi_data::types::library_search::LibrarySearchResults,
) -> Vec<String> {
    results
        .albums
        .iter()
        .filter(|a| !a.image.image_absent)
        .map(|a| a.id.clone())
        .chain(results.songs.iter().filter_map(|s| s.album_id.clone()))
        .collect()
}

/// Fan out each genre's album-id lookup (feeding its 2×2 quad cover)
/// concurrently, mapping `genre name → its album ids`. One failed lookup
/// degrades to an empty tile set (`unwrap_or_default`) rather than dropping the
/// whole fan-out.
///
/// Takes `(name, tag id)` pairs. The request filters on the tag id (the only
/// value `genre_id` matches since Navidrome 0.64); the result is keyed by the
/// NAME, Harbour's genre identity throughout (the quad/collage side-maps,
/// `PlayTarget::GenreRandom`). Shared by the shelf warm (`warm_harbour_artwork`)
/// and the search warm (`fan_out_search_collage_ids`) so both resolve genres
/// identically. See gotchas.md "Genre identity".
async fn resolve_genre_album_ids(
    shell: &AppService,
    genres: Vec<(String, String)>,
) -> Vec<(String, Vec<String>)> {
    let (server_url, cred) = shell.auth().server_config().await;
    let Some(client) = shell.auth().get_client().await else {
        return Vec::new();
    };
    let futures = genres.into_iter().map(|(name, genre_id)| {
        let client = client.clone();
        let server_url = server_url.clone();
        let cred = cred.clone();
        async move {
            let svc =
                nokkvi_data::services::api::genres::GenresApiService::new(client, server_url, cred);
            let ids = svc.load_genre_albums(&genre_id).await.unwrap_or_default();
            (name, ids)
        }
    });
    futures::future::join_all(futures).await
}

/// Playlist mirror of [`resolve_genre_album_ids`]: fan out each playlist's
/// album-id lookup (feeding its 2×2 quad cover), one failed lookup degrading to
/// an empty tile set. Shared by the shelf warm (`warm_harbour_artwork`, for the
/// Random Playlist pick) and the search quad-id fan-out
/// (`fan_out_search_collage_ids`) so both resolve playlists identically.
async fn resolve_playlist_album_ids(
    shell: &AppService,
    playlist_ids: Vec<String>,
) -> Vec<(String, Vec<String>)> {
    let (server_url, cred) = shell.auth().server_config().await;
    let Some(client) = shell.auth().get_client().await else {
        return Vec::new();
    };
    let futures = playlist_ids.into_iter().map(|id| {
        let client = client.clone();
        let server_url = server_url.clone();
        let cred = cred.clone();
        async move {
            let svc = nokkvi_data::services::api::playlists::PlaylistsApiService::new(
                client, server_url, cred,
            );
            let ids = svc.load_playlist_albums(&id).await.unwrap_or_default();
            (id, ids)
        }
    });
    futures::future::join_all(futures).await
}

/// Fetch the [`crate::views::harbour::RANDOM_SONGS_DRAW`]-song random batch for
/// a genre, scoped to the active libraries. ONE fetch shared by the genre play
/// paths (`play_harbour_genre`) and the genre add-to-queue paths
/// (`Nokkvi::enqueue_genre_draw`), so Shift+A enqueues the same shape of batch
/// Enter plays.
///
/// `genre_name` is the genre NAME: Navidrome's `getRandomSongs?genre=` matches
/// against the tag VALUE, while `Genre::id` from `/api/genre` is a separate
/// `tag.id` hash.
async fn random_genre_songs(
    shell: &AppService,
    genre_name: &str,
) -> anyhow::Result<Vec<nokkvi_data::types::song::Song>> {
    let ids = shell.active_library_ids_vec();
    shell
        .random_api()
        .await?
        .get_random_songs(
            crate::views::harbour::RANDOM_SONGS_DRAW,
            Some(genre_name),
            &ids,
        )
        .await
}

/// The success toast every Harbour enqueue path reports. A fn rather than a const
/// because `Message` is not const-constructible; the point is that the copy exists
/// once, so the batch / genre-draw / Random-block paths cannot drift apart.
fn added_to_queue_message() -> Message {
    Message::Toast(crate::app_message::ToastMessage::Push(
        nokkvi_data::types::toast::Toast::new(
            "Added to queue",
            nokkvi_data::types::toast::ToastLevel::Success,
        ),
    ))
}

/// Error context for every Harbour enqueue path (`shell_action_task` renders it
/// as "Failed to {ctx}: {e}"). Paired with [`added_to_queue_message`].
const ADD_TO_QUEUE_CTX: &str = "add to queue";

/// Fold a resolved track list into a `BatchPayload` of song items. Used wherever
/// Harbour enqueues concrete songs rather than an entity id.
fn song_batch(songs: Vec<nokkvi_data::types::song::Song>) -> BatchPayload {
    songs
        .into_iter()
        .fold(BatchPayload::new(), |payload, song| {
            payload.with_item(BatchItem::Song(Box::new(song)))
        })
}

/// The `BatchPayload` a RandomPlay row's pick enqueues — the add-to-queue mirror
/// of what `play_random_kind` plays. A free fn over `&HarbourState` so the
/// per-kind payload shape is unit-testable without an `app_service`: the
/// handler-level test can only observe "no toast", which a silent no-op also
/// produces.
///
/// `None` for [`RandomKind::Genres`], whose batch is a fetch
/// (`Nokkvi::enqueue_genre_draw`), and for any kind whose draw hasn't landed.
pub(super) fn random_kind_batch_payload(
    harbour: &crate::state::HarbourState,
    kind: RandomKind,
) -> Option<BatchPayload> {
    match kind {
        RandomKind::Albums => harbour
            .random_album
            .as_ref()
            .map(|a| BatchPayload::new().with_item(BatchItem::Album(a.id.clone()))),
        RandomKind::Artists => harbour
            .random_artist
            .as_ref()
            .map(|a| BatchPayload::new().with_item(BatchItem::Artist(a.id.clone()))),
        RandomKind::Songs => {
            (!harbour.random_songs.is_empty()).then(|| song_batch(harbour.random_songs.clone()))
        }
        RandomKind::Playlists => harbour
            .random_playlist
            .as_ref()
            .map(|p| BatchPayload::new().with_item(BatchItem::Playlist(p.id.clone()))),
        RandomKind::Genres => None,
    }
}

impl Nokkvi {
    /// Dispatch a Harbour view message. Runs the shared chrome prologue first
    /// (SetOpenMenu / artwork-drag intercepts), then routes page actions.
    pub(crate) fn handle_harbour(&mut self, msg: HarbourMessage) -> Task<Message> {
        if let Some(task) = crate::update::dispatch_view_chrome(self, &msg, crate::View::Harbour) {
            return task;
        }
        match msg {
            HarbourMessage::SlotList(slmsg) => self.handle_harbour_slot_list(slmsg),
            HarbourMessage::SearchChanged(query) => self.handle_harbour_search(query),
            HarbourMessage::SeeAll(section) => {
                let query = self.harbour.search_query.clone();
                self.handle_navigate_with_search(section.target_view(), query)
            }
            HarbourMessage::ToggleSection(id) => {
                self.harbour_page.toggle_section(id);
                // The toggle shifts the row list, so a different row can now
                // sit at the (unmoved) center index — re-warm its large art.
                self.warm_harbour_current_center()
            }
            HarbourMessage::ExpandCenter => self.handle_harbour_expand_center(),
            // Intercepted by the chrome prologue above; kept for exhaustiveness.
            HarbourMessage::SetOpenMenu(_)
            | HarbourMessage::ArtworkColumnDrag(_)
            | HarbourMessage::ArtworkColumnVerticalDrag(_)
            | HarbourMessage::NoOp => Task::none(),
        }
    }

    /// Resolve a Harbour slot-list message against the *live* flattened rows.
    /// Re-derives the same row order the view renders so a centered index maps
    /// to the same row the user sees.
    fn handle_harbour_slot_list(
        &mut self,
        slmsg: crate::widgets::SlotListPageMessage,
    ) -> Task<Message> {
        use crate::widgets::{SlotListPageAction, SlotListPageMessage};

        let rows = build_harbour_rows(
            &self.harbour,
            &self.harbour_page.collapsed,
            &self.trawl_crate,
        );
        let total = rows.len();
        // NavigateUp/Down/SetOffset move the center — warm the new center's art.
        let needs_art = matches!(
            slmsg,
            SlotListPageMessage::NavigateUp
                | SlotListPageMessage::NavigateDown
                | SlotListPageMessage::SetOffset(_, _)
        );
        let action = self.harbour_page.common.handle(slmsg, total);

        match action {
            SlotListPageAction::ActivateCenter(force) => {
                // A centered section header toggles; a centered item plays.
                if self.toggle_centered_harbour_section(&rows, total, false) {
                    // The toggle changed the row list under the stationary
                    // center — re-warm whatever row sits there now.
                    return self.warm_harbour_current_center();
                }
                let center = self.harbour_page.common.get_center_item_index(total);
                // The Trawl door opens its modal — not a play, not a toggle.
                // Opened synchronously (no message hop); the returned task
                // focuses the modal's search input.
                if let Some(HarbourRow::Trawl { .. }) = center.and_then(|i| rows.get(i)) {
                    return self
                        .handle_trawl_modal(crate::widgets::trawl_modal::TrawlModalMessage::Open);
                }
                // A RandomPlay row plays its pre-drawn pick — the one the row
                // previews; only a shelves reload re-rolls it.
                if let Some(HarbourRow::RandomPlay { kind }) = center.and_then(|i| rows.get(i)) {
                    let kind = *kind;
                    return self.play_random_kind(kind, force);
                }
                if let Some(HarbourRow::Item { play, .. }) = center.and_then(|i| rows.get(i)) {
                    let play = play.clone();
                    // `force` carries Ctrl+Enter's one-shot shuffle intent — thread
                    // it through so a Harbour item honors Shuffle Play like every
                    // other view (genre-random is already random and ignores it).
                    self.play_harbour_target(play, force)
                } else {
                    Task::none()
                }
            }
            SlotListPageAction::AddCenterToQueue => {
                let center = self.harbour_page.common.get_center_item_index(total);
                match center.and_then(|i| rows.get(i)) {
                    Some(HarbourRow::Item {
                        play: PlayTarget::Item(batch_item),
                        ..
                    }) => {
                        let payload = BatchPayload::new().with_item(batch_item.clone());
                        self.enqueue_harbour_batch(payload)
                    }
                    // A RandomPlay row previews a concrete pick, so Shift+A must
                    // enqueue it rather than silently doing nothing (Enter on the
                    // very same row already plays it).
                    Some(HarbourRow::RandomPlay { kind }) => {
                        let kind = *kind;
                        self.add_random_kind_to_queue(kind)
                    }
                    // A genre item row's Enter plays a capped random draw, so its
                    // Shift+A enqueues the same shape — the Random Genre row and a
                    // Most Played Genres row must not disagree on the hotkey.
                    Some(HarbourRow::Item {
                        play: PlayTarget::GenreRandom(name),
                        ..
                    }) => {
                        let name = name.clone();
                        self.enqueue_genre_draw(name)
                    }
                    Some(
                        HarbourRow::Section { .. } | HarbourRow::Trawl { .. } | HarbourRow::Hint(_),
                    )
                    | None => Task::none(),
                }
            }
            // `handle_load_harbour` owns the in-flight guard, so the header
            // Refresh button, the `r` hotkey (which routes through
            // `reload_message()` → `Message::LoadHarbour`) and Escape-on-empty-
            // search are all covered by one check.
            SlotListPageAction::RefreshViewData => self.handle_load_harbour(),
            SlotListPageAction::None => {
                // Warm the newly-centered row's large artwork (crisp single
                // cover + a collection's 300px collage) — see
                // `warm_harbour_center_art`.
                if needs_art {
                    let center = self
                        .harbour_page
                        .common
                        .get_center_item_index(total)
                        .and_then(|i| rows.get(i));
                    return self.warm_harbour_center_art(center);
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The freshly-built flattened Harbour rows plus the center index into them
    /// — the single resolver for "which Harbour row is centered", shared by the
    /// two artwork-warm paths (`warm_harbour_center_art` callers) and the
    /// activate-SFX classification, so the centering rule can't drift between
    /// them. Returns the owned rows (the row enum isn't `Clone`) so each caller
    /// resolves the center via `center.and_then(|i| rows.get(i))`.
    pub(crate) fn harbour_centered_rows(&self) -> (Vec<HarbourRow>, Option<usize>) {
        let rows = build_harbour_rows(
            &self.harbour,
            &self.harbour_page.collapsed,
            &self.trawl_crate,
        );
        let center = self.harbour_page.common.get_center_item_index(rows.len());
        (rows, center)
    }

    /// Warm the centered row's large artwork against the CURRENT rows — for
    /// handlers where the data or row list changed under a *stationary* center:
    /// the initial shelf load (`ShelvesLoaded`), quad-id arrival for a centered
    /// collection, a section toggle shifting rows, a search transition swapping
    /// the row list, and view re-entry. Without these calls the large column
    /// stays stuck on its 80px fallback until the user happens to move the
    /// center away and back (the only other warm triggers are
    /// NavigateUp/Down/SetOffset and seek-settle).
    pub(crate) fn warm_harbour_current_center(&mut self) -> Task<Message> {
        let (rows, center) = self.harbour_centered_rows();
        self.warm_harbour_center_art(center.and_then(|i| rows.get(i)))
    }

    /// Warm the newly-centered Harbour row's large artwork. Batches:
    /// 1. the crisp single representative cover (`LoadLarge`) — the whole cover
    ///    for album/song items and album section headers, and the fallback a
    ///    collection shows until its collage lands; and
    /// 2. a centered collection's 300px collage (`LoadCollage`) — a playlist or
    ///    genre item, or a collection section header previewed via its first
    ///    pick — so the large column shows the same crisp mosaic the real
    ///    Playlists/Genres views render, not an upscaled 80px mini.
    ///
    /// Shared by the keyboard/scroll center path in `handle_harbour_slot_list`
    /// and the scroll-settle path in `handle_seek_settled` so both warm alike.
    pub(crate) fn warm_harbour_center_art(&mut self, center: Option<&HarbourRow>) -> Task<Message> {
        // Resolve everything from the borrowed row + harbour state into owned
        // values FIRST, so the `&mut self` collage warm below doesn't conflict.
        // Artist rows warm their large image via the artist endpoint
        // (`ar-{id}` → `large_artwork[artist_id]`, the artist arm first); an
        // album `LoadLarge` on an artist id would 404. Everything else takes the
        // album large cover.
        let (album_large, artist_large) = match center {
            Some(HarbourRow::Item {
                play: PlayTarget::Item(BatchItem::Artist(id)),
                ..
            }) => (None, Some(id.clone())),
            Some(HarbourRow::Item {
                art_album_id: Some(id),
                ..
            }) => (Some(id.clone()), None),
            // The Most Played Artists *header* previews its first artist, whose
            // large image is keyed by artist id — route it through the artist
            // loader too, not the album LoadLarge (which would 404).
            Some(HarbourRow::Section {
                id: HarbourSectionId::MostPlayedArtists,
                ..
            }) => (
                None,
                self.harbour
                    .most_played_artists
                    .first()
                    .map(|a| a.id.clone()),
            ),
            Some(HarbourRow::Section { id, .. }) => (
                crate::views::harbour::section_cover_album_id(&self.harbour, *id),
                None,
            ),
            // A RandomPlay row previews its pre-drawn pick: album/song picks
            // warm the album large cover; the artist pick routes through the
            // artist loader (an album LoadLarge on an artist id would 404);
            // the genre/playlist picks warm their collage below and fall back
            // to their first quad tile as the single cover.
            Some(HarbourRow::RandomPlay { kind }) => match kind {
                RandomKind::Albums => (
                    self.harbour.random_album.as_ref().map(|a| a.id.clone()),
                    None,
                ),
                RandomKind::Artists => (
                    None,
                    self.harbour.random_artist.as_ref().map(|a| a.id.clone()),
                ),
                RandomKind::Songs => (
                    self.harbour
                        .random_songs
                        .first()
                        .and_then(|s| s.album_id.clone()),
                    None,
                ),
                RandomKind::Genres => (
                    self.harbour
                        .random_genre
                        .as_ref()
                        .and_then(|g| g.artwork_album_ids.first().cloned()),
                    None,
                ),
                RandomKind::Playlists => (
                    self.harbour
                        .random_playlist
                        .as_ref()
                        .and_then(|p| p.artwork_album_ids.first().cloned()),
                    None,
                ),
            },
            _ => (None, None),
        };
        let collage = self.harbour_center_collage_target(center);
        // A centered custom-cover playlist warms its resolution-sized cover so
        // the large column shows it crisp (the warm no-ops for album-art
        // playlists). Search items and the Random Playlist pick both qualify;
        // `None` for section headers / non-playlist rows.
        let custom_playlist = match center {
            Some(HarbourRow::Item {
                play: PlayTarget::Item(BatchItem::Playlist(pid)),
                ..
            }) => Some(pid.clone()),
            Some(HarbourRow::RandomPlay {
                kind: RandomKind::Playlists,
            }) => self.harbour.random_playlist.as_ref().map(|p| p.id.clone()),
            _ => None,
        };

        let mut tasks = Vec::new();
        if let Some(id) = album_large {
            tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(id))));
        }
        if let Some(id) = artist_large {
            tasks.push(self.handle_load_artist_large_artwork(id));
        }
        if let Some((target, entity_id, album_ids)) = collage {
            tasks.push(self.warm_harbour_collage(target, entity_id, album_ids));
        }
        if let Some(pid) = custom_playlist {
            tasks.push(self.handle_load_playlist_custom_large(pid));
        }
        Task::batch(tasks)
    }

    /// The collage load a centered row wants, as `(target, entity_id, album_ids)`
    /// — a playlist/genre Item (its own album ids), or a collection Section
    /// header (its first pick's ids, the one the pill names). `None` for
    /// albums/songs and album section headers.
    fn harbour_center_collage_target(
        &self,
        center: Option<&HarbourRow>,
    ) -> Option<(CollageTarget, String, Vec<String>)> {
        match center? {
            // The Trawl action row previews no collection.
            HarbourRow::Trawl { .. } => None,
            // The genre/playlist RandomPlay picks preview their own collage.
            HarbourRow::RandomPlay { kind } => match kind {
                // Keyed by NAME — Harbour's genre-collage key everywhere (the
                // search item rows key via `PlayTarget::GenreRandom`, which
                // carries the name), and the render arm reads the same key.
                RandomKind::Genres => self.harbour.random_genre.as_ref().map(|g| {
                    (
                        CollageTarget::Genre,
                        g.name.clone(),
                        g.artwork_album_ids.clone(),
                    )
                }),
                RandomKind::Playlists => self.harbour.random_playlist.as_ref().map(|p| {
                    (
                        CollageTarget::Playlist,
                        p.id.clone(),
                        p.artwork_album_ids.clone(),
                    )
                }),
                RandomKind::Albums | RandomKind::Artists | RandomKind::Songs => None,
            },
            HarbourRow::Item {
                art_album_ids,
                play,
                ..
            } => match play {
                PlayTarget::Item(BatchItem::Playlist(pid)) => {
                    Some((CollageTarget::Playlist, pid.clone(), art_album_ids.clone()))
                }
                PlayTarget::GenreRandom(gid) => {
                    Some((CollageTarget::Genre, gid.clone(), art_album_ids.clone()))
                }
                PlayTarget::Item(_) => None,
            },
            // Section headers preview their first pick's collage — the section
            // set + entity resolution is shared with the view's preview panel
            // via `section_collage_source` so the two can't drift.
            HarbourRow::Section { id, .. } => {
                crate::views::harbour::section_collage_source(&self.harbour, *id).map(
                    |(target, entity_id, album_ids)| {
                        (target, entity_id.to_string(), album_ids.to_vec())
                    },
                )
            }
            HarbourRow::Hint(_) => None,
        }
    }

    /// Dispatch a 300px collage load for a centered collection into the shared
    /// collage cache (`self.artwork.{playlist,genre}.collage`), reusing the real
    /// views' `LoadCollage` pipeline. De-duped on the cache + pending set;
    /// `pending` is marked before the `app_service` check so the gate engages in
    /// tests too. No-op without album ids (nothing to tile) or a live service.
    fn warm_harbour_collage(
        &mut self,
        target: CollageTarget,
        entity_id: String,
        album_ids: Vec<String>,
    ) -> Task<Message> {
        if album_ids.is_empty() {
            return Task::none();
        }
        {
            let cache = self.collage_cache_mut(target);
            if cache.collage.snapshot.contains_key(&entity_id) || cache.pending.contains(&entity_id)
            {
                return Task::none();
            }
            cache.pending.insert(entity_id.clone());
        }
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let auth_vm = shell.auth().clone();
        Task::perform(
            async move {
                let (server_url, cred) = auth_vm.server_config().await;
                (entity_id, server_url, cred, album_ids)
            },
            move |(id, url, cred, ids)| {
                Message::Artwork(ArtworkMessage::LoadCollage(target, id, url, cred, ids))
            },
        )
    }

    /// Shift+Enter on Harbour: toggle the centered section header, or — centered
    /// on an item row — collapse its owning section (the expansion views'
    /// collapse-from-child contract, so browsing into a section's items never
    /// strands the hotkey). Centered on the Trawl door, a RandomPlay row, or a
    /// hint is a no-op (nothing to expand). Rebuilds rows from the same
    /// `(&harbour, &collapsed)` inputs the view renders, so the centered index
    /// resolves to the row the user sees.
    fn handle_harbour_expand_center(&mut self) -> Task<Message> {
        let rows = build_harbour_rows(
            &self.harbour,
            &self.harbour_page.collapsed,
            &self.trawl_crate,
        );
        let total = rows.len();
        if self.toggle_centered_harbour_section(&rows, total, true) {
            // Rows shifted under the stationary center — re-warm it.
            return self.warm_harbour_current_center();
        }
        Task::none()
    }

    /// If the centered row is a `Section`, toggle its collapsed state and return
    /// `true`. Shared by the ActivateCenter (Enter) and ExpandCenter
    /// (Shift+Enter) paths so both resolve the center against the same rows.
    ///
    /// `collapse_from_child` extends the reach for Shift+Enter only: centered on
    /// an `Item`, collapse its owning section — the nearest `Section` above it
    /// (items only ever render directly under their expanded header) — and
    /// re-center on that header. Enter keeps `false` so activating an item still
    /// plays it.
    fn toggle_centered_harbour_section(
        &mut self,
        rows: &[HarbourRow],
        total: usize,
        collapse_from_child: bool,
    ) -> bool {
        let Some(center) = self.harbour_page.common.get_center_item_index(total) else {
            return false;
        };
        match rows.get(center) {
            Some(HarbourRow::Section { id, .. }) => {
                let id = *id;
                self.harbour_page.toggle_section(id);
                true
            }
            Some(HarbourRow::Item { .. }) if collapse_from_child => {
                let Some((header_idx, id)) =
                    rows[..center].iter().enumerate().rev().find_map(|(i, r)| {
                        if let HarbourRow::Section { id, .. } = r {
                            Some((i, *id))
                        } else {
                            None
                        }
                    })
                else {
                    return false;
                };
                self.harbour_page.toggle_section(id);
                // Re-center on the header — its index is unchanged (collapsing
                // only removes the rows after it), while the centered item's
                // index now points at a different or vanished row. Routed
                // through `handle_set_offset` (clears the click-to-focus
                // marker AND records the scroll, so the transient scrollbar
                // flashes on the jump exactly like `ExpansionState::collapse`).
                let new_total = build_harbour_rows(
                    &self.harbour,
                    &self.harbour_page.collapsed,
                    &self.trawl_crate,
                )
                .len();
                self.harbour_page
                    .common
                    .handle_set_offset(header_idx, new_total);
                true
            }
            _ => false,
        }
    }

    /// Play a resolved Harbour item target (guard radio-to-queue, reset context,
    /// then play the single-item batch / genre-random page). `force` is the
    /// Ctrl+Enter shuffle directive, applied to the batch play; genre-random
    /// already draws server-random songs, so it ignores it.
    fn play_harbour_target(&mut self, play: PlayTarget, force: bool) -> Task<Message> {
        match play {
            PlayTarget::Item(batch) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                self.play_batch_task(BatchPayload::new().with_item(batch), force)
            }
            PlayTarget::GenreRandom(name) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                self.play_harbour_genre(name)
            }
        }
    }

    /// Enqueue a resolved Harbour batch with the shared "Added to queue" toast.
    /// One builder for the item rows and the Random block so both report the
    /// same success/failure copy.
    fn enqueue_harbour_batch(&self, payload: BatchPayload) -> Task<Message> {
        self.shell_action_task(
            move |shell| async move { shell.add_batch_to_queue(payload).await },
            added_to_queue_message(),
            ADD_TO_QUEUE_CTX,
        )
    }

    /// Enqueue a genre's capped random draw. Genres have no pre-resolved track
    /// list — the "pick" is the genre — so the batch is fetched, using the same
    /// [`random_genre_songs`] draw the play path runs. Shared by the Random Genre
    /// row and the genre item rows so Shift+A behaves identically on both.
    fn enqueue_genre_draw(&self, genre_name: String) -> Task<Message> {
        self.shell_action_task(
            move |shell| async move {
                let songs = random_genre_songs(&shell, &genre_name).await?;
                shell.add_batch_to_queue(song_batch(songs)).await
            },
            added_to_queue_message(),
            ADD_TO_QUEUE_CTX,
        )
    }

    /// Add-to-queue twin of [`Self::play_random_kind`]: enqueue the row's
    /// PRE-DRAWN pick so Shift+A adds what Enter would play. The four
    /// entity/track picks resolve synchronously through
    /// [`random_kind_batch_payload`]; Genres alone needs a fetch.
    fn add_random_kind_to_queue(&mut self, kind: RandomKind) -> Task<Message> {
        if !crate::views::harbour::has_random_pick(&self.harbour, kind) {
            return self.random_no_pick_toast(kind);
        }
        if kind == RandomKind::Genres {
            // `BatchItem::Genre` would enqueue the WHOLE genre; the row promises
            // a capped draw, so resolve it through the same fetch the play uses.
            return self
                .harbour
                .random_genre
                .as_ref()
                .map(|g| g.name.clone())
                .map_or_else(Task::none, |name| self.enqueue_genre_draw(name));
        }
        random_kind_batch_payload(&self.harbour, kind)
            .map_or_else(Task::none, |payload| self.enqueue_harbour_batch(payload))
    }

    /// The toast a RandomPlay row shows when its draw hasn't landed. Split from
    /// the activation paths so play and add-to-queue explain the same state the
    /// same way. Deliberately action-neutral (it serves both Enter and Shift+A)
    /// and deliberately silent about the CAUSE once the load has settled: an
    /// absent pick is either a library with none of this kind or a draw that
    /// failed (`recover_pick` collapses both to `None`), and nothing distinguishes
    /// them here — so the copy states the fact and suggests the one action that
    /// might help, without claiming the library is empty.
    fn random_no_pick_toast(&mut self, kind: RandomKind) -> Task<Message> {
        if self.harbour.shelves_loading {
            self.toast_info("Still drawing the Random picks — try again in a moment");
        } else {
            self.toast_info(format!(
                "No random {} drawn — Refresh to try again",
                kind.noun().to_lowercase()
            ));
        }
        Task::none()
    }

    /// Activate a RandomPlay row: play the row's PRE-DRAWN pick — exactly what
    /// the row previews (thumbnail, facts, panel). Picks re-roll on every
    /// shelves load, so the Refresh hotkey is the re-roll. Runs the same
    /// pre-play sequence as every Harbour play (radio guard, new playback
    /// context) and lands on the Queue. `force` carries Ctrl+Enter's one-shot
    /// shuffle for the album/artist/playlist picks; the song draws are already
    /// random and ignore it.
    ///
    /// The playlist arm mirrors the Playlists view's play path: it SETS the
    /// active-playlist context (the queue's "Playing From" strip + quad)
    /// before `play_playlist`, where every other arm clears it.
    pub(crate) fn play_random_kind(
        &mut self,
        kind: crate::views::harbour::RandomKind,
        force: bool,
    ) -> Task<Message> {
        use crate::views::harbour::RandomKind;

        // No pick yet: the shelves are still loading, or the library holds none of
        // this kind — nothing previewed, nothing to play. Gate on the SAME
        // predicate the activate-SFX classifier reads, so the "activates nothing"
        // escape cue and this toast can never disagree. The per-arm `else`
        // branches below stay as defensive extraction failures.
        if !crate::views::harbour::has_random_pick(&self.harbour, kind) {
            return self.random_no_pick_toast(kind);
        }
        let no_pick = |app: &mut Self| app.random_no_pick_toast(kind);

        match kind {
            RandomKind::Albums => {
                let Some(id) = self.harbour.random_album.as_ref().map(|a| a.id.clone()) else {
                    return no_pick(self);
                };
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                self.play_batch_task(BatchPayload::new().with_item(BatchItem::Album(id)), force)
            }
            RandomKind::Artists => {
                let Some(id) = self.harbour.random_artist.as_ref().map(|a| a.id.clone()) else {
                    return no_pick(self);
                };
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                self.play_batch_task(BatchPayload::new().with_item(BatchItem::Artist(id)), force)
            }
            RandomKind::Songs => {
                if self.harbour.random_songs.is_empty() {
                    return no_pick(self);
                }
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                self.clear_active_playlist();
                let songs = self.harbour.random_songs.clone();
                self.shell_action_task(
                    move |shell| async move { shell.play_songs(songs, 0, OneShotShuffle::None).await },
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "play random songs",
                )
            }
            RandomKind::Genres => {
                let Some(name) = self.harbour.random_genre.as_ref().map(|g| g.name.clone()) else {
                    return no_pick(self);
                };
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                self.clear_active_playlist();
                self.play_harbour_genre(name)
            }
            RandomKind::Playlists => {
                let Some(playlist) = self.harbour.random_playlist.clone() else {
                    return no_pick(self);
                };
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                // The Playlists view's PlayAll contract: set the queue header's
                // "Playing From" context BEFORE the play so the strip renders
                // it on arrival (handle_queue_loaded freezes the strip quad
                // from it).
                self.active_playlist_info = Some(
                    crate::state::ActivePlaylistContext::from_playlist(&playlist),
                );
                self.persist_active_playlist_info();
                let shuffle = self.activate_shuffle_directive(force, false);
                let id = playlist.id;
                self.shell_action_task(
                    move |shell| async move { shell.play_playlist(&id, shuffle).await },
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "play playlist",
                )
            }
        }
    }

    /// Header search: immediate (no debounce), gated on a
    /// [`SEARCH_MIN_CHARS`]-char threshold, with per-keystroke generation
    /// stale-drop over the fanned-out result. `pub(crate)` so a library-scope
    /// change / Harbour re-entry can re-fire the active search against the new
    /// scope.
    pub(crate) fn handle_harbour_search(&mut self, query: String) -> Task<Message> {
        self.harbour.search_query = query;
        // Bump every keystroke so an earlier in-flight fan-out is discarded when
        // it lands (even the "cleared" transitions bump, so a late result can't
        // repopulate an emptied query).
        self.harbour.search_generation = self.harbour.search_generation.wrapping_add(1);
        let generation = self.harbour.search_generation;

        // Mirror the live query into the shared slot-list state and reset the
        // viewport to the top — the same two things every other view gets from
        // routing through `handle_search_query_changed`. The mirror is what the
        // generic Escape handler and browsing-panel guard read (an always-empty
        // `common.search_query` made every Escape reload-and-re-roll the random
        // shelves); the offset reset stops a deep shelf scroll from stranding
        // the viewport past the end of a short result list.
        let total = build_harbour_rows(
            &self.harbour,
            &self.harbour_page.collapsed,
            &self.trawl_crate,
        )
        .len();
        self.harbour_page
            .common
            .handle_search_query_changed(self.harbour.search_query.clone(), total);

        let trimmed = self.harbour.search_query.trim().to_string();
        if trimmed.chars().count() < SEARCH_MIN_CHARS {
            self.harbour.search_results = None;
            self.harbour.search_loading = false;
            // The row list just swapped back to shelves (or to the keep-typing
            // hint) under the stationary center — re-warm it.
            return self.warm_harbour_current_center();
        }

        self.harbour.search_loading = true;
        self.shell_task(
            move |shell| async move {
                let ids = shell.active_library_ids_vec();
                shell
                    .search_library(&trimmed, SEARCH_PREVIEW_LIMIT, &ids)
                    .await
            },
            move |result| {
                Message::HarbourLoader(HarbourLoaderMessage::SearchLoaded {
                    generation,
                    result: result.map(Box::new).map_err(|e| format!("{e:#}")),
                })
            },
        )
    }

    /// Play [`crate::views::harbour::RANDOM_SONGS_DRAW`] server-random songs of
    /// a genre. Uses Subsonic `getRandomSongs` (`genre` + `musicFolderId`) rather
    /// than `BatchItem::Genre`, which would enqueue the entire genre, and rather
    /// than a `_sort=random` songs page, which would re-seed Navidrome's
    /// seeded-random ordering and corrupt an in-progress Songs "Random"-sort
    /// pagination (see `services::api::random`). Takes the genre NAME — the
    /// endpoint's `genre` param matches the tag VALUE, and `Genre::id` from
    /// `/api/genre` is a separate `tag.id` hash. Assumes the caller already ran
    /// the play guard + new-context reset.
    fn play_harbour_genre(&mut self, genre_name: String) -> Task<Message> {
        self.shell_action_task(
            move |shell| async move {
                let songs = random_genre_songs(&shell, &genre_name).await?;
                shell.play_songs(songs, 0, OneShotShuffle::None).await
            },
            Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
            "play genre",
        )
    }

    /// Load the Harbour shelves in one joined fetch. Bumps the stale-drop
    /// generation and arms the loading flag before dispatching, so a result that
    /// lands after a newer load (or a library-filter change) is discarded by
    /// [`Self::handle_harbour_loader`].
    ///
    /// Skips while a load is already in flight. The guard lives HERE rather than
    /// at the call sites because four paths reach this function — the header
    /// Refresh button (`RefreshViewData`), the `r` hotkey and Escape-on-empty-
    /// search (both via `reload_message()` → `Message::LoadHarbour`), the
    /// start-view load, and the library-filter change — and a per-site guard
    /// covered only the first. Without it, a held `r` fires N overlapping
    /// ~10-request fan-outs (100-song draw included) of which the generation gate
    /// then discards all but one. `shelves_loading` is set synchronously below, so
    /// the guard is exact rather than one frame late; `invalidate_shelves` clears
    /// it, which is what lets a library-filter change supersede an in-flight load.
    pub(crate) fn handle_load_harbour(&mut self) -> Task<Message> {
        if self.harbour.shelves_loading {
            return Task::none();
        }
        self.harbour.shelves_generation = self.harbour.shelves_generation.wrapping_add(1);
        let generation = self.harbour.shelves_generation;
        self.harbour.shelves_loading = true;

        self.shell_task(
            move |shell| async move {
                let ids = shell.active_library_ids_vec();
                let (url, cred) = shell.auth().server_config().await;

                // Every fetch below goes through a PER-CALL API service, not
                // the shared Songs/Albums/Artists service singletons: the
                // singletons' raw-page wrappers write the browse views' shared
                // `total_count` reactive, and a background shelf load must not
                // clobber an in-flight Albums/Artists/Songs pagination read
                // (same rationale as `search_library`). The API calls take an
                // explicit sort order, so there is no per-entity wrapper
                // default (songs/albums DESC vs artists ASC) left to drift.

                // Recently Played is song-level (the actual tracks played,
                // sorted by play date via `/api/song?_sort=recentlyPlayed`);
                // Recently Added stays album-level.
                let recently_played_fut = async {
                    let api = shell.songs_api().await?;
                    api.load_songs(
                        "recentlyPlayed",
                        "DESC",
                        None,
                        None,
                        &ids,
                        Some(0),
                        Some(HOT_PICKS_PER_SECTION),
                    )
                    .await
                    .map(|(songs, _total)| songs)
                };
                let recently_added_fut = async {
                    let api = shell.albums_api().await?;
                    api.load_albums(
                        "recentlyAdded",
                        "DESC",
                        None,
                        None,
                        &ids,
                        Some(0),
                        Some(HOT_PICKS_PER_SECTION),
                    )
                    .await
                    .map(|(albums, _total)| albums)
                };

                // "Most Played" shelves (play_count DESC). The tracks fetch is
                // deliberately deep (MOST_PLAYED_TALLY_POOL): it doubles as the
                // genre-tally sample, so Most Played Tracks + Most Played Genres
                // ride one request.
                let most_played_songs_fut = async {
                    let api = shell.songs_api().await?;
                    api.load_songs(
                        "mostPlayed",
                        "DESC",
                        None,
                        None,
                        &ids,
                        Some(0),
                        Some(MOST_PLAYED_TALLY_POOL),
                    )
                    .await
                    .map(|(songs, _total)| songs)
                };
                let most_played_albums_fut = async {
                    let api = shell.albums_api().await?;
                    api.load_albums(
                        "mostPlayed",
                        "DESC",
                        None,
                        None,
                        &ids,
                        Some(0),
                        Some(HOT_PICKS_PER_SECTION),
                    )
                    .await
                    .map(|(albums, _total)| albums)
                };
                // `album_artists_only = true` matches the standard Artists browse
                // (role=albumartist), keeping featuring-only artists out.
                let most_played_artists_fut = async {
                    let api = shell.artists_api().await?;
                    api.load_artists(
                        "mostPlayed",
                        "DESC",
                        None,
                        None,
                        &ids,
                        true,
                        Some(0),
                        Some(HOT_PICKS_PER_SECTION),
                    )
                    .await
                    .map(|(artists, _total)| artists)
                };

                // The Random block's pre-drawn picks — re-rolled on every
                // shelves load (so the Refresh hotkey doubles as the re-roll),
                // previewed on the rows, and played verbatim on activation.
                //
                // NONE of these may use `_sort=random`: Navidrome re-seeds its
                // per-(table, user) seeded-random ordering for every such query
                // that arrives with `_start=0`, which silently re-permutes an
                // in-progress Albums/Songs "Random"-sort pagination in the
                // browse views (dup/gap on the next page). Albums + artists take
                // the uniform count+offset draw over a stable sort; songs take
                // Subsonic `getRandomSongs`, whose `ORDER BY random()` touches no
                // shared seed. Genres/playlists shuffle client-side inside their
                // loaders, so `first()` IS the draw.
                let random_album_fut = async {
                    let api = shell.albums_api().await?;
                    api.load_random_album(&ids).await
                };
                let random_artist_fut = async {
                    let api = shell.artists_api().await?;
                    api.load_random_artist(&ids, true).await
                };
                let random_songs_fut = async {
                    let api = shell.random_api().await?;
                    api.get_random_songs(crate::views::harbour::RANDOM_SONGS_DRAW, None, &ids)
                        .await
                };
                // The full genre list, client-shuffled: `pick_random_genre` draws
                // the Random Genre pick from it, and `stamp_tally_genre_ids`
                // gives the Most Played genres their tag ids, with no extra
                // request.
                let genre_list_fut = async {
                    let svc = shell.genres_api().await?;
                    svc.load_genres_with_libraries("random", "ASC", None, &ids)
                        .await
                        .map(|(genres, _total)| genres)
                };
                // Playlists keep a hard playability gate: `song_count` comes from
                // the native `/api/playlist` (always sent), and an empty playlist
                // is ordinary — a fresh "New Playlist" would turn the one-press
                // play into an error toast.
                let random_playlist_fut = async {
                    let svc = shell.playlists_api().await?;
                    svc.load_playlists_with_libraries("random", "ASC", None, &ids)
                        .await
                        .map(|(playlists, _total)| playlists.into_iter().find(|p| p.song_count > 0))
                };

                let (
                    recently_played,
                    recently_added,
                    most_played_songs,
                    most_played_albums,
                    most_played_artists,
                    random_album,
                    random_artist,
                    random_songs,
                    genre_list,
                    random_playlist,
                ) = futures::join!(
                    recently_played_fut,
                    recently_added_fut,
                    most_played_songs_fut,
                    most_played_albums_fut,
                    most_played_artists_fut,
                    random_album_fut,
                    random_artist_fut,
                    random_songs_fut,
                    genre_list_fut,
                    random_playlist_fut
                );

                // Recently-added is the backbone: a hard failure there is an
                // auth/network fault worth surfacing. The other shelves degrade
                // to empty (warn-logged) so one flaky sort doesn't blank the
                // whole home view.
                let project =
                    |albums: Vec<nokkvi_data::types::album::Album>| -> Vec<AlbumUIViewData> {
                        albums
                            .iter()
                            .map(|a| AlbumUIViewData::from_album(a, &url, &cred))
                            .collect()
                    };

                let recently_added = match recently_added {
                    Ok(a) => project(a),
                    Err(e) => return Err(format!("{e:#}")),
                };

                // Tally the full tracks pool by genre BEFORE truncating it to the
                // Most Played Tracks shelf's top picks.
                let mut most_played_songs = recover_shelf("most-played-tracks", most_played_songs);
                let genre_list = recover_shelf("genre-list", genre_list);
                let mut most_played_genres = tally_genres_by_play(&most_played_songs);
                stamp_tally_genre_ids(&mut most_played_genres, &genre_list);
                most_played_songs.truncate(HOT_PICKS_PER_SECTION);

                let most_played_albums =
                    project(recover_shelf("most-played-albums", most_played_albums));
                let mut most_played_artists =
                    recover_shelf("most-played-artists", most_played_artists);
                most_played_artists.truncate(HOT_PICKS_PER_SECTION);

                let random_album = recover_pick("random-album", random_album)
                    .map(|a| AlbumUIViewData::from_album(&a, &url, &cred));
                let random_playlist = recover_pick("random-playlist", random_playlist)
                    .map(nokkvi_data::backend::playlists::PlaylistUIViewData::from);
                let random_genre = pick_random_genre(&genre_list).map(GenreUIViewData::from);

                Ok(Box::new(HarbourShelvesData {
                    recently_played: recover_shelf("recently-played", recently_played),
                    recently_added,
                    most_played_songs,
                    most_played_albums,
                    most_played_artists,
                    most_played_genres,
                    random_album,
                    random_artist: recover_pick("random-artist", random_artist),
                    random_songs: recover_shelf("random-songs", random_songs),
                    random_genre,
                    random_playlist,
                }))
            },
            move |result| {
                Message::HarbourLoader(HarbourLoaderMessage::ShelvesLoaded { generation, result })
            },
        )
    }

    /// Handle a Harbour backend result. Every arm is generation-gated: a result
    /// whose captured generation no longer matches the current one is stale
    /// (a newer load or a library-filter change superseded it) and dropped.
    pub(crate) fn handle_harbour_loader(&mut self, msg: HarbourLoaderMessage) -> Task<Message> {
        match msg {
            HarbourLoaderMessage::ShelvesLoaded { generation, result } => {
                if generation != self.harbour.shelves_generation {
                    return Task::none();
                }
                self.harbour.shelves_loading = false;
                match result {
                    Ok(data) => {
                        let data = *data;
                        self.harbour.recently_played = data.recently_played;
                        self.harbour.recently_added = data.recently_added;
                        self.harbour.most_played_songs = data.most_played_songs;
                        self.harbour.most_played_albums = data.most_played_albums;
                        self.harbour.most_played_artists = data.most_played_artists;
                        self.harbour.most_played_genres = data.most_played_genres;
                        self.harbour.random_album = data.random_album;
                        self.harbour.random_artist = data.random_artist;
                        self.harbour.random_songs = data.random_songs;
                        self.harbour.random_genre = data.random_genre;
                        self.harbour.random_playlist = data.random_playlist;
                        // A fresh shelf load never moves the center, so no
                        // navigation event warms the centered row — without the
                        // explicit center warm the large column stays stuck on
                        // its 80px fallback until the user moves away and back.
                        let shelf_warm = self.warm_harbour_artwork(generation);
                        let center_warm = self.warm_harbour_current_center();
                        Task::batch([shelf_warm, center_warm])
                    }
                    Err(e) => {
                        if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
                            return self.handle_session_expired();
                        }
                        self.toast_error(format!("Failed to load Harbour: {e}"));
                        Task::none()
                    }
                }
            }
            HarbourLoaderMessage::PlaylistQuadIdsLoaded {
                generation,
                results,
            } => {
                if generation != self.harbour.shelves_generation {
                    return Task::none();
                }
                for (playlist_id, album_ids) in results {
                    if let Some(playlist) = self
                        .harbour
                        .random_playlist
                        .as_mut()
                        .filter(|p| p.id == playlist_id)
                    {
                        playlist.artwork_album_ids = album_ids;
                    }
                }
                // The freshly-resolved ids are exactly what a centered
                // collection's 300px collage warm needs — the ShelvesLoaded-time
                // center warm no-op'd while they were still empty.
                let quads = self.warm_harbour_playlist_quads();
                let center_warm = self.warm_harbour_current_center();
                Task::batch([quads, center_warm])
            }
            HarbourLoaderMessage::GenreQuadIdsLoaded {
                generation,
                results,
            } => {
                if generation != self.harbour.shelves_generation {
                    return Task::none();
                }
                for (genre_name, album_ids) in results {
                    // Matched by NAME (the fan-out's key): the name can belong
                    // to the Random Genre pick, a Most Played genre, or both —
                    // set every match. `g.id` would miss the pick, whose id is
                    // a `tag.id` hash.
                    for genre in self
                        .harbour
                        .most_played_genres
                        .iter_mut()
                        .chain(self.harbour.random_genre.iter_mut())
                        .filter(|g| g.name == genre_name)
                    {
                        genre.artwork_album_ids = album_ids.clone();
                    }
                }
                // Genre mirror of the playlist arm: resolved ids unlock a
                // centered collection's collage warm.
                let quads = self.warm_harbour_genre_quads();
                let center_warm = self.warm_harbour_current_center();
                Task::batch([quads, center_warm])
            }
            HarbourLoaderMessage::SearchLoaded { generation, result } => {
                if generation != self.harbour.search_generation {
                    return Task::none();
                }
                self.harbour.search_loading = false;
                match result {
                    Ok(results) => {
                        self.harbour.search_results = Some(*results);
                        let generation = self.harbour.search_generation;
                        Task::batch([
                            self.warm_harbour_search_artwork(),
                            self.fan_out_search_collage_ids(generation),
                        ])
                    }
                    Err(e) => {
                        if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
                            return self.handle_session_expired();
                        }
                        // Drop the PREVIOUS query's results too — leaving them
                        // would keep rendering rows that no longer match the
                        // query the user typed (the view shows the failed-search
                        // hint instead).
                        self.harbour.search_results = None;
                        self.toast_error(format!("Search failed: {e}"));
                        Task::none()
                    }
                }
            }
            HarbourLoaderMessage::SearchCollageIdsLoaded {
                generation,
                target,
                results,
            } => {
                // A genre/playlist's album ids depend only on the entity id, not
                // the query, so store them even for a stale generation — it
                // dedups the fan-out across keystrokes (the next `SearchLoaded`
                // skips any id already in the map). Only warm/re-render for the
                // current query.
                let map = match target {
                    CollageTarget::Genre => &mut self.harbour.search_genre_album_ids,
                    CollageTarget::Playlist => &mut self.harbour.search_playlist_album_ids,
                };
                for (id, album_ids) in results {
                    map.insert(id, album_ids);
                }
                if generation != self.harbour.search_generation {
                    return Task::none();
                }
                // Warm the quad tiles now resolvable for these rows.
                self.warm_harbour_search_artwork()
            }
        }
    }

    /// After the shelves land: warm 80px shelf covers and kick off the
    /// per-genre album-id fan-out that feeds the Most Played Genres quad tiles.
    /// Also re-run on Harbour re-entry (`pub(super)` for the switch-view arm) —
    /// the LRU may have evicted shelf covers while the user browsed other
    /// views, and every fetch here is cache/pending/failed-gated so a warm
    /// cache re-runs free.
    pub(super) fn warm_harbour_artwork(&mut self, generation: u64) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();

        let mut tasks: Vec<Task<Message>> = Vec::new();

        // Recently Added shelf covers (album-id-keyed, retry-wrapped).
        let triples = self.harbour.shelf_album_art_triples();
        if !triples.is_empty() {
            let cached: HashSet<&String> = self.artwork.album_art.iter().map(|(k, _)| k).collect();
            tasks.extend(super::components::expansion_album_artwork_tasks(
                &cached,
                &self.artwork.album_art_versions,
                &self.artwork.failed_art,
                &self.artwork.album_art_pending,
                albums_vm.clone(),
                triples,
            ));
        }

        // The song shelves (Recently Played, Most Played Tracks) plus the
        // Random Songs pick's teaser (its first song): warm each song's 80px
        // album cover by `album_id` (skip songs with no album). Reuses the
        // by-id quad warmer with single-element slices, inserting the queued
        // ids into `album_art_pending` exactly like the genre quad warmer does
        // — without this the song rows and the section preview panel would
        // show no thumbnail.
        let song_album_ids: Vec<Vec<String>> = self
            .harbour
            .recently_played
            .iter()
            .chain(self.harbour.most_played_songs.iter())
            .chain(self.harbour.random_songs.first())
            .filter_map(|s| s.album_id.clone())
            .map(|id| vec![id])
            .collect();
        if !song_album_ids.is_empty() {
            tasks.extend(self.warm_harbour_quad_ids(&albums_vm, song_album_ids));
        }

        // The artist rows (Most Played Artists shelf + the Random Artist pick):
        // warm each artist's `ar-{id}` 80px mini into album_art (the rows' only
        // cover source — the album/song warmers don't cover artist ids).
        let artist_ids = self.harbour.shelf_artist_ids();
        let artist_tasks = self.artist_mini_warm_tasks(artist_ids, &albums_vm);
        tasks.extend(artist_tasks);

        // The Random Playlist pick's album-id fan-out feeding
        // PlaylistQuadIdsLoaded, which then warms the individual quad tiles.
        let playlist_ids: Vec<String> = self
            .harbour
            .random_playlist
            .iter()
            .filter(|p| p.artwork_album_ids.is_empty())
            .map(|p| p.id.clone())
            .collect();
        if !playlist_ids.is_empty() {
            tasks.push(self.shell_task(
                move |shell| async move { resolve_playlist_album_ids(&shell, playlist_ids).await },
                move |results| {
                    Message::HarbourLoader(HarbourLoaderMessage::PlaylistQuadIdsLoaded {
                        generation,
                        results,
                    })
                },
            ));
        }

        // Per-genre album-id fan-out feeding GenreQuadIdsLoaded (the Most
        // Played Genres shelf + the Random Genre pick). Each request filters on
        // the tag id; the reply is keyed by NAME, like every genre key in
        // Harbour, which also dedups a genre that is both the pick and a tally
        // row.
        let genres_needing_ids = genres_needing_quad_ids(&self.harbour);
        if !genres_needing_ids.is_empty() {
            tasks.push(
                self.shell_task(
                    move |shell| async move {
                        resolve_genre_album_ids(&shell, genres_needing_ids).await
                    },
                    move |results| {
                        Message::HarbourLoader(HarbourLoaderMessage::GenreQuadIdsLoaded {
                            generation,
                            results,
                        })
                    },
                ),
            );
        }

        Task::batch(tasks)
    }

    /// Build 80px quad-tile warm tasks for a set of album-id groups AND mark the
    /// queued ids pending — the load-bearing three-step dance (build the cached
    /// set, call [`quad_album_artwork_tasks_for_ids`], then insert the queued ids
    /// into `album_art_pending`) that every Harbour quad-warm site shares. The
    /// pending-insert is structurally separate from the call that produces
    /// `queued_ids` and easy to omit in a copy, so it lives here once. Takes
    /// owned id groups so callers can materialize them from `self.harbour`
    /// (ending that borrow) before this `&mut self` runs.
    pub(crate) fn warm_harbour_quad_ids(
        &mut self,
        albums_vm: &AlbumsService,
        id_groups: Vec<Vec<String>>,
    ) -> Vec<Task<Message>> {
        let cached: HashSet<&String> = self.artwork.album_art.iter().map(|(k, _)| k).collect();
        let (queued_ids, tasks) = super::components::quad_album_artwork_tasks_for_ids(
            &cached,
            &self.artwork.failed_art,
            &self.artwork.album_art_pending,
            albums_vm.clone(),
            id_groups.iter().map(Vec::as_slice),
        );
        drop(cached);
        for id in queued_ids {
            self.artwork.album_art_pending.insert(id);
        }
        tasks
    }

    /// Warm the Random Playlist pick's quad tiles once its album ids resolve.
    /// Mirrors the collage prefetch's `album_art_pending` bookkeeping.
    fn warm_harbour_playlist_quads(&mut self) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let id_groups: Vec<Vec<String>> = self
            .harbour
            .random_playlist
            .iter()
            .map(|p| p.artwork_album_ids.clone())
            .collect();
        Task::batch(self.warm_harbour_quad_ids(&albums_vm, id_groups))
    }

    /// Warm the quad tiles for every Harbour genre (Most Played + the Random
    /// pick) whose album ids are now resolved. Genre mirror of
    /// [`Self::warm_harbour_playlist_quads`].
    fn warm_harbour_genre_quads(&mut self) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let id_groups: Vec<Vec<String>> = self
            .harbour
            .most_played_genres
            .iter()
            .chain(self.harbour.random_genre.iter())
            .map(|g| g.artwork_album_ids.clone())
            .collect();
        Task::batch(self.warm_harbour_quad_ids(&albums_vm, id_groups))
    }

    /// Warm the 80px `ar-{id}` cover for each artist id into `album_art` (keyed
    /// by the artist id — the single-mini path the Artists view uses), dedup-gated
    /// on cache/pending/failed. Shared by the search rows and the Most Played
    /// Artists shelf so both warm identically.
    pub(crate) fn artist_mini_warm_tasks(
        &mut self,
        artist_ids: impl IntoIterator<Item = String>,
        albums_vm: &AlbumsService,
    ) -> Vec<Task<Message>> {
        use nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE;

        let mut tasks = Vec::new();
        for id in artist_ids {
            if self.artwork.album_art.contains(&id)
                || self.artwork.album_art_pending.contains(&id)
                || self.artwork.art_failed_at(&id, &None)
            {
                continue;
            }
            self.artwork.album_art_pending.insert(id.clone());
            let art_id = format!("ar-{id}");
            let vm = albums_vm.clone();
            tasks.push(Task::perform(
                async move {
                    let art = crate::app_message::MiniArt::from_fetch(
                        vm.fetch_album_artwork(&art_id, Some(THUMBNAIL_SIZE), None)
                            .await,
                    );
                    (id, art)
                },
                |(id, art)| Message::Artwork(ArtworkMessage::Loaded(id, None, art)),
            ));
        }
        tasks
    }

    /// Warm artwork for the whole-library search results. The shelves batch-warm
    /// their covers on load (`warm_harbour_artwork`); search results need the
    /// same one-shot warm, otherwise their thumbnails only appear when the cover
    /// happens to already be in the 80px cache from a shelf or another view —
    /// which is why *some* search covers show and some do not. Warms every search
    /// row type's 80px thumbnail, by-id and dedup/failed/pending-gated:
    /// 1. album rows (their id) and song rows (their `album_id`);
    /// 2. artist rows via the `ar-{id}` endpoint, stored in `album_art` keyed by
    ///    the artist id — the same single-mini path the Artists view uses;
    /// 3. genre/playlist rows' resolved quad tiles (from the search collage-id
    ///    side-maps, filled by `fan_out_search_collage_ids`); and
    /// 4. the centered row's large cover, since a fresh search does not fire a
    ///    scroll-driven center change.
    ///
    /// Called on `SearchLoaded` and again after each `SearchCollageIdsLoaded`
    /// resolves more quad ids; the gated warmer dedups the repeats.
    fn warm_harbour_search_artwork(&mut self) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();

        // 80px thumbnail id sets: albums + songs (by album id), plus the resolved
        // quad tiles of genre/playlist rows present in the current results.
        let mut id_slices: Vec<Vec<String>> = Vec::new();
        let mut artist_ids: Vec<String> = Vec::new();
        if let Some(r) = &self.harbour.search_results {
            id_slices.extend(search_warm_album_ids(r).into_iter().map(|id| vec![id]));
            for g in &r.genres {
                if let Some(ids) = self.harbour.search_genre_album_ids.get(&g.name) {
                    id_slices.push(ids.clone());
                }
            }
            for p in &r.playlists {
                if let Some(ids) = self.harbour.search_playlist_album_ids.get(&p.id) {
                    id_slices.push(ids.clone());
                }
            }
            artist_ids = r
                .artists
                .iter()
                .filter(|a| !a.image.image_absent)
                .map(|a| a.id.clone())
                .collect();
        }

        let mut tasks: Vec<Task<Message>> = Vec::new();
        if !id_slices.is_empty() {
            tasks.extend(self.warm_harbour_quad_ids(&albums_vm, id_slices));
        }

        // Artist images: the `ar-{id}` cover endpoint → `album_art[artist_id]`.
        tasks.extend(self.artist_mini_warm_tasks(artist_ids, &albums_vm));

        // Large cover for the centered search row (the thumbnails cover the rest).
        let (rows, center) = self.harbour_centered_rows();
        tasks.push(self.warm_harbour_center_art(center.and_then(|i| rows.get(i))));

        Task::batch(tasks)
    }

    /// Resolve the quad album ids for search-result genres and playlists that
    /// aren't resolved yet (their raw search types carry none). Mirrors the shelf
    /// quad-id fan-out but keyed for search: results feed `SearchCollageIdsLoaded`
    /// (gated on `generation` == `search_generation`) which fills the side-maps
    /// `build_harbour_rows` reads. Skips ids already in a side-map so a re-search
    /// never re-resolves a known entity.
    fn fan_out_search_collage_ids(&mut self, generation: u64) -> Task<Message> {
        let (genre_pairs, playlist_ids) = match &self.harbour.search_results {
            Some(r) => (
                r.genres
                    .iter()
                    .filter(|g| !self.harbour.search_genre_album_ids.contains_key(&g.name))
                    .map(|g| (g.name.clone(), g.id.clone()))
                    .collect::<Vec<_>>(),
                r.playlists
                    .iter()
                    .map(|p| p.id.clone())
                    .filter(|id| !self.harbour.search_playlist_album_ids.contains_key(id))
                    .collect::<Vec<_>>(),
            ),
            None => (Vec::new(), Vec::new()),
        };

        let mut tasks: Vec<Task<Message>> = Vec::new();
        if !genre_pairs.is_empty() {
            tasks.push(self.shell_task(
                move |shell| async move { resolve_genre_album_ids(&shell, genre_pairs).await },
                move |results| {
                    Message::HarbourLoader(HarbourLoaderMessage::SearchCollageIdsLoaded {
                        generation,
                        target: CollageTarget::Genre,
                        results,
                    })
                },
            ));
        }
        if !playlist_ids.is_empty() {
            tasks.push(self.shell_task(
                move |shell| async move { resolve_playlist_album_ids(&shell, playlist_ids).await },
                move |results| {
                    Message::HarbourLoader(HarbourLoaderMessage::SearchCollageIdsLoaded {
                        generation,
                        target: CollageTarget::Playlist,
                        results,
                    })
                },
            ));
        }
        Task::batch(tasks)
    }
}
