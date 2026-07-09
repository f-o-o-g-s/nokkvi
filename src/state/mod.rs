//! App-level state types, grouped by domain.
//!
//! Each sub-module owns one cluster of related state (panes, playback,
//! artwork, …). Everything is re-exported here so callers continue to use
//! `crate::state::Foo` paths without caring which file the type lives in.
//!
//! # Construction convention
//!
//! State holders pick one of three shapes; prefer the simpler one when a
//! type's fields permit it. Don't force a `Default` impl onto a type that
//! doesn't have a sensible empty starting state — the absence is informative.
//!
//! 1. `#[derive(Default)]` — preferred when every field has a `Default`
//!    impl that produces the value the holder should start at. Used by
//!    `EngineState`, `ScrobbleState`, `PlaybackModes`, `LibraryData`,
//!    `LibraryCounts`, `ToastState`, `PaneFocus`, `ActivePlayback`,
//!    `PendingExpandState`.
//!
//! 2. Hand-written `impl Default` — when the holder has a sensible empty
//!    starting state but some field defaults aren't what the holder wants
//!    (e.g. `SfxState::volume = 0.68`, `WindowState::width = 1200.0`,
//!    `CrossPaneDragUi::selection_count = 1`, or a string field that should
//!    start as `"Not Playing"`). Reach for this over `new()` when no
//!    constructor arguments are needed; reserve `new()` for shapes that
//!    genuinely need parameters or `NonZeroUsize` capacities that
//!    `#[derive]` can't express.
//!
//! 3. Neither — for types whose existence on the parent always implies a
//!    populated value (e.g. `StoredSession`, `ActivePlaylistContext`,
//!    `RouletteState`, `CrossPaneDragState`, `RadioPlaybackState`,
//!    `SimilarSongsState`, `PendingExpand`, `PendingTopPin`). The parent
//!    holds these inside `Option<T>` and constructs the inner only when
//!    the feature is actually active, so a blank Default would be a
//!    misleading shape.
//!
//! `ArtworkState` uses pattern (2) because it threads non-zero LRU
//! capacities into `SnapshottedLru::new`; `CollageArtworkCache::new()`
//! exists for the same reason — `#[derive(Default)]` cannot express
//! `NonZeroUsize` capacities at compile time.

mod artwork;
mod audio;
mod harbour;
mod library;
mod panes;
mod pending;
mod playback;
mod playlist_editor;
mod roulette;
mod scrobble;
mod session;
mod similar;
mod snapshotted_lru;
mod toast;
mod window;

pub(crate) use artwork::*;
pub(crate) use audio::*;
pub(crate) use harbour::*;
pub(crate) use library::*;
pub(crate) use panes::*;
pub(crate) use pending::*;
pub(crate) use playback::*;
pub(crate) use playlist_editor::*;
pub(crate) use roulette::*;
pub(crate) use scrobble::*;
pub(crate) use session::*;
pub(crate) use similar::*;
pub(crate) use toast::*;
pub(crate) use window::*;
