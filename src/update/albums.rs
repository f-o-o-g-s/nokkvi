//! Album data loading and component message handlers

use iced::{Task, widget::image};
use nokkvi_data::{backend::albums::AlbumUIViewData, types::ItemKind};
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, FindMessage, Message, MiniArt, NavigationMessage},
    update::AlbumsTarget,
    views::{self, AlbumsAction, AlbumsMessage, HasCommonAction},
};

impl Nokkvi {
    /// Per-entity fetch closure for `Nokkvi::load_paged::<AlbumsTarget>`.
    ///
    /// The shared invariant body (page_size, defensive gate, `PaginatedFetch`,
    /// debug log, `set_loading(true)`) lives in `loader_target.rs`; this
    /// closure owns only the per-entity backend call and the UI projection.
    async fn fetch_albums_page(
        shell: nokkvi_data::backend::app_service::AppService,
        params: super::components::PaginatedFetch,
    ) -> (Result<Vec<AlbumUIViewData>, String>, usize) {
        let albums_vm = shell.albums().clone();
        let library_ids = shell.active_library_ids_vec();
        match albums_vm
            .load_raw_albums_page_with_libraries(
                &params.query(&library_ids),
                params.offset,
                params.page_size,
            )
            .await
        {
            Ok(albums) => {
                let (url, cred) = albums_vm.get_server_config().await;
                let ui_albums: Vec<AlbumUIViewData> = albums
                    .iter()
                    .map(|album| AlbumUIViewData::from_album(album, &url, &cred))
                    .collect();
                (Ok(ui_albums), albums_vm.get_total_count() as usize)
            }
            Err(e) => (Err(format!("{e:#}")), 0),
        }
    }

    pub(crate) fn handle_load_albums(
        &mut self,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.load_paged::<AlbumsTarget, _, _, _>(
            0,
            false,
            move |(result, total_count)| {
                Message::AlbumsLoader(crate::app_message::AlbumsLoaderMessage::Loaded {
                    result,
                    total_count,
                    background,
                    anchor_id,
                })
            },
            Self::fetch_albums_page,
        )
    }

    /// Load a subsequent page of albums (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_albums_load_page(&mut self, offset: usize) -> Task<Message> {
        self.load_paged::<AlbumsTarget, _, _, _>(
            offset,
            false,
            |(result, total_count)| {
                Message::AlbumsLoader(crate::app_message::AlbumsLoaderMessage::PageLoaded(
                    result,
                    total_count,
                ))
            },
            Self::fetch_albums_page,
        )
    }

    /// Force-load an albums page regardless of the scroll-edge `needs_fetch`
    /// gate. Used by the find-and-expand chain to page through the full
    /// library while the viewport stays at 0 (the user hasn't scrolled
    /// because they're waiting for the target to appear).
    pub(crate) fn force_load_albums_page(&mut self, offset: usize) -> Task<Message> {
        self.load_paged::<AlbumsTarget, _, _, _>(
            offset,
            true,
            |(result, total_count)| {
                Message::AlbumsLoader(crate::app_message::AlbumsLoaderMessage::PageLoaded(
                    result,
                    total_count,
                ))
            },
            Self::fetch_albums_page,
        )
    }

    pub(crate) fn handle_albums_page_loaded(
        &mut self,
        result: Result<Vec<AlbumUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.handle_page_loaded_with::<AlbumsTarget>(result, total_count)
    }

    pub(crate) fn handle_albums_loaded(
        &mut self,
        result: Result<Vec<AlbumUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.handle_loaded_with::<AlbumsTarget>(result, total_count, background, anchor_id)
    }

    pub(crate) fn handle_artwork_loaded(
        &mut self,
        id: String,
        updated_at: Option<String>,
        art: MiniArt,
    ) -> Task<Message> {
        // Navidrome 0.64+: the cover changed under the cached large art. Drop
        // it and reload the centered row's.
        if self.record_mini_art(&id, &updated_at, art)
            && self.large_art_is_stale(&id, updated_at.as_deref())
        {
            self.artwork.large_artwork.pop(&id);
            self.artwork.large_artwork_hashes.remove(&id);
            return self
                .center_large_artwork_load_task(self.current_view)
                .unwrap_or_else(Task::none);
        }
        Task::none()
    }

    /// Record an arrived 80px thumbnail for `id` (album or `ar-` artist id),
    /// fetched at `version`, into the shared `album_art` cache and its gate
    /// bookkeeping. The one body behind `handle_artwork_loaded` and
    /// `handle_song_artwork_loaded`. Returns whether an image landed.
    pub(crate) fn record_mini_art(
        &mut self,
        id: &str,
        version: &Option<String>,
        art: MiniArt,
    ) -> bool {
        // Release the in-flight claim on every outcome, so a throttled or
        // failed fetch is re-attempted by the next prefetch pass instead of
        // staying blocked.
        self.artwork.album_art_pending.remove(id);
        match art {
            MiniArt::Loaded(h) => {
                // Record the version the URL carried, in lockstep with the
                // handle, so a later server cover change is a version-aware miss
                // (N17). album_art evicts at capacity; `should_refetch` guards the
                // skew by also checking album_art membership.
                self.artwork.failed_art.remove(id);
                self.artwork
                    .album_art_versions
                    .insert(id.to_owned(), version.clone());
                self.artwork.album_art.put(id.to_owned(), h);
                true
            }
            MiniArt::Missing => {
                // Deterministic: the server has no art for this id. Negatively
                // cache it (keyed by the version that failed; the passive
                // surfaces feed None) so the membership-based prefetch gates stop
                // re-queuing it on every scroll/resize. A changed version, a
                // later success, a user Refresh Artwork, or logout all re-enable
                // it. Logged once (then suppressed).
                debug!(" Mini artwork not found on server for: {}", id);
                self.artwork
                    .failed_art
                    .insert(id.to_owned(), version.clone());
                false
            }
            MiniArt::Transient => {
                // Throttle/timeout/empty — record NOTHING so the next prefetch
                // revisit re-attempts (the documented throttle-recovery path).
                // Negative-caching here would blank a cover that actually exists.
                // Silent on purpose: expected under throttle and self-healing;
                // the retry wrapper already logs a give-up for the retry surfaces.
                false
            }
        }
    }

    /// Claim the 80px fetches a planner just queued: `album_art_pending`
    /// holds them until the loaded handlers release them on every outcome.
    /// Each one's ThumbHash (Navidrome 0.64+) is decoded into
    /// `placeholders` on the way, so the row, and the big panel over it, draw
    /// a blurred stand-in until the cover lands. Every 80px path claims
    /// through here (`every_mini_claim_goes_through_claim_minis`).
    pub(crate) fn claim_minis(&mut self, ids: impl IntoIterator<Item = String>) {
        let ids: Vec<String> = ids.into_iter().collect();
        if ids.is_empty() {
            return;
        }
        // One walk over the rows for the whole batch: a refill claims tens of
        // ids, and a library paged deep holds tens of thousands of rows.
        let fresh: Vec<(String, String)> = self
            .thumb_hashes_of(&ids)
            .into_iter()
            .filter(|(id, hash)| {
                !self
                    .artwork
                    .placeholders
                    .peek(id)
                    .is_some_and(|cached| &cached.thumb_hash == hash)
            })
            .collect();
        // A decode is a 32 px picture, tens of microseconds, and only for a
        // cover that is about to be fetched.
        for (id, thumb_hash) in fresh {
            if let Some(picture) = nokkvi_data::utils::thumbhash::decode_thumb_hash(&thumb_hash) {
                let handle = image::Handle::from_rgba(picture.width, picture.height, picture.rgba);
                self.artwork
                    .placeholders
                    .put(id, crate::state::Placeholder { thumb_hash, handle });
            }
        }
        self.artwork.album_art_pending.extend(ids);
    }

    /// The ThumbHash the rows nokkvi holds give each of `ids`: albums and
    /// artists, the albums an expanded artist or genre lists included, in
    /// `album_image` / `artist_image`'s order (the first row with an id
    /// wins). Rows the server marked absent give none, so absent art keeps
    /// its plain square. One pass, stopping once every id is found.
    fn thumb_hashes_of(&self, ids: &[String]) -> Vec<(String, String)> {
        use nokkvi_data::types::image_info::ImageInfo;

        fn take<'a>(
            rows: impl Iterator<Item = (&'a str, &'a ImageInfo)>,
            wanted: &mut std::collections::HashSet<&str>,
            found: &mut Vec<(String, String)>,
        ) {
            for (id, image) in rows {
                if wanted.is_empty() {
                    return;
                }
                if !wanted.contains(id) {
                    continue;
                }
                // The first row with the id decides, as in `album_image`.
                wanted.remove(id);
                if let Some(hash) = image.thumb_hash.as_deref()
                    && !image.image_absent
                {
                    found.push((id.to_owned(), hash.to_owned()));
                }
            }
        }

        let mut wanted: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut found = Vec::new();
        let harbour = &self.harbour;
        let search = harbour.search_results.iter();
        take(
            self.library
                .albums
                .iter()
                .chain(harbour.recently_added.iter())
                .chain(harbour.most_played_albums.iter())
                .chain(harbour.random_album.iter())
                .map(|a| (a.id.as_str(), &a.image)),
            &mut wanted,
            &mut found,
        );
        take(
            search
                .clone()
                .flat_map(|r| r.albums.iter())
                .map(|a| (a.id.as_str(), &a.image)),
            &mut wanted,
            &mut found,
        );
        take(
            self.library
                .artists
                .iter()
                .map(|a| (a.id.as_str(), &a.image)),
            &mut wanted,
            &mut found,
        );
        take(
            harbour
                .most_played_artists
                .iter()
                .chain(harbour.random_artist.iter())
                .chain(search.flat_map(|r| r.artists.iter()))
                .map(|a| (a.id.as_str(), &a.image)),
            &mut wanted,
            &mut found,
        );
        take(
            self.artists_page
                .expansion
                .children
                .iter()
                .chain(self.genres_page.expansion.children.iter())
                .map(|a| (a.id.as_str(), &a.image)),
            &mut wanted,
            &mut found,
        );
        found
    }

    /// The image info (Navidrome 0.64+) of an album row nokkvi holds: the
    /// Albums view or Harbour's shelves. `None` for an unknown id; the
    /// default on older servers.
    fn album_image(&self, album_id: &str) -> Option<&nokkvi_data::types::image_info::ImageInfo> {
        self.library
            .albums
            .iter()
            .chain(self.harbour.recently_added.iter())
            .chain(self.harbour.most_played_albums.iter())
            .chain(self.harbour.random_album.iter())
            .find(|a| a.id == album_id)
            .map(|a| &a.image)
            .or_else(|| {
                // Harbour's search rows are raw `Album`s.
                self.harbour
                    .search_results
                    .iter()
                    .flat_map(|r| r.albums.iter())
                    .find(|a| a.id == album_id)
                    .map(|a| &a.image)
            })
    }

    /// The valid image hash the rows nokkvi holds currently give `id`, an
    /// album or an artist (Navidrome 0.64+). `None` on older servers.
    fn current_art_hash(&self, id: &str) -> Option<&str> {
        self.album_image(id)
            .or_else(|| self.artist_image(id))
            .and_then(|image| image.valid_hash())
    }

    /// Whether a mini arriving at `arriving` shows the cached large art for
    /// `id` is stale: the large art was requested at a different hash, and
    /// the arriving hash is the one the rows give now. Comparing with the
    /// large art's own recorded hash (not the mini's previous version) keeps
    /// the passive surfaces' `None` versions from hiding a change, and the
    /// current-row check keeps a surface refetching an OLD hash (Harbour's
    /// previous-visit shelves) from dropping the correct large art. Never
    /// true on an older server, which sends no hashes.
    fn large_art_is_stale(&self, id: &str, arriving: Option<&str>) -> bool {
        let Some(arriving) = arriving else {
            return false;
        };
        self.artwork.large_artwork.contains(&id.to_owned())
            && self
                .artwork
                .large_artwork_hashes
                .get(id)
                .is_some_and(|large| large != arriving)
            && self.current_art_hash(id) == Some(arriving)
    }

    /// Record the hash a large-art request for `id` carries, or clear a
    /// stale record when it carries none.
    pub(crate) fn record_large_art_hash(&mut self, id: &str, version: Option<&str>) {
        match version.filter(|v| nokkvi_data::types::image_info::is_valid_image_hash(v)) {
            Some(hash) => {
                self.artwork
                    .large_artwork_hashes
                    .insert(id.to_owned(), hash.to_owned());
            }
            None => {
                self.artwork.large_artwork_hashes.remove(id);
            }
        }
    }

    /// Whether the server marked this album's art absent (Navidrome 0.64+).
    /// Unknown ids and older servers read as not absent.
    pub(crate) fn album_image_absent(&self, album_id: &str) -> bool {
        self.album_image(album_id)
            .is_some_and(|image| image.image_absent)
    }

    /// The version for a large-art or Refresh fetch of `album_id`: its image
    /// hash when a row carries one (Navidrome 0.64+), else the Albums view
    /// row's `updated_at`, which is exactly what these fetches used before.
    pub(crate) fn album_art_version(&self, album_id: &str) -> Option<String> {
        let updated_at = self
            .library
            .albums
            .iter()
            .find(|a| a.id == album_id)
            .and_then(|a| a.updated_at.as_deref());
        match self.album_image(album_id) {
            Some(image) => nokkvi_data::types::image_info::artwork_version(image, updated_at),
            None => updated_at.map(str::to_owned),
        }
    }

    pub(crate) fn handle_load_large_artwork(&mut self, album_id: String) -> Task<Message> {
        // Skip fetching if already cached - makes back-navigation instant
        if self.artwork.large_artwork.peek(&album_id).is_some() {
            let handle = self.artwork.large_artwork.peek(&album_id).cloned();
            return Task::done(Message::Artwork(ArtworkMessage::LargeLoaded(
                album_id, handle,
            )));
        }
        // Absent art: no request, and the panel keeps its blank placeholder.
        if self.album_image_absent(&album_id) {
            return Task::none();
        }

        self.artwork.loading_large_artwork = Some(album_id.clone());
        let version = self.album_art_version(&album_id);
        // Already on its way at this version: the completion lands in the
        // cache, where the panel picks it up. A held arrow key re-centers the
        // same row on every repeat, and without this a slow server stacked a
        // full-size request per repeat.
        if self
            .artwork
            .large_artwork_pending
            .holds(&album_id, &version)
        {
            return Task::none();
        }
        self.record_large_art_hash(&album_id, version.as_deref());

        if let Some(shell) = &self.app_service {
            let albums_vm = shell.albums().clone();
            let artwork_size = self.settings.artwork_resolution.to_size();
            // The bare album_id, which `fetch_album_artwork` normalizes with
            // the `al-` prefix, plus its version (hash on 0.64+, else the
            // Albums view row's `updated_at`, else none).
            let art_id = album_id.clone();
            self.artwork
                .large_artwork_pending
                .claim(album_id, version.clone());

            return Task::perform(
                async move {
                    let bytes = albums_vm
                        .fetch_album_artwork(&art_id, artwork_size, version.as_deref())
                        .await
                        .ok();
                    (art_id, bytes.map(image::Handle::from_bytes))
                },
                |(id, handle)| Message::Artwork(ArtworkMessage::LargeLoaded(id, handle)),
            );
        }
        Task::none()
    }

    /// A navigation step in `source`'s list (`None`: the Similar tab, as in
    /// [`Self::current_target_view`]) centred `target`. A step in the list
    /// the user is steering goes through the big panel's gate
    /// ([`crate::state::PanelArtGate`]): it loads now when no full-size cover
    /// is on its way or its own is cached, else it waits as the newest row
    /// until that cover lands. A step in any other list (a shelf or a buffer
    /// landing behind another view) loads directly and leaves the gate
    /// alone, so it can neither replace nor clear the waiting row. A load
    /// with another trigger (a view switch, an expand, a page landing, the
    /// playing song) calls its loader directly.
    pub(crate) fn panel_art_step(
        &mut self,
        source: Option<View>,
        target: crate::state::PanelArt,
    ) -> Task<Message> {
        use crate::state::{PANEL_ART_OVERDUE, PanelArtStep};

        if source != self.current_target_view() {
            return self.load_panel_art(target);
        }
        let cached = self.artwork.large_artwork.contains(&target.id().to_owned());
        match self.artwork.panel_gate.step(source, target, cached) {
            PanelArtStep::Now(target) => self.send_panel_art(target),
            // The timer is made when the task first runs, inside the
            // runtime, not here.
            PanelArtStep::Wait(target) => Task::perform(
                async { tokio::time::sleep(PANEL_ART_OVERDUE).await },
                move |()| Message::Artwork(ArtworkMessage::PanelArtOverdue { source, target }),
            ),
        }
    }

    /// Load `target` through the gate: when its loader sends a fetch (a claim
    /// in `large_artwork_pending`, also when one was already out), that fetch
    /// is the cover on its way. A cached, absent or sessionless load sends
    /// none, so nothing waits behind it.
    fn send_panel_art(&mut self, target: crate::state::PanelArt) -> Task<Message> {
        let task = self.load_panel_art(target.clone());
        if self.artwork.large_artwork_pending.contains(target.id()) {
            self.artwork.panel_gate.sent(target);
        }
        task
    }

    /// The cover a waiting row trusted to land is overdue: fetch the row past
    /// it, when it is still the newest waiting and its list is still steered.
    pub(crate) fn handle_panel_art_overdue(
        &mut self,
        source: Option<View>,
        target: crate::state::PanelArt,
    ) -> Task<Message> {
        if self.artwork.panel_gate.overdue(&target) && source == self.current_target_view() {
            self.send_panel_art(target)
        } else {
            Task::none()
        }
    }

    fn load_panel_art(&mut self, target: crate::state::PanelArt) -> Task<Message> {
        match target {
            crate::state::PanelArt::Album(id) => self.handle_load_large_artwork(id),
            crate::state::PanelArt::Artist(id) => self.handle_load_artist_large_artwork(id),
        }
    }

    /// Force-refresh a specific album's artwork (user-initiated, with toasts).
    /// A changed cover also refreshes on its own on Navidrome 0.64+: the
    /// image hash is the prefetch version, and `handle_artwork_loaded` drops
    /// the stale large art when the hash changes.
    pub(crate) fn handle_refresh_album_artwork(&mut self, album_id: String) -> Task<Message> {
        use tracing::info;

        info!(" [REFRESH] Refreshing artwork for album {}", album_id);

        // The server marked this album's art absent: a refetch would only
        // cache its placeholder picture.
        if self.album_image_absent(&album_id) {
            self.toast_warn("No artwork found on server for this album");
            return Task::none();
        }

        // Only evict large artwork so the panel shows a placeholder during refresh.
        // Do NOT evict from album_art — that would gray out every slot list row
        // sharing this album_id. The old mini thumbnail stays visible until
        // RefreshComplete replaces it atomically.
        self.artwork.large_artwork.pop(&album_id);

        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let id = album_id.clone();
        let artwork_size = self.settings.artwork_resolution.to_size();
        let updated_at = self.album_art_version(&album_id);

        let refresh_task = Task::perform(
            async move {
                use nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE;

                // No client cache to evict — go straight to the server. The
                // server's `Cache-Control: max-age=315360000` is irrelevant on
                // our side now; Navidrome's own ImageCacheSize keeps the
                // response fast.
                let thumb_bytes = albums_vm
                    .fetch_album_artwork(&id, Some(THUMBNAIL_SIZE), updated_at.as_deref())
                    .await
                    .ok();
                let large_bytes = albums_vm
                    .fetch_album_artwork(&id, artwork_size, updated_at.as_deref())
                    .await
                    .ok();

                // `Handle::from_bytes` derives a unique Id per call, busting Iced's
                // GPU texture cache so the new bytes upload (refresh path requires this).
                let thumb_handle = thumb_bytes.map(image::Handle::from_bytes);
                let large_handle = large_bytes.map(image::Handle::from_bytes);

                (id, thumb_handle, large_handle)
            },
            move |(id, thumb, large)| {
                Message::Artwork(ArtworkMessage::RefreshComplete(id, thumb, large))
            },
        );

        let toast_task = Task::done(Message::Toast(crate::app_message::ToastMessage::Push(
            nokkvi_data::types::toast::Toast::new(
                "Refreshing artwork…".to_string(),
                nokkvi_data::types::toast::ToastLevel::Info,
            ),
        )));
        Task::batch([toast_task, refresh_task])
    }

    /// Handle the result of an artwork refresh — cache both mini and large atomically.
    pub(crate) fn handle_refresh_complete(
        &mut self,
        album_id: String,
        thumb: Option<image::Handle>,
        large: Option<image::Handle>,
    ) -> Task<Message> {
        // A refresh is an explicit re-attempt signal: drop any negative-cache
        // entry so the next prefetch re-fetches even if the recorded failed
        // version still matches (the user may have fixed the cover server-side).
        self.artwork.failed_art.remove(&album_id);
        if thumb.is_none() && large.is_none() {
            self.toast_warn("No artwork found on server for this album");
            return Task::none();
        }
        if let Some(h) = thumb {
            // Re-sync the recorded version to the album's current version (the
            // same one the Albums view prefetch records) so a prefetch tick
            // right after a manual refresh doesn't see a phantom mismatch and
            // re-fetch (N17). Falls back to None when no row knows the album.
            let version = self.album_art_version(&album_id);
            self.artwork
                .album_art_versions
                .insert(album_id.clone(), version);
            self.artwork.album_art.put(album_id.clone(), h);
        }
        if let Some(h) = large {
            let version = self.album_art_version(&album_id);
            self.record_large_art_hash(&album_id, version.as_deref());
            self.artwork.large_artwork.put(album_id, h);
        }
        self.toast_success("Artwork refreshed");
        Task::none()
    }

    pub(crate) fn handle_large_artwork_loaded(
        &mut self,
        id: String,
        handle: Option<image::Handle>,
    ) -> Task<Message> {
        // Release on every outcome so a failed fetch can be retried.
        self.artwork.large_artwork_pending.release(&id);
        // Always cache artwork that arrives (even if user navigated away)
        // This fixes the bug where rapid navigation discarded completed loads
        if let Some(h) = handle {
            self.artwork.large_artwork.put(id.clone(), h);
        }
        // Clear loading_large_artwork if this was the most recent request
        if self.artwork.loading_large_artwork.as_ref() == Some(&id) {
            self.artwork.loading_large_artwork = None;
        }
        // The big panel's cover on its way landed (on any outcome): the
        // newest row that waited for it goes next, through the gate again
        // (a cached cover shows at once), while its list is still the one
        // being steered; after a view switch or a pane change it is dropped.
        match self.artwork.panel_gate.landed(&id) {
            Some((source, next)) if source == self.current_target_view() => {
                self.panel_art_step(source, next)
            }
            Some(_) | None => Task::none(),
        }
    }

    pub(crate) fn handle_albums(&mut self, msg: views::AlbumsMessage) -> Task<Message> {
        if let Some(task) = crate::update::dispatch_view_chrome(self, &msg, crate::View::Albums) {
            return task;
        }
        // The page's `update` calls `set_children` for `TracksLoaded`, which
        // wipes `selected_offset` to clamp the viewport into the post-
        // expansion flat list. For find-chain navigations we want the
        // highlight to stay on the target, so pre-extract the album_id and
        // re-pin afterwards. Cloned because `msg` moves into the page update.
        let pin_after_tracks = if let AlbumsMessage::TracksLoaded(ref id, _) = msg {
            Some(id.clone())
        } else {
            None
        };
        let (cmd, action) =
            self.albums_page
                .update(msg, self.library.albums.len(), &self.library.albums);

        if let Some(loaded_id) = pin_after_tracks {
            self.pin_after_load(
                &loaded_id,
                |pin, id| matches!(pin, crate::state::PendingTopPin::Album(p) if p == id),
                |app, id| app.library.albums.iter().position(|a| a.id == id),
                |app, idx| {
                    let total = app.albums_page.expansion.flattened_len(&app.library.albums);
                    app.albums_page.common.slot_list.pin_selected(idx, total);
                },
            );
        }

        // User-driven changes (search edit, sort, refresh) supersede any
        // in-flight find-and-expand chain. FocusAndExpand dispatched by the
        // chain itself produces ExpandAlbum, which is not in this list.
        if matches!(
            action,
            AlbumsAction::SearchChanged(_)
                | AlbumsAction::SortModeChanged(_)
                | AlbumsAction::SortOrderChanged(_)
                | AlbumsAction::RefreshViewData
        ) {
            self.cancel_pending_expand();
        }

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadAlbums,
            "persist_albums_prefs",
            self.albums_page.common.current_sort_mode,
            self.albums_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_albums_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            AlbumsAction::PlayAlbum(album_id_str, force) => {
                // Browsing panel: redirect play → add to queue (insert at
                // drag-drop position when one is pending, else append). An
                // add, so it runs before the play prologue.
                let id_ref = album_id_str.as_str();
                if let Some(task) = self.redirect_play_to_queue_in_browsing_panel(
                    |app| {
                        app.add_entity_to_queue_task(
                            &app.library.albums,
                            id_ref,
                            "album",
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id| async move { shell.add_album_to_queue(&id).await },
                        )
                    },
                    |app, pos| {
                        app.insert_entity_to_queue_at_position_task(
                            &app.library.albums,
                            id_ref,
                            "album",
                            pos,
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id, position| async move {
                                shell.insert_album_at_position(&id, position).await
                            },
                        )
                    },
                ) {
                    return task;
                }
                // Each branch runs its own play prologue once it has a row to
                // play: Append and Play only guards (it keeps the queue and
                // anything still filling it); `play_entity_task` guards and
                // enters a new playback context.
                // AppendAndPlay: append album songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.settings.enter_behavior == EnterBehavior::AppendAndPlay
                    && let Ok(index) = album_id_str.parse::<usize>()
                    && let Some(album) = self.library.albums.get(index)
                {
                    let id = album.id.clone();
                    let name = album.name.clone();
                    let shuffle = self.activate_shuffle_directive(force, false);
                    self.guard_play_action();
                    self.clear_active_playlist();
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_album_and_play(&id, shuffle).await },
                        format!("Playing '{name}'"),
                        "append album and play",
                    );
                }
                // PlayAll / PlaySingle: replace queue with album (PlaySingle = PlayAll for albums)
                let shuffle = self.activate_shuffle_directive(force, false);
                return self.play_entity_task(
                    |app| &app.library.albums,
                    &album_id_str,
                    "album",
                    |a| a.id.clone(),
                    move |shell, id| async move { shell.play_album(&id, shuffle).await },
                );
            }
            AlbumsAction::AddBatchToQueue(payload) => {
                return self.add_or_insert_batch_to_queue_task(payload);
            }
            AlbumsAction::PlayBatch(payload, force) => {
                self.albums_page
                    .common
                    .slot_list
                    .clear_selection_indices_only();
                return self.play_batch_task(payload, force);
            }
            AlbumsAction::PlaySelection(payload, force) => {
                self.albums_page
                    .common
                    .slot_list
                    .clear_selection_indices_only();
                return self.play_selection_task(payload, force);
            }
            AlbumsAction::LoadLargeArtwork(album_id_str) => {
                if let Ok(index) = album_id_str.parse::<usize>() {
                    // Resolve the actual album ID using the expansion state and
                    // the passed index. Drop the borrow before calling &mut self
                    // methods below. Albums uses expansion-aware lookup; the
                    // other paged views (Artists, Songs) use direct lookups.
                    let resolved_album_id = self
                        .albums_page
                        .expansion
                        .get_entry_at(index, &self.library.albums, |a| &a.id)
                        .map(|entry| match entry {
                            crate::views::expansion::SlotListEntry::Parent(album) => {
                                album.id.clone()
                            }
                            crate::views::expansion::SlotListEntry::Child(_song, parent_id) => {
                                parent_id
                            }
                        });

                    let mut tasks = Vec::new();
                    // Direct call (rather than dispatching Message::Artwork(LoadLarge))
                    // so the load starts in the same tick as the action; a step
                    // behind a cover still on its way waits for it (`panel_art_step`).
                    if let Some(album_id) = resolved_album_id {
                        tasks.push(self.panel_art_step(
                            Some(View::Albums),
                            crate::state::PanelArt::Album(album_id),
                        ));
                    }

                    // Prefetch viewport mini artwork + chain a page-load if
                    // scrolling near the loaded edge.
                    tasks.extend(self.prefetch_and_maybe_load_next_page::<AlbumsTarget>(
                        Self::handle_albums_load_page,
                    ));

                    if !tasks.is_empty() {
                        return Task::batch(tasks);
                    }
                }
            }
            AlbumsAction::LoadPage(offset) => {
                return self.handle_albums_load_page(offset);
            }
            AlbumsAction::ExpandAlbum(album_id) => {
                // Load tracks for the album and send them back to the view
                let id = album_id.clone();

                // CRITICAL FIX: Ensure large artwork is fetched when expanding an album via mouse click.
                // Standard navigation triggers this automatically, but FocusAndExpand skips standard focus.
                let artwork_task = self.handle_load_large_artwork(id.clone());

                let tracks_task = self.expand_load_children_task(
                    move |shell| async move {
                        let albums_vm = shell.albums().clone();
                        albums_vm.load_album_songs(&id).await
                    },
                    move |songs: Vec<nokkvi_data::types::song::Song>| {
                        let tracks: Vec<nokkvi_data::backend::songs::SongUIViewData> =
                            songs.into_iter().map(Into::into).collect();
                        Message::Albums(AlbumsMessage::TracksLoaded(album_id, tracks))
                    },
                    "load album tracks",
                );

                return Task::batch([artwork_task, tracks_task]);
            }
            AlbumsAction::PlayAlbumFromTrack(album_id, track_idx, force) => {
                // Browsing panel: redirect play → add song to queue, before
                // the play prologue since it is an add.
                if self.library_play_adds_to_queue() {
                    let song_id = self
                        .albums_page
                        .expansion
                        .children
                        .get(track_idx)
                        .map(|s| s.id.clone());
                    if let Some(sid) = song_id {
                        let title = self
                            .albums_page
                            .expansion
                            .children
                            .get(track_idx)
                            .map_or_else(|| "song".to_string(), |s| s.title.clone());
                        return self.shell_fire_and_forget_task(
                            move |shell| async move {
                                shell.add_song_to_queue_by_id(&sid, &album_id).await
                            },
                            format!("Added '{title}' to queue"),
                            "add song to queue",
                        );
                    }
                    return Task::none();
                }
                self.guard_play_action();
                self.enter_new_playback_context();
                let shuffle = self.activate_shuffle_directive(force, true);
                return self.shell_action_task(
                    move |shell| async move {
                        shell
                            .play_album_from_track(&album_id, track_idx, shuffle)
                            .await
                    },
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "play album from track",
                );
            }

            AlbumsAction::SetRating(item_id, kind, new_rating) => {
                let current = if matches!(kind, ItemKind::Album) {
                    Self::find_current_rating(
                        &self.library.albums,
                        &item_id,
                        |a| &a.id,
                        |a| a.rating,
                    )
                } else {
                    // Song within expanded album
                    Self::find_current_rating(
                        &self.albums_page.expansion.children,
                        &item_id,
                        |s| &s.id,
                        |s| s.rating,
                    )
                };
                return self.set_item_rating_task(item_id, kind, new_rating, current);
            }
            AlbumsAction::ToggleStar(item_id, kind, star) => {
                return self.toggle_star_with_revert_task(item_id, kind, star);
            }
            AlbumsAction::AddBatchToMix(seeds) => {
                return self.add_seeds_to_mix(seeds);
            }
            AlbumsAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            AlbumsAction::PlayNext(id) => {
                use nokkvi_data::types::batch::{BatchItem, BatchPayload};
                let payload = BatchPayload::new().with_item(BatchItem::Album(id));
                return self.play_next_batch_task(payload);
            }
            AlbumsAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            AlbumsAction::ShowInFolder(album_id) => {
                return self.show_album_in_folder_task(album_id);
            }
            AlbumsAction::ShowSongInFolder(path) => {
                return self.handle_show_in_folder(path);
            }
            AlbumsAction::RefreshArtwork(album_id) => {
                return self.update(Message::Artwork(ArtworkMessage::RefreshAlbumArtwork(
                    album_id,
                )));
            }
            AlbumsAction::FindSimilar(id, seed_name) => {
                return Task::done(Message::Find(FindMessage::Similar { id, seed_name }));
            }
            AlbumsAction::TopSongs(artist_name, artist_id) => {
                return Task::done(Message::Find(FindMessage::TopSongs {
                    artist_name,
                    artist_id,
                }));
            }
            AlbumsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_column_visibility(col, value);
            }
            // Common actions: `handle_common_view_action` above returned early for
            // each of these. `None` has nothing to do.
            AlbumsAction::SearchChanged(_)
            | AlbumsAction::SortModeChanged(_)
            | AlbumsAction::SortOrderChanged(_)
            | AlbumsAction::RefreshViewData
            | AlbumsAction::CenterOnPlaying
            | AlbumsAction::NavigateAndFilter(..)
            | AlbumsAction::NavigateAndExpandArtist(_)
            | AlbumsAction::NavigateAndExpandGenre(_)
            | AlbumsAction::None => {}
        }

        cmd.map(Message::Albums)
    }

    /// Routes `Message::AlbumsLoader(...)` arrivals to the existing
    /// `handle_albums_loaded` / `handle_albums_page_loaded` handlers.
    /// Mirrors the Genres dispatcher; the paged shape adds a second
    /// variant for `PageLoaded`.
    pub(crate) fn dispatch_albums_loader(
        &mut self,
        msg: crate::app_message::AlbumsLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::AlbumsLoaderMessage;
        match msg {
            AlbumsLoaderMessage::Loaded {
                result,
                total_count,
                background,
                anchor_id,
            } => self.handle_albums_loaded(result, total_count, background, anchor_id),
            AlbumsLoaderMessage::PageLoaded(result, total_count) => {
                self.handle_albums_page_loaded(result, total_count)
            }
        }
    }
}
