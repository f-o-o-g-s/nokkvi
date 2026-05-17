//! `define_labeled_enum!` — declarative macro for settings enums with paired
//! human-readable labels and snake_case wire strings.
//!
//! Many settings enums in `types/player_settings/` share an identical triple
//! of methods: `from_label(&str) -> Self`, `as_label(self) -> &'static str`,
//! and `impl Display` emitting the snake_case wire form. The macro generates
//! all three from a single per-variant declaration so adding a variant is a
//! one-line edit.
//!
//! # Shape
//!
//! ```ignore
//! use nokkvi_data::define_labeled_enum;
//! use serde::{Deserialize, Serialize};
//!
//! define_labeled_enum! {
//!     /// Doc comment for the enum.
//!     #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
//!     #[serde(rename_all = "snake_case")]
//!     pub enum Example {
//!         #[default] Alpha { label: "Alpha", wire: "alpha" },
//!         Beta { label: "Beta", wire: "beta" },
//!     }
//! }
//! ```
//!
//! # Emission
//!
//! For the call above, the macro expands to:
//! - The `pub enum Example { #[default] Alpha, Beta }` definition with every
//!   outer attribute preserved verbatim.
//! - `impl Example { pub fn from_label(label: &str) -> Self }` — matches each
//!   variant's `label` literal; the catch-all returns `Self::default()`, so
//!   the fallback variant is whichever the `#[default]` attribute marks.
//! - `impl Example { pub fn as_label(self) -> &'static str }` — total match
//!   over variants emitting the `label` literal.
//! - `impl std::fmt::Display for Example` — total match emitting the `wire`
//!   literal.
//!
//! # Notes
//!
//! - The outer `#[serde(rename_all = "...")]` attribute (snake_case, lowercase,
//!   etc.) is preserved exactly. The macro-emitted `Display` impl uses the
//!   per-variant `wire` literal, which the caller writes to match the serde
//!   wire format.
//! - `from_label` returns `Self::default()` on unknown input, so every enum
//!   passed through this macro must derive `Default` and mark its fallback
//!   variant with `#[default]`.
//! - Per-variant attributes (e.g. `#[default]`, `#[serde(...)]`) other than
//!   the `{ label, wire }` table are forwarded onto the emitted variant.

/// See module-level docs.
#[macro_export]
macro_rules! define_labeled_enum {
    (
        $(#[$enum_attr:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_attr:meta])*
                $variant:ident { label: $label:literal, wire: $wire:literal $(,)? }
            ),* $(,)?
        }
    ) => {
        $(#[$enum_attr])*
        $vis enum $name {
            $(
                $(#[$variant_attr])*
                $variant,
            )*
        }

        impl $name {
            /// Parse from the human-readable settings GUI label. Unknown
            /// labels fall back to `Self::default()` (the variant marked
            /// `#[default]`).
            pub fn from_label(label: &str) -> Self {
                match label {
                    $($label => Self::$variant,)*
                    _ => Self::default(),
                }
            }

            /// Render the human-readable settings GUI label.
            pub fn as_label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)*
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    $(Self::$variant => f.write_str($wire),)*
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    // Tiny synthetic enum exercising the macro emission shape.
    define_labeled_enum! {
        /// Doc comment passes through to the emitted enum.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum TestEnum {
            #[default]
            Alpha { label: "Alpha Label", wire: "alpha" },
            Beta { label: "Beta Label", wire: "beta" },
            GammaDelta { label: "Gamma Delta", wire: "gamma_delta" },
        }
    }

    #[test]
    fn from_label_known_variants() {
        assert_eq!(TestEnum::from_label("Alpha Label"), TestEnum::Alpha);
        assert_eq!(TestEnum::from_label("Beta Label"), TestEnum::Beta);
        assert_eq!(TestEnum::from_label("Gamma Delta"), TestEnum::GammaDelta);
    }

    #[test]
    fn from_label_unknown_falls_back_to_default() {
        assert_eq!(TestEnum::from_label("not a real label"), TestEnum::Alpha);
        assert_eq!(TestEnum::from_label(""), TestEnum::Alpha);
    }

    #[test]
    fn as_label_is_byte_identical_to_declaration() {
        assert_eq!(TestEnum::Alpha.as_label(), "Alpha Label");
        assert_eq!(TestEnum::Beta.as_label(), "Beta Label");
        assert_eq!(TestEnum::GammaDelta.as_label(), "Gamma Delta");
    }

    #[test]
    fn display_emits_wire_literal() {
        assert_eq!(TestEnum::Alpha.to_string(), "alpha");
        assert_eq!(TestEnum::Beta.to_string(), "beta");
        assert_eq!(TestEnum::GammaDelta.to_string(), "gamma_delta");
    }

    #[test]
    fn default_matches_attribute_marker() {
        assert_eq!(TestEnum::default(), TestEnum::Alpha);
    }

    #[test]
    fn label_roundtrip_for_every_variant() {
        for v in [TestEnum::Alpha, TestEnum::Beta, TestEnum::GammaDelta] {
            assert_eq!(TestEnum::from_label(v.as_label()), v);
        }
    }

    #[test]
    fn display_matches_serde_wire_format() {
        // The macro-emitted Display string must match the serde wire format
        // (which is governed by the outer `#[serde(rename_all = ...)]`).
        for v in [TestEnum::Alpha, TestEnum::Beta, TestEnum::GammaDelta] {
            let json = serde_json::to_string(&v).unwrap();
            // serde wraps in quotes; strip them.
            let serde_wire = json.trim_matches('"');
            assert_eq!(serde_wire, v.to_string(), "variant {v:?}");
        }
    }
}
