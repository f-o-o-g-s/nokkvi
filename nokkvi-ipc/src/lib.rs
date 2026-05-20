//! IPC layer for nokkvi external control. See ~/nokkvi-new-feats.md §14.
//!
//! Structural invariant: this crate is iced-free. The fork-before-iced
//! client path links it directly, before iced's runtime is constructed,
//! so adding iced here would break startup latency (and the build).

pub fn ping() -> &'static str {
    "pong"
}
