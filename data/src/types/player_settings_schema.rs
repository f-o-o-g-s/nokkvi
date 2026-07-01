//! `player_settings_schema!` — ONE field table emitting the
//! `PersistedPlayerSettings` / `LivePlayerSettings` twins (M4).
//!
//! The two structs share all common fields and diverge in exactly two
//! directions (`light_mode` persist-only, `visualizer` live-only) plus three
//! f64/f32 type splits. Before this macro, every field had to be declared
//! twice by hand and the pair could drift silently; now a field exists in
//! both structs (or the declared one) from a single row.
//!
//! # Row grammar (order preserved — see the byte-stability note)
//!
//! ```ignore
//! /// doc comments (forwarded to BOTH structs)
//! #[serde(...)]                    // forwarded to Persisted ONLY
//! same     name: Ty          = default_expr,   // both structs, same type
//! split    name: PTy | LTy   = default_expr,   // Persisted PTy, Live LTy
//! persist_only name: Ty      = default_expr,   // Persisted only (light_mode)
//! live_only    name: Ty,                       // Live only (visualizer) —
//!                                              // no default: Live derives
//!                                              // Default
//! ```
//!
//! # Emission
//!
//! - `PersistedPlayerSettings`: derives `Debug, Clone, Serialize,
//!   Deserialize`; docs + serde attrs forwarded verbatim; a manual
//!   `impl Default` built from the per-row default expressions.
//! - `LivePlayerSettings`: derives `Debug, Clone, Default` (derived Default —
//!   NOT the persisted defaults; `ViewColumns` etc. supply their own); docs
//!   forwarded, serde attrs DROPPED (helper attrs don't compile on a
//!   non-serde struct).
//!
//! # Byte-stability (LOAD-BEARING)
//!
//! The M4 golden-bytes tests pin the exact serde_json output, which follows
//! struct FIELD DECLARATION ORDER — transcribe/edit rows only in the order
//! the goldens pin. serde attrs and default expressions are the redb compat
//! surface: never rename a field, change a default value, or drop a serde
//! attr without a migration story (the goldens fail on any of these).

/// See module-level docs.
#[macro_export]
macro_rules! player_settings_schema {
    // ── same: both structs, identical type ───────────────────────────────
    (@munch
        persist = [ $($p:tt)* ]
        live = [ $($l:tt)* ]
        defaults = [ $($d:tt)* ]
        rest = [
            $(#[doc = $doc:literal])*
            $(#[serde $serde_args:tt])*
            same $name:ident : $ty:ty = $default:expr,
            $($rest:tt)*
        ]
    ) => {
        $crate::player_settings_schema!(@munch
            persist = [ $($p)* $(#[doc = $doc])* $(#[serde $serde_args])* pub $name : $ty, ]
            live = [ $($l)* $(#[doc = $doc])* pub $name : $ty, ]
            defaults = [ $($d)* $name : $default, ]
            rest = [ $($rest)* ]
        );
    };

    // ── split: Persisted type | Live type ────────────────────────────────
    (@munch
        persist = [ $($p:tt)* ]
        live = [ $($l:tt)* ]
        defaults = [ $($d:tt)* ]
        rest = [
            $(#[doc = $doc:literal])*
            $(#[serde $serde_args:tt])*
            split $name:ident : $pty:ty | $lty:ty = $default:expr,
            $($rest:tt)*
        ]
    ) => {
        $crate::player_settings_schema!(@munch
            persist = [ $($p)* $(#[doc = $doc])* $(#[serde $serde_args])* pub $name : $pty, ]
            live = [ $($l)* $(#[doc = $doc])* pub $name : $lty, ]
            defaults = [ $($d)* $name : $default, ]
            rest = [ $($rest)* ]
        );
    };

    // ── persist_only: Persisted (+ Default) only ─────────────────────────
    (@munch
        persist = [ $($p:tt)* ]
        live = [ $($l:tt)* ]
        defaults = [ $($d:tt)* ]
        rest = [
            $(#[doc = $doc:literal])*
            $(#[serde $serde_args:tt])*
            persist_only $name:ident : $ty:ty = $default:expr,
            $($rest:tt)*
        ]
    ) => {
        $crate::player_settings_schema!(@munch
            persist = [ $($p)* $(#[doc = $doc])* $(#[serde $serde_args])* pub $name : $ty, ]
            live = [ $($l)* ]
            defaults = [ $($d)* $name : $default, ]
            rest = [ $($rest)* ]
        );
    };

    // ── live_only: Live only. NO default expression — Live derives
    //    Default, so a declared default would be silently discarded and the
    //    table would lie about the field's initial value. ──────────────────
    (@munch
        persist = [ $($p:tt)* ]
        live = [ $($l:tt)* ]
        defaults = [ $($d:tt)* ]
        rest = [
            $(#[doc = $doc:literal])*
            live_only $name:ident : $ty:ty,
            $($rest:tt)*
        ]
    ) => {
        $crate::player_settings_schema!(@munch
            persist = [ $($p)* ]
            live = [ $($l)* $(#[doc = $doc])* pub $name : $ty, ]
            defaults = [ $($d)* ]
            rest = [ $($rest)* ]
        );
    };

    // ── Done: emit both structs + the Persisted Default ──────────────────
    (@munch
        persist = [ $($p:tt)* ]
        live = [ $($l:tt)* ]
        defaults = [ $($d:tt)* ]
        rest = []
    ) => {
        /// Persisted player settings — the redb `user_settings.player` wire
        /// shape (serde_json, name-keyed). Emitted from the
        /// [`player_settings_schema!`][crate::player_settings_schema] field
        /// table together with its `LivePlayerSettings` twin; field
        /// DECLARATION ORDER, names, serde attrs, and default values are
        /// pinned by the M4 golden-bytes tests.
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct PersistedPlayerSettings {
            $($p)*
        }

        impl Default for PersistedPlayerSettings {
            fn default() -> Self {
                Self {
                    $($d)*
                }
            }
        }

        /// Live in-memory union of every user-tunable knob — the UI-facing
        /// twin of `PersistedPlayerSettings`, emitted from the same
        /// [`player_settings_schema!`][crate::player_settings_schema] table
        /// (no serde: this struct never touches a wire; its derived
        /// `Default` is the all-zero/type-default state, NOT the persisted
        /// defaults). Canonical path:
        /// `crate::types::player_settings::LivePlayerSettings` (re-export).
        #[derive(Debug, Clone, Default)]
        pub struct LivePlayerSettings {
            $($l)*
        }
    };

    // ── Entry point (MUST stay the last arm: its catch-all matcher would
    //    otherwise shadow every @munch arm and recurse forever) ────────────
    ( $($rows:tt)* ) => {
        $crate::player_settings_schema!(@munch
            persist = []
            live = []
            defaults = []
            rest = [ $($rows)* ]
        );
    };
}
