//! Runtime-togglable UI mode flags — the consolidated `UI_MODE` atomics and
//! their typed accessors (track-info strip, nav layout, slot rows, artwork
//! column, toolbar auto-hide, mini-player chrome).

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use nokkvi_data::types::player_settings::{RoundedMode, TrackInfoDisplay};
use tracing::debug;

use crate::atomic_u8_enum::{AtomicU8Enum, atomic_u8_enum};

// ============================================================================
// UI Mode Flags (grouped to avoid scattered statics)
// ============================================================================

/// All runtime-togglable UI mode flags, consolidated into one struct.
/// Each flag uses interior atomics for thread-safe lock-free access.
pub(super) struct UiModeFlags {
    /// Light/dark theme toggle
    pub(super) light_mode: AtomicBool,
    /// Rounded corner borders — tri-state (Off / On / PlayerOnly). Backed
    /// by `AtomicU8` via the `atomic_u8_enum!` impl on `RoundedMode`.
    pub(super) rounded_mode: AtomicU8,
    /// Track info display mode (`TrackInfoDisplay` discriminant)
    pub(super) track_info_display: AtomicU8,
    /// Navigation layout (`NavLayout` discriminant: Top / Side / None)
    pub(super) nav_layout: AtomicU8,
    /// Navigation display (`NavDisplayMode` discriminant)
    pub(super) nav_display_mode: AtomicU8,
    /// Target row height for slot lists (`SlotRowHeight` discriminant)
    pub(super) slot_row_height: AtomicU8,
    /// Whether the opacity gradient on non-center slots is enabled
    pub(super) opacity_gradient: AtomicBool,
    /// Whether clickable text links in slot list items are enabled
    pub(super) slot_text_links: AtomicBool,
    /// How the slot-list scrollbar is shown (`ScrollbarVisibility` discriminant:
    /// OnHover / Always / Hidden)
    pub(super) scrollbar_visibility: AtomicU8,
    /// Whether volume sliders are displayed horizontally in the player bar
    pub(super) horizontal_volume: AtomicBool,
    /// Whether the view-header toolbar auto-hides to a thin line until hovered
    pub(super) autohide_toolbar: AtomicBool,
    /// Collapsed auto-hide toolbar height in px (user-configurable)
    pub(super) autohide_toolbar_height: AtomicU8,
    /// Whether the collapsed auto-hide toolbar shows a centered accent grip bar
    pub(super) autohide_toolbar_grip: AtomicBool,
    /// What the collapsed auto-hide toolbar shows (Hairline / Hidden / Count strip)
    pub(super) autohide_collapsed_appearance: AtomicU8,
    /// Whether the mini-player bar shows the volume slider (mini-player mode only)
    pub(super) mini_player_show_volume: AtomicBool,
    /// Whether the mini-player bar shows the mode toggles / kebab menu
    /// (mini-player mode only)
    pub(super) mini_player_show_modes: AtomicBool,
    /// Whether the title field is shown in the track info strip
    pub(super) strip_show_title: AtomicBool,
    /// Whether the artist field is shown in the track info strip
    pub(super) strip_show_artist: AtomicBool,
    /// Whether the album field is shown in the track info strip
    pub(super) strip_show_album: AtomicBool,
    /// Whether format info (codec/kHz/kbps) is shown in the track info strip
    pub(super) strip_show_format_info: AtomicBool,
    /// Whether the metastrip renders artist/album/title as a single shared
    /// scrolling unit with one set of bookends.
    pub(super) strip_merged_mode: AtomicBool,
    /// Strip click action (`StripClickAction` discriminant)
    pub(super) strip_click_action: AtomicU8,
    /// Whether `title:` / `artist:` / `album:` labels are prepended to fields
    pub(super) strip_show_labels: AtomicBool,
    /// Strip merged-mode separator (`StripSeparator` discriminant)
    pub(super) strip_separator: AtomicU8,
    /// Whether the metadata text overlay is rendered on the large artwork in Albums view
    pub(super) albums_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Artists view
    pub(super) artists_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Songs view
    pub(super) songs_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Playlists view
    pub(super) playlists_artwork_overlay: AtomicBool,
    /// Artwork column display mode (`ArtworkColumnMode` discriminant)
    pub(super) artwork_column_mode: AtomicU8,
    /// Artwork stretch fit when column mode is AlwaysStretched or
    /// AlwaysVerticalStretched (`ArtworkStretchFit` discriminant)
    pub(super) artwork_column_stretch_fit: AtomicU8,
    /// Artwork column width as fraction of window width (f32 bits, 0.05..=0.80)
    pub(super) artwork_column_width_pct: AtomicU32,
    /// Auto-mode max artwork size as fraction of window short axis
    /// (f32 bits, 0.30..=0.70). Read by the Auto-mode resolver in
    /// base_slot_list_layout.rs to size both the horizontal candidate and the
    /// vertical-portrait fallback.
    pub(super) artwork_auto_max_pct: AtomicU32,
    /// Always-Vertical artwork height as fraction of window height
    /// (f32 bits, 0.10..=0.80). Read by the Always-Vertical resolver branch
    /// in base_slot_list_layout.rs to size the stacked artwork.
    pub(super) artwork_vertical_height_pct: AtomicU32,
}

pub(super) static UI_MODE: UiModeFlags = UiModeFlags {
    light_mode: AtomicBool::new(false),
    // RoundedMode::Off (not the enum's `#[default]`). PlayerSettings load
    // corrects this to the user's preference on first dump.
    rounded_mode: AtomicU8::new(RoundedMode::Off as u8),
    track_info_display: AtomicU8::new(TrackInfoDisplay::Off as u8),
    nav_layout: AtomicU8::new(NavLayout::Top as u8),
    nav_display_mode: AtomicU8::new(NavDisplayMode::TextOnly as u8),
    slot_row_height: AtomicU8::new(SlotRowHeight::Default as u8),
    opacity_gradient: AtomicBool::new(true),
    slot_text_links: AtomicBool::new(true),
    scrollbar_visibility: AtomicU8::new(ScrollbarVisibility::OnHover as u8),
    horizontal_volume: AtomicBool::new(false),
    autohide_toolbar: AtomicBool::new(false),
    autohide_toolbar_height: AtomicU8::new(6),
    autohide_toolbar_grip: AtomicBool::new(true),
    autohide_collapsed_appearance: AtomicU8::new(CollapsedAppearance::Hairline as u8),
    mini_player_show_volume: AtomicBool::new(true),
    mini_player_show_modes: AtomicBool::new(true),
    strip_show_title: AtomicBool::new(true),
    strip_show_artist: AtomicBool::new(true),
    strip_show_album: AtomicBool::new(true),
    strip_show_format_info: AtomicBool::new(true),
    strip_merged_mode: AtomicBool::new(false),
    strip_click_action: AtomicU8::new(StripClickAction::GoToQueue as u8),
    strip_show_labels: AtomicBool::new(true),
    strip_separator: AtomicU8::new(StripSeparator::Slash as u8),
    albums_artwork_overlay: AtomicBool::new(true),
    artists_artwork_overlay: AtomicBool::new(true),
    songs_artwork_overlay: AtomicBool::new(true),
    playlists_artwork_overlay: AtomicBool::new(true),
    artwork_column_mode: AtomicU8::new(ArtworkColumnMode::Auto as u8),
    artwork_column_stretch_fit: AtomicU8::new(ArtworkStretchFit::Cover as u8),
    // Initial values mirror the data-crate defaults in
    // `nokkvi_data::types::player_settings::artwork`. `f32::to_bits` is `const`
    // so the bit pattern is derived at compile time — no magic hex.
    artwork_column_width_pct: AtomicU32::new(
        nokkvi_data::types::player_settings::ARTWORK_COLUMN_WIDTH_PCT_DEFAULT.to_bits(),
    ),
    artwork_auto_max_pct: AtomicU32::new(
        nokkvi_data::types::player_settings::ARTWORK_AUTO_MAX_PCT_DEFAULT.to_bits(),
    ),
    artwork_vertical_height_pct: AtomicU32::new(
        nokkvi_data::types::player_settings::ARTWORK_VERTICAL_HEIGHT_PCT_DEFAULT.to_bits(),
    ),
};

atomic_u8_enum! {
    TrackInfoDisplay {
        Off,
        PlayerBar,
        TopBar,
        TopBarUnder,
        MiniPlayer,
    } default Off
}

/// Returns the current track info display mode
#[inline]
pub(crate) fn track_info_display() -> TrackInfoDisplay {
    TrackInfoDisplay::from_u8(UI_MODE.track_info_display.load(Ordering::Relaxed))
}

/// Set track info display mode (call when user changes the setting)
#[inline]
pub(crate) fn set_track_info_display(mode: TrackInfoDisplay) {
    UI_MODE
        .track_info_display
        .store(mode.to_u8(), Ordering::Relaxed);
    debug!(" Track info display changed: {}", mode);
}

/// Whether the player bar should show the track info strip below controls.
///
/// True when `TrackInfoDisplay::PlayerBar` is active.
///
/// **Single source of truth** — use this instead of ad-hoc compound checks.
#[inline]
pub(crate) fn show_player_bar_strip() -> bool {
    track_info_display() == TrackInfoDisplay::PlayerBar
}

/// Whether the top bar track info strip should be rendered above content.
///
/// True when the active strip mode renders a strip above the main content in
/// side-nav or none-nav layouts. Both `TopBar` and `TopBarUnder` map to the
/// same above-content position there — they only diverge in top-nav layout
/// (where `TopBar` lives inline in the nav row and `TopBarUnder` becomes its
/// own row below the nav — see [`show_top_bar_under_strip`]).
///
/// **Single source of truth** — use this instead of ad-hoc compound checks.
#[inline]
pub(crate) fn show_top_bar_strip() -> bool {
    matches!(
        track_info_display(),
        TrackInfoDisplay::TopBar | TrackInfoDisplay::TopBarUnder,
    ) && !is_top_nav()
}

/// Whether the player-bar-styled metadata strip should be rendered directly
/// beneath the top nav bar.
///
/// True when `TrackInfoDisplay::TopBarUnder` AND top-nav layout are both
/// active. The other layouts route through [`show_top_bar_strip`] instead.
#[inline]
pub(crate) fn show_top_bar_under_strip() -> bool {
    track_info_display() == TrackInfoDisplay::TopBarUnder && is_top_nav()
}

/// Whether the artwork-elevation feature is *enabled* by the user's theme
/// settings.
///
/// True when the top-nav layout is active AND the metadata strip lives
/// somewhere other than the top bar (i.e. `Off`, `PlayerBar`, or
/// `MiniPlayer`) — in those modes the top nav doesn't carry any
/// now-playing metadata, so its right portion is free real estate that
/// the artwork can take over. `MiniPlayer` keeps its own artwork inside
/// the player bar; that doesn't conflict with the top-nav elevation
/// because they live on different rows.
///
/// `TopBar` keeps the regular column-stacked layout because the metadata
/// strip still needs the full nav width. `TopBarUnder` likewise opts out:
/// its strip sits as its own full-width row directly beneath the nav, so
/// elevating the artwork into that band would either cover the strip or
/// require the strip to span only the slot-list column. Easier to just
/// disable elevation whenever a top-area strip is visible.
///
/// This is only the *theme* gate — `Nokkvi::elevated_artwork_extent`
/// additionally excludes split-view, ineligible views, and the Auto-mode
/// portrait fallback before publishing the result through each `*ViewData`
/// as `BaseSlotListLayoutConfig::elevated`, which `horizontal_layout`
/// finally reads.
#[inline]
pub(crate) fn is_artwork_elevated() -> bool {
    is_top_nav()
        && !matches!(
            track_info_display(),
            TrackInfoDisplay::TopBar | TrackInfoDisplay::TopBarUnder,
        )
}

// ============================================================================
// Nav Layout Control
// ============================================================================

use nokkvi_data::types::player_settings::{NavDisplayMode, NavLayout};

atomic_u8_enum! {
    NavLayout {
        Top,
        Side,
        None,
    } default Top
}

/// Returns true if side navigation layout is active
#[inline]
pub(crate) fn is_side_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == NavLayout::Side as u8
}

/// Returns true if the minimalist (no-chrome) layout is active
#[inline]
pub(crate) fn is_none_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == NavLayout::None as u8
}

/// Returns true if the top-bar navigation layout is active (the default)
#[inline]
pub(crate) fn is_top_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == NavLayout::Top as u8
}

/// Current navigation layout — bytes round-trip through `NavLayout::from_u8`
/// (unknown bytes fall back to `Top`, the declared default; see the
/// `atomic_u8_enum!` macro's defensive-fallback contract).
///
/// Test-only: production code uses `is_top_nav()` / `is_side_nav()` /
/// `is_none_nav()` directly; the enum-shaped reader is here so chrome-math
/// tests can save/restore the active variant in a single hop.
#[cfg(test)]
#[inline]
pub(crate) fn nav_layout() -> NavLayout {
    NavLayout::from_u8(UI_MODE.nav_layout.load(Ordering::Relaxed))
}

/// Set the navigation layout from a NavLayout enum value
#[inline]
pub(crate) fn set_nav_layout(layout: NavLayout) {
    UI_MODE.nav_layout.store(layout.to_u8(), Ordering::Relaxed);
    debug!(" Nav layout changed: nav_layout={}", layout);
}

// ============================================================================
// Nav Display Mode Control
// ============================================================================

atomic_u8_enum! {
    NavDisplayMode {
        TextOnly,
        TextAndIcons,
        IconsOnly,
    } default TextOnly
}

/// Get the current navigation display mode
#[inline]
pub(crate) fn nav_display_mode() -> NavDisplayMode {
    NavDisplayMode::from_u8(UI_MODE.nav_display_mode.load(Ordering::Relaxed))
}

/// Set the navigation display mode from a NavDisplayMode enum value
#[inline]
pub(crate) fn set_nav_display_mode(mode: NavDisplayMode) {
    UI_MODE
        .nav_display_mode
        .store(mode.to_u8(), Ordering::Relaxed);
    debug!(" Nav display mode changed: nav_display_mode={}", mode);
}

// ============================================================================
// Slot Row Height Control
// ============================================================================

/// Get the current target row height for slot lists (in pixels)
#[inline]
pub(crate) fn slot_row_height() -> f32 {
    let variant = slot_row_height_variant();
    variant.to_pixels() as f32
}

use nokkvi_data::types::player_settings::SlotRowHeight;

atomic_u8_enum! {
    SlotRowHeight {
        Compact,
        Default,
        Comfortable,
        Spacious,
    } default Default
}

/// Get the current slot row height enum variant
#[inline]
pub(crate) fn slot_row_height_variant() -> SlotRowHeight {
    SlotRowHeight::from_u8(UI_MODE.slot_row_height.load(Ordering::Relaxed))
}

/// Set the target row height for slot lists
#[inline]
pub(crate) fn set_slot_row_height(height: SlotRowHeight) {
    UI_MODE
        .slot_row_height
        .store(height.to_u8(), Ordering::Relaxed);
    debug!(
        " Slot row height changed: {} ({}px)",
        height.as_label(),
        height.to_pixels()
    );
}

// ============================================================================
// Opacity Gradient Control
// ============================================================================

/// Returns true if the distance-based opacity gradient is enabled on slot lists
#[inline]
pub(crate) fn is_opacity_gradient() -> bool {
    UI_MODE.opacity_gradient.load(Ordering::Relaxed)
}

/// Set opacity gradient state (call when user toggles the setting)
#[inline]
pub(crate) fn set_opacity_gradient(enabled: bool) {
    UI_MODE.opacity_gradient.store(enabled, Ordering::Relaxed);
    debug!(" Opacity gradient changed: opacity_gradient={}", enabled);
}

// ============================================================================
// Slot Text Links Control
// ============================================================================

/// Returns true if clickable text links in slot list items are enabled
#[inline]
pub(crate) fn is_slot_text_links() -> bool {
    UI_MODE.slot_text_links.load(Ordering::Relaxed)
}

/// Set slot text links state (call when user toggles the setting)
#[inline]
pub(crate) fn set_slot_text_links(enabled: bool) {
    UI_MODE.slot_text_links.store(enabled, Ordering::Relaxed);
    debug!(" Slot text links changed: slot_text_links={}", enabled);
}

// ============================================================================
// Horizontal Volume Control
// ============================================================================

/// Returns true if horizontal volume sliders are enabled in the player bar
#[inline]
pub(crate) fn is_horizontal_volume() -> bool {
    UI_MODE.horizontal_volume.load(Ordering::Relaxed)
}

/// Set horizontal volume slider mode (call when user toggles the setting)
#[inline]
pub(crate) fn set_horizontal_volume(enabled: bool) {
    UI_MODE.horizontal_volume.store(enabled, Ordering::Relaxed);
    debug!(" Horizontal volume changed: horizontal_volume={}", enabled);
}

/// Returns true if the view-header toolbar auto-hides until hovered / shortcut
#[inline]
pub(crate) fn is_autohide_toolbar() -> bool {
    UI_MODE.autohide_toolbar.load(Ordering::Relaxed)
}

/// Set view-header toolbar auto-hide mode (call when user toggles the setting)
#[inline]
pub(crate) fn set_autohide_toolbar(enabled: bool) {
    UI_MODE.autohide_toolbar.store(enabled, Ordering::Relaxed);
    debug!(" Autohide toolbar changed: autohide_toolbar={}", enabled);
}

/// Collapsed auto-hide toolbar height in px (user-configurable).
#[inline]
pub(crate) fn autohide_toolbar_height_px() -> u8 {
    UI_MODE.autohide_toolbar_height.load(Ordering::Relaxed)
}

/// Set the collapsed auto-hide toolbar height in px.
#[inline]
pub(crate) fn set_autohide_toolbar_height_px(px: u8) {
    UI_MODE.autohide_toolbar_height.store(px, Ordering::Relaxed);
}

/// Returns true if the collapsed auto-hide toolbar shows its accent grip bar.
#[inline]
pub(crate) fn is_autohide_toolbar_grip() -> bool {
    UI_MODE.autohide_toolbar_grip.load(Ordering::Relaxed)
}

/// Set whether the collapsed auto-hide toolbar shows its accent grip bar.
#[inline]
pub(crate) fn set_autohide_toolbar_grip(enabled: bool) {
    UI_MODE
        .autohide_toolbar_grip
        .store(enabled, Ordering::Relaxed);
}

use nokkvi_data::types::player_settings::CollapsedAppearance;

atomic_u8_enum! {
    CollapsedAppearance {
        Hairline,
        Hidden,
        CountStrip,
    } default Hairline
}

/// What the collapsed auto-hide toolbar shows (Hairline / Hidden / Count strip).
#[inline]
pub(crate) fn autohide_collapsed_appearance() -> CollapsedAppearance {
    CollapsedAppearance::from_u8(
        UI_MODE
            .autohide_collapsed_appearance
            .load(Ordering::Relaxed),
    )
}

/// Set the collapsed auto-hide toolbar appearance.
#[inline]
pub(crate) fn set_autohide_collapsed_appearance(mode: CollapsedAppearance) {
    UI_MODE
        .autohide_collapsed_appearance
        .store(mode.to_u8(), Ordering::Relaxed);
}

use nokkvi_data::types::player_settings::ScrollbarVisibility;

atomic_u8_enum! {
    ScrollbarVisibility {
        OnHover,
        Always,
        Hidden,
    } default OnHover
}

/// How the slot-list scrollbar is shown (OnHover / Always / Hidden). Read by
/// `wrap_with_scroll_indicator` + `ScrollbarOverlay` at draw time.
#[inline]
pub(crate) fn scrollbar_visibility() -> ScrollbarVisibility {
    ScrollbarVisibility::from_u8(UI_MODE.scrollbar_visibility.load(Ordering::Relaxed))
}

/// Set the slot-list scrollbar visibility mode.
#[inline]
pub(crate) fn set_scrollbar_visibility(mode: ScrollbarVisibility) {
    UI_MODE
        .scrollbar_visibility
        .store(mode.to_u8(), Ordering::Relaxed);
}

/// Returns true if the mini-player bar shows the volume slider (mini-player
/// mode only)
#[inline]
pub(crate) fn mini_player_show_volume() -> bool {
    UI_MODE.mini_player_show_volume.load(Ordering::Relaxed)
}

/// Set whether the mini-player bar shows the volume slider (call when the user
/// toggles the setting)
#[inline]
pub(crate) fn set_mini_player_show_volume(shown: bool) {
    UI_MODE
        .mini_player_show_volume
        .store(shown, Ordering::Relaxed);
    debug!("Mini-player show volume changed: mini_player_show_volume={shown}");
}

/// Returns true if the mini-player bar shows the mode toggles / kebab menu
/// (mini-player mode only)
#[inline]
pub(crate) fn mini_player_show_modes() -> bool {
    UI_MODE.mini_player_show_modes.load(Ordering::Relaxed)
}

/// Set whether the mini-player bar shows the mode toggles / kebab menu (call
/// when the user toggles the setting)
#[inline]
pub(crate) fn set_mini_player_show_modes(shown: bool) {
    UI_MODE
        .mini_player_show_modes
        .store(shown, Ordering::Relaxed);
    debug!("Mini-player show modes changed: mini_player_show_modes={shown}");
}

// ============================================================================
// Strip Field Visibility Controls
// ============================================================================

use nokkvi_data::types::player_settings::{StripClickAction, StripSeparator};

atomic_u8_enum! {
    StripClickAction {
        GoToQueue,
        GoToAlbum,
        GoToArtist,
        CopyTrackInfo,
        DoNothing,
    } default GoToQueue
}

atomic_u8_enum! {
    StripSeparator {
        Dot,
        Bullet,
        Pipe,
        EmDash,
        Slash,
        Bar,
    } default Slash
}

/// Returns true if the title field is visible in the track info strip
#[inline]
pub(crate) fn strip_show_title() -> bool {
    UI_MODE.strip_show_title.load(Ordering::Relaxed)
}

/// Set strip title visibility
#[inline]
pub(crate) fn set_strip_show_title(enabled: bool) {
    UI_MODE.strip_show_title.store(enabled, Ordering::Relaxed);
}

/// Returns true if the artist field is visible in the track info strip
#[inline]
pub(crate) fn strip_show_artist() -> bool {
    UI_MODE.strip_show_artist.load(Ordering::Relaxed)
}

/// Set strip artist visibility
#[inline]
pub(crate) fn set_strip_show_artist(enabled: bool) {
    UI_MODE.strip_show_artist.store(enabled, Ordering::Relaxed);
}

/// Returns true if the album field is visible in the track info strip
#[inline]
pub(crate) fn strip_show_album() -> bool {
    UI_MODE.strip_show_album.load(Ordering::Relaxed)
}

/// Set strip album visibility
#[inline]
pub(crate) fn set_strip_show_album(enabled: bool) {
    UI_MODE.strip_show_album.store(enabled, Ordering::Relaxed);
}

/// Returns true if format info (codec/kHz/kbps) is visible in the track info strip
#[inline]
pub(crate) fn strip_show_format_info() -> bool {
    UI_MODE.strip_show_format_info.load(Ordering::Relaxed)
}

/// Set strip format info visibility
#[inline]
pub(crate) fn set_strip_show_format_info(enabled: bool) {
    UI_MODE
        .strip_show_format_info
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metastrip renders artist/album/title as a single
/// shared scrolling unit with one set of bookend separators.
#[inline]
pub(crate) fn strip_merged_mode() -> bool {
    UI_MODE.strip_merged_mode.load(Ordering::Relaxed)
}

/// Set strip merged mode
#[inline]
pub(crate) fn set_strip_merged_mode(enabled: bool) {
    UI_MODE.strip_merged_mode.store(enabled, Ordering::Relaxed);
}

/// Returns the current strip click action
#[inline]
pub(crate) fn strip_click_action() -> StripClickAction {
    StripClickAction::from_u8(UI_MODE.strip_click_action.load(Ordering::Relaxed))
}

/// Set strip click action (call when user changes the setting)
#[inline]
pub(crate) fn set_strip_click_action(action: StripClickAction) {
    UI_MODE
        .strip_click_action
        .store(action.to_u8(), Ordering::Relaxed);
}

/// Returns true if `title:` / `artist:` / `album:` labels are shown in the strip
#[inline]
pub(crate) fn strip_show_labels() -> bool {
    UI_MODE.strip_show_labels.load(Ordering::Relaxed)
}

/// Set strip label visibility
#[inline]
pub(crate) fn set_strip_show_labels(enabled: bool) {
    UI_MODE.strip_show_labels.store(enabled, Ordering::Relaxed);
}

/// Returns the active strip merged-mode field separator
#[inline]
pub(crate) fn strip_separator() -> StripSeparator {
    StripSeparator::from_u8(UI_MODE.strip_separator.load(Ordering::Relaxed))
}

/// Set strip merged-mode field separator
#[inline]
pub(crate) fn set_strip_separator(sep: StripSeparator) {
    UI_MODE
        .strip_separator
        .store(sep.to_u8(), Ordering::Relaxed);
}

// ============================================================================
// Per-View Artwork Text Overlay Controls
// ============================================================================

/// Returns true if the metadata text overlay is shown on the large artwork in Albums view
#[inline]
pub(crate) fn albums_artwork_overlay() -> bool {
    UI_MODE.albums_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Albums view artwork text overlay visibility
#[inline]
pub(crate) fn set_albums_artwork_overlay(enabled: bool) {
    UI_MODE
        .albums_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metadata text overlay is shown on the large artwork in Artists view
#[inline]
pub(crate) fn artists_artwork_overlay() -> bool {
    UI_MODE.artists_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Artists view artwork text overlay visibility
#[inline]
pub(crate) fn set_artists_artwork_overlay(enabled: bool) {
    UI_MODE
        .artists_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metadata text overlay is shown on the large artwork in Songs view
#[inline]
pub(crate) fn songs_artwork_overlay() -> bool {
    UI_MODE.songs_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Songs view artwork text overlay visibility
#[inline]
pub(crate) fn set_songs_artwork_overlay(enabled: bool) {
    UI_MODE
        .songs_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metadata text overlay is shown on the large artwork in Playlists view
#[inline]
pub(crate) fn playlists_artwork_overlay() -> bool {
    UI_MODE.playlists_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Playlists view artwork text overlay visibility
#[inline]
pub(crate) fn set_playlists_artwork_overlay(enabled: bool) {
    UI_MODE
        .playlists_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

// ============================================================================
// Artwork Column Layout
// ============================================================================

use nokkvi_data::types::player_settings::{
    ARTWORK_AUTO_MAX_PCT_MAX, ARTWORK_AUTO_MAX_PCT_MIN, ARTWORK_COLUMN_WIDTH_PCT_MAX,
    ARTWORK_COLUMN_WIDTH_PCT_MIN, ARTWORK_VERTICAL_HEIGHT_PCT_MAX, ARTWORK_VERTICAL_HEIGHT_PCT_MIN,
    ArtworkColumnMode, ArtworkStretchFit,
};

// Encoding NOTE: the bytes are the enum's declaration discriminants and are
// a transient in-process cache encoding only — nothing persists them
// (persistence goes through serde wire strings), so renumbering variants is
// safe. The `atomic_u8_enum!` loader falls back to `Auto` for unknown values
// and the store half is enum-exhaustive (so adding a variant to the
// data-crate enum forces a compile error here).
atomic_u8_enum! {
    ArtworkColumnMode {
        Auto,
        AlwaysNative,
        AlwaysStretched,
        AlwaysVerticalNative,
        AlwaysVerticalStretched,
        Never,
    } default Auto
}

atomic_u8_enum! {
    ArtworkStretchFit {
        Cover,
        Fill,
    } default Cover
}

/// Returns the active artwork column display mode.
#[inline]
pub(crate) fn artwork_column_mode() -> ArtworkColumnMode {
    ArtworkColumnMode::from_u8(UI_MODE.artwork_column_mode.load(Ordering::Relaxed))
}

/// Set the artwork column display mode (call when user changes the setting).
#[inline]
pub(crate) fn set_artwork_column_mode(mode: ArtworkColumnMode) {
    UI_MODE
        .artwork_column_mode
        .store(mode.to_u8(), Ordering::Relaxed);
}

/// Returns the active artwork stretch fit (only meaningful in AlwaysStretched mode).
#[inline]
pub(crate) fn artwork_column_stretch_fit() -> ArtworkStretchFit {
    ArtworkStretchFit::from_u8(UI_MODE.artwork_column_stretch_fit.load(Ordering::Relaxed))
}

/// Set the artwork stretch fit.
#[inline]
pub(crate) fn set_artwork_column_stretch_fit(fit: ArtworkStretchFit) {
    UI_MODE
        .artwork_column_stretch_fit
        .store(fit.to_u8(), Ordering::Relaxed);
}

/// Returns the artwork column width fraction (0.05..=0.80).
#[inline]
pub(crate) fn artwork_column_width_pct() -> f32 {
    f32::from_bits(UI_MODE.artwork_column_width_pct.load(Ordering::Relaxed))
}

/// Set the artwork column width fraction. Clamps into the data-crate range.
#[inline]
pub(crate) fn set_artwork_column_width_pct(pct: f32) {
    let clamped = pct.clamp(ARTWORK_COLUMN_WIDTH_PCT_MIN, ARTWORK_COLUMN_WIDTH_PCT_MAX);
    UI_MODE
        .artwork_column_width_pct
        .store(clamped.to_bits(), Ordering::Relaxed);
}

/// Returns the Auto-mode max artwork fraction (0.30..=0.70). The resolver
/// uses this for both the horizontal candidate and the portrait-fallback
/// vertical candidate.
#[inline]
pub(crate) fn artwork_auto_max_pct() -> f32 {
    f32::from_bits(UI_MODE.artwork_auto_max_pct.load(Ordering::Relaxed))
}

/// Set the Auto-mode max artwork fraction. Clamps into the data-crate range.
#[inline]
pub(crate) fn set_artwork_auto_max_pct(pct: f32) {
    let clamped = pct.clamp(ARTWORK_AUTO_MAX_PCT_MIN, ARTWORK_AUTO_MAX_PCT_MAX);
    UI_MODE
        .artwork_auto_max_pct
        .store(clamped.to_bits(), Ordering::Relaxed);
}

/// Returns the Always-Vertical artwork height fraction (0.10..=0.80). Read
/// by the resolver for AlwaysVerticalNative and AlwaysVerticalStretched.
#[inline]
pub(crate) fn artwork_vertical_height_pct() -> f32 {
    f32::from_bits(UI_MODE.artwork_vertical_height_pct.load(Ordering::Relaxed))
}

/// Set the Always-Vertical artwork height fraction. Clamps into the data-crate range.
#[inline]
pub(crate) fn set_artwork_vertical_height_pct(pct: f32) {
    let clamped = pct.clamp(
        ARTWORK_VERTICAL_HEIGHT_PCT_MIN,
        ARTWORK_VERTICAL_HEIGHT_PCT_MAX,
    );
    UI_MODE
        .artwork_vertical_height_pct
        .store(clamped.to_bits(), Ordering::Relaxed);
}
