//! Cover colors: keep the theme's accent and/or the visualizer colors in step
//! with the playing cover ("Accent From Album Art", "Visualizer From Album
//! Art").
//!
//! The 100 ms playback tick level-sets everything, so no event has to be
//! caught: a track change, a cover arriving late, a stop, the setting, radio
//! and logout all resolve on the next tick. The tick finds the playing item's
//! artwork, has its palette read off-thread once, and hands it to the theme,
//! which fits it to the active palette (`theme::set_cover_colors`).

use iced::{Task, advanced::image::Id, widget::image::Handle};

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, Message},
    state::ACCENT_HOLD_TICKS,
    theme::{self, CoverFollow, CoverPalette},
    update::components::cover_job::{CoverJob, cover_job},
};

/// What one tick decided, worked out while the artwork caches are borrowed.
enum Step {
    /// The accent on screen already belongs to what is playing.
    Keep,
    /// A remembered palette for this item: show it.
    Show(Id, Option<CoverPalette>),
    /// Read this handle off-thread; `None` when it holds nothing to decode.
    Extract(Id, Option<CoverJob<CoverPalette>>),
    /// A newly playing item with no artwork cached (yet).
    Artless,
}

impl Nokkvi {
    /// Level-set the accent against what is playing. Idempotent, and cheap
    /// when nothing changed.
    ///
    /// Gated on the transport (`playing || paused`), like the lyrics overlay:
    /// the current song survives a stop and is seeded from the restored queue
    /// at login, and a stopped player wears the theme's own accent. A stop is
    /// held for [`ACCENT_HOLD_TICKS`] first, since a skip can pass through a
    /// stopped state between two tracks.
    pub(crate) fn dynamic_accent_tick(&mut self) -> Task<Message> {
        let follow = self.cover_follow();
        if !follow.any() {
            self.dynamic_accent_release();
            return Task::none();
        }
        if !self.playback.has_track() {
            let state = &mut self.dynamic_accent;
            state.idle_ticks = state.idle_ticks.saturating_add(1);
            if state.idle_ticks > ACCENT_HOLD_TICKS {
                self.dynamic_accent_release();
            }
            return Task::none();
        }
        self.dynamic_accent.idle_ticks = 0;
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
                } else if let Some((source, palette)) = state.seeds.peek(&cover.owner)
                    && cached(source)
                {
                    Step::Show(*source, palette.clone())
                } else if let Some((owner, source)) = &state.pending
                    && same_owner(owner)
                    && cached(source)
                {
                    Step::Keep
                } else {
                    Step::Extract(
                        handle.id(),
                        cover_job(
                            handle,
                            theme::palette_from_encoded,
                            theme::palette_from_rgba,
                        ),
                    )
                }
            }
            // The same item whose art was merely evicted keeps what it has.
            None if state.shown.as_ref().is_some_and(|(o, _)| same_owner(o)) => Step::Keep,
            None => Step::Artless,
        };
        let owner = cover.owner;

        let state = &mut self.dynamic_accent;
        match step {
            Step::Keep => {
                state.waiting = None;
                // A setting flipped (accent and/or visualizer) while the item
                // stayed: re-apply what is shown under the new switches.
                if state.applied && state.applied_follow != follow {
                    let palette = state
                        .shown
                        .as_ref()
                        .and_then(|(o, _)| state.seeds.peek(o))
                        .and_then(|(_, p)| p.clone());
                    self.dynamic_accent_show(palette);
                }
                Task::none()
            }
            Step::Show(source, palette) => {
                state.waiting = None;
                state.seeds.promote(&owner);
                state.shown = Some((owner, Some(source)));
                self.dynamic_accent_show(palette);
                Task::none()
            }
            Step::Extract(source, Some(job)) => {
                // The previous accent stays up for the few milliseconds the
                // read takes, so a track change recolors once.
                state.waiting = None;
                state.pending = Some((owner.clone(), source));
                Task::perform(
                    async move { tokio::task::spawn_blocking(job).await.ok().flatten() },
                    move |palette| {
                        Message::Artwork(ArtworkMessage::AccentExtracted {
                            owner: owner.clone(),
                            source,
                            palette,
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
                // Playback warms the playing album's 80 px cover on every
                // song change (`now_playing_artwork_to_warm`), so this only
                // waits for it; with no cover at all, the hold runs out.
                let ticks = match &state.waiting {
                    Some((waiting, ticks)) if *waiting == owner => ticks.saturating_add(1),
                    _ => 1,
                };
                if ticks > ACCENT_HOLD_TICKS {
                    state.waiting = None;
                    state.shown = Some((owner, None));
                    self.dynamic_accent_show(None);
                } else {
                    state.waiting = Some((owner, ticks));
                }
                Task::none()
            }
        }
    }

    /// Record a finished extraction. The next tick shows it — or does not, if
    /// playback has moved on; the palette is kept for when its item plays
    /// again. A palette is only ever shown against the handle it was read
    /// from, so a late result can be remembered without a staleness check.
    pub(crate) fn handle_accent_extracted(
        &mut self,
        owner: String,
        source: Id,
        palette: Option<CoverPalette>,
    ) -> Task<Message> {
        let state = &mut self.dynamic_accent;
        if state
            .pending
            .as_ref()
            .is_some_and(|(o, s)| *o == owner && *s == source)
        {
            state.pending = None;
        }
        state.seeds.put(owner, (source, palette));
        Task::none()
    }

    /// What the cover recolors, from the two settings.
    fn cover_follow(&self) -> CoverFollow {
        CoverFollow {
            accent: self.settings.dynamic_accent,
            visualizer: self.settings.dynamic_visualizer,
        }
    }

    /// Put `palette` on screen under the current switches, or the theme's own
    /// colors for `None`. The theme state is process-global, so it is only
    /// written back to `None` by the app that set it.
    fn dynamic_accent_show(&mut self, palette: Option<CoverPalette>) {
        if palette.is_none() && !self.dynamic_accent.applied {
            return;
        }
        let follow = self.cover_follow();
        self.dynamic_accent.applied = palette.is_some();
        self.dynamic_accent.applied_follow = follow;
        theme::set_cover_colors(palette, follow);
    }

    /// Back to the theme's own colors: both settings are off or nothing is
    /// playing. Remembered palettes are kept for the next play.
    fn dynamic_accent_release(&mut self) {
        let state = &mut self.dynamic_accent;
        state.shown = None;
        state.pending = None;
        state.waiting = None;
        state.idle_ticks = 0;
        self.dynamic_accent_show(None);
    }

    /// Forget the cover colors and their cache (logout).
    pub(crate) fn dynamic_accent_reset(&mut self) {
        self.dynamic_accent_release();
        self.dynamic_accent.seeds.clear();
    }
}
