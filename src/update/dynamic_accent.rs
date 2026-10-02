//! Dynamic accent: keep the theme's accent in step with the playing cover.
//!
//! The 100 ms playback tick level-sets everything, so no event has to be
//! caught: a track change, a cover arriving late, a stop, the setting, radio
//! and logout all resolve on the next tick. The tick finds the playing item's
//! artwork, has its color read off-thread once, and hands the seed to the
//! theme, which fits it to the active palette (`theme::set_dynamic_accent`).

use iced::{Task, advanced::image::Id, widget::image::Handle};

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, Message},
    state::ARTLESS_HOLD_TICKS,
    theme::{self, AccentSeed},
};

/// The off-thread read of one cover.
type SeedJob = Box<dyn FnOnce() -> Option<AccentSeed> + Send>;

/// What one tick decided, worked out while the artwork caches are borrowed.
enum Step {
    /// The accent on screen already belongs to what is playing.
    Keep,
    /// A remembered seed for this item: show it.
    Show(Id, Option<AccentSeed>),
    /// Read this handle off-thread; `None` when it holds nothing to decode.
    Extract(Id, Option<SeedJob>),
    /// A newly playing item with no artwork cached (yet).
    Artless,
}

/// The job that reads `handle`'s seed. A path handle has no pixels in memory.
fn seed_job(handle: &Handle) -> Option<SeedJob> {
    match handle {
        Handle::Bytes(_, bytes) => {
            let bytes = bytes.clone();
            Some(Box::new(move || theme::seed_from_encoded(&bytes)))
        }
        Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } => {
            let (width, height, pixels) = (*width, *height, pixels.clone());
            Some(Box::new(move || {
                theme::seed_from_rgba(width, height, &pixels)
            }))
        }
        Handle::Path(..) => None,
    }
}

impl Nokkvi {
    /// Level-set the accent against what is playing. Idempotent, and cheap
    /// when nothing changed.
    ///
    /// Gated on the transport (`playing || paused`), like the lyrics overlay:
    /// the current song survives a stop and is seeded from the restored queue
    /// at login, and a stopped player wears the theme's own accent.
    pub(crate) fn dynamic_accent_tick(&mut self) -> Task<Message> {
        if !self.settings.dynamic_accent || !self.playback.has_track() {
            self.dynamic_accent_release();
            return Task::none();
        }
        let Some(cover) = self.playing_cover() else {
            self.dynamic_accent_release();
            return Task::none();
        };

        let state = &self.dynamic_accent;
        // The mini decodes fastest; either size carries the same color.
        let preferred = cover.mini.or(cover.large);
        let ids = [cover.mini.map(Handle::id), cover.large.map(Handle::id)];
        let cached = |id: &Id| ids.contains(&Some(*id));
        let same_owner = |owner: &String| *owner == cover.owner;

        let step = match preferred {
            Some(handle) => {
                // A source still in the caches settles the item whichever
                // size it was read from: the large cover landing after the
                // mini must not re-read the album and nudge the accent.
                if let Some((owner, Some(source))) = &state.shown
                    && same_owner(owner)
                    && cached(source)
                {
                    Step::Keep
                } else if let Some((source, seed)) = state.seeds.peek(&cover.owner)
                    && cached(source)
                {
                    Step::Show(*source, *seed)
                } else if let Some((owner, source)) = &state.pending
                    && same_owner(owner)
                    && cached(source)
                {
                    Step::Keep
                } else {
                    Step::Extract(handle.id(), seed_job(handle))
                }
            }
            // The same item whose art was merely evicted keeps what it has.
            None if state.shown.as_ref().is_some_and(|(o, _)| same_owner(o)) => Step::Keep,
            None => Step::Artless,
        };
        let owner = cover.owner;
        let album = cover.album_id.map(str::to_string);

        let state = &mut self.dynamic_accent;
        match step {
            Step::Keep => {
                state.waiting = None;
                Task::none()
            }
            Step::Show(source, seed) => {
                state.waiting = None;
                state.seeds.promote(&owner);
                state.shown = Some((owner, Some(source)));
                self.dynamic_accent_show(seed);
                Task::none()
            }
            Step::Extract(source, Some(job)) => {
                // The previous accent stays up for the few milliseconds the
                // read takes, so a track change recolors once.
                state.waiting = None;
                state.pending = Some((owner.clone(), source));
                Task::perform(
                    async move { tokio::task::spawn_blocking(job).await.ok().flatten() },
                    move |seed| {
                        Message::Artwork(ArtworkMessage::AccentExtracted {
                            owner: owner.clone(),
                            source,
                            seed,
                        })
                    },
                )
            }
            Step::Extract(source, None) => {
                state.waiting = None;
                state.seeds.put(owner, (source, None));
                Task::none()
            }
            Step::Artless => {
                let ticks = match &state.waiting {
                    Some((waiting, ticks)) if *waiting == owner => ticks.saturating_add(1),
                    _ => 1,
                };
                // Nothing loads the playing album's cover outside the Queue
                // and Theater Mode; ask for it once.
                let request =
                    album.filter(|a| !a.is_empty() && state.art_requested.as_ref() != Some(a));
                if let Some(album) = &request {
                    state.art_requested = Some(album.clone());
                }
                if ticks > ARTLESS_HOLD_TICKS {
                    state.waiting = None;
                    state.shown = Some((owner, None));
                    self.dynamic_accent_show(None);
                } else {
                    state.waiting = Some((owner, ticks));
                }
                request.map_or_else(Task::none, |album| {
                    Task::done(Message::Artwork(ArtworkMessage::LoadLarge(album)))
                })
            }
        }
    }

    /// Record a finished extraction. The next tick shows it — or does not, if
    /// playback has moved on; the seed is kept for when its item plays again.
    /// A seed is only ever shown against the handle it was read from, so a
    /// late result can be remembered without a staleness check.
    pub(crate) fn handle_accent_extracted(
        &mut self,
        owner: String,
        source: Id,
        seed: Option<AccentSeed>,
    ) -> Task<Message> {
        let state = &mut self.dynamic_accent;
        if state
            .pending
            .as_ref()
            .is_some_and(|(o, s)| *o == owner && *s == source)
        {
            state.pending = None;
        }
        state.seeds.put(owner, (source, seed));
        Task::none()
    }

    /// Put `seed` on screen, or the theme's own accent for `None`. The theme
    /// state is process-global, so it is only written back to `None` by the
    /// app that set it.
    fn dynamic_accent_show(&mut self, seed: Option<AccentSeed>) {
        if seed.is_none() && !self.dynamic_accent.applied {
            return;
        }
        self.dynamic_accent.applied = seed.is_some();
        theme::set_dynamic_accent(seed);
    }

    /// Back to the theme's own accent: the setting is off or nothing is
    /// playing. Remembered seeds are kept for the next play.
    fn dynamic_accent_release(&mut self) {
        let state = &mut self.dynamic_accent;
        state.shown = None;
        state.pending = None;
        state.waiting = None;
        state.art_requested = None;
        self.dynamic_accent_show(None);
    }

    /// Forget the accent and its cache (logout).
    pub(crate) fn dynamic_accent_reset(&mut self) {
        self.dynamic_accent_release();
        self.dynamic_accent.seeds.clear();
    }
}
