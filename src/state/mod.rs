//! App-level state types, grouped by domain.
//!
//! Each sub-module owns one cluster of related state (panes, playback,
//! artwork, …). Everything is re-exported here so callers continue to use
//! `crate::state::Foo` paths without caring which file the type lives in.

mod artwork;
mod audio;
mod library;
mod panes;
mod pending;
mod playback;
mod roulette;
mod scrobble;
mod session;
mod similar;
mod toast;
mod window;

pub(crate) use artwork::*;
pub(crate) use audio::*;
pub(crate) use library::*;
pub(crate) use panes::*;
pub(crate) use pending::*;
pub(crate) use playback::*;
pub(crate) use roulette::*;
pub(crate) use scrobble::*;
pub(crate) use session::*;
pub(crate) use similar::*;
pub(crate) use toast::*;
pub(crate) use window::*;
