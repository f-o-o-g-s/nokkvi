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
