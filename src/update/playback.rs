//! Playback control handlers

use std::time::Duration;

use iced::Task;
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{Message, PlaybackMessage, ScrobbleMessage},
    views,
};

/// Bundled MPRIS state pushed to D-Bus on each playback tick.
struct MprisUpdate<'a> {
    playing: bool,
    paused: bool,
    album: &'a str,
    duration: u32,
    position: u32,
    art_url: Option<&'a str>,
    repeat: bool,
    repeat_queue: bool,
    random: bool,
}

impl Nokkvi {
    pub(crate) fn handle_tick(&mut self) -> Task<Message> {
        // Pre-login: nothing to poll. Returning early avoids `shell_task`
        // logging a "called before app_service initialized" warning every
        // 100ms while session resume is in flight at boot.
        if self.app_service.is_none() {
            return Task::none();
        }

        let radio_station = self.active_playback.radio_station().cloned();
        let icy_url = if let crate::state::ActivePlayback::Radio(ref state) = self.active_playback {
            state.icy_url.clone()
        } else {
            None
        };

        self.shell_task(
            move |shell| async move {
                let engine_arc = shell.audio_engine();
                let engine = engine_arc.lock().await;
                let pos = engine.position();
                let dur = engine.duration();
                let playing = engine.playing();
                let paused = engine.immediate_paused();
                let sample_rate = engine.sample_rate();
                // Live compressed bitrate from decoder (0 if not yet decoding)
                let engine_live_bitrate = engine.live_bitrate();
                let engine_live_icy_metadata = engine.live_icy_metadata();
                let engine_live_codec = engine.live_codec();
                drop(engine);

                let qm_arc = shell.queue().queue_manager();
                let qm = qm_arc.lock().await;
                let song = qm.get_current_song();
                let current_index = qm.get_queue().current_index;
                let (title, artist, album, cover_art, album_id, song_id, format_suffix, bitrate) =
                    if let Some(station) = &radio_station {
                        (
                            station.name.clone(),
                            String::new(), // Artist handles name sometimes? No, artist is empty
                            String::new(),
                            None,
                            None,
                            Some(station.id.clone()),
                            engine_live_codec.unwrap_or_else(|| "radio".to_string()),
                            engine_live_bitrate,
                        )
                    } else if let Some(s) = &song {
                        // Extract format suffix from file path (e.g., "flac" from "/path/to/song.flac")
                        let suffix = s
                            .path
                            .rsplit('.')
                            .next()
                            .map(|ext| ext.to_lowercase())
                            .unwrap_or_default();
                        // Prefer live decoder bitrate over static Navidrome API value
                        let br = if engine_live_bitrate > 0 {
                            engine_live_bitrate
                        } else {
                            s.bitrate.unwrap_or(0)
                        };
                        (
                            s.title.clone(),
                            s.artist.clone(),
                            s.album.clone(),
                            s.cover_art.clone(),
                            s.album_id.clone(),
                            Some(s.id.clone()),
                            suffix,
                            br,
                        )
                    } else {
                        (
                            "Not Playing".to_string(),
                            String::new(),
                            String::new(),
                            None,
                            None,
                            None,
                            String::new(),
                            0,
                        )
                    };
                drop(qm);

                // Build artwork URL for MPRIS - use cover_art if available, otherwise fall back to album_id
                // (Navidrome API songs don't include coverArt, but album art uses the album ID)
                let art_id = cover_art.as_ref().or(album_id.as_ref());
                let art_url = if let Some(cover_id) = art_id {
                    let (server_url, subsonic_credential) = shell.queue().get_server_config().await;
                    let url = nokkvi_data::utils::artwork_url::build_cover_art_url(
                        cover_id,
                        &server_url,
                        &subsonic_credential,
                        None, // Use default high-res size
                    );
                    if url.is_empty() { None } else { Some(url) }
                } else if radio_station.is_some() {
                    icy_url
                } else {
                    None
                };

                let (random, repeat, repeat_queue, consume) = shell.get_modes().await;

                // Convert from milliseconds to seconds for UI display
                // If engine duration is 0 (unknown/garbage), use song's metadata duration as fallback
                let dur_seconds = if dur == 0 {
                    // Fallback to song metadata duration
                    let qm = shell.queue().queue_manager();
                    let qm_guard = qm.lock().await;
                    qm_guard.get_current_song().map_or(0, |s| s.duration)
                } else {
                    (dur / 1000) as u32
                };

                crate::app_message::PlaybackStateUpdate {
                    position: (pos / 1000) as u32,
                    duration: dur_seconds,
                    playing,
                    paused,
                    title,
                    artist,
                    album,
                    art_url,
                    random,
                    repeat,
                    repeat_queue,
                    consume,
                    current_index,
                    song_id,
                    format_suffix,
                    sample_rate,
                    bitrate,
                    live_icy_metadata: engine_live_icy_metadata,
                }
            },
            |update| Message::Playback(PlaybackMessage::PlaybackStateUpdated(Box::new(update))),
        )
    }

    pub(crate) fn handle_playback_state_updated(
        &mut self,
        update: crate::app_message::PlaybackStateUpdate,
    ) -> Task<Message> {
        // Destructure the update struct for cleaner access
        let crate::app_message::PlaybackStateUpdate {
            position: pos,
            duration: dur,
            playing,
            paused,
            title,
            artist,
            album,
            art_url,
            random,
            repeat,
            repeat_queue,
            consume,
            current_index,
            song_id,
            format_suffix,
            sample_rate,
            bitrate,
            live_icy_metadata,
        } = update;

        // Detect transition from playing to stopped (not paused)
        // This happens when the last track in the queue finishes naturally
        let was_playing = self.playback.playing && !self.playback.paused;
        let is_stopped = !playing && !paused;
        let playback_stopped = was_playing && is_stopped;

        // Process radio metadata updates
        if let Some(icy_meta) = live_icy_metadata
            && self.active_playback.is_radio()
        {
            let mut extracted_title = String::new();
            let mut extracted_url = None;

            // Parse StreamTitle and StreamUrl from the raw ICY payload.
            // Example format: "StreamTitle='Artist - Song';StreamUrl='https://...';"
            for part in icy_meta.split("';") {
                let part = part.trim_end_matches('\0');
                if let Some(idx) = part.find("='") {
                    let key = &part[..idx];
                    let val = &part[idx + 2..];
                    if key == "StreamTitle" && !val.is_empty() {
                        extracted_title = val.to_string();
                    } else if key == "StreamUrl" && !val.is_empty() {
                        extracted_url = Some(val.to_string());
                    }
                }
            }

            // Split icy meta "Artist - Title" format. Not all stations follow this exactly,
            // but it's the standard convention used by majority of SHOUTcast/Icecast stations.
            let mut parts = extracted_title.splitn(2, " - ");
            let (artist, title) = if let (Some(artist), Some(title)) = (parts.next(), parts.next())
            {
                // It had a dash, treat as Artist - Title
                (
                    Some(artist.trim().to_string()),
                    Some(title.trim().to_string()),
                )
            } else {
                // No dash found, fallback: put everything in title
                (None, Some(extracted_title.trim().to_string()))
            };

            // Dispatch the metadata update directly
            // (Using handle_radio_metadata_update directly since we are already in the update fn)
            let _ = self.handle_radio_metadata_update(artist, title, extracted_url);
        }

        // Update playback and mode fields
        self.playback.position = pos;
        self.playback.duration = dur;
        self.playback.playing = playing;
        self.playback.paused = paused;
        self.playback.title = title;
        self.playback.artist = artist;
        self.playback.album = album.clone();
        self.playback.format_suffix = format_suffix;
        self.playback.sample_rate = sample_rate;
        self.playback.bitrate = bitrate;
        self.modes.random = random;
        self.modes.repeat = repeat;
        self.modes.repeat_queue = repeat_queue;
        self.modes.consume = consume;

        // Reset visualizer when playback stops (clears bars instead of freezing)
        if playback_stopped && let Some(ref viz) = self.visualizer {
            viz.reset();
        }

        let mut tasks: Vec<Task<Message>> = Vec::new();

        // Scrobble: song change detection + previous-song submission
        let song_changed = self.scrobble.current_song_id != song_id;

        // PipeWire stream description update
        let pw_title = if is_stopped || self.playback.title.is_empty() {
            "Nokkvi".to_string()
        } else if self.playback.artist.is_empty() {
            format!("Nokkvi ({})", self.playback.title)
        } else {
            format!(
                "Nokkvi ({} - {})",
                self.playback.title, self.playback.artist
            )
        };

        // PipeWire stream description update - detect title transitions to catch late-arriving metadata
        let emit_title_update = if playback_stopped {
            true
        } else {
            match &self.playback.pw_last_title {
                Some(last) => last != &pw_title,
                None => true,
            }
        };

        if emit_title_update {
            self.playback.pw_last_title = Some(pw_title.clone());
            self.sfx_engine.set_output_title(pw_title);
        }

        if self.active_playback.is_queue() {
            if song_changed {
                // Reset gapless flag BEFORE scrobble updates current_song_id.
                // Must happen here because consume mode can cause the queue index
                // to round-trip (0→1→0), making the index-based reset in
                // handle_queue_focus_change miss the song transition.
                self.engine.gapless_preparing = false;
                self.handle_scrobble_on_song_change(&song_id, pos, &mut tasks);
            }

            // Scrobble: track listening time (anti-seek-fraud)
            self.track_listening_time(playing, paused, &song_id, pos, dur, &mut tasks);

            // Queue focus tracking + gapless preparation
            self.handle_queue_focus_change(current_index, &mut tasks);
            if playing && !paused && dur > 0 {
                let threshold = (f64::from(dur) * 0.8) as u32;
                if pos >= threshold {
                    tasks.push(Task::done(Message::Playback(
                        PlaybackMessage::PrepareNextForGapless,
                    )));
                }
                // NOTE: crossfade triggering has moved to the renderer
                // (render_buffers queue-size check). The tick handler only
                // handles gapless preparation now.
            }
        }

        // MPRIS: push state to D-Bus
        self.push_mpris_state(MprisUpdate {
            playing,
            paused,
            album: &album,
            duration: dur,
            position: pos,
            art_url: art_url.as_deref(),
            repeat,
            repeat_queue,
            random,
        });

        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    /// Handle scrobble logic when a song change is detected.
    ///
    /// Submits the previous song if scrobble conditions were met, resets scrobble
    /// state for the new song, and starts the "now playing" debounce timer.
    /// Queue UI refresh for consume mode is handled separately by the
    /// queue_changed_subscription channel.
    fn handle_scrobble_on_song_change(
        &mut self,
        song_id: &Option<String>,
        pos: u32,
        tasks: &mut Vec<Task<Message>>,
    ) {
        // Scrobble previous song if conditions were met but not yet scrobbled
        if let Some(prev_song_id) = &self.scrobble.current_song_id
            && self
                .scrobble
                .should_scrobble(self.playback.duration, self.scrobble_threshold)
        {
            debug!(
                "📊 [SCROBBLE] Submitting previous song on change: {} (listened {:.0}s)",
                prev_song_id, self.scrobble.listening_time
            );
            tasks.push(Task::done(Message::Scrobble(ScrobbleMessage::Submit(
                prev_song_id.clone(),
            ))));
        }

        // Reset scrobble state for new song
        self.scrobble
            .reset_for_new_song(song_id.clone(), pos as f32);

        // Start 2-second debounce timer for "now playing"
        if let Some(sid) = song_id {
            self.scrobble.now_playing_timer_id += 1;
            let timer_id = self.scrobble.now_playing_timer_id;
            let sid_clone = sid.clone();
            debug!(
                "📊 [SCROBBLE] Song changed, starting now-playing timer (id={}) for: {}",
                timer_id, sid
            );
            tasks.push(Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    (timer_id, sid_clone)
                },
                |(timer_id, song_id)| {
                    Message::Scrobble(ScrobbleMessage::NowPlaying(timer_id, song_id))
                },
            ));
        }

        // CONSUME MODE: Queue UI refresh is handled by the queue_changed_subscription
        // channel, which fires AFTER the completion callback finishes consuming and
        // refreshing queue state. This eliminates the race where LoadQueue reads
        // stale pre-consume data.
    }

    /// Track listening time for scrobble anti-seek-fraud detection.
    ///
    /// Only counts forward progress in 0-10 second range (excludes seeks).
    /// Submits a scrobble when conditions are met mid-playback.
    ///
    /// Repeat-one loop handling is done by the engine-level `TrackLooped` signal
    /// (via `PlaybackController::looped_callback`), not here.
    fn track_listening_time(
        &mut self,
        playing: bool,
        paused: bool,
        song_id: &Option<String>,
        pos: u32,
        dur: u32,
        tasks: &mut Vec<Task<Message>>,
    ) {
        if !playing || paused || song_id.is_none() {
            return;
        }

        let current_pos = pos as f32;
        let delta = current_pos - self.scrobble.last_position;

        // Only count forward progress in reasonable range (0-10 seconds)
        // This excludes seeks (large jumps) and going backwards
        if delta > 0.0 && delta < 10.0 {
            self.scrobble.listening_time += delta;
        }
        self.scrobble.last_position = current_pos;

        // Check scrobble conditions (once per song)
        if self.scrobble.should_scrobble(dur, self.scrobble_threshold)
            && let Some(sid) = song_id
        {
            debug!(
                "📊 [SCROBBLE] Conditions met! Submitting: {} (listened {:.0}s / {} total)",
                sid, self.scrobble.listening_time, dur
            );
            self.scrobble.submitted = true;
            tasks.push(Task::done(Message::Scrobble(ScrobbleMessage::Submit(
                sid.clone(),
            ))));
        }
    }

    /// Handle queue focus changes when the current track index changes.
    ///
    /// Resets the gapless preparation flag and focuses the queue view on
    /// the currently playing song (by queue index for correct duplicate handling).
    ///
    /// Note: the primary gapless_preparing reset for consume mode lives in
    /// handle_playback_state_updated (song_changed block) because the consume
    /// index adjustment can cause the index to round-trip and appear unchanged.
    fn handle_queue_focus_change(
        &mut self,
        current_index: Option<usize>,
        tasks: &mut Vec<Task<Message>>,
    ) {
        let index_changed = self.last_queue_current_index != current_index;
        if !index_changed {
            return;
        }

        debug!(
            "🎯 [FOCUS] Index changed: {:?} -> {:?}",
            self.last_queue_current_index, current_index
        );
        self.last_queue_current_index = current_index;

        // Reset gapless preparation flag for the new track
        self.engine.gapless_preparing = false;

        // Use queue index for focus (not song_id) to correctly handle duplicate tracks
        if let Some(idx) = current_index
            && self.current_view == View::Queue
            && self.auto_follow_playing
        {
            debug!(
                "🎯 [FOCUS] Triggering FocusCurrentPlaying({}) with queue reload",
                idx
            );
            tasks.push(Task::done(Message::LoadQueue));
            // Suppress auto-scroll if this track change was triggered by a click-play
            // (the flag was set by the click handler). Otherwise, auto-follow the track.
            if self.suppress_next_auto_center {
                debug!("🎯 [FOCUS] Suppressing auto-center (click-initiated play)");
                self.suppress_next_auto_center = false;
            } else {
                tasks.push(Task::done(Message::Queue(
                    views::QueueMessage::FocusCurrentPlaying(idx, true),
                )));
            }
        }
    }

    /// Push playback state to MPRIS D-Bus interface.
    fn push_mpris_state(&mut self, u: MprisUpdate<'_>) {
        let Some(ref conn) = self.mpris_connection else {
            return;
        };

        // Determine playback status
        let status = if u.playing && !u.paused {
            mpris_server::PlaybackStatus::Playing
        } else if u.paused {
            mpris_server::PlaybackStatus::Paused
        } else {
            mpris_server::PlaybackStatus::Stopped
        };

        // Determine loop status for MPRIS
        let loop_status = if u.repeat {
            mpris_server::LoopStatus::Track
        } else if u.repeat_queue {
            mpris_server::LoopStatus::Playlist
        } else {
            mpris_server::LoopStatus::None
        };

        let duration_us = i64::from(u.duration) * 1_000_000;
        let position_us = i64::from(u.position) * 1_000_000;

        // Detect position discontinuities (seeks, song changes).
        // The tick interval is 100ms, so normal forward progress at 1x speed
        // produces ~100ms deltas. A jump of > 2 seconds indicates a seek or
        // song change — emit the Seeked D-Bus signal so desktop shells
        // immediately re-sync their progress bars.
        let delta_us = position_us - self.last_mpris_position_us;
        let discontinuity = delta_us.abs() > 2_000_000 || delta_us < -100_000;
        self.last_mpris_position_us = position_us;

        // For radio streams, override title/artist with ICY metadata so MPRIS
        // consumers see the actual artist/track instead of the station name.
        // self.playback.title/artist are set to station name / empty by handle_tick;
        // the richer ICY data lives in RadioPlaybackState.
        let (mpris_title, mpris_artist) =
            if let crate::state::ActivePlayback::Radio(ref state) = self.active_playback {
                let title = state.icy_title.as_deref().unwrap_or(&self.playback.title);
                let artist = state.icy_artist.as_deref().unwrap_or(&self.playback.artist);
                (title, artist)
            } else {
                (self.playback.title.as_str(), self.playback.artist.as_str())
            };

        // Push state via channel (synchronous, non-blocking)
        conn.set_playback_status(status);
        conn.set_metadata(mpris_title, mpris_artist, u.album, duration_us, u.art_url);
        conn.set_position(position_us);
        conn.set_loop_status(loop_status);
        conn.set_shuffle(u.random);

        // Emit Seeked signal on discontinuity so shells re-sync immediately
        if discontinuity && (u.playing || u.paused) {
            conn.seeked(position_us);
        }

        // Mirror Play/Pause label + tooltip title to the system tray.
        if let Some(ref tray) = self.tray_connection {
            let title = if mpris_title.is_empty() {
                "Nokkvi".to_string()
            } else if mpris_artist.is_empty() {
                mpris_title.to_string()
            } else {
                format!("{mpris_title} — {mpris_artist}")
            };
            tray.set_playing_state(u.playing && !u.paused, title);
        }
    }

    pub(crate) fn handle_toggle_play(&mut self) -> Task<Message> {
        // Nothing loaded and nothing in the queue — toast instead of silently failing.
        if !self.playback.playing && !self.playback.paused && self.library.queue_songs.is_empty() {
            self.toast_info("Queue is empty");
            return Task::none();
        }

        // Optimistic UI update: toggle play/pause immediately so buttons
        // don't flicker while waiting for the async engine call + tick roundtrip.
        if self.playback.playing && !self.playback.paused {
            self.playback.paused = true;
        } else {
            self.playback.playing = true;
            self.playback.paused = false;
        }
        self.shell_task(
            |shell| async move {
                let _ = shell.play_pause().await;
            },
            |_| Message::Playback(PlaybackMessage::Tick),
        )
    }

    pub(crate) fn handle_play(&mut self) -> Task<Message> {
        // If a track is already loaded (playing or paused), just resume via play().
        // This covers the pause→play case.
        if self.playback.has_track() {
            self.playback.playing = true;
            self.playback.paused = false;
            return self.shell_task(
                |shell| async move {
                    let _ = shell.play().await;
                },
                |_| Message::Playback(PlaybackMessage::Tick),
            );
        }

        // Nothing loaded — cold start. Use the same path as Enter-key:
        // play_song_from_queue() with the selected (or first) queue item.
        // This ensures all metadata, artwork, and navigator wiring fires correctly.
        let selected_index = self
            .queue_page
            .common
            .slot_list
            .get_center_item_index(self.library.queue_songs.len())
            .unwrap_or(0);
        let selected_song = self.library.queue_songs.get(selected_index).map(|s| {
            let queue_index = s.track_number as usize - 1;
            (s.id.clone(), queue_index)
        });

        if let Some((song_id, queue_index)) = selected_song {
            // Optimistic UI update
            self.playback.playing = true;
            self.playback.paused = false;
            self.queue_page.common.slot_list.flash_center();
            self.suppress_next_auto_center = true;
            return self.shell_task(
                move |shell| async move { shell.play_song_from_queue(&song_id, queue_index).await },
                |result| match result {
                    Ok(()) => Message::Playback(PlaybackMessage::Tick),
                    Err(e) => {
                        tracing::error!(" Play button cold-start failed: {}", e);
                        Message::Toast(crate::app_message::ToastMessage::Push(
                            nokkvi_data::types::toast::Toast::new(
                                format!("Failed to start playback: {e}"),
                                nokkvi_data::types::toast::ToastLevel::Error,
                            ),
                        ))
                    }
                },
            );
        }

        // Queue is empty — toast instead of silently failing
        self.toast_info("Queue is empty");
        Task::none()
    }

    pub(crate) fn handle_pause(&mut self) -> Task<Message> {
        // Optimistic UI update: show "paused" immediately
        self.playback.paused = true;
        self.shell_task(
            |shell| async move {
                let _ = shell.pause().await;
            },
            |_| Message::Playback(PlaybackMessage::Tick),
        )
    }

    pub(crate) fn handle_stop(&mut self) -> Task<Message> {
        // Optimistic UI update: show "stopped" immediately
        self.playback.playing = false;
        self.playback.paused = false;
        // Reset visualizer to clear bars (stop should clear, not freeze like pause)
        if let Some(ref viz) = self.visualizer {
            viz.reset();
        }
        self.shell_task(
            |shell| async move {
                let _ = shell.stop().await;
            },
            |_| Message::Playback(PlaybackMessage::Tick),
        )
    }

    pub(crate) fn handle_next_track(&mut self) -> Task<Message> {
        if self.active_playback.is_radio() {
            return self.cycle_radio_station(true);
        }
        // NOTE: We intentionally do NOT reset the visualizer here.
        // The auto-sensitivity naturally adapts between tracks. Resetting it
        // causes a 2-4 second delay while it recalibrates to the new track's volume.
        // Only reset visualizer on stop/queue clear, not on track changes.
        let is_consume = self.modes.consume;
        self.shell_task(
            move |shell| async move {
                let advanced = shell.next().await.unwrap_or(false);
                (advanced, is_consume)
            },
            |(advanced, consume)| {
                if !advanced {
                    // End of queue — show toast (works for both button and MPRIS)
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            "No next track",
                            nokkvi_data::types::toast::ToastLevel::Info,
                        ),
                    ))
                } else if consume {
                    Message::LoadQueue
                } else {
                    Message::Playback(PlaybackMessage::Tick)
                }
            },
        )
    }

    pub(crate) fn handle_prev_track(&mut self) -> Task<Message> {
        if self.active_playback.is_radio() {
            return self.cycle_radio_station(false);
        }
        // NOTE: We intentionally do NOT reset the visualizer here.
        // The auto-sensitivity naturally adapts between tracks. Resetting it
        // causes a 2-4 second delay while it recalibrates to the new track's volume.
        // Only reset visualizer on stop/queue clear, not on track changes.
        let is_consume = self.modes.consume;
        self.shell_task(
            move |shell| async move {
                let _ = shell.previous().await;
                is_consume
            },
            |consume| {
                if consume {
                    Message::LoadQueue
                } else {
                    Message::Playback(PlaybackMessage::Tick)
                }
            },
        )
    }

    /// Cycles through the library's radio stations in order and begins playback.
    fn cycle_radio_station(&mut self, forward: bool) -> Task<Message> {
        if let crate::state::ActivePlayback::Radio(ref state) = self.active_playback {
            if self.library.radio_stations.is_empty() {
                return Task::none();
            }

            let current_id = &state.station.id;
            let current_idx = self
                .library
                .radio_stations
                .iter()
                .position(|s| s.id == *current_id)
                .unwrap_or(0);

            let new_idx = if forward {
                (current_idx + 1) % self.library.radio_stations.len()
            } else {
                if current_idx == 0 {
                    self.library.radio_stations.len() - 1
                } else {
                    current_idx - 1
                }
            };

            let next_station = self.library.radio_stations[new_idx].clone();

            self.active_playback =
                crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
                    station: next_station.clone(),
                    icy_artist: None,
                    icy_title: None,
                    icy_url: None,
                });

            let stream_url = next_station.stream_url;
            let station_id = next_station.id.clone();

            // Optimistic UI toggle immediately
            self.playback.playing = true;
            self.playback.paused = false;

            let mut tasks = vec![];

            // Auto follow for Radios page
            if self.current_view == crate::View::Radios && self.auto_follow_playing {
                tasks.push(Task::done(Message::Radios(
                    crate::views::RadiosMessage::FocusCurrentPlaying(station_id),
                )));
            }

            tasks.push(self.shell_action_task(
                move |shell| async move {
                    shell.playback().stop().await?;
                    let engine_arc = shell.playback().audio_engine();
                    let mut engine = engine_arc.lock().await;
                    engine.set_source(stream_url).await;
                    engine.play().await?;
                    Ok(())
                },
                Message::Playback(PlaybackMessage::Tick),
                "cycle radio station",
            ));

            return Task::batch(tasks);
        }
        Task::none()
    }

    pub(crate) fn handle_toggle_random(&mut self) -> Task<Message> {
        // Optimistic UI update: toggle immediately so the button doesn't flicker
        // while waiting for the async API response. The tick handler reconciles
        // with server groundtruth every cycle.
        self.modes.random = !self.modes.random;
        self.shell_task(
            |shell| async move { shell.toggle_random().await.unwrap_or(false) },
            |r| Message::Playback(PlaybackMessage::RandomToggled(r)),
        )
    }

    pub(crate) fn handle_random_toggled(&mut self, random: bool) -> Task<Message> {
        self.modes.random = random;
        // Allow gapless prep to re-trigger with the new shuffled/unshuffled order
        self.engine.gapless_preparing = false;
        Task::none()
    }

    pub(crate) fn handle_toggle_repeat(&mut self) -> Task<Message> {
        // Optimistic UI update: cycle through repeat modes immediately so the
        // button doesn't flicker while waiting for the async API response.
        // Cycle: off -> repeat_one -> repeat_queue -> off
        let (new_repeat, new_repeat_queue) = match (self.modes.repeat, self.modes.repeat_queue) {
            (false, false) => (true, false), // off -> repeat one
            (true, false) => (false, true),  // repeat one -> repeat queue
            (false, true) => (false, false), // repeat queue -> off
            _ => (false, false),             // invalid state -> off
        };
        self.modes.repeat = new_repeat;
        self.modes.repeat_queue = new_repeat_queue;
        self.shell_task(
            |shell| async move { shell.cycle_repeat().await.unwrap_or((false, false)) },
            |(r, rq)| Message::Playback(PlaybackMessage::RepeatToggled(r, rq)),
        )
    }

    pub(crate) fn handle_repeat_toggled(
        &mut self,
        repeat: bool,
        repeat_queue: bool,
    ) -> Task<Message> {
        self.modes.repeat = repeat;
        self.modes.repeat_queue = repeat_queue;
        // Allow gapless prep to re-trigger with the new repeat mode
        self.engine.gapless_preparing = false;
        Task::none()
    }

    pub(crate) fn handle_toggle_consume(&mut self) -> Task<Message> {
        // Optimistic UI update: toggle immediately so the button doesn't flicker
        // while waiting for the async API response. The tick handler reconciles
        // with server groundtruth every cycle.
        self.modes.consume = !self.modes.consume;
        self.shell_task(
            |shell| async move { shell.toggle_consume().await.unwrap_or(false) },
            |c| Message::Playback(PlaybackMessage::ConsumeToggled(c)),
        )
    }

    pub(crate) fn handle_consume_toggled(&mut self, consume: bool) -> Task<Message> {
        self.modes.consume = consume;
        // Allow gapless prep to re-trigger with the new consume setting
        self.engine.gapless_preparing = false;
        Task::none()
    }

    pub(crate) fn handle_toggle_sound_effects(&mut self) -> Task<Message> {
        self.sfx.enabled = !self.sfx.enabled;
        self.sfx_engine.set_enabled(self.sfx.enabled);

        // Persist to storage
        let enabled = self.sfx.enabled;
        self.shell_spawn("persist_sfx_enabled", move |shell| async move {
            shell.settings().set_sound_effects_enabled(enabled).await
        });
        Task::none()
    }

    pub(crate) fn handle_sfx_volume_changed(&mut self, vol: f32) -> Task<Message> {
        self.sfx.volume = vol.clamp(0.0, 1.0);
        self.sfx_engine.set_volume(self.sfx.volume);
        Self::push_volume_toast(&mut self.toast, "SFX Volume", self.sfx.volume);

        // Persist to storage
        let vol = self.sfx.volume;
        self.shell_spawn("persist_sfx_volume", move |shell| async move {
            shell.settings().set_sfx_volume(vol).await
        });

        Task::none()
    }

    pub(crate) fn handle_cycle_visualization(&mut self) -> Task<Message> {
        self.engine.visualization_mode = self.engine.visualization_mode.next();

        // Persist to storage
        let mode = self.engine.visualization_mode;
        self.shell_spawn("persist_vis_mode", move |shell| async move {
            shell.settings().set_visualization_mode(mode).await
        });
        Task::none()
    }

    pub(crate) fn handle_toggle_crossfade(&mut self) -> Task<Message> {
        self.engine.crossfade_enabled = !self.engine.crossfade_enabled;

        // Persist to storage and sync to audio engine
        let enabled = self.engine.crossfade_enabled;
        self.shell_spawn("persist_crossfade_toggle", move |shell| async move {
            shell.settings().set_crossfade_enabled(enabled).await?;
            let engine_arc = shell.audio_engine();
            let mut engine = engine_arc.lock().await;
            engine.set_crossfade_enabled(enabled);
            Ok(())
        });
        Task::none()
    }

    pub(crate) fn handle_seek(&mut self, val: f32) -> Task<Message> {
        if self.active_playback.is_radio() {
            return Task::none();
        }
        // Slider sends position in seconds, shell.seek expects seconds
        let pos_secs = f64::from(val);
        self.shell_task(
            move |shell| async move {
                let _ = shell.seek(pos_secs).await;
            },
            |_| Message::Playback(PlaybackMessage::Tick),
        )
    }

    pub(crate) fn handle_volume_changed(&mut self, val: f32) -> Task<Message> {
        use std::time::Instant;

        // Minimum interval between storage persistence (500ms) - longer than volume updates
        // Volume changes go directly to PipeWire, but storage persists less frequently
        const MIN_PERSIST_INTERVAL_MS: u128 = 500;

        // Always update UI state immediately for smooth visual feedback
        self.playback.volume = val;
        Self::push_volume_toast(&mut self.toast, "Volume", val);

        // Sync volume to MPRIS D-Bus (this is fast, no throttling needed)
        if let Some(ref conn) = self.mpris_connection {
            conn.set_volume(f64::from(val));
        }

        // Mirror volume to PipeWire stream (updates shell mixer display).
        // Non-blocking: sends via pw::channel, processed on the PW thread.
        self.sfx_engine.set_output_volume(val);

        // Set volume on the audio engine (atomic via rodio stream handle).
        // With rodio, set_volume() is non-blocking — no channel needed.
        // Throttle the async persist to storage (engine volume is instant regardless).
        let should_persist = {
            let now = Instant::now();
            match self.playback.volume_persist_throttle {
                Some(t) if now.duration_since(t).as_millis() < MIN_PERSIST_INTERVAL_MS => false,
                _ => {
                    self.playback.volume_persist_throttle = Some(now);
                    true
                }
            }
        };

        if should_persist {
            self.shell_task(
                move |shell| async move {
                    let _ = shell.set_volume(val).await;
                },
                |_| Message::NoOp,
            )
        } else {
            // Still set engine volume even when not persisting
            self.shell_task(
                move |shell| async move {
                    let engine = shell.audio_engine();
                    let mut eng = engine.lock().await;
                    eng.set_volume(val as f64);
                },
                |_| Message::NoOp,
            )
        }
    }

    pub(crate) fn handle_prepare_next_for_gapless(&mut self) -> Task<Message> {
        if self.engine.gapless_preparing {
            return Task::none();
        }
        self.engine.gapless_preparing = true;

        self.shell_task(
            |shell| async move {
                let _ = shell.prepare_next_for_gapless().await;
            },
            |_| Message::NoOp,
        )
    }

    pub(crate) fn handle_player_settings_loaded(
        &mut self,
        settings: crate::app_message::PlayerSettings,
    ) -> Task<Message> {
        self.playback.volume = settings.volume;
        self.sfx.volume = settings.sfx_volume;
        self.sfx.enabled = settings.sound_effects_enabled;
        self.engine.visualization_mode = settings.visualization_mode;

        // Crossfade settings
        self.engine.crossfade_enabled = settings.crossfade_enabled;
        self.engine.crossfade_duration_secs = settings.crossfade_duration_secs;

        // Volume normalization settings
        self.engine.volume_normalization = settings.volume_normalization;
        self.engine.normalization_level = settings.normalization_level;
        self.engine.replay_gain_preamp_db = settings.replay_gain_preamp_db;
        self.engine.replay_gain_fallback_db = settings.replay_gain_fallback_db;
        self.engine.replay_gain_fallback_to_agc = settings.replay_gain_fallback_to_agc;
        self.engine.replay_gain_prevent_clipping = settings.replay_gain_prevent_clipping;

        // Apply EQ settings
        self.playback.eq_state.set_enabled(settings.eq_enabled);
        for (i, &gain) in settings.eq_gains.iter().enumerate() {
            self.playback.eq_state.set_band_gain(i, gain);
        }

        // Load custom EQ presets into UI cache
        self.window.custom_eq_presets = settings.custom_eq_presets;

        debug!(
            "⚙️ [SETTINGS LOADED] crossfade_enabled={}, crossfade_duration_secs={}, volume_normalization={}, normalization_level={:?}",
            settings.crossfade_enabled,
            settings.crossfade_duration_secs,
            settings.volume_normalization,
            settings.normalization_level
        );

        // Push crossfade + normalization settings to the audio engine (accumulated, not early-returned)
        let crossfade_task = if let Some(shell) = &self.app_service {
            let shell = shell.clone();
            let enabled = settings.crossfade_enabled;
            let duration_secs = settings.crossfade_duration_secs;
            let mode = settings.volume_normalization;
            let norm_target = settings.normalization_level.target_level();
            let preamp_db = settings.replay_gain_preamp_db;
            let fallback_db = settings.replay_gain_fallback_db;
            let fallback_to_agc = settings.replay_gain_fallback_to_agc;
            let prevent_clipping = settings.replay_gain_prevent_clipping;
            let eq_state = self.playback.eq_state.clone();
            Task::perform(
                async move {
                    let engine = shell.audio_engine();
                    let mut engine_guard = engine.lock().await;
                    engine_guard.set_crossfade_enabled(enabled);
                    engine_guard.set_crossfade_duration(duration_secs);
                    engine_guard.set_volume_normalization(
                        mode,
                        norm_target,
                        preamp_db,
                        fallback_db,
                        fallback_to_agc,
                        prevent_clipping,
                    );
                    engine_guard.set_eq_state(eq_state);
                },
                |_| Message::NoOp,
            )
        } else {
            Task::none()
        };

        // General settings
        self.scrobbling_enabled = settings.scrobbling_enabled;
        self.scrobble_threshold = settings.scrobble_threshold;
        self.start_view = settings.start_view.clone();
        self.stable_viewport = settings.stable_viewport;
        self.auto_follow_playing = settings.auto_follow_playing;
        self.enter_behavior = settings.enter_behavior;
        self.local_music_path = settings.local_music_path.clone();
        self.library_page_size = settings.library_page_size;
        self.default_playlist_id = settings.default_playlist_id.clone();
        self.default_playlist_name = settings.default_playlist_name.clone();
        self.quick_add_to_playlist = settings.quick_add_to_playlist;
        self.queue_show_default_playlist = settings.queue_show_default_playlist;
        self.verbose_config = settings.verbose_config;
        self.artwork_resolution = settings.artwork_resolution;
        self.show_album_artists_only = settings.show_album_artists_only;
        self.suppress_library_refresh_toasts = settings.suppress_library_refresh_toasts;
        self.show_tray_icon = settings.show_tray_icon;
        self.close_to_tray = settings.close_to_tray;

        // Restore queue column visibility from persisted settings.
        self.queue_page.column_visibility.stars = settings.queue_show_stars;
        self.queue_page.column_visibility.album = settings.queue_show_album;
        self.queue_page.column_visibility.duration = settings.queue_show_duration;
        self.queue_page.column_visibility.love = settings.queue_show_love;
        self.queue_page.column_visibility.plays = settings.queue_show_plays;
        self.queue_page.column_visibility.index = settings.queue_show_index;
        self.queue_page.column_visibility.thumbnail = settings.queue_show_thumbnail;

        // Restore Artists view column visibility.
        self.artists_page.column_visibility.stars = settings.artists_show_stars;
        self.artists_page.column_visibility.albumcount = settings.artists_show_albumcount;
        self.artists_page.column_visibility.songcount = settings.artists_show_songcount;
        self.artists_page.column_visibility.plays = settings.artists_show_plays;
        self.artists_page.column_visibility.love = settings.artists_show_love;
        self.artists_page.column_visibility.index = settings.artists_show_index;
        self.artists_page.column_visibility.thumbnail = settings.artists_show_thumbnail;

        // Restore Genres view column visibility.
        self.genres_page.column_visibility.thumbnail = settings.genres_show_thumbnail;

        // Restore Albums view column visibility.
        self.albums_page.column_visibility.stars = settings.albums_show_stars;
        self.albums_page.column_visibility.songcount = settings.albums_show_songcount;
        self.albums_page.column_visibility.plays = settings.albums_show_plays;
        self.albums_page.column_visibility.love = settings.albums_show_love;
        self.albums_page.column_visibility.index = settings.albums_show_index;
        self.albums_page.column_visibility.thumbnail = settings.albums_show_thumbnail;

        // Restore Songs view column visibility.
        self.songs_page.column_visibility.stars = settings.songs_show_stars;
        self.songs_page.column_visibility.album = settings.songs_show_album;
        self.songs_page.column_visibility.duration = settings.songs_show_duration;
        self.songs_page.column_visibility.plays = settings.songs_show_plays;
        self.songs_page.column_visibility.love = settings.songs_show_love;
        self.songs_page.column_visibility.index = settings.songs_show_index;
        self.songs_page.column_visibility.thumbnail = settings.songs_show_thumbnail;

        // Restore active playlist context from persisted settings
        self.active_playlist_info =
            settings
                .active_playlist_id
                .clone()
                .map(|id| crate::state::ActivePlaylistContext {
                    id,
                    name: settings.active_playlist_name.clone(),
                    comment: settings.active_playlist_comment.clone(),
                });

        // Apply start_view on first load (one-shot: only before first application)
        let mut start_view_task = Task::none();
        if !self.start_view_applied {
            self.start_view_applied = true;
            self.current_view = match settings.start_view.as_str() {
                "Albums" => crate::View::Albums,
                "Artists" => crate::View::Artists,
                "Songs" => crate::View::Songs,
                "Genres" => crate::View::Genres,
                "Playlists" => crate::View::Playlists,
                _ => crate::View::Queue,
            };
            // Trigger data load for the start view.
            // Albums + Artists are also loaded by ViewPreferencesLoaded with persisted
            // sort prefs, but start_view may render before prefs arrive — trigger load
            // here to avoid an empty-state flash. The second load from ViewPreferencesLoaded
            // harmlessly replaces the buffer with correctly-sorted data.
            start_view_task = match self.current_view {
                crate::View::Albums => Task::done(Message::LoadAlbums),
                crate::View::Artists => Task::done(Message::LoadArtists),
                crate::View::Songs => Task::done(Message::LoadSongs),
                crate::View::Genres => Task::done(Message::LoadGenres),
                crate::View::Playlists => Task::done(Message::LoadPlaylists),
                _ => Task::none(), // Queue always loaded in handle_login_result
            };
        }

        // Apply settings to engines
        self.sfx_engine.set_enabled(self.sfx.enabled);
        self.sfx_engine.set_volume(self.sfx.volume);

        // Send initial volume to PipeWire so the shell mixer shows the
        // correct percentage from startup (before user drags the slider).
        self.sfx_engine.set_output_volume(self.playback.volume);

        // Apply theme mode from config.toml (single source of truth)
        let config_light_mode = crate::theme_config::load_light_mode_from_config();
        crate::theme::set_light_mode(config_light_mode);

        // Apply rounded mode from persisted settings
        crate::theme::set_rounded_mode(settings.rounded_mode);

        // Apply nav layout from persisted settings
        crate::theme::set_nav_layout(settings.nav_layout);

        // Apply nav display mode from persisted settings
        crate::theme::set_nav_display_mode(settings.nav_display_mode);

        // Apply track info display mode from persisted settings
        crate::theme::set_track_info_display(settings.track_info_display);

        // Apply slot row height from persisted settings
        crate::theme::set_slot_row_height(settings.slot_row_height);

        // Apply opacity gradient from persisted settings
        crate::theme::set_opacity_gradient(settings.opacity_gradient);

        // Apply slot text links from persisted settings
        crate::theme::set_slot_text_links(settings.slot_text_links);

        // Apply horizontal volume mode from persisted settings
        crate::theme::set_horizontal_volume(settings.horizontal_volume);

        // Apply font family from persisted settings
        crate::theme::set_font_family(settings.font_family.clone());

        // Apply strip field visibility from persisted settings
        crate::theme::set_strip_show_title(settings.strip_show_title);
        crate::theme::set_strip_show_artist(settings.strip_show_artist);
        crate::theme::set_strip_show_album(settings.strip_show_album);
        crate::theme::set_strip_show_format_info(settings.strip_show_format_info);
        crate::theme::set_strip_merged_mode(settings.strip_merged_mode);
        crate::theme::set_strip_click_action(settings.strip_click_action);
        crate::theme::set_strip_show_labels(settings.strip_show_labels);
        crate::theme::set_strip_separator(settings.strip_separator);

        // Apply per-view artwork text overlay visibility from persisted settings
        crate::theme::set_albums_artwork_overlay(settings.albums_artwork_overlay);
        crate::theme::set_artists_artwork_overlay(settings.artists_artwork_overlay);
        crate::theme::set_songs_artwork_overlay(settings.songs_artwork_overlay);
        crate::theme::set_playlists_artwork_overlay(settings.playlists_artwork_overlay);

        // Apply artwork column layout settings from persisted settings
        crate::theme::set_artwork_column_mode(settings.artwork_column_mode);
        crate::theme::set_artwork_column_stretch_fit(settings.artwork_column_stretch_fit);
        crate::theme::set_artwork_column_width_pct(settings.artwork_column_width_pct);

        // Sync volume to MPRIS D-Bus (prevents initial 100% jump on first playerctl command)
        if let Some(ref conn) = self.mpris_connection {
            conn.set_volume(f64::from(settings.volume));
        }

        // Apply volume to audio engine
        let vol = self.playback.volume;
        self.shell_spawn("apply_volume", move |shell| async move {
            shell.set_volume(vol).await
        });

        Task::batch([start_view_task, crossfade_task])
    }

    pub(crate) fn handle_initialize_scrobble_state(
        &mut self,
        song_id: Option<String>,
    ) -> Task<Message> {
        // Initialize scrobble state with the current song from the persisted queue
        // This prevents false "song change" detection on app startup
        if let Some(id) = song_id {
            debug!(" [SCROBBLE] Initializing scrobble state with song: {}", id);
            self.scrobble.current_song_id = Some(id);
        } else {
            debug!(" [SCROBBLE] No current song in queue, scrobble state remains None");
        }
        Task::none()
    }

    pub(crate) fn handle_view_preferences_loaded(
        &mut self,
        prefs: crate::app_message::AllViewPreferences,
    ) -> Task<Message> {
        debug!(" Loading saved view preferences...");

        // Apply albums preferences
        self.albums_page.common.current_sort_mode = prefs.albums.sort_mode;
        self.albums_page.common.sort_ascending = prefs.albums.sort_ascending;
        debug!(
            "Albums: {:?}, asc={}",
            prefs.albums.sort_mode, prefs.albums.sort_ascending
        );

        // Apply artists preferences
        self.artists_page.common.current_sort_mode = prefs.artists.sort_mode;
        self.artists_page.common.sort_ascending = prefs.artists.sort_ascending;
        debug!(
            "Artists: {:?}, asc={}",
            prefs.artists.sort_mode, prefs.artists.sort_ascending
        );

        // Apply songs preferences
        self.songs_page.common.current_sort_mode = prefs.songs.sort_mode;
        self.songs_page.common.sort_ascending = prefs.songs.sort_ascending;
        debug!(
            "Songs: {:?}, asc={}",
            prefs.songs.sort_mode, prefs.songs.sort_ascending
        );

        // Apply genres preferences
        self.genres_page.common.current_sort_mode = prefs.genres.sort_mode;
        self.genres_page.common.sort_ascending = prefs.genres.sort_ascending;
        debug!(
            "Genres: {:?}, asc={}",
            prefs.genres.sort_mode, prefs.genres.sort_ascending
        );

        // Apply playlists preferences
        self.playlists_page.common.current_sort_mode = prefs.playlists.sort_mode;
        self.playlists_page.common.sort_ascending = prefs.playlists.sort_ascending;
        debug!(
            "Playlists: {:?}, asc={}",
            prefs.playlists.sort_mode, prefs.playlists.sort_ascending
        );

        // Apply queue preferences
        self.queue_page.queue_sort_mode = prefs.queue.sort_mode;
        self.queue_page.common.sort_ascending = prefs.queue.sort_ascending;
        debug!(
            "Queue: {:?}, asc={}",
            prefs.queue.sort_mode, prefs.queue.sort_ascending
        );

        // Reload views with the correct (persisted) sort preferences applied.
        // These loads are deferred from login to avoid racing with default sort.
        Task::batch([
            Task::done(Message::LoadAlbums),
            Task::done(Message::LoadArtists), // Needed for artist artwork prefetch
            Task::done(Message::LoadQueue),
        ])
    }

    /// Push a right-aligned, short-duration "Volume: NN%" / "SFX Volume: NN%" toast.
    /// Single source of truth for volume-change feedback across slider drag,
    /// scroll-wheel, and MPRIS — replaces the old hover-tooltip percentage.
    fn push_volume_toast(toast: &mut crate::state::ToastState, label: &str, vol: f32) {
        let pct = (vol.clamp(0.0, 1.0) * 100.0) as u32;
        toast.push(
            nokkvi_data::types::toast::Toast::info_short(format!("{label}: {pct}%"))
                .right_aligned(),
        );
    }

    pub(crate) fn handle_radio_metadata_update(
        &mut self,
        icy_artist: Option<String>,
        icy_title: Option<String>,
        icy_url: Option<String>,
    ) -> Task<Message> {
        if let crate::state::ActivePlayback::Radio(state) = &mut self.active_playback {
            state.icy_artist = icy_artist;
            state.icy_title = icy_title;
            state.icy_url = icy_url;
        }
        Task::none()
    }
}
