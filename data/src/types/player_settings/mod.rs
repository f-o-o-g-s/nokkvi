//! Player settings — the live in-memory union of every user-tunable knob.
//!
//! Per-domain enum families live in sub-modules and are re-exported here so
//! callers continue to use `crate::types::player_settings::Foo` paths
//! regardless of which file the type lives in.

mod appearance;
mod artwork;
mod library;
mod navigation;
mod playback;
mod slot_list;
mod strip;
mod verbose;
mod visualizer;

pub use appearance::*;
pub use artwork::*;
pub use library::*;
pub use navigation::*;
pub use playback::*;
pub use slot_list::*;
pub use strip::*;
pub use verbose::*;
pub use visualizer::*;

/// Live, UI-facing player settings — emitted from the
/// `player_settings_schema!` table alongside its persisted twin in
/// `crate::types::settings` (M4); re-exported here so the canonical
/// `crate::types::player_settings::LivePlayerSettings` path keeps
/// resolving everywhere (including the paths `define_settings!` emits).
pub use crate::types::settings::LivePlayerSettings;
