//! Window event handlers

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::{audio, utils::artwork_url::THUMBNAIL_SIZE};

use super::components::{
    passive_artwork_version, prefetch_album_artwork_tasks, prefetch_song_artwork_tasks,
};
use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
};

impl Nokkvi {
    pub(crate) fn handle_window_resized(&mut self, width: f32, height: f32) -> Task<Message> {
        self.window.width = width;
        self.window.height = height;
        // Anchored overlays (context menus, checkbox dropdowns) point at
        // pixel positions captured pre-resize; close them so they don't end
        // up in the wrong spot.
        self.open_menu = None;

        // Recompute the player bar's responsive layout with per-mode
        // hysteresis. Done here (not in view()) so view() stays pure — it
        // just reads the already-computed layout off self.
        self.player_bar_layout =
            crate::widgets::player_bar::compute_layout(width, self.player_bar_layout);

        // Recompute dynamic slot count for the new window size and sync it
        // to every view's SlotListView. Without this, prefetch_indices() uses
        // a stale slot_count (default 9) and under-fetches artwork for tall windows.
        self.resync_slot_counts();

        // Re-prefetch mini artwork for the active view since more slots may
        // now be visible.
        self.prefetch_viewport_artwork()
    }

    /// Recompute every page's `slot_count` to match what the view actually
    /// renders given the current window dimensions AND the active artwork
    /// column mode. Vertical artwork modes (and Auto's portrait fallback)
    /// stack the artwork above the slot list, eating ~`layout.extent` + pad
    /// pixels from the available height — without re-syncing here, the
    /// stored slot_count stays at the horizontal-layout value and
    /// `pending_expand_resolve` lands the auto-expanded row above the visible
    /// viewport (the row only the user can rescue with a manual scroll).
    pub(crate) fn resync_slot_counts(&mut self) {
        use crate::widgets::{
            base_slot_list_layout::{BaseSlotListLayoutConfig, vertical_artwork_chrome},
            slot_list::{SlotListConfig, chrome_height_with_header},
        };

        // The vertical artwork chrome is governed by the window + artwork
        // column only, so it's shared across pages. Probe it with the expanded
        // footprint — the collapse delta is far too small to flip the
        // landscape/portrait artwork fit.
        let probe = BaseSlotListLayoutConfig {
            window_width: self.content_pane_width(),
            window_height: self.window.height,
            show_artwork_column: true,
            slot_list_chrome: chrome_height_with_header(false),
            elevated: false,
        };
        let vertical = vertical_artwork_chrome(&probe);

        // Per-page collapse state: when the auto-hide toolbar is enabled, a page
        // whose toolbar isn't currently revealed renders the SHORTER collapsed
        // header and therefore packs MORE slots. The stored count must reflect
        // that — a hardcoded expanded footprint desyncs every consumer that
        // reads slot_count without first revealing the toolbar: find-and-expand
        // row landing centers on `slot_count/2` (lands the row a slot too low),
        // and the drag mapper used to grab the wrong row (now anchored on
        // hovered_slot, but kept honest here too). The reveal-on-read
        // assumption holds for the keyboard scroll path but not these (nor
        // center-on-playing, which no longer reveals the toolbar — it relies on
        // the stored collapsed count just like find-and-expand).
        //
        // When auto-hide is OFF, `toolbar_collapsed` is always `false`, so this
        // reduces to the previous expanded-footprint behavior exactly.
        let autohide = crate::theme::is_autohide_toolbar();
        let window_height = self.window.height;
        let sc = |collapsed: bool| {
            SlotListConfig::with_dynamic_slots(
                window_height,
                chrome_height_with_header(collapsed) + vertical,
            )
            .slot_count
        };
        // At most two distinct footprints exist; compute each once instead of
        // re-running the float-heavy `with_dynamic_slots` per page. With
        // auto-hide off, every page is `expanded`, so `sc_collapsed` is never
        // computed.
        let sc_expanded = sc(false);
        let sc_collapsed = if autohide { sc(true) } else { sc_expanded };

        // One loop over the shared page-commons array keeps the read and write
        // for each page on the same binding — and reusing
        // `all_slot_list_commons_mut` (instead of a second hand-maintained
        // list) means a page added there is sized here automatically.
        for common in self.all_slot_list_commons_mut() {
            common.slot_list.slot_count = if common.toolbar_collapsed(autohide, false) {
                sc_collapsed
            } else {
                sc_expanded
            };
        }

        // The playlist editor is NOT one of the pooled pages: it renders in
        // place of the view header (its edit bar instead) at the narrower
        // queue-pane width, so its footprint differs from every page above.
        // Size it from its OWN view() chrome so the within-list drag maps
        // slots→items against the real rendered count — otherwise the stored
        // count stays at the SlotListView default and every drag off a >N-track
        // (or scrolled) playlist grabs/drops the wrong row.
        if self.playlist_editor.is_some() {
            let width = self.content_pane_width() * crate::app_view::QUEUE_PANE_FRACTION;
            if let Some(editor) = self.playlist_editor.as_mut()
                // Tracks sessions only — a Rules session renders its evaluated
                // preview through its own state, not `editor.common.slot_list`,
                // and at a different pane width, so the Tracks chrome would set a
                // wrong (and unused) count there.
                && editor.rules_session().is_none()
            {
                let chrome = crate::views::playlist_editor::view::editor_effective_chrome(
                    width,
                    window_height,
                    editor.columns.select,
                );
                editor.common.slot_list.slot_count =
                    SlotListConfig::with_dynamic_slots(window_height, chrome).slot_count;
            }
        }
    }

    /// Prefetch mini artwork for whatever slots are currently visible in the
    /// active view, plus the centered slot's large artwork. Called on window
    /// resize (slot_count change), on view switch, and on every roulette tick
    /// once the spin advances to a new offset (throttled at the call site).
    ///
    /// The center-large dispatch matches the per-view tab/click flow so
    /// scroll-driven offset changes (window resize, roulette) keep the
    /// right-side panel updating in step with the wheel — without it,
    /// Albums/Artists/Songs/Queue would show whatever was cached when the
    /// spin started, since their normal-flow `LoadLargeArtwork` is wired to
    /// `NavigateUp/Down`/`SetOffset` actions, not to viewport prefetch.
    pub(crate) fn prefetch_viewport_artwork(&mut self) -> Task<Message> {
        let shell = match &self.app_service {
            Some(s) => s,
            None => return Task::none(),
        };

        match self.current_view {
            View::Albums => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> =
                    self.artwork.album_art.iter().map(|(k, _)| k).collect();
                let mut tasks = prefetch_album_artwork_tasks(
                    &self.albums_page.common.slot_list,
                    &self.library.albums,
                    &cached,
                    &self.artwork.album_art_versions,
                    &self.artwork.failed_art,
                    albums_vm,
                    |album| {
                        (
                            album.id.clone(),
                            album.updated_at.clone(),
                            album.artwork_url.clone(),
                        )
                    },
                );
                if let Some(task) = self.center_large_artwork_load_task(View::Albums) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Queue => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> =
                    self.artwork.album_art.iter().map(|(k, _)| k).collect();
                let items = self.filter_queue_songs();
                let mut tasks = prefetch_album_artwork_tasks(
                    &self.queue_page.common.slot_list,
                    &items,
                    &cached,
                    &self.artwork.album_art_versions,
                    &self.artwork.failed_art,
                    albums_vm,
                    |song| {
                        (
                            song.album_id.clone(),
                            passive_artwork_version(&song.updated_at),
                            song.artwork_url.clone(),
                        )
                    },
                );
                if let Some(task) = self.center_large_artwork_load_task(View::Queue) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Songs => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> =
                    self.artwork.album_art.iter().map(|(k, _)| k).collect();
                let mut tasks = prefetch_song_artwork_tasks(
                    &self.songs_page.common.slot_list,
                    &self.library.songs,
                    &cached,
                    &self.artwork.album_art_versions,
                    &self.artwork.failed_art,
                    albums_vm,
                    |s| {
                        s.album_id
                            .as_ref()
                            .map(|id| (id, passive_artwork_version(&s.updated_at)))
                    },
                );
                if let Some(task) = self.center_large_artwork_load_task(View::Songs) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Artists => {
                let mut tasks = vec![self.prefetch_artist_mini_artwork_tasks()];
                if let Some(task) = self.center_large_artwork_load_task(View::Artists) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Genres if !self.library.genres.is_empty() => {
                // Re-dispatch a SetOffset to trigger collage artwork loading
                let offset = self.genres_page.common.slot_list.viewport_offset;
                Task::done(Message::Genres(crate::views::GenresMessage::SlotList(
                    crate::widgets::SlotListPageMessage::SetOffset(
                        offset,
                        iced::keyboard::Modifiers::default(),
                    ),
                )))
            }
            View::Playlists if !self.library.playlists.is_empty() => {
                // Re-dispatch a SetOffset to trigger collage artwork loading
                let offset = self.playlists_page.common.slot_list.viewport_offset;
                Task::done(Message::Playlists(
                    crate::views::PlaylistsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::SetOffset(
                            offset,
                            iced::keyboard::Modifiers::default(),
                        ),
                    ),
                ))
            }
            View::Radios => {
                let mut tasks = vec![self.prefetch_radio_logo_tasks()];
                if let Some(task) = self.center_large_artwork_load_task(View::Radios) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            // Harbour warms its shelf artwork through the Harbour loader,
            // not the slot-list viewport prefetch.
            View::Harbour
            | View::Genres
            | View::Playlists
            | View::Settings
            | View::PlaylistEditor => Task::none(),
        }
    }

    pub(crate) fn handle_scale_factor_changed(&mut self, scale_factor: f32) -> Task<Message> {
        self.window.scale_factor = scale_factor;
        Task::none()
    }

    pub(crate) fn handle_play_sfx(&mut self, sfx_type: audio::SfxType) -> Task<Message> {
        self.sfx_engine.play(sfx_type);
        Task::none()
    }

    /// Dispatch the per-view "load large artwork for the centered slot"
    /// task, mirroring what each view's normal `NavigateUp`/`NavigateDown`/
    /// `SetOffset` action emits. Returns `None` when the view doesn't have
    /// a per-slot large-artwork concept (Genres/Playlists drive their
    /// collage panel through a separate `LoadArtwork` action that
    /// `prefetch_viewport_artwork` re-synthesizes via `SetOffset`), when
    /// the centered item's artwork is already cached, or when there is no
    /// active session.
    pub(crate) fn center_large_artwork_load_task(&mut self, view: View) -> Option<Task<Message>> {
        self.app_service.as_ref()?;

        match view {
            View::Albums => {
                let total = self.library.albums.len();
                let center_idx = self
                    .albums_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let album_id = self.library.albums.get(center_idx)?.id.clone();
                if self.artwork.large_artwork.peek(&album_id).is_some() {
                    return None;
                }
                Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    album_id,
                ))))
            }
            View::Songs => {
                let total = self.library.songs.len();
                let center_idx = self
                    .songs_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let album_id = self.library.songs.get(center_idx)?.album_id.clone()?;
                if self.artwork.large_artwork.peek(&album_id).is_some() {
                    return None;
                }
                Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    album_id,
                ))))
            }
            View::Queue => {
                // queue/view.rs falls back to the centered song's artwork
                // whenever the playing song's isn't cached, so we always
                // want the centered slot's large artwork ready (matches
                // `load_queue_viewport_artwork`'s unconditional dispatch).
                let songs = self.filter_queue_songs();
                let total = songs.len();
                let center_idx = self
                    .queue_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let album_id = songs.get(center_idx)?.album_id.clone();
                if self.artwork.large_artwork.peek(&album_id).is_some() {
                    return None;
                }
                Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    album_id,
                ))))
            }
            View::Artists => {
                let total = self.library.artists.len();
                let center_idx = self
                    .artists_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let artist_id = self.library.artists.get(center_idx)?.id.clone();
                if self.artwork.large_artwork.peek(&artist_id).is_some() {
                    return None;
                }
                Some(self.handle_load_artist_large_artwork(artist_id))
            }
            // Shared with the `RadiosAction::None` browse arm so both center-load
            // paths agree on which station to load (see the helper).
            View::Radios => self.radio_center_large_load_task(),
            View::Harbour
            | View::Genres
            | View::Playlists
            | View::Settings
            | View::PlaylistEditor => None,
        }
    }

    /// Load task for the centered Radios station's large logo — the single
    /// source of truth shared by the view-enter/resize prefetch
    /// ([`Self::center_large_artwork_load_task`]) and the `RadiosAction::None`
    /// browse arm in `handle_radios`. Uses the EFFECTIVE center
    /// (`common.get_center_item_index`, which honors the click-focus
    /// `selected_offset`) so the prefetch targets the station the panel actually
    /// shows — not the raw viewport center the two paths previously disagreed on.
    /// Returns `None` when there's no session, the centered station has no
    /// uploaded logo (logo-less stations get their large panel from the ICY
    /// capture), or its large art is already cached.
    pub(crate) fn radio_center_large_load_task(&self) -> Option<Task<Message>> {
        self.app_service.as_ref()?;
        let stations = self.filter_radio_stations();
        let center_idx = self
            .radios_page
            .common
            .get_center_item_index(stations.len())?;
        let station = stations.get(center_idx)?;
        station.logo_cover_art()?;
        let station_id = station.id.clone();
        if self.artwork.radio_large_art.peek(&station_id).is_some() {
            return None;
        }
        Some(Task::done(Message::Artwork(
            ArtworkMessage::LoadRadioLarge(station_id),
        )))
    }

    /// Load mini artist artwork from disk cache for all prefetch-visible slots.
    ///
    /// Dispatch async fetches for any uncached artist mini artwork in the
    /// current viewport. Returns a batch of tasks producing
    /// `ArtworkMessage::Loaded`.
    ///
    /// Shared by: `handle_artists_loaded`, `handle_artists` (slot list change),
    /// and `prefetch_viewport_artwork` (window resize / view switch).
    pub(crate) fn prefetch_artist_mini_artwork_tasks(&self) -> Task<Message> {
        let total = self.library.artists.len();
        if total == 0 {
            return Task::none();
        }
        let albums_vm = match self.app_service.as_ref() {
            Some(svc) => svc.albums().clone(),
            None => return Task::none(),
        };

        let mut tasks = Vec::new();
        for idx in self.artists_page.common.slot_list.prefetch_indices(total) {
            if let Some(artist) = self.library.artists.get(idx)
                && !self.artwork.album_art.contains(&artist.id)
                && !self.artwork.art_failed_at(&artist.id, &None)
            {
                let id = artist.id.clone();
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
                    |(id, art)| {
                        // Artist art has no album `updated_at`; the id-only
                        // contains() gate above already dedups, so record None.
                        Message::Artwork(crate::app_message::ArtworkMessage::Loaded(id, None, art))
                    },
                ));
            }
        }
        Task::batch(tasks)
    }
}
