//! System font discovery using fontconfig (via font-kit)
//!
//! Provides a cached list of all installed font family names on the system.
//! Used by the settings GUI font picker to populate the selection slot list.

use std::sync::LazyLock;

use tracing::{debug, warn};

/// Lazily discovered and cached system font families
static SYSTEM_FONTS: LazyLock<Vec<String>> =
    LazyLock::new(
        || match font_kit::source::SystemSource::new().all_families() {
            Ok(mut families) => {
                families.sort_unstable_by_key(|f| f.to_lowercase());
                families.dedup();
                debug!("Discovered {} system font families", families.len());
                families
            }
            Err(e) => {
                warn!("Failed to discover system fonts: {e}");
                Vec::new()
            }
        },
    );

/// Return all installed system font families (cached after first call).
pub fn discover_system_fonts() -> Vec<String> {
    SYSTEM_FONTS.clone()
}

/// Return the distinct font weights available for `family`, each rounded to the
/// nearest CSS hundred and clamped to `100..=900` (e.g. `[400]` for a
/// Regular-only family, `[400, 500, 700]` for a multi-weight one). Returns an
/// empty vector when the family is unknown or cannot be introspected, which
/// callers treat as "trust the requested weight".
///
/// The UI font layer uses this to down-grade weighted text (Bold/Medium/…) to a
/// weight the family actually ships. Single-weight fonts such as pixel fonts
/// (e.g. Departure Mono, which ships only `Regular`) otherwise fall back to a
/// generic serif/sans for every non-Normal weight: iced's cosmic-text stack
/// drops a family entirely on a weight miss instead of reusing the in-family
/// face.
pub fn family_weights(family: &str) -> Vec<u16> {
    let handle = match font_kit::source::SystemSource::new().select_family_by_name(family) {
        Ok(handle) => handle,
        Err(e) => {
            debug!("font weights: family '{family}' not introspectable: {e}");
            return Vec::new();
        }
    };

    let mut weights: Vec<u16> = handle
        .fonts()
        .iter()
        .filter_map(|face| Some(round_css_weight(face.load().ok()?.properties().weight.0)))
        .collect();
    weights.sort_unstable();
    weights.dedup();
    weights
}

/// Round a raw CSS weight (e.g. `400.0`, `372.0`) to the nearest hundred,
/// clamped to the `100..=900` range iced's `Weight` enum can express.
fn round_css_weight(weight: f32) -> u16 {
    let hundreds = (weight / 100.0).round().clamp(1.0, 9.0) as u16;
    hundreds * 100
}

#[cfg(test)]
mod tests {
    use super::round_css_weight;

    #[test]
    fn rounds_css_weight_to_hundreds() {
        assert_eq!(round_css_weight(400.0), 400);
        assert_eq!(round_css_weight(372.0), 400);
        assert_eq!(round_css_weight(449.0), 400);
        assert_eq!(round_css_weight(451.0), 500);
        assert_eq!(round_css_weight(50.0), 100); // clamp low
        assert_eq!(round_css_weight(1000.0), 900); // clamp high
    }
}
