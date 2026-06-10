//! UI font configuration — independent of theme, stored in config.toml.

use std::sync::LazyLock;

use iced::Font;
use parking_lot::RwLock;

// ============================================================================
// Font Configuration (independent of theme — stored in config.toml)
// ============================================================================

/// Font family name, stored separately from themes so theme switches
/// don't change the user's font preference.
static FONT_FAMILY: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new(String::new()));

/// Cached font storage to avoid leaking memory on every reload.
/// Stores (font_name, resolved_font) pairs.
static FONT_CACHE: LazyLock<RwLock<(String, Font)>> =
    LazyLock::new(|| RwLock::new((String::new(), Font::DEFAULT)));

/// Get the UI font - loaded from config.toml (hot-reloadable)
/// Default: System sans-serif font (works on all systems)
#[inline]
pub(crate) fn ui_font() -> Font {
    let current_family = { FONT_FAMILY.read().clone() };

    // Fast path: check if cached font matches current config
    {
        let cache = FONT_CACHE.read();
        if cache.0 == current_family {
            return cache.1;
        }
    }

    // Slow path: font changed, update cache
    let new_font = if current_family.is_empty() {
        Font::DEFAULT
    } else {
        // Family::name() interns the string in a global FxHashSet (leaked once, deduped)
        Font::with_family(iced::font::Family::name(&current_family))
    };

    let mut cache = FONT_CACHE.write();
    *cache = (current_family, new_font);

    new_font
}

/// Set the font family (called on startup and when changed in settings).
pub(crate) fn set_font_family(family: String) {
    let mut guard = FONT_FAMILY.write();
    *guard = family;
}

/// Get the current font family name (for settings UI display).
pub(crate) fn font_family() -> String {
    FONT_FAMILY.read().clone()
}

// ----------------------------------------------------------------------------
// Title font (independent of body font — supports italic/serif title
// typefaces like IBM Plex Sans without affecting JetBrains-Mono-style body text)
// ----------------------------------------------------------------------------

// Title-font family was intended to give L1 hero titles + active-breadcrumb
// segments a typographic distinction from the body font, but the
// `set_title_font_family` setter never landed (the L5 settings lane it was
// dead-coded for was descoped). Every `theme::title_font()` call therefore
// returned the body font, shipping the design's distinction disabled. The
// machinery (`TITLE_FONT_FAMILY` static + `TITLE_FONT_CACHE` + the public
// readers) was removed — callers now use `ui_font()` directly. If a future
// design re-introduces the distinction, the simpler shape is a config-side
// `theme.title_font_family` field threaded through `ResolvedTheme` rather
// than a global mutable static.
