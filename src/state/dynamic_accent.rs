//! Cover-color bookkeeping: which cover the accent and visualizer colors on
//! screen came from. The colors themselves live in the theme
//! (`theme::set_cover_colors`).

use std::num::NonZeroUsize;

use iced::advanced::image::Id;
use lru::LruCache;

use crate::theme::{CoverFollow, CoverPalette};

/// Palettes remembered per album / station, so a replay recolors without a
/// decode.
const SEED_CACHE_CAPACITY: NonZeroUsize = NonZeroUsize::new(256).expect("capacity must be > 0");

/// Ticks (100 ms each) the accent is held through a gap before it returns to
/// the theme's own: a newly playing item whose cover has not arrived, or a
/// stopped transport between two tracks. A track change then recolors once
/// rather than twice.
pub(crate) const ACCENT_HOLD_TICKS: u8 = 15;

/// State of the colors that follow the playing cover — the accent and/or the
/// visualizer (see `update/dynamic_accent.rs`).
#[derive(Debug)]
pub struct DynamicAccentState {
    /// The playing item (album id, or `radio:<station id>`) the accent on
    /// screen was settled for, with the artwork handle its seed came from.
    /// The handle is `None` when the item had no artwork and the accent went
    /// back to the theme's own.
    pub shown: Option<(String, Option<Id>)>,
    /// Whether this app has cover colors laid over the theme right now.
    pub applied: bool,
    /// What those colors recolor (the two settings when they were applied).
    pub applied_follow: CoverFollow,
    /// Extraction in flight, as `(item, artwork handle)`.
    pub pending: Option<(String, Id)>,
    /// A newly playing item still without artwork, and for how many ticks.
    pub waiting: Option<(String, u8)>,
    /// Consecutive ticks with nothing playing or paused.
    pub idle_ticks: u8,
    /// Extracted palettes by item, each with the handle it was read from. A
    /// `None` palette is a cover without a usable color.
    pub seeds: LruCache<String, (Id, Option<CoverPalette>)>,
}

impl Default for DynamicAccentState {
    fn default() -> Self {
        Self {
            shown: None,
            applied: false,
            applied_follow: CoverFollow::default(),
            pending: None,
            waiting: None,
            idle_ticks: 0,
            seeds: LruCache::new(SEED_CACHE_CAPACITY),
        }
    }
}
