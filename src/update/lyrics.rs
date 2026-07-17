//! Synced-lyrics resolve pipeline: debounced dispatch on song change, the
//! stale-guarded application of async results, and the next-track prefetch.
//!
//! The chain itself (store → `getLyricsBySongId` → LRCLIB) lives on
//! `AppService::resolve_lyrics`; these handlers only decide *when* to run it
//! and land its results. Two hot-path guards keep the network honest:
//! a ~450 ms debounce (a skip storm never sends per-skip requests — the
//! epoch/id guard drops superseded windows before dispatch) and a Queue-view
//! gate (lyrics render only on Queue, so no off-surface third-party traffic).

use std::sync::Arc;

use iced::Task;
use nokkvi_data::{services::lyrics_source::ResolveOpts, types::lyrics::LyricsIndex};

use crate::{
    Nokkvi, View,
    app_message::{LyricsLoaderMessage, Message},
};

/// Debounce window between a song change and the cold-path resolve dispatch
/// (mirrors Feishin's 500 ms song-change debounce).
const LYRICS_RESOLVE_DEBOUNCE_MS: u64 = 450;

/// Glide duration for a one-line advance (crossfade-tier settle).
pub(crate) const LYRICS_SETTLE_MS: u32 = 600;
/// Floor so a capped glide still reads as motion, not a flicker.
pub(crate) const LYRICS_MIN_GLIDE_MS: u32 = 80;
/// Index distance beyond which a retarget snaps instead of gliding — a jump of
/// more than this many slots is seek-sized. (Index-based deliberately: time
/// thresholds false-fire on this corpus, where 38% of ordinary line gaps
/// exceed 5s — ambient gaps are normal, not seeks.)
pub(crate) const LYRICS_SNAP_INDEX_DELTA: usize = 4;

/// Glide duration for a retarget to `new_active`: the settle time, capped
/// below the gap to the NEXT line so fast passages finish each glide before
/// the next line lands (no perpetual smear on dense lyrics).
pub(crate) fn lyrics_glide_duration(
    lines: &[nokkvi_data::types::lyrics::LrcLine],
    new_active: usize,
) -> u32 {
    let gap_to_next = lines
        .get(new_active + 1)
        .zip(lines.get(new_active))
        .map_or(u32::MAX, |(next, current)| {
            next.time_ms.saturating_sub(current.time_ms)
        });
    LYRICS_SETTLE_MS
        .min(gap_to_next.saturating_mul(4) / 5)
        .max(LYRICS_MIN_GLIDE_MS)
}

impl Nokkvi {
    /// The lyrics-store index finished building at boot.
    pub(crate) fn handle_lyrics_index_ready(&mut self, index: Arc<LyricsIndex>) -> Task<Message> {
        self.lyrics.index = Some(index);
        // Re-drive the current song if it played before the index landed
        // (e.g. session-resume playback beat the boot-time build).
        self.lyrics_kick_if_unresolved()
    }

    /// Dispatch a resolve for the current song when lyrics are enabled, the
    /// Queue view is showing, and the current track isn't already resolved.
    /// Used by the index-ready re-drive and the enter-Queue hook.
    pub(crate) fn lyrics_kick_if_unresolved(&mut self) -> Task<Message> {
        if self.lyrics.enabled
            && self.current_view == View::Queue
            && self.active_playback.is_queue()
            && let Some(current) = self.scrobble.current_song_id.clone()
            && self.lyrics.matched_song_id.as_deref() != Some(current.as_str())
        {
            return self.dispatch_lyrics_resolve(current);
        }
        Task::none()
    }

    /// Timer for the post-song-change debounce window. Pure delay; the
    /// receiving handler re-validates identity + epoch before any dispatch.
    pub(crate) fn lyrics_debounce_task(song_id: String, epoch: u64) -> Task<Message> {
        Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(LYRICS_RESOLVE_DEBOUNCE_MS))
                    .await;
                (song_id, epoch)
            },
            |(song_id, epoch)| {
                Message::LyricsLoader(LyricsLoaderMessage::DebounceElapsed { song_id, epoch })
            },
        )
    }

    /// Gating options for the resolve chain, snapshotted at dispatch time.
    fn lyrics_resolve_opts(&self) -> ResolveOpts {
        ResolveOpts {
            songlyrics_ext: self.supports_song_lyrics(),
            // The user's privacy gate for the direct third-party LRCLIB
            // channel (the server channel is their own Navidrome and always
            // participates while lyrics are enabled).
            fetch_online: self.settings.lyrics_fetch_online,
        }
    }

    /// Run the resolve chain for `song_id` (assumed current). The result lands
    /// as `Loaded` under the stale guard; a `None` resolve arrives as an empty
    /// doc, which the apply path records as "resolved, no match".
    pub(crate) fn dispatch_lyrics_resolve(&mut self, song_id: String) -> Task<Message> {
        let epoch = self.lyrics.load_epoch;
        let index = self.lyrics.index.clone();
        let opts = self.lyrics_resolve_opts();
        let sid = song_id.clone();
        self.shell_task(
            move |shell| async move {
                let qm_arc = shell.queue().queue_manager();
                let song = {
                    let qm = qm_arc.lock().await;
                    qm.get_current_song()
                };
                match song {
                    Some(song) if song.id == sid => shell.resolve_lyrics(&song, index, opts).await,
                    _ => None,
                }
            },
            move |doc| {
                Message::LyricsLoader(LyricsLoaderMessage::Loaded {
                    song_id,
                    doc: Box::new(doc.unwrap_or_default()),
                    epoch,
                })
            },
        )
    }

    /// Prefetch the NEXT physical row's lyrics into `pending_next` so the
    /// transition promotes synchronously (no blank gap). Piggybacks the gapless
    /// prep edge (fires once per transition via `gapless_preparing`). Skipped in
    /// shuffle — the play order lives backend-side and the peek contract
    /// (`peek_next_song` → `transition`) is off-limits to side channels; the
    /// debounced cold path covers those transitions instead.
    pub(crate) fn lyrics_prefetch_next_task(&self) -> Task<Message> {
        if !self.lyrics.enabled
            || self.current_view != View::Queue
            || self.modes.random
            || self.lyrics.pending_next.is_some()
        {
            return Task::none();
        }
        let Some(current_id) = self.scrobble.current_song_id.clone() else {
            return Task::none();
        };
        let index = self.lyrics.index.clone();
        let opts = self.lyrics_resolve_opts();
        self.shell_task(
            move |shell| async move {
                let qm_arc = shell.queue().queue_manager();
                let next = {
                    let qm = qm_arc.lock().await;
                    qm.current_index()
                        .and_then(|idx| qm.rows().get(idx + 1).map(|row| row.song_id.clone()))
                        .and_then(|id| qm.get_song(&id).cloned())
                };
                match next {
                    Some(song) if song.id != current_id => {
                        let id = song.id.clone();
                        shell
                            .resolve_lyrics(&song, index, opts)
                            .await
                            .map(|doc| (id, doc))
                    }
                    _ => None,
                }
            },
            |result| match result {
                Some((song_id, doc)) => {
                    Message::LyricsLoader(LyricsLoaderMessage::PrefetchLoaded {
                        song_id,
                        doc: Box::new(doc),
                    })
                }
                None => Message::NoOp,
            },
        )
    }

    /// Land pipeline results under the stale-load guard.
    pub(crate) fn handle_lyrics_loader(&mut self, msg: LyricsLoaderMessage) -> Task<Message> {
        match msg {
            LyricsLoaderMessage::DebounceElapsed { song_id, epoch } => {
                if self.lyrics.enabled
                    && epoch == self.lyrics.load_epoch
                    && self.scrobble.current_song_id.as_deref() == Some(song_id.as_str())
                {
                    self.dispatch_lyrics_resolve(song_id)
                } else {
                    Task::none()
                }
            }
            LyricsLoaderMessage::Loaded {
                song_id,
                doc,
                epoch,
            } => {
                // Apply iff this resolve is still for the current track AND no
                // clear/promote superseded it while it was in flight.
                if epoch == self.lyrics.load_epoch
                    && self.scrobble.current_song_id.as_deref() == Some(song_id.as_str())
                {
                    // An unsynced/empty doc is a no-match: record the identity
                    // (so nothing re-fires for this track) with an empty doc,
                    // which renders as the empty state. Nothing is faked.
                    self.lyrics.doc = if doc.synced && !doc.lines.is_empty() {
                        *doc
                    } else {
                        Default::default()
                    };
                    self.lyrics.matched_song_id = Some(song_id);
                    self.lyrics.active_index = crate::state::active_line_at(
                        &self.lyrics.doc.lines,
                        self.lyrics.position_ms,
                    );
                    // A doc landing mid-track snaps straight to its line — the
                    // user hasn't watched the column move yet, so there is
                    // nothing to glide from.
                    if let Some(active) = self.lyrics.active_index {
                        self.lyrics.retarget_scroll(active, active as f32, 0);
                    }
                }
                Task::none()
            }
            LyricsLoaderMessage::PrefetchLoaded { song_id, doc } => {
                // Park only a real synced doc, and only for a track that isn't
                // already current (a late prefetch for the now-playing track is
                // useless — the cold path already handled it).
                if doc.synced
                    && !doc.lines.is_empty()
                    && self.scrobble.current_song_id.as_deref() != Some(song_id.as_str())
                {
                    self.lyrics.pending_next = Some((song_id, *doc));
                }
                Task::none()
            }
        }
    }
}

/// Working-image cap for the cover blur: the source downscales to at most
/// this before blurring (blur destroys the detail a bigger raster would
/// preserve, and the GPU upscale of a blurred image is visually lossless —
/// this keeps the CPU pass in the low tens of milliseconds even for
/// Original-resolution covers).
const LYRICS_BLUR_MAX_DIM: u32 = 800;

/// Decode → downscale → blur → RGBA handle. Runs on a blocking thread.
/// `None` = the bytes failed to decode (negative-cached by the caller).
fn blur_cover_bytes(bytes: &[u8], sigma: f32) -> Option<iced::widget::image::Handle> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.thumbnail(LYRICS_BLUR_MAX_DIM, LYRICS_BLUR_MAX_DIM);
    let blurred = img.fast_blur(sigma).to_rgba8();
    let (w, h) = blurred.dimensions();
    Some(iced::widget::image::Handle::from_rgba(
        w,
        h,
        blurred.into_raw(),
    ))
}

impl Nokkvi {
    /// Kick the cover-blur job for the lyrics backdrop when every gate holds:
    /// lyrics showing on the Queue, a non-`Off` blur level, the playing
    /// track's large cover cached as bytes, and no fresh blur / in-flight job
    /// for this exact `(album, source, level)`. Called from the 100 ms
    /// playback tick — idempotent, cheap when gated out.
    pub(crate) fn lyrics_blur_task(&mut self) -> Option<Task<Message>> {
        let level = self.settings.lyrics_backdrop_blur;
        let sigma = level.sigma()?;
        if !self.lyrics.enabled || self.current_view != View::Queue {
            return None;
        }
        let album_id = self.current_queue_song_album_id()?.to_string();
        let source = self.artwork.large_artwork.snapshot.get(&album_id)?;
        let source_id = source.id();
        if let Some(cached) = &self.artwork.lyrics_blurred
            && cached.album_id == album_id
            && cached.source_id == source_id
            && cached.level == level
        {
            // Fresh — or negative-cached (handle None) for this exact source.
            return None;
        }
        if self.artwork.lyrics_blur_pending.as_ref() == Some(&(album_id.clone(), level)) {
            return None;
        }
        // Only byte-backed handles can be re-decoded (every fetched cover is;
        // `from_rgba`/path handles have no compressed source to blur).
        let iced::widget::image::Handle::Bytes(_, bytes) = source else {
            return None;
        };
        let bytes = bytes.clone();
        self.artwork.lyrics_blur_pending = Some((album_id.clone(), level));
        Some(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || blur_cover_bytes(&bytes, sigma))
                    .await
                    .ok()
                    .flatten()
            },
            move |handle| {
                Message::Artwork(crate::app_message::ArtworkMessage::LyricsBlurReady(
                    album_id.clone(),
                    level,
                    source_id,
                    handle,
                ))
            },
        ))
    }

    /// Store the finished blur (or its decode failure) and release the
    /// in-flight guard. A stale result — one matching neither the live
    /// pending job nor the current track — is dropped rather than stored, so
    /// a slow job from a skipped-past track can't overwrite a fresher cache
    /// entry (the view resolver additionally re-checks `(album, source,
    /// level)` on read).
    pub(crate) fn handle_lyrics_blur_ready(
        &mut self,
        album_id: String,
        level: nokkvi_data::types::player_settings::LyricsBackdropBlur,
        source_id: iced::advanced::image::Id,
        handle: Option<iced::widget::image::Handle>,
    ) -> Task<Message> {
        let matches_pending =
            self.artwork.lyrics_blur_pending.as_ref() == Some(&(album_id.clone(), level));
        if matches_pending {
            self.artwork.lyrics_blur_pending = None;
        }
        if matches_pending || self.current_queue_song_album_id() == Some(album_id.as_str()) {
            self.artwork.lyrics_blurred = Some(crate::state::LyricsBlurredCover {
                album_id,
                source_id,
                level,
                handle,
            });
        }
        Task::none()
    }
}
