//! Update handlers for the Harbour home view.
//!
//! Built up across milestones:
//! - M2: message dispatch skeleton — search-query capture + load lifecycle flag.
//! - M3 (here): the joined shelf fetch + generation-gated population +
//!   artwork warm-up (shelf covers, playlist quad tiles).
//! - M4: card / genre play actions.
//! - M5: the whole-library search fan-out + grouped results.

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
        filter::LibraryFilter,
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
            HOT_PICKS_PER_SECTION, HarbourRow, HarbourSectionId, PlayTarget, SEARCH_MIN_CHARS,
            SEARCH_PREVIEW_LIMIT, build_harbour_rows,
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
                id: name.clone(),
                name,
                album_count: 0,
                song_count: track_count,
            })
        })
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
        .map(|a| a.id.clone())
        .chain(results.songs.iter().filter_map(|s| s.album_id.clone()))
        .collect()
}

/// Fan out each genre's album-id lookup (feeding its 2×2 quad cover)
/// concurrently, mapping `genre_id → its album ids`. One failed lookup degrades
/// to an empty tile set (`unwrap_or_default`) rather than dropping the whole
/// fan-out. `load_genre_albums` keys on the genre NAME, which equals
/// `GenreUIViewData::id` (the `LibraryFilter::GenreId` convention
/// `play_harbour_genre` relies on). Shared by the shelf warm
/// (`warm_harbour_artwork`) and the search warm (`fan_out_search_collage_ids`)
/// so both resolve genres identically.
async fn resolve_genre_album_ids(
    shell: &AppService,
    genre_ids: Vec<String>,
) -> Vec<(String, Vec<String>)> {
    let (server_url, cred) = shell.auth().server_config().await;
    let Some(client) = shell.auth().get_client().await else {
        return Vec::new();
    };
    let futures = genre_ids.into_iter().map(|id| {
        let client = client.clone();
        let server_url = server_url.clone();
        let cred = cred.clone();
        async move {
            let svc =
                nokkvi_data::services::api::genres::GenresApiService::new(client, server_url, cred);
            let ids = svc.load_genre_albums(&id).await.unwrap_or_default();
            (id, ids)
        }
    });
    futures::future::join_all(futures).await
}

/// Playlist mirror of [`resolve_genre_album_ids`]: fan out each playlist's
/// album-id lookup (feeding its 2×2 quad cover), one failed lookup degrading to
/// an empty tile set. Shared by the shelf and search quad-id fan-outs.
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
                if self.toggle_centered_harbour_section(&rows, total) {
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
                if let Some(HarbourRow::Item {
                    play: PlayTarget::Item(batch_item),
                    ..
                }) = center.and_then(|i| rows.get(i))
                {
                    let payload = BatchPayload::new().with_item(batch_item.clone());
                    self.shell_action_task(
                        move |shell| async move { shell.add_batch_to_queue(payload).await },
                        Message::Toast(crate::app_message::ToastMessage::Push(
                            nokkvi_data::types::toast::Toast::new(
                                "Added to queue",
                                nokkvi_data::types::toast::ToastLevel::Success,
                            ),
                        )),
                        "add to queue",
                    )
                } else {
                    Task::none()
                }
            }
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
            _ => (None, None),
        };
        let collage = self.harbour_center_collage_target(center);
        // A centered custom-cover playlist warms its resolution-sized cover so
        // the large column shows it crisp (the warm no-ops for album-art
        // playlists). `None` for section headers / non-playlist items.
        let custom_playlist = match center {
            Some(HarbourRow::Item {
                play: PlayTarget::Item(BatchItem::Playlist(pid)),
                ..
            }) => Some(pid.clone()),
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

    /// Shift+Enter on Harbour: if a section header is centered, toggle its
    /// collapsed state (mirrors the other views' expand-center hotkey). Centered
    /// on an item or hint is a no-op. Rebuilds rows from the same
    /// `(&harbour, &collapsed)` inputs the view renders, so the centered index
    /// resolves to the row the user sees.
    fn handle_harbour_expand_center(&mut self) -> Task<Message> {
        let rows = build_harbour_rows(
            &self.harbour,
            &self.harbour_page.collapsed,
            &self.trawl_crate,
        );
        let total = rows.len();
        if self.toggle_centered_harbour_section(&rows, total) {
            // Rows shifted under the stationary center — re-warm it.
            return self.warm_harbour_current_center();
        }
        Task::none()
    }

    /// If the centered row is a `Section`, toggle its collapsed state and return
    /// `true`. Shared by the ActivateCenter (Enter) and ExpandCenter
    /// (Shift+Enter) paths so both resolve the center against the same rows.
    fn toggle_centered_harbour_section(&mut self, rows: &[HarbourRow], total: usize) -> bool {
        let center = self.harbour_page.common.get_center_item_index(total);
        if let Some(HarbourRow::Section { id, .. }) = center.and_then(|i| rows.get(i)) {
            let id = *id;
            self.harbour_page.toggle_section(id);
            true
        } else {
            false
        }
    }

    /// Play a resolved Harbour item target (guard radio-to-queue, reset context,
    /// then play the single-item batch / genre-random page). `force` is the
    /// Ctrl+Enter shuffle directive, applied to the batch play; genre-random
    /// already draws ~100 server-random songs, so it ignores it.
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

    /// Play ~100 server-random songs of a genre. Uses a songs page fetch (with
    /// the `GenreId` filter + `random` sort, capped at 100) rather than
    /// `BatchItem::Genre`, which would enqueue the entire genre; this path also
    /// respects the active library filter. Assumes the caller already ran the
    /// play guard + new-context reset.
    fn play_harbour_genre(&mut self, genre_name: String) -> Task<Message> {
        self.shell_action_task(
            move |shell| async move {
                let ids = shell.active_library_ids_vec();
                let filter = LibraryFilter::GenreId {
                    id: genre_name.clone(),
                    name: genre_name,
                };
                // Per-call API service, NOT the shared SongsService singleton:
                // the raw-page wrapper writes the browse views' shared
                // `total_count` reactive, which a background genre play must
                // not clobber (same rationale as `search_library`).
                let (songs, _total) = shell
                    .songs_api()
                    .await?
                    .load_songs(
                        "random",
                        "ASC",
                        None,
                        Some(&filter),
                        &ids,
                        Some(0),
                        Some(100),
                    )
                    .await?;
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
    pub(crate) fn handle_load_harbour(&mut self) -> Task<Message> {
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

                // Playlists + genres load with a `random` sort — the loaders
                // shuffle client-side (resolve_random_sort_mode), so the order is
                // fixed once here at load time, not re-rolled every frame.
                let playlists_fut = async {
                    let svc = shell.playlists_api().await?;
                    svc.load_playlists_with_libraries("random", "ASC", None, &ids)
                        .await
                        .map(|(playlists, _total)| playlists)
                };
                let genres_fut = async {
                    let svc = shell.genres_api().await?;
                    svc.load_genres_with_libraries("random", "ASC", None, &ids)
                        .await
                        .map(|(genres, _total)| genres)
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

                let (
                    recently_played,
                    recently_added,
                    playlists,
                    genres,
                    most_played_songs,
                    most_played_albums,
                    most_played_artists,
                ) = futures::join!(
                    recently_played_fut,
                    recently_added_fut,
                    playlists_fut,
                    genres_fut,
                    most_played_songs_fut,
                    most_played_albums_fut,
                    most_played_artists_fut
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

                let mut playlists = recover_shelf("playlists", playlists);
                playlists.truncate(HOT_PICKS_PER_SECTION);
                let mut genres = recover_shelf("genres", genres);
                genres.truncate(HOT_PICKS_PER_SECTION);

                // Tally the full tracks pool by genre BEFORE truncating it to the
                // Most Played Tracks shelf's top picks.
                let mut most_played_songs = recover_shelf("most-played-tracks", most_played_songs);
                let most_played_genres = tally_genres_by_play(&most_played_songs);
                most_played_songs.truncate(HOT_PICKS_PER_SECTION);

                let most_played_albums =
                    project(recover_shelf("most-played-albums", most_played_albums));
                let mut most_played_artists =
                    recover_shelf("most-played-artists", most_played_artists);
                most_played_artists.truncate(HOT_PICKS_PER_SECTION);

                Ok(Box::new(HarbourShelvesData {
                    recently_played: recover_shelf("recently-played", recently_played),
                    recently_added,
                    most_played_songs,
                    most_played_albums,
                    most_played_artists,
                    most_played_genres,
                    playlists: playlists
                        .into_iter()
                        .map(nokkvi_data::backend::playlists::PlaylistUIViewData::from)
                        .collect(),
                    genres: genres
                        .into_iter()
                        .map(nokkvi_data::backend::genres::GenreUIViewData::from)
                        .collect(),
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
                        self.harbour.playlists = data.playlists;
                        self.harbour.genres = data.genres;
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
                        .playlists
                        .iter_mut()
                        .find(|p| p.id == playlist_id)
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
                for (genre_id, album_ids) in results {
                    // A genre can appear on both the Random and Most Played Genres
                    // shelves — set its ids on whichever holds it.
                    for genre in self
                        .harbour
                        .genres
                        .iter_mut()
                        .chain(self.harbour.most_played_genres.iter_mut())
                        .filter(|g| g.id == genre_id)
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
    /// per-playlist album-id fan-out that feeds the quad tiles. Also re-run on
    /// Harbour re-entry (`pub(super)` for the switch-view arm) — the LRU may
    /// have evicted shelf covers while the user browsed other views, and every
    /// fetch here is cache/pending/failed-gated so a warm cache re-runs free.
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

        // Recently Played is songs now: warm each song's 80px album cover by
        // `album_id` (skip songs with no album). Reuses the by-id quad warmer
        // with single-element slices, inserting the queued ids into
        // `album_art_pending` exactly like the playlist/genre quad warmers do —
        // without this the song rows and the section preview panel would show no
        // thumbnail.
        let song_album_ids: Vec<Vec<String>> = self
            .harbour
            .recently_played
            .iter()
            .chain(self.harbour.most_played_songs.iter())
            .filter_map(|s| s.album_id.clone())
            .map(|id| vec![id])
            .collect();
        if !song_album_ids.is_empty() {
            tasks.extend(self.warm_harbour_quad_ids(&albums_vm, song_album_ids));
        }

        // Most Played Artists shelf: warm each artist's `ar-{id}` 80px mini into
        // album_art (the shelf's only cover source — the album/song warmers don't
        // cover artist ids).
        let artist_ids: Vec<String> = self
            .harbour
            .most_played_artists
            .iter()
            .map(|a| a.id.clone())
            .collect();
        let artist_tasks = self.artist_mini_warm_tasks(artist_ids, &albums_vm);
        tasks.extend(artist_tasks);

        // Per-playlist album-id fan-out feeding PlaylistQuadIdsLoaded, which then
        // warms the individual quad tiles.
        let playlist_ids: Vec<String> = self
            .harbour
            .playlists
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

        // Per-genre album-id fan-out feeding GenreQuadIdsLoaded, the genre
        // mirror of the playlist path.
        let mut seen_genre_ids = HashSet::new();
        let genres_needing_ids: Vec<String> = self
            .harbour
            .genres
            .iter()
            .chain(self.harbour.most_played_genres.iter())
            .filter(|g| g.artwork_album_ids.is_empty())
            .map(|g| g.id.clone())
            .filter(|id| seen_genre_ids.insert(id.clone()))
            .collect();
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

    /// Warm the quad tiles for every Harbour playlist whose album ids are now
    /// resolved. Mirrors the collage prefetch's `album_art_pending` bookkeeping.
    fn warm_harbour_playlist_quads(&mut self) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let id_groups: Vec<Vec<String>> = self
            .harbour
            .playlists
            .iter()
            .map(|p| p.artwork_album_ids.clone())
            .collect();
        Task::batch(self.warm_harbour_quad_ids(&albums_vm, id_groups))
    }

    /// Warm the quad tiles for every Harbour genre whose album ids are now
    /// resolved. Genre mirror of [`Self::warm_harbour_playlist_quads`].
    fn warm_harbour_genre_quads(&mut self) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let id_groups: Vec<Vec<String>> = self
            .harbour
            .genres
            .iter()
            .chain(self.harbour.most_played_genres.iter())
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
            artist_ids = r.artists.iter().map(|a| a.id.clone()).collect();
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
        let (genre_ids, playlist_ids) = match &self.harbour.search_results {
            Some(r) => (
                r.genres
                    .iter()
                    .map(|g| g.name.clone())
                    .filter(|name| !self.harbour.search_genre_album_ids.contains_key(name))
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
        if !genre_ids.is_empty() {
            tasks.push(self.shell_task(
                move |shell| async move { resolve_genre_album_ids(&shell, genre_ids).await },
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
