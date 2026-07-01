// The player_settings_schema! tt-muncher recurses once per field row (~90
// rows across the settings twins), which overflows the default limit of 128
// when combined with the crate's other macro expansion depth.
#![recursion_limit = "256"]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::print_stderr,
        clippy::field_reassign_with_default
    )
)]
//! Nokkvi Data Crate
//!
//! Iced-free business logic: types, backend services, audio engine, utilities,
//! credentials, and configuration.

/// `User-Agent` sent on every outbound HTTP request — the music-server API,
/// audio stream/chunk fetches, the SSE event stream, internet radio, and remote
/// artwork. Kept identical across all clients so Navidrome registers nokkvi as a
/// single "player" (it keys players on client name + User-Agent). Deliberately
/// not a fake browser string — the old Chrome disguise proved unnecessary (radio
/// works without it) — and deliberately versionless: these clients live in
/// `nokkvi-data`, versioned independently of the app crate, so a version here
/// would be misleading.
pub const USER_AGENT: &str = "nokkvi";

pub mod audio;
pub mod backend;
pub mod credentials;
pub mod services;
pub mod types;
pub mod utils;
