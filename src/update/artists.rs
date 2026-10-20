//! Artist data loading and component message handlers

use iced::{Task, widget::image};
use nokkvi_data::{
    backend::artists::ArtistUIViewData,
    types::{
        ItemKind,
        artist_recs::{ARTIST_RECS_SHOWN, ArtistRec, RecSources},
        paged_buffer::PageSlot,
    },
};
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, FindMessage, Message, NavigationMessage},
    update::{ArtistsTarget, PagedLoad, Reload},
    views::{self, ArtistsAction, ArtistsMessage, HasCommonAction},
    widgets,
};

/// How long an artist must stay centred before the Artists view asks the
/// server for its info (`getArtistInfo2`).
const ARTIST_INFO_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

impl Nokkvi {
    /// Sort UI artists in-place by rating (Some > None, then desc by value).
    /// Used by the rating-sort carve-out after every page load — the
    /// Subsonic API doesn't expose rating sort, so we emulate it client-side.
    pub(crate) fn artists_rating_sort(ui_artists: &mut [ArtistUIViewData]) {
        ui_artists.sort_by(|a, b| match (a.rating, b.rating) {
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(ra), Some(rb)) => rb.cmp(&ra),
            (None, None) => std::cmp::Ordering::Equal,
        });
    }

    /// Per-entity fetch body for `Nokkvi::load_paged::<ArtistsTarget>`.
    ///
    /// Takes the rating-sort and album-artists-only flags as explicit args (the
    /// call sites snapshot them before invoking `load_paged` so each dispatch
    /// sees a consistent value); the shared invariant body (page_size,
    /// defensive gate, `PaginatedFetch`, debug log, `set_loading(true)`) lives
    /// in `loader_target.rs`. The `album_artists_only` plumbing stays on this
    /// fn rather than on `PaginatedFetch` so the Albums/Songs fetch paths keep
    /// their clean signature — Artists is the only entity with this carve-out.
    async fn fetch_artists_page(
        shell: nokkvi_data::backend::app_service::AppService,
        params: super::components::PaginatedFetch,
        is_rating_sort: bool,
        album_artists_only: bool,
    ) -> (Result<Vec<ArtistUIViewData>, String>, usize) {
        let artists_vm = shell.artists().clone();
        let library_ids = shell.active_library_ids_vec();
        match artists_vm
            .load_raw_artists_page_with_libraries(
                &params.query(&library_ids),
                album_artists_only,
                params.offset,
                params.page_size,
            )
            .await
        {
            Ok(artists) => {
                let mut ui_artists: Vec<ArtistUIViewData> =
                    artists.into_iter().map(ArtistUIViewData::from).collect();
                if is_rating_sort {
                    Nokkvi::artists_rating_sort(&mut ui_artists);
                }
                (Ok(ui_artists), artists_vm.get_total_count() as usize)
            }
            Err(e) => (Err(format!("{e:#}")), 0),
        }
    }

    pub(crate) fn handle_load_artists(&mut self, reload: Option<Reload>) -> Task<Message> {
        let background = reload.is_some();
        let (load, anchor_id) = Reload::first_rows(reload);
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let album_artists_only = self.settings.show_album_artists_only;
        debug!(album_artists_only, "LoadArtists per-call flags");
        self.load_paged::<ArtistsTarget, _, _, _>(
            load,
            move |outcome| {
                Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::Loaded {
                    result: outcome.result,
                    total_count: outcome.total_count,
                    background,
                    anchor_id,
                    replace: outcome.replace,
                })
            },
            move |shell, params| {
                Self::fetch_artists_page(shell, params, is_rating_sort, album_artists_only)
            },
        )
    }

    /// Load a subsequent page of artists (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_artists_load_page(&mut self, offset: usize) -> Task<Message> {
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let album_artists_only = self.settings.show_album_artists_only;
        let slot = self.library.artists.page_slot(offset);
        self.load_paged::<ArtistsTarget, _, _, _>(
            PagedLoad::Page(offset),
            move |outcome| {
                Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::PageLoaded {
                    slot,
                    result: outcome.result,
                    total_count: outcome.total_count,
                })
            },
            move |shell, params| {
                Self::fetch_artists_page(shell, params, is_rating_sort, album_artists_only)
            },
        )
    }

    /// Force-load an artists page regardless of the scroll-edge gate. Used
    /// by `try_resolve_pending_expand_artist` to walk the full library.
    pub(crate) fn force_load_artists_page(&mut self, offset: usize) -> Task<Message> {
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let album_artists_only = self.settings.show_album_artists_only;
        let slot = self.library.artists.page_slot(offset);
        self.load_paged::<ArtistsTarget, _, _, _>(
            PagedLoad::ForcedPage(offset),
            move |outcome| {
                Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::PageLoaded {
                    slot,
                    result: outcome.result,
                    total_count: outcome.total_count,
                })
            },
            move |shell, params| {
                Self::fetch_artists_page(shell, params, is_rating_sort, album_artists_only)
            },
        )
    }

    /// Fetch the 500 px artist artwork and stash it in `large_artwork`.
    /// Skipped when already cached. The external poster URL (Navidrome may
    /// return one) resolves from `library.artists`, falling back to Harbour's
    /// artist lists (Most Played shelf / search results) for artists never
    /// loaded into the Artists view; unresolved falls back to the `ar-{id}`
    /// cover endpoint.
    ///
    /// Shared by `LoadLargeArtwork` (settled-scroll / hotkey navigation) and
    /// the `ExpandArtist` action — `FocusAndExpand` from a queue/songs link
    /// click bypasses every scroll-driven trigger, so the expand path has to
    /// kick the fetch itself or the artwork column would stay blank until
    /// the user scrolled away and back.
    /// The artist rows nokkvi holds, as `(id, image info, poster)`, in lookup
    /// order: the Artists view, then Harbour's Most Played Artists shelf, its
    /// random pick and its search results (raw `Artist`s that may never enter
    /// the Artists view's buffer). The one list behind [`Self::artist_image`]
    /// (absent art, versions), the big panel's poster pick and the placeholder
    /// decode, so a surface that renders artists joins it here and nowhere
    /// else (`every_held_list_answers_the_shared_lookups`); a surface missing
    /// from it showed local `ar-{id}` art, or a blank square, where the shelf
    /// showed the poster.
    pub(crate) fn held_artists(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            &nokkvi_data::types::image_info::ImageInfo,
            Option<&str>,
        ),
    > {
        self.library
            .artists
            .iter()
            .map(|a| (a.id.as_str(), &a.image, a.poster()))
            .chain(
                self.harbour
                    .most_played_artists
                    .iter()
                    .chain(self.harbour.random_artist.iter())
                    .chain(
                        self.harbour
                            .search_results
                            .iter()
                            .flat_map(|r| r.artists.iter()),
                    )
                    .map(|a| (a.id.as_str(), &a.image, a.poster())),
            )
    }

    /// The image info (Navidrome 0.64+) of an artist row nokkvi holds
    /// ([`Self::held_artists`]). `None` for an unknown id; the default on
    /// older servers.
    pub(crate) fn artist_image(
        &self,
        artist_id: &str,
    ) -> Option<&nokkvi_data::types::image_info::ImageInfo> {
        self.held_artists()
            .find(|(id, _, _)| *id == artist_id)
            .map(|(_, image, _)| image)
    }

    /// Whether the server marked this artist's art absent (Navidrome 0.64+).
    /// Unknown ids and older servers read as not absent.
    pub(crate) fn artist_image_absent(&self, artist_id: &str) -> bool {
        self.artist_image(artist_id)
            .is_some_and(|image| image.image_absent)
    }

    pub(crate) fn handle_load_artist_large_artwork(&mut self, artist_id: String) -> Task<Message> {
        if self.artwork.large_artwork.peek(&artist_id).is_some() {
            return Task::none();
        }

        // The poster the rows and thumbnails share (`poster`: large, falling
        // back to medium), from the first held row of the artist that has one:
        // an Artists-view row loaded before the agents filled the link gives
        // way to a Harbour row that has it.
        let external_url = self
            .held_artists()
            .filter(|(id, _, _)| *id == artist_id)
            .find_map(|(_, _, poster)| poster)
            .map(str::to_owned);

        // The server marked the artist's art absent (Navidrome 0.64+) and no
        // external poster is known: no request, the panel stays blank. An
        // external poster is still fetched as before.
        if external_url.is_none() && self.artist_image_absent(&artist_id) {
            return Task::none();
        }

        // Set the in-flight marker before the `app_service` check so it
        // matches the Albums helper's ordering — the marker is the
        // observable side-effect tests rely on.
        self.artwork.loading_large_artwork = Some(artist_id.clone());

        // The artist's image hash rides the `ar-` id on 0.64+ (an external
        // poster URL passes through the builder untouched).
        let version = self
            .artist_image(&artist_id)
            .and_then(|image| nokkvi_data::types::image_info::artwork_version(image, None));
        // Already on its way at this version (same gate as the Albums helper).
        if self
            .artwork
            .large_artwork_pending
            .holds(&artist_id, &version)
        {
            return Task::none();
        }
        self.record_large_art_hash(&artist_id, version.as_deref());

        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let vm = shell.albums().clone();
        self.artwork
            .large_artwork_pending
            .claim(artist_id.clone(), version.clone());

        let id = artist_id;
        Task::perform(
            async move {
                let art_id = external_url.unwrap_or_else(|| format!("ar-{id}"));
                match vm
                    .fetch_album_artwork(&art_id, Some(500), version.as_deref())
                    .await
                {
                    Ok(bytes) if bytes.len() > 100 => (id, Some(image::Handle::from_bytes(bytes))),
                    _ => (id, None),
                }
            },
            // Always dispatch `LargeArtistLoaded`, with `handle = None` on
            // failure / undersized payload. The handler clears the
            // `loading_large_artwork` marker by id, so a no-handle result
            // still releases the lock and lets the user retry. Returning
            // `NoOp` here used to leave the marker stuck forever.
            |(id, handle)| Message::Artwork(ArtworkMessage::LargeArtistLoaded(id, handle)),
        )
    }

    /// Fetch the centred artist's info (`getArtistInfo2`) unless it is
    /// cached or already in flight. Navidrome asks its agents for an
    /// artist's biography only on this call, so without it the bio stays
    /// blank on a server no other client browses.
    ///
    /// The in-flight mark is set before the `app_service` check, like the
    /// large-artwork marker: it is the observable side effect tests use. In
    /// the app it is never left behind without a request: `InfoSettled` only
    /// comes from a timer `artist_center_tasks` starts with a service, and
    /// logout (which drops the service) clears the marks.
    pub(crate) fn artist_info_task(&mut self, artist_id: String) -> Task<Message> {
        let Some(generation) = self.artist_info.begin(&artist_id) else {
            return Task::none();
        };
        let id = artist_id.clone();
        self.shell_task(
            move |shell| async move { shell.artist_info_api().await?.get_artist_info(&id).await },
            move |result| Self::artist_info_result_message(generation, artist_id, result),
        )
    }

    /// The large artwork and the info for the artist the Artists view just
    /// centred. Every centring path goes through this, so the biography
    /// follows the same trigger as the artwork panel it sits on. The info is
    /// asked for only once the artist has stayed centred for
    /// [`ARTIST_INFO_SETTLE`]: a first ask makes the server run its agents
    /// and broadcast a library refresh, too much to do for every row a held
    /// arrow key scrolls past.
    pub(crate) fn artist_center_tasks(&mut self, artist_id: String) -> Task<Message> {
        let artwork = self.handle_load_artist_large_artwork(artist_id.clone());
        Task::batch([artwork, self.artist_info_settle_task(artist_id)])
    }

    /// [`Self::artist_center_tasks`] for a navigation step: the large artwork
    /// goes through the big panel's gate (`panel_art_step`), one full-size
    /// cover on its way at a time.
    pub(crate) fn artist_step_tasks(&mut self, artist_id: String) -> Task<Message> {
        let artwork = self.panel_art_step(
            Some(View::Artists),
            crate::state::PanelArt::Artist(artist_id.clone()),
        );
        Task::batch([artwork, self.artist_info_settle_task(artist_id)])
    }

    /// Start the settle timer for a newly centred artist, when its info or
    /// its similar artists (with a source on) are still to be asked for.
    /// One timer serves both: they are asked for at the same moment.
    fn artist_info_settle_task(&mut self, artist_id: String) -> Task<Message> {
        let info_wanted = self.artist_info.centre(&artist_id);
        let recs_wanted = self.rec_sources().any() && self.artist_recs.centre(&artist_id);
        if (info_wanted || recs_wanted) && self.app_service.is_some() {
            Task::perform(tokio::time::sleep(ARTIST_INFO_SETTLE), move |()| {
                Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::InfoSettled(
                    artist_id,
                ))
            })
        } else {
            Task::none()
        }
    }

    /// An artist's settle timer fired: ask for its info and its similar
    /// artists, each if it is still the one that cache centred last.
    pub(crate) fn handle_artist_info_settled(&mut self, artist_id: String) -> Task<Message> {
        let info = if self.artist_info.settled(&artist_id) {
            self.artist_info_task(artist_id.clone())
        } else {
            Task::none()
        };
        let recs = if self.artist_recs.settled(&artist_id) {
            self.artist_recs_task(artist_id)
        } else {
            Task::none()
        };
        Task::batch([info, recs])
    }

    /// The artist whose panel the Artists view shows: the effective centre
    /// (a clicked row's highlight, else the viewport centre), with an album
    /// row of an expanded artist resolving to that artist. The same row the
    /// view's artwork pill resolves.
    pub(crate) fn centred_artist_id(&self) -> Option<String> {
        let artists = &self.library.artists;
        let page = &self.artists_page;
        let idx = page
            .common
            .get_center_item_index(page.expansion.flattened_len(artists))?;
        match page.expansion.get_entry_at(idx, artists, |a| &a.id)? {
            views::expansion::SlotListEntry::Parent(artist) => Some(artist.id.clone()),
            views::expansion::SlotListEntry::Child(_, parent_id) => Some(parent_id),
        }
    }

    /// The message a finished `getArtistInfo2` fetch sends: a rejected
    /// session goes to the login screen, anything else lands in
    /// [`Self::handle_artist_info_loaded`].
    pub(crate) fn artist_info_result_message(
        generation: u64,
        artist_id: String,
        result: anyhow::Result<nokkvi_data::types::artist::ArtistInfo>,
    ) -> Message {
        if let Err(e) = &result
            && let Some(msg) = super::components::session_expired_message(e)
        {
            return msg;
        }
        Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::InfoLoaded {
            generation,
            artist_id,
            result: result.map_err(|e| format!("{e:#}")),
        })
    }

    /// Land a `getArtistInfo2` answer: cache it, or on failure release the
    /// artist so the next centring asks again.
    ///
    /// A failure is logged and not toasted: the biography is a garnish, and
    /// a toast per centred artist would bury the view while the server is
    /// unreachable. The native row's biography still shows.
    pub(crate) fn handle_artist_info_loaded(
        &mut self,
        generation: u64,
        artist_id: &str,
        result: Result<nokkvi_data::types::artist::ArtistInfo, String>,
    ) -> Task<Message> {
        let info = match result {
            Ok(info) => Some(info),
            Err(e) => {
                debug!("getArtistInfo2 for {artist_id} failed (asked again on next centring): {e}");
                None
            }
        };
        let landed = info.is_some();
        if !self.artist_info.finish(generation, artist_id, info) {
            debug!("getArtistInfo2 for {artist_id} landed after logout; dropped");
            return Task::none();
        }
        // Similar artists waiting on this answer's MusicBrainz id: ask now
        // (Deezer by name when the answer has no id), or settle on none when
        // there is nothing to ask with. A failed answer ends the wait; with
        // Deezer off the next centring asks for both again, with it on
        // Deezer is asked by name now.
        if self.artist_recs.take_awaiting(artist_id) {
            if landed || self.rec_sources().deezer {
                return self.artist_recs_task(artist_id.to_string());
            }
            self.sync_similar_artists_modal(artist_id, false);
        }
        Task::none()
    }

    /// Which services the similar-artists lists ask: the two Similar
    /// Artists settings.
    pub(crate) fn rec_sources(&self) -> RecSources {
        RecSources {
            listenbrainz: self.settings.similar_artists_online,
            deezer: self.settings.similar_artists_deezer,
        }
    }

    /// The similar-artist sources went from `previous` to the settings' (a
    /// Settings row, a config edit). The lists fetched for the old mix, and
    /// any ask still out for it, are dropped, and the centred artist and the
    /// open modal's artist are asked again, so both show the new mix now
    /// rather than at the next centring. Unchanged sources (every hot reload
    /// re-delivers them) keep everything.
    pub(crate) fn rec_sources_changed(&mut self, previous: RecSources) -> Task<Message> {
        use crate::widgets::similar_artists_modal::SimilarArtistsStatus;

        let sources = self.rec_sources();
        if sources == previous {
            return Task::none();
        }
        debug!("similar-artist sources changed to {sources:?}; lists asked for again");
        // A logout's reset is the same forgetting: lists, asks in flight
        // (their answers are dropped by generation) and the id wait.
        self.artist_recs.reset_for_session();
        let modal_artist = self.similar_artists_modal.as_mut().map(|state| {
            state.set_recs(Vec::new());
            state.status = if sources.any() {
                SimilarArtistsStatus::Loading
            } else {
                SimilarArtistsStatus::Off
            };
            state.artist_id.clone()
        });
        if !sources.any() {
            return Task::none();
        }
        let artists: Vec<String> = self
            .centred_artist_id()
            .into_iter()
            .chain(modal_artist)
            .collect();
        Task::batch(
            artists
                .into_iter()
                .flat_map(|artist_id| {
                    [
                        self.artist_info_task(artist_id.clone()),
                        self.artist_recs_task(artist_id),
                    ]
                })
                .collect::<Vec<_>>(),
        )
    }

    /// The similar-artist lists the Artists panel shows, by artist id: none
    /// while both Similar Artists sources are off, so lists fetched before
    /// they went off stop showing too.
    pub(crate) fn shown_artist_recs(&self) -> &std::collections::HashMap<String, Vec<ArtistRec>> {
        static NONE: std::sync::LazyLock<std::collections::HashMap<String, Vec<ArtistRec>>> =
            std::sync::LazyLock::new(std::collections::HashMap::new);
        if self.rec_sources().any() {
            &self.artist_recs.cache.snapshot
        } else {
            &NONE
        }
    }

    /// The name of `artist_id`, which Deezer is asked by: the library row's,
    /// else the open modal's when it is about that artist (a queue row's
    /// artist the loaded Artists pages may not hold).
    pub(crate) fn artist_name(&self, artist_id: &str) -> Option<String> {
        let from_row = self
            .library
            .artists
            .iter()
            .find(|a| a.id == artist_id)
            .map(|a| a.name.clone());
        from_row
            .or_else(|| {
                self.similar_artists_modal
                    .as_ref()
                    .filter(|state| state.artist_id == artist_id)
                    .map(|state| state.artist_name.clone())
            })
            .filter(|name| !name.trim().is_empty())
    }

    /// The MusicBrainz id known for `artist_id`: the native row's (a library
    /// tagged with one), else the fetched info's (Navidrome's agents look
    /// one up). Empty strings count as none.
    pub(crate) fn artist_mbid(&self, artist_id: &str) -> Option<String> {
        let present = |id: Option<&String>| id.filter(|m| !m.trim().is_empty()).cloned();
        let from_row = self
            .library
            .artists
            .iter()
            .find(|a| a.id == artist_id)
            .and_then(|a| present(a.mbz_artist_id.as_ref()));
        from_row.or_else(|| {
            self.artist_info
                .cache
                .peek(&artist_id.to_string())
                .and_then(|info| present(info.musicbrainz_id.as_ref()))
        })
    }

    /// Fetch the centred artist's similar artists (ListenBrainz and Deezer
    /// as their settings say, blended, each checked against the library)
    /// unless both are off, the list is cached or a fetch is in flight.
    /// ListenBrainz needs the artist's MusicBrainz id: without one, the ask
    /// waits for the info fetch when that is still out
    /// (`handle_artist_info_loaded` resumes it), so ListenBrainz can join
    /// even when Deezer could be asked by name at once. With neither an id
    /// for ListenBrainz nor a name for Deezer, it settles on an empty list.
    pub(crate) fn artist_recs_task(&mut self, artist_id: String) -> Task<Message> {
        let sources = self.rec_sources();
        if !sources.any()
            || self.artist_recs.is_cached(&artist_id)
            || self.artist_recs.is_pending(&artist_id)
        {
            return Task::none();
        }
        let mbid = self
            .artist_mbid(&artist_id)
            .filter(|_| sources.listenbrainz);
        if sources.listenbrainz && mbid.is_none() && self.artist_info.is_pending(&artist_id) {
            self.artist_recs.wait_for_mbid(&artist_id);
            return Task::none();
        }
        let name = self.artist_name(&artist_id).filter(|_| sources.deezer);
        if mbid.is_none() && name.is_none() {
            debug!("no MusicBrainz id or name to ask with for {artist_id}; no similar artists");
            self.artist_recs.settle_with(&artist_id, Vec::new());
            self.sync_similar_artists_modal(&artist_id, true);
            return Task::none();
        }
        let Some(generation) = self.artist_recs.begin(&artist_id) else {
            return Task::none();
        };
        debug!(
            "asking for artists similar to {artist_id} (ListenBrainz by mbid {mbid:?}, \
             Deezer by name {name:?})"
        );
        let id = artist_id.clone();
        self.shell_task(
            move |shell| async move {
                shell
                    .similar_artists(mbid.as_deref(), name.as_deref(), sources, ARTIST_RECS_SHOWN)
                    .await
            },
            move |result| Self::artist_recs_result_message(generation, id, result),
        )
    }

    /// The message a finished similar-artists fetch sends: a rejected
    /// session (a library lookup refused) goes to the login screen, anything
    /// else lands in [`Self::handle_artist_recs_loaded`].
    pub(crate) fn artist_recs_result_message(
        generation: u64,
        artist_id: String,
        result: anyhow::Result<Vec<ArtistRec>>,
    ) -> Message {
        if let Err(e) = &result
            && let Some(msg) = super::components::session_expired_message(e)
        {
            return msg;
        }
        Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::RecsLoaded {
            generation,
            artist_id,
            result: result.map_err(|e| format!("{e:#}")),
        })
    }

    /// Land a similar-artists answer: cache it, or on failure release the
    /// artist so the next centring asks again. Not toasted, like the
    /// biography: the list is a garnish, and a source being unreachable
    /// would otherwise toast once per centred artist. A list one source
    /// filled while the other failed is cached like any other.
    pub(crate) fn handle_artist_recs_loaded(
        &mut self,
        generation: u64,
        artist_id: &str,
        result: Result<Vec<ArtistRec>, String>,
    ) -> Task<Message> {
        let recs = match result {
            Ok(recs) => {
                let in_library = recs.iter().filter(|r| r.library_id().is_some()).count();
                debug!(
                    "{} similar artists for {artist_id}, {in_library} in the library",
                    recs.len()
                );
                Some(recs)
            }
            Err(e) => {
                debug!(
                    "similar artists for {artist_id} failed (asked again on next centring): {e}"
                );
                None
            }
        };
        let landed = recs.is_some();
        if !self.artist_recs.finish(generation, artist_id, recs) {
            debug!("similar artists for {artist_id} landed after logout; dropped");
            return Task::none();
        }
        self.sync_similar_artists_modal(artist_id, landed);
        Task::none()
    }

    pub(crate) fn handle_artists_page_loaded(
        &mut self,
        slot: PageSlot,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.handle_page_loaded_with::<ArtistsTarget>(slot, result, total_count)
    }

    pub(crate) fn handle_artists_loaded(
        &mut self,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
        replace: Option<nokkvi_data::types::paged_buffer::ReplaceTicket>,
    ) -> Task<Message> {
        self.handle_loaded_with::<ArtistsTarget>(
            result,
            total_count,
            background,
            anchor_id,
            replace,
        )
    }

    pub(crate) fn handle_artists(&mut self, msg: views::ArtistsMessage) -> Task<Message> {
        if let Some(task) = crate::update::dispatch_view_chrome(self, &msg, crate::View::Artists) {
            return task;
        }
        if let ArtistsMessage::OpenExternalUrl(url) = msg {
            if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                tracing::warn!("Failed to open URL '{}': {}", url, e);
            }
            return Task::none();
        }
        // Capture child album ids before consuming `msg` so we can fan out
        // mini-artwork fetches for the newly-loaded expansion children.
        let expansion_album_ids = match &msg {
            ArtistsMessage::AlbumsLoaded(_, albums) => {
                super::components::expansion_child_album_ids(albums)
            }
            _ => Vec::new(),
        };
        // Capture the loaded artist id too — the page's `update` runs
        // `set_children` (which clears `selected_offset`), and the find-
        // chain pin needs to re-pin the highlight on the target afterwards.
        let pin_after_albums = if let ArtistsMessage::AlbumsLoaded(ref id, _) = msg {
            Some(id.clone())
        } else {
            None
        };
        let (cmd, action) =
            self.artists_page
                .update(msg, self.library.artists.len(), &self.library.artists);

        if let Some(loaded_id) = pin_after_albums {
            self.pin_after_load(
                &loaded_id,
                |pin, id| matches!(pin, crate::state::PendingTopPin::Artist(p) if p == id),
                |app, id| app.library.artists.iter().position(|a| a.id == id),
                |app, idx| {
                    let total = app
                        .artists_page
                        .expansion
                        .flattened_len(&app.library.artists);
                    app.artists_page.common.slot_list.pin_selected(idx, total);
                },
            );
        }

        // User-driven changes supersede any in-flight find-and-expand chain.
        if matches!(
            action,
            ArtistsAction::SearchChanged(_)
                | ArtistsAction::SortModeChanged(_)
                | ArtistsAction::SortOrderChanged(_)
                | ArtistsAction::RefreshViewData
        ) {
            self.cancel_pending_expand();
        }

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadArtists,
            "persist_artists_prefs",
            self.artists_page.common.current_sort_mode,
            self.artists_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_artists_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            ArtistsAction::PlayArtist(artist_id_str, force) => {
                // Browsing panel: redirect play → add to queue (insert at
                // drag-drop position when one is pending, else append). An
                // add, so it runs before the play prologue.
                let id_ref = artist_id_str.as_str();
                if let Some(task) = self.redirect_play_to_queue_in_browsing_panel(
                    |app| {
                        app.add_entity_to_queue_task(
                            &app.library.artists,
                            id_ref,
                            "artist",
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id| async move { shell.add_artist_to_queue(&id).await },
                        )
                    },
                    |app, pos| {
                        app.insert_entity_to_queue_at_position_task(
                            &app.library.artists,
                            id_ref,
                            "artist",
                            pos,
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id, position| async move {
                                shell.insert_artist_at_position(&id, position).await
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
                // AppendAndPlay: append artist songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.settings.enter_behavior == EnterBehavior::AppendAndPlay
                    && let Ok(index) = artist_id_str.parse::<usize>()
                    && let Some(artist) = self.library.artists.get(index)
                {
                    let id = artist.id.clone();
                    let name = artist.name.clone();
                    let shuffle = self.activate_shuffle_directive(force, false);
                    self.guard_play_action();
                    self.clear_active_playlist();
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_artist_and_play(&id, shuffle).await },
                        format!("Playing '{name}'"),
                        "append artist and play",
                    );
                }
                // PlayAll / PlaySingle: replace queue with artist (PlaySingle = PlayAll for artists)
                let shuffle = self.activate_shuffle_directive(force, false);
                return self.play_entity_task(
                    |app| &app.library.artists,
                    &artist_id_str,
                    "artist",
                    |a| a.id.clone(),
                    move |shell, id| async move { shell.play_artist(&id, shuffle).await },
                );
            }
            ArtistsAction::PlayBatch(payload, force) => {
                self.artists_page
                    .common
                    .slot_list
                    .clear_selection_indices_only();
                return self.play_batch_task(payload, force);
            }
            ArtistsAction::PlaySelection(payload, force) => {
                self.artists_page
                    .common
                    .slot_list
                    .clear_selection_indices_only();
                return self.play_selection_task(payload, force);
            }
            ArtistsAction::AddBatchToQueue(payload) => {
                return self.add_or_insert_batch_to_queue_task(payload);
            }
            ArtistsAction::PlayAlbum(album_id, force) => {
                // Browsing panel: redirect play → add to queue, before the
                // play prologue since it is an add.
                if self.library_play_adds_to_queue() {
                    let name = self
                        .artists_page
                        .expansion
                        .children
                        .iter()
                        .find(|a| a.id == album_id)
                        .map_or_else(|| "album".to_string(), |a| a.name.clone());
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_album_to_queue(&album_id).await },
                        format!("Added '{name}' to queue"),
                        "add album to queue",
                    );
                }
                let shuffle = self.activate_shuffle_directive(force, false);
                return self.queue_play_task(
                    move |shell| async move { shell.play_album(&album_id, shuffle).await },
                    || Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "Failed to play album".into(),
                );
            }

            ArtistsAction::ExpandArtist(artist_id) => {
                // Load albums for the artist and send them back to the view
                let id = artist_id.clone();

                // FocusAndExpand (link-text click in queue/songs) skips the
                // scroll-driven `LoadLargeArtwork` path, so the artwork
                // column would stay blank until the user nudged the list.
                // Mirror the Albums fix and kick the fetch from here.
                let artwork_task = self.artist_center_tasks(id.clone());

                let albums_task = self.expand_load_children_task(
                    move |shell| async move {
                        let artists_vm = shell.artists().clone();
                        let albums_vm = shell.albums().clone();
                        let albums = artists_vm.load_artist_albums(&id).await?;
                        let (url, cred) = albums_vm.get_server_config().await;
                        let ui_albums: Vec<nokkvi_data::backend::albums::AlbumUIViewData> = albums
                            .iter()
                            .map(|album| {
                                nokkvi_data::backend::albums::AlbumUIViewData::from_album(
                                    album, &url, &cred,
                                )
                            })
                            .collect();
                        Ok(ui_albums)
                    },
                    move |albums| Message::Artists(ArtistsMessage::AlbumsLoaded(artist_id, albums)),
                    "load artist albums",
                );

                return Task::batch([artwork_task, albums_task]);
            }
            ArtistsAction::StarArtist(artist_id) => {
                return self.toggle_star_with_revert_task(artist_id, ItemKind::Artist, true);
            }
            ArtistsAction::UnstarArtist(artist_id) => {
                return self.toggle_star_with_revert_task(artist_id, ItemKind::Artist, false);
            }
            ArtistsAction::SetRating(item_id, kind, new_rating) => {
                let current = if matches!(kind, ItemKind::Artist) {
                    Self::find_current_rating(
                        &self.library.artists,
                        &item_id,
                        |a| &a.id,
                        |a| a.rating,
                    )
                } else {
                    // Album within expanded artist
                    Self::find_current_rating(
                        &self.artists_page.expansion.children,
                        &item_id,
                        |a| &a.id,
                        |a| a.rating,
                    )
                };
                return self.set_item_rating_task(item_id, kind, new_rating, current);
            }
            ArtistsAction::ToggleStar(item_id, kind, star) => {
                return self.toggle_star_with_revert_task(item_id, kind, star);
            }
            ArtistsAction::LoadPage(offset) => {
                return self.handle_artists_load_page(offset);
            }
            ArtistsAction::AddBatchToMix(seeds) => {
                return self.add_seeds_to_mix(seeds);
            }
            ArtistsAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            ArtistsAction::PlayNextBatch(payload) => {
                return self.play_next_batch_task(payload);
            }
            ArtistsAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            ArtistsAction::ShowAlbumInFolder(album_id) => {
                return self.show_album_in_folder_task(album_id);
            }
            ArtistsAction::ShowSongInFolder(path) => {
                return self.handle_show_in_folder(path);
            }
            ArtistsAction::FindSimilar(id, seed_name) => {
                return Task::done(Message::Find(FindMessage::Similar { id, seed_name }));
            }
            ArtistsAction::TopSongs(artist_name, artist_id) => {
                return Task::done(Message::Find(FindMessage::TopSongs {
                    artist_name,
                    artist_id,
                }));
            }
            ArtistsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_column_visibility(col, value);
            }
            ArtistsAction::LoadLargeArtwork => {
                // Settled-scroll / hotkey navigation: refresh viewport mini
                // artwork, fetch 500px artwork for the new
                // center artist, and chain a page-fetch if the viewport is
                // near the loaded edge. Mid-drag scrolling does NOT enter
                // here — `SlotListScrollSeek` returns `None` and lets the
                // 150 ms `SeekSettled` debounce synthesise a `SetOffset`
                // that lands in this arm exactly once per drag.
                let mut batch: Vec<Task<Message>> = vec![cmd.map(Message::Artists)];

                if let Some(id) = self.centred_artist_id() {
                    batch.push(self.artist_step_tasks(id));
                }

                // Prefetch viewport mini artwork + chain a page-load if
                // scrolling near the loaded edge.
                batch.extend(self.prefetch_and_maybe_load_next_page::<ArtistsTarget>(
                    Self::handle_artists_load_page,
                ));

                return Task::batch(batch);
            }
            // Common actions: `handle_common_view_action` above returned early for
            // each of these. `None` has nothing to do.
            ArtistsAction::SearchChanged(_)
            | ArtistsAction::SortModeChanged(_)
            | ArtistsAction::SortOrderChanged(_)
            | ArtistsAction::RefreshViewData
            | ArtistsAction::CenterOnPlaying
            | ArtistsAction::NavigateAndFilter(..)
            | ArtistsAction::NavigateAndExpandAlbum(_)
            | ArtistsAction::NavigateAndExpandArtist(_)
            | ArtistsAction::SimilarArtists(_)
            | ArtistsAction::None => {}
        }

        self.append_expansion_album_prefetch(cmd.map(Message::Artists), expansion_album_ids)
    }

    /// Routes `Message::ArtistsLoader(...)` arrivals to the existing
    /// `handle_artists_loaded` / `handle_artists_page_loaded` handlers.
    /// Mirrors `dispatch_genres_loader` — the per-domain dispatcher pattern
    /// keeps the loader-result routing co-located with its handlers.
    pub(crate) fn dispatch_artists_loader(
        &mut self,
        msg: crate::app_message::ArtistsLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::ArtistsLoaderMessage;
        match msg {
            ArtistsLoaderMessage::Loaded {
                result,
                total_count,
                background,
                anchor_id,
                replace,
            } => self.handle_artists_loaded(result, total_count, background, anchor_id, replace),
            ArtistsLoaderMessage::PageLoaded {
                slot,
                result,
                total_count,
            } => self.handle_artists_page_loaded(slot, result, total_count),
            ArtistsLoaderMessage::InfoLoaded {
                generation,
                artist_id,
                result,
            } => self.handle_artist_info_loaded(generation, &artist_id, result),
            ArtistsLoaderMessage::RecsLoaded {
                generation,
                artist_id,
                result,
            } => self.handle_artist_recs_loaded(generation, &artist_id, result),
            ArtistsLoaderMessage::InfoSettled(artist_id) => {
                self.handle_artist_info_settled(artist_id)
            }
        }
    }
}
