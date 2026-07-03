//! Radio-station artwork handlers.
//!
//! Three sources, resolved by gating rather than a runtime precedence check
//! (so the view just reads `radio_art` / `radio_large_art` and falls back to
//! the radio-tower glyph):
//!
//! - **Uploaded logo** — fetched via `getCoverArt?id=<coverArt token>` only when
//!   [`RadioStation::logo_cover_art`] is present. A logo-less station is never
//!   requested, so Navidrome's generic `album-placeholder.webp` is never pulled.
//! - **Live now-playing stream (ICY) art** — captured while a station plays;
//!   always feeds the large now-playing panel.
//! - **Remembered stream art** — for a logo-LESS station, the last-played ICY
//!   image becomes the idle row thumbnail and is persisted to the on-disk
//!   `RadioArtStore` in [`Nokkvi::handle_radio_icy_art_loaded`] (the playback
//!   tick triggers the capture; `main` hydrates it back at launch).
//!
//! [`RadioStation::logo_cover_art`]: nokkvi_data::types::radio_station::RadioStation::logo_cover_art

use iced::{Task, widget::image};
use nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE;

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, CustomArtworkOutcome, Message, MiniArt},
    update::components::custom_artwork,
};

impl Nokkvi {
    /// Prefetch mini station LOGOS for the Radios viewport. Only stations whose
    /// OpenSubsonic `coverArt` token is present (a real admin-uploaded logo) are
    /// fetched; logo-less stations keep the tower glyph and are never requested.
    pub(crate) fn prefetch_radio_logo_tasks(&self) -> Task<Message> {
        let stations = self.filter_radio_stations();
        let total = stations.len();
        if total == 0 {
            return Task::none();
        }
        let albums_vm = match self.app_service.as_ref() {
            Some(svc) => svc.albums().clone(),
            None => return Task::none(),
        };

        let mut tasks = Vec::new();
        for idx in self.radios_page.common.slot_list.prefetch_indices(total) {
            let Some(station) = stations.get(idx) else {
                continue;
            };
            // Gate on a real logo token; never synthesize a bare `ra-<id>`.
            let Some(token) = station.logo_cover_art() else {
                continue;
            };
            if self.artwork.radio_art.contains(&station.id) {
                continue;
            }
            let station_id = station.id.clone();
            let art_id = token.to_string();
            let vm = albums_vm.clone();
            tasks.push(Task::perform(
                async move {
                    let art = MiniArt::from_fetch(
                        vm.fetch_album_artwork(&art_id, Some(THUMBNAIL_SIZE), None)
                            .await,
                    );
                    (station_id, art)
                },
                |(station_id, art)| {
                    Message::Artwork(ArtworkMessage::RadioArtLoaded(station_id, art))
                },
            ));
        }
        Task::batch(tasks)
    }

    /// Store a fetched mini station logo into `radio_art`.
    pub(crate) fn handle_radio_art_loaded(
        &mut self,
        station_id: String,
        art: MiniArt,
    ) -> Task<Message> {
        // Logo fetches are gated on a present `coverArt` token, so a miss is
        // rare; drop `Missing`/`Transient` and let the tower glyph stand (a
        // later prefetch re-attempts). No negative cache for radio art.
        if let MiniArt::Loaded(h) = art {
            self.artwork.radio_art.put(station_id, h);
        }
        Task::none()
    }

    /// Fetch the resolution-sized station LOGO for the centered station.
    /// Mirrors [`Nokkvi::handle_load_large_artwork`] for albums.
    pub(crate) fn handle_load_radio_large(&mut self, station_id: String) -> Task<Message> {
        // Serve from cache for instant back-navigation.
        if let Some(handle) = self.artwork.radio_large_art.peek(&station_id).cloned() {
            return Task::done(Message::Artwork(ArtworkMessage::RadioLargeLoaded(
                station_id,
                Some(handle),
            )));
        }
        // Only logo stations have a large logo to fetch; logo-less stations get
        // their large panel from the now-playing ICY capture instead.
        let Some(token) = self
            .library
            .radio_stations
            .iter()
            .find(|s| s.id == station_id)
            .and_then(|s| s.logo_cover_art())
            .map(str::to_string)
        else {
            return Task::none();
        };
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let artwork_size = self.settings.artwork_resolution.to_size();
        Task::perform(
            async move {
                let bytes = albums_vm
                    .fetch_album_artwork(&token, artwork_size, None)
                    .await
                    .ok();
                (station_id, bytes.map(image::Handle::from_bytes))
            },
            |(station_id, handle)| {
                Message::Artwork(ArtworkMessage::RadioLargeLoaded(station_id, handle))
            },
        )
    }

    /// Store a fetched large station logo into `radio_large_art`.
    pub(crate) fn handle_radio_large_loaded(
        &mut self,
        station_id: String,
        handle: Option<image::Handle>,
    ) -> Task<Message> {
        if let Some(h) = handle {
            self.artwork.radio_large_art.put(station_id, h);
        }
        Task::none()
    }

    /// Ensure the now-playing LOGO station's large logo is loaded so the
    /// MiniPlayer / player-bar shows it even when the station never sat in the
    /// Radios viewport — reached via the next/prev-station hotkey, MPRIS, or a
    /// session restore. Logo-LESS stations get their art from the ICY capture,
    /// so this is a no-op for them (and when the logo is already cached or
    /// there's no session). The MiniPlayer reads `radio_large_art` first, so the
    /// large logo covers it; the row mini is filled by the viewport prefetch
    /// when the station scrolls into view. [code-review finding 1]
    pub(crate) fn ensure_playing_radio_logo_task(&self) -> Task<Message> {
        if self.app_service.is_none() {
            return Task::none();
        }
        let Some(station) = self.active_playback.radio_station() else {
            return Task::none();
        };
        if station.logo_cover_art().is_none() {
            return Task::none();
        }
        let station_id = station.id.clone();
        if self.artwork.radio_large_art.peek(&station_id).is_some() {
            return Task::none();
        }
        Task::done(Message::Artwork(ArtworkMessage::LoadRadioLarge(station_id)))
    }

    /// Remembered/now-playing stream (ICY) art for a logo-LESS station — its
    /// row thumbnail AND large panel — AND the on-disk persist.
    ///
    /// Persistence lives HERE rather than in the fire-and-forget fetch task so
    /// that the same guards which decide whether to *apply* the art also decide
    /// whether to *persist* it: a "Refresh Artwork" (or a newer track) that
    /// clears `radio_icy_captured` therefore suppresses a late in-flight capture
    /// from re-writing the cleared thumbnail to disk.
    ///
    /// `maybe_capture_radio_icy_art` only fetches for logo-less stations, so a
    /// station with an uploaded logo keeps that logo as its identity everywhere
    /// in-app; its live track art stays on MPRIS only.
    pub(crate) fn handle_radio_icy_art_loaded(
        &mut self,
        station_id: String,
        source_url: String,
        bytes: Option<Vec<u8>>,
    ) -> Task<Message> {
        // Drop a stale/cleared completion: a newer StreamUrl superseded this one
        // — or a "Refresh Artwork" removed it — in the dedup map
        // (artwork.radio_icy_captured). This gate guards BOTH the cache apply
        // and the disk persist below.
        if self
            .artwork
            .radio_icy_captured
            .get(&station_id)
            .map(String::as_str)
            != Some(source_url.as_str())
        {
            return Task::none();
        }
        let Some(bytes) = bytes else {
            return Task::none();
        };
        // Defense-in-depth: a station that gained an uploaded logo mid-session
        // (library re-fetched) keeps the logo — derive from the STABLE library,
        // never the transient active station.
        let has_logo = self
            .library
            .radio_stations
            .iter()
            .find(|s| s.id == station_id)
            .and_then(|s| s.logo_cover_art())
            .is_some();
        if has_logo {
            return Task::none();
        }
        let handle = image::Handle::from_bytes(bytes.clone());
        self.artwork
            .radio_large_art
            .put(station_id.clone(), handle.clone());
        self.artwork.radio_art.put(station_id.clone(), handle);

        // Persist the logo-less station's remembered thumbnail off-thread. The
        // dedup-match + logo-less gates above already passed, so a cleared or
        // superseded capture never reaches disk.
        if self.app_service.is_none() {
            return Task::none();
        }
        let url = source_url;
        self.shell_task(
            move |shell| async move {
                let (server_url, _cred) = shell.queue().get_server_config().await;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let store = nokkvi_data::services::radio_art_store::RadioArtStore::new(
                    shell.storage().clone(),
                );
                match store.put(&server_url, &station_id, &url, &bytes, now) {
                    Ok(()) => tracing::info!(
                        "radio art: persisted {} bytes for station {station_id}",
                        bytes.len()
                    ),
                    Err(e) => {
                        tracing::warn!("failed to persist radio art for {station_id}: {e}");
                    }
                }
            },
            |()| Message::NoOp,
        )
    }

    /// Capture the active station's live now-playing (ICY) stream art when its
    /// `StreamUrl` changes. The playback tick re-parses ICY metadata every
    /// ~100ms, so this dedups per-station against the last-captured URL and
    /// fires the external fetch only on a new track. On completion the bytes
    /// feed the large now-playing panel (always); for a logo-LESS station they
    /// also become the idle row thumbnail and are persisted to the on-disk
    /// `RadioArtStore` — both done in [`Self::handle_radio_icy_art_loaded`], not
    /// here, so the dedup-match guard suppresses a cleared/superseded write.
    ///
    /// Returns the fetch task to fold into the caller's batch, or `None` when
    /// there is nothing new to capture.
    pub(crate) fn maybe_capture_radio_icy_art(
        &mut self,
        url: Option<String>,
    ) -> Option<Task<Message>> {
        let url = url.filter(|u| !u.is_empty())?;
        // Only real http(s) image URLs — stations often put a homepage or junk
        // in StreamUrl, and fetching arbitrary schemes is not worth the risk.
        // (Private/loopback hosts are refused deeper, in fetch_external_image_capped.)
        // URL schemes are case-insensitive (RFC 3986 §3.1), so compare lowercased
        // — a spec-valid `HTTP://…` must not be silently dropped.
        let scheme = url.split_once("://").map(|(s, _)| s.to_ascii_lowercase());
        if !matches!(scheme.as_deref(), Some("http" | "https")) {
            return None;
        }
        let station_id = {
            let station = self.active_playback.radio_station()?;
            // A station with an uploaded logo keeps that logo as its identity
            // everywhere in-app; skip the external fetch + disk write entirely.
            if station.logo_cover_art().is_some() {
                return None;
            }
            station.id.clone()
        };

        // Dedup: skip when we already captured THIS url for THIS station.
        if self.artwork.radio_icy_captured.get(&station_id) == Some(&url) {
            return None;
        }

        // Bail BEFORE recording the dedup when there's no backend session (e.g.
        // mid session-resume / re-login): otherwise the url is permanently
        // marked captured but never fetched, so no retry fires once the session
        // is restored. Returning here leaves the record absent so the next tick
        // re-attempts. [code-review finding 8]
        self.app_service.as_ref()?;

        // Record the dedup BEFORE the fetch so the ~100ms tick can't storm
        // duplicate requests; a TRANSIENT failure is therefore not retried until
        // the StreamUrl changes (next track) — an accepted self-healing tradeoff
        // (a 10Hz retry would be worse than a brief missing thumbnail).
        self.artwork
            .radio_icy_captured
            .insert(station_id.clone(), url.clone());

        // Fetch only; the persist happens in `handle_radio_icy_art_loaded` after
        // the dedup-match + logo-less guards, so a cleared/superseded capture
        // never reaches disk. `Some(bytes)` on success, `None` on any failure.
        Some(self.shell_task(
            move |shell| async move {
                let bytes = shell.albums().fetch_external_image_capped(&url).await.ok();
                (station_id, url, bytes)
            },
            |(station_id, url, bytes)| {
                Message::Artwork(ArtworkMessage::RadioIcyArtLoaded(station_id, url, bytes))
            },
        ))
    }

    /// Hydrate remembered radio (ICY) artwork from the on-disk `RadioArtStore`
    /// into the in-RAM caches at launch, so logo-less stations show their
    /// last-played thumbnail immediately without a refetch. Seeds the ICY
    /// dedup map too, so an unchanged now-playing URL isn't re-fetched.
    pub(crate) fn hydrate_radio_art(&mut self) -> Task<Message> {
        if self.app_service.is_none() {
            return Task::none();
        }
        self.shell_task(
            move |shell| async move {
                let (server_url, _cred) = shell.queue().get_server_config().await;
                let store = nokkvi_data::services::radio_art_store::RadioArtStore::new(
                    shell.storage().clone(),
                );
                // Merge any pre-`_v2` blob forward (one-time) before loading.
                let (records, migrated) = store.load_migrating(&server_url);
                tracing::info!(
                    "radio art: hydrated {} remembered thumbnail(s) ({} migrated from legacy)",
                    records.len(),
                    migrated
                );
                records
                    .into_iter()
                    .map(|r| (r.station_id, r.source_url, r.bytes))
                    .collect::<Vec<_>>()
            },
            |records| Message::Artwork(ArtworkMessage::RadioArtHydrated(records)),
        )
    }

    /// Forget a station's remembered artwork (user "Refresh Artwork"): clear the
    /// in-memory caches + the on-disk record so it reverts to the tower glyph,
    /// then re-fetch the uploaded logo if the station has one (a logo-less
    /// station re-captures on its next play). Lets the user clear a stale or
    /// wrong thumbnail.
    pub(crate) fn handle_refresh_radio_station_artwork(
        &mut self,
        station: nokkvi_data::types::radio_station::RadioStation,
    ) -> Task<Message> {
        let station_id = station.id.clone();
        let remove_task = self.clear_radio_station_artwork_caches(&station_id);

        self.toast_info("Station artwork cleared");

        // A logo station re-fetches its uploaded logo now; a logo-less station
        // shows the tower glyph until it next plays and re-captures.
        if station.logo_cover_art().is_some() {
            Task::batch([
                remove_task,
                self.prefetch_radio_logo_tasks(),
                self.handle_load_radio_large(station_id),
            ])
        } else {
            remove_task
        }
    }

    /// FULL clear of a station's cached artwork identities: the in-memory
    /// row + panel handles, the on-disk `RadioArtStore` record, AND the ICY
    /// dedup record — dropping the dedup is what lets the next play
    /// re-capture stream art, which is exactly what "Refresh Artwork" and
    /// the custom-artwork RESET want. The custom-artwork SET path must NOT
    /// use this — see [`Self::clear_radio_station_art_handles`].
    pub(crate) fn clear_radio_station_artwork_caches(&mut self, station_id: &str) -> Task<Message> {
        self.artwork.radio_icy_captured.remove(station_id);
        self.clear_radio_station_art_handles(station_id)
    }

    /// Art handles + on-disk record only — KEEPS `radio_icy_captured`. The
    /// custom-artwork SET success path uses this: wiping the dedup record
    /// there would let the ~100ms playback tick immediately re-capture the
    /// stream's ICY now-playing art, racing the station-list reload and
    /// masking the just-uploaded logo for the rest of the session (worst
    /// case persisting the stream art to `RadioArtStore`). The returned task
    /// is the off-thread disk removal (or `Task::none()` pre-login).
    pub(crate) fn clear_radio_station_art_handles(&mut self, station_id: &str) -> Task<Message> {
        let key = station_id.to_string();
        self.artwork.radio_art.pop(&key);
        self.artwork.radio_large_art.pop(&key);

        if self.app_service.is_none() {
            return Task::none();
        }
        self.shell_task(
            move |shell| async move {
                let (server_url, _cred) = shell.queue().get_server_config().await;
                let store = nokkvi_data::services::radio_art_store::RadioArtStore::new(
                    shell.storage().clone(),
                );
                if let Err(e) = store.remove_station(&server_url, &key) {
                    tracing::warn!("failed to clear radio art for {key}: {e}");
                }
            },
            |()| Message::NoOp,
        )
    }

    /// "Set Custom Artwork…" on a radio station: open the native file picker,
    /// read the chosen image, and upload it to Navidrome's
    /// `POST /api/radio/{id}/image` — all inside one async task. The
    /// completion lands as [`ArtworkMessage::RadioCustomArtworkSet`], where
    /// [`Self::handle_radio_custom_artwork_set`] invalidates + reloads.
    pub(crate) fn handle_set_radio_station_artwork(
        &mut self,
        station: nokkvi_data::types::radio_station::RadioStation,
    ) -> Task<Message> {
        let station_id = station.id.clone();
        self.shell_task(
            move |shell| async move {
                let outcome = custom_artwork::pick_and_upload(|bytes, filename| async move {
                    shell
                        .radios_api()
                        .await?
                        .upload_image(&station_id, bytes, &filename)
                        .await
                })
                .await;
                (station, outcome)
            },
            |(station, outcome)| {
                Message::Artwork(ArtworkMessage::RadioCustomArtworkSet(station, outcome))
            },
        )
    }

    /// "Reset Artwork" on a radio station: `DELETE /api/radio/{id}/image`.
    /// Completion lands as [`ArtworkMessage::RadioCustomArtworkReset`].
    pub(crate) fn handle_reset_radio_station_artwork(
        &mut self,
        station: nokkvi_data::types::radio_station::RadioStation,
    ) -> Task<Message> {
        let station_id = station.id.clone();
        self.shell_task(
            move |shell| async move {
                let outcome = custom_artwork::outcome_from_result(
                    async { shell.radios_api().await?.delete_image(&station_id).await }.await,
                );
                (station, outcome)
            },
            |(station, outcome)| {
                Message::Artwork(ArtworkMessage::RadioCustomArtworkReset(station, outcome))
            },
        )
    }

    /// Completion of the radio "Set Custom Artwork…" upload. On success,
    /// invalidate every cached identity for the station and reload the
    /// station list — the fresh list carries the new `coverArt` token, and
    /// [`Self::handle_radio_stations_loaded`] re-warms the row logo + the
    /// panel (playing/centered station) from it.
    pub(crate) fn handle_radio_custom_artwork_set(
        &mut self,
        station: nokkvi_data::types::radio_station::RadioStation,
        outcome: CustomArtworkOutcome,
    ) -> Task<Message> {
        self.finish_radio_custom_artwork(
            station,
            outcome,
            "Artwork upload",
            // SET keeps the ICY dedup record — see clear_radio_station_art_handles.
            true,
            |name| format!("Custom artwork set for '{name}'"),
        )
    }

    /// Completion of the radio "Reset Artwork" delete. On success the same
    /// invalidate + reload runs; the reloaded station has no `coverArt`
    /// token, so the tower glyph stands until the next play re-captures the
    /// stream's ICY art.
    pub(crate) fn handle_radio_custom_artwork_reset(
        &mut self,
        station: nokkvi_data::types::radio_station::RadioStation,
        outcome: CustomArtworkOutcome,
    ) -> Task<Message> {
        self.finish_radio_custom_artwork(
            station,
            outcome,
            "Artwork reset",
            // RESET drops it — the next play SHOULD re-capture stream art.
            false,
            |name| format!("Artwork reset for '{name}' — automatic artwork returns"),
        )
    }

    /// Shared completion body for the radio Set/Reset flows: cancel is a
    /// silent no-op; failure maps 401 → session expiry and everything else →
    /// a friendly error toast (caches untouched — the server didn't change);
    /// success toasts, drops every cached identity, and reloads the station
    /// list so the fresh `coverArt` token drives the re-fetch.
    fn finish_radio_custom_artwork(
        &mut self,
        station: nokkvi_data::types::radio_station::RadioStation,
        outcome: CustomArtworkOutcome,
        action_label: &'static str,
        preserve_icy_dedup: bool,
        success_toast: impl FnOnce(&str) -> String,
    ) -> Task<Message> {
        match outcome {
            CustomArtworkOutcome::Cancelled => Task::none(),
            // LOCAL pick/read failure: plain toast, verbatim. Deliberately
            // bypasses the Unauthorized/Forbidden/400 classifiers — the
            // detail embeds the user-picked path (see CustomArtworkOutcome).
            CustomArtworkOutcome::LocalFailed(detail) => {
                tracing::error!(
                    "{action_label} failed locally for station {}: {detail}",
                    station.id
                );
                self.toast_error(format!("{action_label} failed: {detail}"));
                Task::none()
            }
            CustomArtworkOutcome::Failed(detail) => {
                if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&detail) {
                    return self.handle_session_expired();
                }
                tracing::error!("{action_label} failed for station {}: {detail}", station.id);
                self.toast_error(custom_artwork::custom_artwork_error_toast(
                    action_label,
                    &detail,
                ));
                Task::none()
            }
            CustomArtworkOutcome::Applied => {
                self.toast_success(success_toast(&station.name));
                let clear_task = if preserve_icy_dedup {
                    self.clear_radio_station_art_handles(&station.id)
                } else {
                    self.clear_radio_station_artwork_caches(&station.id)
                };
                Task::batch([clear_task, self.handle_load_radio_stations()])
            }
        }
    }

    /// Apply hydrated records (`station_id`, `source_url`, `bytes`) from the
    /// on-disk store: decode each into a `Handle` for the row thumbnail and
    /// seed the ICY dedup map so an unchanged now-playing URL isn't re-fetched.
    pub(crate) fn handle_radio_art_hydrated(
        &mut self,
        records: Vec<(String, String, Vec<u8>)>,
    ) -> Task<Message> {
        for (station_id, source_url, bytes) in records {
            // A station that now carries an uploaded logo must show that logo,
            // not a remembered stream thumbnail from when it was logo-less:
            // seeding `radio_art` here would make `prefetch_radio_logo_tasks`
            // skip the logo fetch (it gates on `radio_art.contains`). Leave logo
            // stations to the logo prefetch; the stale disk record clears on its
            // own (eviction / "Refresh Artwork").
            let has_logo = self
                .library
                .radio_stations
                .iter()
                .find(|s| s.id == station_id)
                .and_then(|s| s.logo_cover_art())
                .is_some();
            // Purely additive otherwise: never overwrite art already warmed this
            // session (a prefetched logo or a live now-playing capture both take
            // precedence over the stale on-disk thumbnail).
            if !has_logo && !self.artwork.radio_art.contains(&station_id) {
                self.artwork
                    .radio_art
                    .put(station_id.clone(), image::Handle::from_bytes(bytes));
            }
            self.artwork
                .radio_icy_captured
                .entry(station_id)
                .or_insert(source_url);
        }
        Task::none()
    }
}
