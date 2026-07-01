//! `wire_enum!` — declarative macro for settings enums keyed by snake_case
//! wire strings (the visualizer-config convention).
//!
//! Companion to [`define_labeled_enum!`][crate::define_labeled_enum] (which
//! pairs human-readable GUI labels with wire strings). The visualizer enums
//! instead key their settings dropdowns on the WIRE string itself, carry
//! explicit `#[repr(u32)]` discriminants consumed by the GPU shaders, and
//! tolerate unknown wire input by falling back to `Default` (mirroring
//! `deserialize_or_default`). This macro generates that whole method family
//! from a single per-variant declaration.
//!
//! # Shape
//!
//! ```ignore
//! nokkvi_data::wire_enum! {
//!     /// Doc comment for the enum.
//!     #[repr(u32)]
//!     pub enum Example {
//!         #[default]
//!         FallFade = 0 => "fall_fade",
//!         FallAccel = 1 => "fall_accel",
//!     }
//! }
//! ```
//!
//! # Emission
//!
//! - The enum definition with every outer attribute preserved verbatim, a
//!   fixed derive set (`Debug, Clone, Copy, PartialEq, Eq, Default,
//!   Serialize, Deserialize`), each variant's explicit discriminant, and a
//!   per-variant `#[serde(rename = <wire>)]` so the serde wire format is tied
//!   to the same literal `as_wire_str` returns — they cannot drift.
//! - `pub const ALL: &'static [Self]` — every variant in declaration order
//!   (declaration order == settings-dropdown display order).
//! - `pub fn as_wire_str(&self) -> &'static str` — the wire literal.
//! - `pub fn all_wire_strs() -> Vec<&'static str>` — wire literals in
//!   declaration order.
//! - `pub fn from_wire_str(&str) -> Self` — parse a wire string; unknown
//!   input falls back to `Self::default()` (the `#[default]` variant), the
//!   same tolerance `deserialize_or_default` gives the serde path.
//! - `pub fn as_u32(&self) -> u32` — the explicit discriminant, for GPU
//!   uniform packing.
//!
//! # Notes
//!
//! - Wire literals are EXPLICIT per variant. `macro_rules!` cannot derive
//!   snake_case from a variant ident (`stringify!(FallFade)` yields
//!   `"FallFade"`; word-boundary underscores need a proc-macro), so the
//!   caller declares each literal — the `define_labeled_enum!` precedent.
//! - Discriminants are EXPLICIT and may skip values (e.g. `BarsGradientMode`
//!   pins `Static = 0, Wave = 2` with `1` intentionally dead — no shader
//!   branch exists for it).
//! - Every enum passed through this macro must mark its fallback variant
//!   with `#[default]`.

/// See module-level docs.
#[macro_export]
macro_rules! wire_enum {
    (
        $(#[$enum_attr:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_attr:meta])*
                $variant:ident = $disc:literal => $wire:literal
            ),* $(,)?
        }
    ) => {
        $(#[$enum_attr])*
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Default,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        $vis enum $name {
            $(
                $(#[$variant_attr])*
                #[serde(rename = $wire)]
                $variant = $disc,
            )*
        }

        impl $name {
            /// Every variant in declaration order (settings-dropdown display
            /// order).
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )* ];

            /// The serde wire string for this variant — matches the
            /// per-variant `#[serde(rename = ...)]` exactly, by construction.
            pub fn as_wire_str(&self) -> &'static str {
                match self {
                    $( Self::$variant => $wire, )*
                }
            }

            /// Wire strings for every variant in declaration order.
            pub fn all_wire_strs() -> ::std::vec::Vec<&'static str> {
                Self::ALL.iter().map(|v| v.as_wire_str()).collect()
            }

            /// Parse a wire string. Unknown input falls back to
            /// `Self::default()` (the `#[default]` variant), mirroring the
            /// `deserialize_or_default` tolerance on the serde path.
            pub fn from_wire_str(s: &str) -> Self {
                match s {
                    $( $wire => Self::$variant, )*
                    _ => Self::default(),
                }
            }

            /// The explicit discriminant as `u32`, for GPU uniform packing.
            pub fn as_u32(&self) -> u32 {
                *self as u32
            }
        }
    };
}

#[cfg(test)]
mod tests {
    // Synthetic enum exercising the macro emission shape, including a
    // deliberately skipped discriminant (2..=3 dead) to prove explicit
    // non-contiguous values survive — the `BarsGradientMode {0,2}` case.
    crate::wire_enum! {
        /// Doc comment passes through to the emitted enum.
        #[repr(u32)]
        pub enum SynthWire {
            #[default]
            FallFade = 0 => "fall_fade",
            FallAccel = 1 => "fall_accel",
            Wave = 4 => "wave",
        }
    }

    #[test]
    fn wire_enum_as_wire_str_matches_explicit_literal() {
        assert_eq!(SynthWire::FallFade.as_wire_str(), "fall_fade");
        assert_eq!(SynthWire::FallAccel.as_wire_str(), "fall_accel");
        assert_eq!(SynthWire::Wave.as_wire_str(), "wave");
    }

    #[test]
    fn wire_enum_serde_roundtrips_via_rename() {
        for v in SynthWire::ALL {
            let json = serde_json::to_string(v).unwrap();
            assert_eq!(json.trim_matches('"'), v.as_wire_str(), "variant {v:?}");
            let back: SynthWire = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *v, "variant {v:?}");
        }
    }

    #[test]
    fn wire_enum_all_contains_every_variant_once() {
        assert_eq!(
            SynthWire::ALL,
            &[SynthWire::FallFade, SynthWire::FallAccel, SynthWire::Wave]
        );
        assert_eq!(
            SynthWire::all_wire_strs(),
            vec!["fall_fade", "fall_accel", "wave"]
        );
    }

    #[test]
    fn wire_enum_from_wire_str_parses_and_falls_back() {
        assert_eq!(SynthWire::from_wire_str("fall_accel"), SynthWire::FallAccel);
        assert_eq!(SynthWire::from_wire_str("wave"), SynthWire::Wave);
        // Unknown input falls back to the #[default] variant.
        assert_eq!(SynthWire::from_wire_str("garbage"), SynthWire::FallFade);
        assert_eq!(SynthWire::from_wire_str(""), SynthWire::FallFade);
        // Round-trip every variant through its own wire string.
        for v in SynthWire::ALL {
            assert_eq!(SynthWire::from_wire_str(v.as_wire_str()), *v);
        }
    }

    #[test]
    fn wire_enum_as_u32_matches_discriminant() {
        assert_eq!(SynthWire::FallFade.as_u32(), 0);
        assert_eq!(SynthWire::FallAccel.as_u32(), 1);
        // The skipped range (2..=3) stays dead; the explicit value survives.
        assert_eq!(SynthWire::Wave.as_u32(), 4);
    }
}
