//! `atomic_u8_enum!` macro — paired `u8`→variant and variant→`u8` conversions
//! for enums backed by an `AtomicU8`.
//!
//! Several `UiModeFlags` discriminants in `crate::theme` are stored as
//! `AtomicU8` so they can be read lock-free from the render thread. Each one
//! used to ship a hand-written `match` body inside both the loader and the
//! store helper — the encoding lived in two places per enum and could drift.
//!
//! This macro takes a single integer-to-variant table per enum and emits both
//! halves of a UI-crate-local [`AtomicU8Enum`] impl:
//!
//! 1. `fn from_u8(v: u8) -> Self` — loader: maps each listed byte to its named
//!    variant; unknown bytes fall back to the declared `default` variant. This
//!    matches the original `match ... { _ => Default }` shape and preserves the
//!    redb on-disk back-compat contract (legacy `app.redb` files with future
//!    bytes still load cleanly).
//! 2. `fn to_u8(self) -> u8` — store: enumerates every variant explicitly so
//!    adding a variant without updating the table is a compile error (just like
//!    the original enum-exhaustive `match` blocks).
//!
//! ## Why a local trait instead of `From<u8>` / `From<Enum> for u8`?
//!
//! The enums migrated by this macro (`TrackInfoDisplay`, `NavLayout`, etc.)
//! live in the `nokkvi-data` crate. Rust's orphan rules forbid the UI crate
//! from implementing `From<u8>` for those foreign enums (neither type would
//! be local to the impl). The local `AtomicU8Enum` trait sidesteps the
//! orphan rule entirely.
//!
//! ## Usage
//!
//! ```ignore
//! atomic_u8_enum! {
//!     TrackInfoDisplay {
//!         0 => Off,
//!         1 => PlayerBar,
//!         2 => TopBar,
//!         3 => ProgressTrack,
//!     } default Off
//! }
//! ```
//!
//! Call sites in `theme.rs` become 1-liners:
//!
//! ```ignore
//! pub(crate) fn track_info_display() -> TrackInfoDisplay {
//!     TrackInfoDisplay::from_u8(UI_MODE.track_info_display.load(Ordering::Relaxed))
//! }
//!
//! pub(crate) fn set_track_info_display(mode: TrackInfoDisplay) {
//!     UI_MODE
//!         .track_info_display
//!         .store(mode.to_u8(), Ordering::Relaxed);
//! }
//! ```

/// UI-crate-local conversion trait between `AtomicU8`-backed enums and their
/// raw byte discriminants. Implemented for each `UiModeFlags` enum via the
/// [`atomic_u8_enum!`] macro.
///
/// Local to the UI crate so we can blanket-impl it for `nokkvi-data` enums
/// without tripping Rust's orphan rules.
pub(crate) trait AtomicU8Enum: Sized + Copy {
    /// Convert a stored byte back into the enum variant. Unknown bytes fall
    /// back to the type's declared default (preserves redb back-compat with
    /// app.redb files written by a future build with extra variants).
    fn from_u8(value: u8) -> Self;

    /// Convert the enum variant into its stored-byte discriminant.
    fn to_u8(self) -> u8;
}

/// Emit a paired [`AtomicU8Enum`] impl for an enum: loader (unknown→default
/// fallback) + store (enum-exhaustive).
///
/// See the module-level docs for the full rationale and call-site shape.
macro_rules! atomic_u8_enum {
    (
        $enum:ident {
            $( $byte:literal => $variant:ident ),* $(,)?
        } default $default:ident
    ) => {
        impl $crate::atomic_u8_enum::AtomicU8Enum for $enum {
            #[inline]
            fn from_u8(value: u8) -> Self {
                match value {
                    $( $byte => Self::$variant, )*
                    _ => Self::$default,
                }
            }

            #[inline]
            fn to_u8(self) -> u8 {
                match self {
                    $( $enum::$variant => $byte, )*
                }
            }
        }
    };
}

pub(crate) use atomic_u8_enum;
