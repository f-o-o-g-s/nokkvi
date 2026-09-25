//! Theater Mode state: the now-playing layout that hides the slot list, the
//! toolbar and the nav, leaving the cover, the over-cover visualizer and the
//! lyrics on screen. Transient by design: never persisted, never entered on
//! the Login screen. Entry and exit go through `Nokkvi::enter_theater` /
//! `Nokkvi::exit_theater` (`update/theater.rs`).

/// Theater Mode's root-owned state (`Nokkvi.theater`).
#[derive(Debug, Default)]
pub struct TheaterState {
    /// The theater layout replaces the home layout while this is set.
    pub active: bool,
}
