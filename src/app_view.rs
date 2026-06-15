//! View functions for Nokkvi
//!
//! Contains all rendering logic: view(), login_view(), home_view(), navigation_bar(), main_content()

use iced::{
    Element, Length,
    widget::{Stack, column, container},
};

use crate::{
    Nokkvi, Screen, View,
    app_message::{Message, NavigationMessage},
    views, widgets,
};

// ============================================================================
// View ⇄ NavView conversions
// ============================================================================
//
// `View` is the app's screen enum; `NavView` is the nav-bar widget's column
// enum. Settings and PlaylistEditor are contextual destinations with no
// nav-bar column, so the conversion is naturally lossy in one direction —
// `Option<NavView>` handles that without per-call-site `match` boilerplate.
// The variants that do have columns pair 1:1 by name, so each impl is just a
// small renaming table.

impl From<View> for Option<widgets::NavView> {
    fn from(v: View) -> Self {
        match v {
            View::Queue => Some(widgets::NavView::Queue),
            View::Albums => Some(widgets::NavView::Albums),
            View::Artists => Some(widgets::NavView::Artists),
            View::Genres => Some(widgets::NavView::Genres),
            View::Songs => Some(widgets::NavView::Songs),
            View::Playlists => Some(widgets::NavView::Playlists),
            View::Radios => Some(widgets::NavView::Radios),
            View::Settings => None,
            // Contextual destinations with no permanent nav tab.
            View::PlaylistEditor => None,
        }
    }
}

impl From<widgets::NavView> for View {
    fn from(nav: widgets::NavView) -> Self {
        match nav {
            widgets::NavView::Queue => View::Queue,
            widgets::NavView::Albums => View::Albums,
            widgets::NavView::Artists => View::Artists,
            widgets::NavView::Genres => View::Genres,
            widgets::NavView::Songs => View::Songs,
            widgets::NavView::Playlists => View::Playlists,
            widgets::NavView::Radios => View::Radios,
        }
    }
}

/// Extract `(is_open, trigger_bounds)` for a view's column-visibility
/// `checkbox_dropdown` from the root-level open-menu state. Returns the open
/// flag plus the captured trigger bounds, or `(false, None)` if a different
/// menu (or none) is open.
fn column_dropdown_state(
    open_menu: &Option<crate::app_message::OpenMenu>,
    view: View,
) -> (bool, Option<iced::Rectangle>) {
    use crate::app_message::OpenMenu;
    match open_menu {
        Some(OpenMenu::CheckboxDropdown {
            view: v,
            trigger_bounds,
        }) if *v == view => (true, Some(*trigger_bounds)),
        _ => (false, None),
    }
}

/// Sibling of [`column_dropdown_state`] for the Similar view, which uses its
/// own `OpenMenu` variant because it has no matching `View` enum value.
fn similar_column_dropdown_state(
    open_menu: &Option<crate::app_message::OpenMenu>,
) -> (bool, Option<iced::Rectangle>) {
    use crate::app_message::OpenMenu;
    match open_menu {
        Some(OpenMenu::CheckboxDropdownSimilar { trigger_bounds }) => (true, Some(*trigger_bounds)),
        _ => (false, None),
    }
}

/// Sibling of [`column_dropdown_state`] for the library selector popover.
/// Called from `navigation_bar` to feed `(is_open, trigger_bounds)` into
/// the nav-bar trigger; the paired popover overlay reads the same state.
fn library_selector_state(
    open_menu: &Option<crate::app_message::OpenMenu>,
) -> (bool, Option<iced::Rectangle>) {
    use crate::app_message::OpenMenu;
    match open_menu {
        Some(OpenMenu::LibrarySelector { trigger_bounds }) => (true, Some(*trigger_bounds)),
        _ => (false, None),
    }
}

/// Extract `(is_open, position)` for the now-playing strip context menu (only
/// one strip is on-screen at a time, so the `Strip` id is unambiguous).
fn strip_context_state(
    open_menu: &Option<crate::app_message::OpenMenu>,
) -> (bool, Option<iced::Point>) {
    use crate::app_message::{ContextMenuId, OpenMenu};
    match open_menu {
        Some(OpenMenu::Context {
            id: ContextMenuId::Strip,
            position,
        }) => (true, Some(*position)),
        _ => (false, None),
    }
}

/// Library-filter chrome state aggregated for the nav-bar trigger + popover.
///
/// Built by `Nokkvi::library_filter_view_data()`; consumed by both the
/// top-nav `NavBarViewData` builder and the side-nav `SideNavBarData`
/// builder so the two share the same library list / counter / popover
/// state without re-deriving it from `AppService`.
pub(crate) struct LibraryFilterViewData {
    pub count: usize,
    pub active_count: usize,
    pub rows: Vec<(i32, String, Option<u32>, bool)>,
    pub popover_open: bool,
    pub trigger_bounds: Option<iced::Rectangle>,
}

/// Convert a `NavBarMessage` into the root `Message` type.
///
/// Shared between the horizontal nav bar (top mode) and the vertical
/// side nav bar — avoids duplicating the `NavView → View` mapping.
fn map_nav_bar_message(msg: widgets::NavBarMessage) -> Message {
    match msg {
        widgets::NavBarMessage::SwitchView(nav_view) => {
            Message::Navigation(NavigationMessage::SwitchView(nav_view.into()))
        }
        widgets::NavBarMessage::SwitchToEditor => {
            Message::Navigation(NavigationMessage::SwitchView(View::PlaylistEditor))
        }
        widgets::NavBarMessage::ToggleLightMode => Message::ToggleLightMode,
        widgets::NavBarMessage::OpenSettings => {
            Message::Navigation(NavigationMessage::SwitchView(View::Settings))
        }
        widgets::NavBarMessage::StripClicked => Message::StripClicked,
        widgets::NavBarMessage::StripContextAction(entry) => Message::StripContextAction(entry),
        widgets::NavBarMessage::SetOpenMenu(next) => Message::SetOpenMenu(next),
        widgets::NavBarMessage::LibraryOpenChange {
            open,
            trigger_bounds,
        } => Message::Library(crate::app_message::LibraryMessage::OpenChange {
            open,
            trigger_bounds,
        }),
        widgets::NavBarMessage::LibraryToggle(id) => {
            Message::Library(crate::app_message::LibraryMessage::Toggle(id))
        }
        widgets::NavBarMessage::About => {
            Message::AboutModal(crate::widgets::about_modal::AboutModalMessage::Open)
        }
        widgets::NavBarMessage::Quit => Message::QuitApp,
    }
}

// ============================================================================
// Split-view pane proportions (single source of truth)
// ============================================================================
//
// The browsing-panel split lays the two panes out with iced `FillPortion`
// (a relative integer weight), while each pane's slot-list math needs an
// absolute pixel width derived as a fraction of `content_pane_width()`.
// Both derive from the SAME two portion weights so a retune (e.g. 60/40)
// can never leave the fraction and the FillPortion out of step. The derived
// f32 fractions are bit-identical to the prior hardcoded 0.55 / 0.45.

/// iced `FillPortion` weight for the queue (left) pane in split view.
const QUEUE_PANE_PORTION: u16 = 55;
/// iced `FillPortion` weight for the browsing (right) pane in split view.
const BROWSER_PANE_PORTION: u16 = 45;

/// Fraction of `content_pane_width()` a pane occupies, derived from its
/// `FillPortion` weight relative to the total. `const fn` so both the
/// compile-time guard and the call sites share one definition.
const fn pane_width_fraction(portion: u16) -> f32 {
    portion as f32 / (QUEUE_PANE_PORTION + BROWSER_PANE_PORTION) as f32
}

/// Slot-math width fraction for the queue (left) pane.
const QUEUE_PANE_FRACTION: f32 = pane_width_fraction(QUEUE_PANE_PORTION);
/// Slot-math width fraction for the browsing (right) pane.
const BROWSER_PANE_FRACTION: f32 = pane_width_fraction(BROWSER_PANE_PORTION);

// Rendered-value parity guard: the derived fractions must equal the literals
// they replaced, so a portion retune cannot silently shift pixels off-ratio.
const _: () = assert!(QUEUE_PANE_FRACTION + BROWSER_PANE_FRACTION == 1.0);

impl Nokkvi {
    // =========================================================================
    // SECTION: View Functions
    // =========================================================================

    /// Aggregate the library-filter chrome state that the nav-bar trigger +
    /// popover read every frame.
    ///
    /// `library_count == 0` (pre-login or before `refresh_libraries` lands)
    /// suppresses the trigger so the user never sees a chrome flicker
    /// before the count is known.
    ///
    /// `active.is_empty()` is the "all libraries" convention — every row
    /// renders as checked and the counter reads `{total} / {total}`. Rows
    /// are sorted by name so the popover order stays stable across reloads.
    ///
    /// Shared between the top-nav `navigation_bar()` and the side-nav's
    /// `SideNavBarData` builder so a future tweak (different sort order,
    /// added per-library metadata column, etc.) lands at one site.
    pub(crate) fn library_filter_view_data(&self) -> LibraryFilterViewData {
        let (count, active_count, rows) = match &self.app_service {
            Some(svc) => {
                let active = svc.active_library_ids();
                let all_checked = active.is_empty();
                let mut rows: Vec<(i32, String, Option<u32>, bool)> = svc
                    .all_libraries()
                    .into_iter()
                    .map(|lib| {
                        let checked = all_checked || active.contains(&lib.id);
                        (lib.id, lib.name, lib.song_count, checked)
                    })
                    .collect();
                rows.sort_by(|a, b| a.1.cmp(&b.1));
                let active_for_display = if all_checked {
                    rows.len()
                } else {
                    active.len()
                };
                (svc.library_count(), active_for_display, rows)
            }
            None => (0, 0, Vec::new()),
        };
        let (open, bounds) = library_selector_state(&self.open_menu);
        LibraryFilterViewData {
            count,
            active_count,
            rows,
            popover_open: open,
            trigger_bounds: bounds,
        }
    }

    /// The currently-playing queue track's `album_id`, or `None` for radio
    /// playback / no current song / the song being absent from
    /// `library.queue_songs`.
    ///
    /// Shared resolution root for the now-playing artwork surfaces
    /// ([`mini_player_artwork`](Self::mini_player_artwork) and
    /// [`now_playing_artwork_to_warm`](Self::now_playing_artwork_to_warm)) so
    /// the two cannot drift on how "the playing album" is derived.
    fn current_queue_song_album_id(&self) -> Option<&str> {
        if self.active_playback.is_radio() {
            return None;
        }
        let sid = self.scrobble.current_song_id.as_deref()?;
        self.library
            .queue_songs
            .iter()
            .find(|s| s.id == sid)
            .map(|s| s.album_id.as_str())
    }

    /// Resolve the artwork handle for the player-bar mini-player section.
    ///
    /// Returns `None` whenever any of the following holds, in order:
    /// - `track_info_display() != TrackInfoDisplay::MiniPlayer` (every other
    ///   strip mode hides the mini-player section, so resolving artwork is
    ///   wasted work — short-circuit before walking the queue).
    /// - Radio playback is active, no `scrobble.current_song_id`, or the
    ///   current song isn't in `library.queue_songs` (see
    ///   [`current_queue_song_album_id`](Self::current_queue_song_album_id)).
    /// - Neither the large nor the mini LRU has a cached handle for the song's
    ///   `album_id`.
    ///
    /// Otherwise returns the cached handle (large preferred, mini fallback so
    /// the thumbnail still appears while the full-size art is in flight).
    pub(crate) fn mini_player_artwork(&self) -> Option<iced::widget::image::Handle> {
        use nokkvi_data::types::player_settings::TrackInfoDisplay;
        if crate::theme::track_info_display() != TrackInfoDisplay::MiniPlayer {
            return None;
        }
        let album_id = self.current_queue_song_album_id()?;
        self.artwork
            .large_artwork
            .snapshot
            .get(album_id)
            .or_else(|| self.artwork.album_art.snapshot.get(album_id))
            .cloned()
    }

    /// The now-playing queue track's `album_id` when its artwork is cached in
    /// **neither** the large nor the mini LRU — i.e. the album that a
    /// song-change-driven warm must fetch so the now-playing surfaces have art
    /// independent of the slot-list viewport. `None` otherwise (radio, no
    /// current song, song absent from the queue, or the album already warm in
    /// either LRU).
    ///
    /// This is the inverse of [`mini_player_artwork`](Self::mini_player_artwork)'s
    /// cache lookup. Crucially it treats the album as warm when **either** LRU
    /// holds it: the mini-player paints the 80 px `album_art` fallback, so an
    /// album with only the mini cached is not a gray box and re-fetching it
    /// would be wasted work.
    ///
    /// Deliberately **not** gated on `track_info_display() == MiniPlayer`:
    /// warming the now-playing album on song change is cheap (one deduped 80 px
    /// fetch) and also benefits the queue view's now-playing artwork tier, and
    /// leaving the strip-mode gate out keeps the warm path off the process-wide
    /// `UI_MODE` atomic.
    pub(crate) fn now_playing_artwork_to_warm(&self) -> Option<String> {
        let album_id = self.current_queue_song_album_id()?;
        if self.artwork.large_artwork.snapshot.contains_key(album_id)
            || self.artwork.album_art.snapshot.contains_key(album_id)
        {
            None
        } else {
            Some(album_id.to_string())
        }
    }

    /// Horizontal extent of the content pane — everything inside the outer
    /// chrome that the per-view widgets render into.
    ///
    /// In `NavLayout::Side` the vertical sidebar consumes the live width
    /// reported by `side_nav_total_width()` (icons + border, mode-sensitive:
    /// 33 px flat / 41 px rounded). The views must
    /// size their artwork-resolver math, drag handles, and slot-list rects
    /// against the REMAINING width. Top / None nav layouts subtract nothing
    /// and this returns the raw window width.
    ///
    /// Use this in place of `self.window.width` whenever a value flows
    /// into `BaseSlotListLayoutConfig.window_width` or a view-data
    /// `window_width` field — otherwise the Auto-mode portrait fallback
    /// shows top/bottom letterbox bars and the horizontal candidate
    /// over-counts the leftover slot-list width by the side-nav footprint.
    pub(crate) fn content_pane_width(&self) -> f32 {
        let nav_chrome = if crate::theme::is_side_nav() {
            crate::widgets::side_nav_bar::side_nav_total_width()
        } else {
            0.0
        };
        (self.window.width - nav_chrome).max(0.0)
    }

    /// Width of the artwork *column* (the `bg0_soft` image container) when
    /// elevation is active, or `None` when it does not apply.
    ///
    /// Returns `Some(extent)` when all of these hold:
    /// - Top-nav layout is active (the only layout this elevation reshapes).
    /// - `track_info_display` is neither `TopBar` nor `TopBarUnder` — the nav
    ///   bar's right portion is otherwise reserved for the metadata strip
    ///   (TopBar) or the strip occupies its own row beneath the nav
    ///   (TopBarUnder), and either case keeps the artwork in its regular
    ///   column-stacked spot beneath the chrome.
    /// - The browsing panel is not open — split-view has its own dual-pane
    ///   shape and skips elevation.
    /// - The current view's `ViewPage` reports `uses_horizontal_artwork_column()`
    ///   true (Albums, Artists, Songs, Genres, Queue, Playlists today).
    ///   Settings has no `ViewPage`; Radios overrides to false.
    /// - The active `ArtworkColumnMode` resolves to a Horizontal layout for
    ///   the current window using **the same config the view passes**
    ///   (raw `window.height`, not the player-bar-adjusted variant).
    ///   Otherwise the view's `base_slot_list_layout` falls into the
    ///   no-artwork branch (no top spacer added to the slot-list column),
    ///   and elevating anyway would hide the view header behind the nav
    ///   bar overlay.
    ///
    /// Once we've confirmed the view will render Horizontal artwork, the
    /// returned *value* uses a second `resolve_artwork_layout` call with
    /// `window.height - player_bar_height` to match iced's `responsive`
    /// natural-size math for the in-tree panel (Auto-mode square sized
    /// against the actual row height, not the raw window height). Without
    /// the second call the nav-bar overlay under-reaches the artwork's
    /// real left edge and the stripe peeks through above the nav.
    ///
    /// The returned extent is the artwork's **inner column width** —
    /// excluding the 1 px `border()` stripe (Auto/Always) and the 6 px drag
    /// handle (Always only). `home_view` computes
    /// `nav_visual_width = content_pane_width - extent`, which makes the
    /// nav-overlay's right edge align with the inner artwork's LEFT edge:
    /// the stripe/handle sit *underneath* the nav-bar band in the top
    /// `NAV_BAR_HEIGHT` strip, then become visible as designed below it.
    /// Subtracting the stripe/handle from `nav_visual_width` would invert
    /// that — the stripe would peek through above the nav.
    pub(crate) fn elevated_artwork_extent(&self) -> Option<f32> {
        if !crate::theme::is_artwork_elevated()
            || !crate::theme::is_top_nav()
            || self.browsing_panel.is_some()
        {
            return None;
        }
        // View eligibility — `ViewPage::uses_horizontal_artwork_column` is
        // the single source of truth (overridden `true` on each view whose
        // `show_artwork_column: true` config flows into `horizontal_layout`).
        // A new horizontal-artwork view becomes elevation-eligible by
        // overriding that method, with no second list to maintain here.
        let view_eligible = self
            .view_page(self.current_view)
            .is_some_and(|p| p.uses_horizontal_artwork_column());
        if !view_eligible {
            return None;
        }
        use crate::widgets::base_slot_list_layout::{
            ArtworkOrientation, BaseSlotListLayoutConfig, resolve_artwork_layout,
        };
        // Probe config shared by both resolver passes — only `window_height`
        // differs between Step 1 (raw, matches the view's own call) and
        // Step 2 (player-bar-adjusted, matches the responsive's bbox).
        let probe_config = |window_height: f32| BaseSlotListLayoutConfig {
            window_width: self.content_pane_width(),
            window_height,
            show_artwork_column: true,
            slot_list_chrome: 0.0,
            elevated: false,
        };
        // Step 1 — does the view actually render Horizontal artwork?
        //          The view's call uses raw `window.height`, so we must too.
        let view_layout = resolve_artwork_layout(&probe_config(self.window.height))?;
        match view_layout.orientation {
            ArtworkOrientation::Horizontal => {}
            ArtworkOrientation::Vertical => return None,
        }
        // Step 2 — size the overlay against the responsive's actual square.
        //          The in-tree responsive widget receives a height of
        //          `window.height - player_bar_height` from main_content's
        //          row, so mirror that here. Auto-mode square shrinks
        //          accordingly; Always-mode `window_width * pct` extent
        //          is height-independent (same value either way).
        let adjusted_height =
            (self.window.height - crate::widgets::player_bar::player_bar_height()).max(0.0);
        let adjusted_layout = resolve_artwork_layout(&probe_config(adjusted_height))?;
        match adjusted_layout.orientation {
            ArtworkOrientation::Horizontal => Some(adjusted_layout.extent),
            ArtworkOrientation::Vertical => None,
        }
    }

    /// Root view dispatcher.
    ///
    /// Daemon-mode signature: `_window` is unused (single window only).
    pub fn view(&self, _window: iced::window::Id) -> Element<'_, Message> {
        let screen_view = match self.screen {
            Screen::Login => self.login_view(),
            Screen::Home => self.home_view(),
        };

        self.wrap_with_global_overlays(screen_view)
    }

    // -------------------------------------------------------------------------
    // Login View: Delegate to LoginPage
    // -------------------------------------------------------------------------

    /// Login screen view - delegates to LoginPage component
    fn login_view(&self) -> Element<'_, Message> {
        self.login_page.view().map(Message::Login)
    }

    /// Home screen layout (nav bar + content + player bar)
    fn home_view(&self) -> Element<'_, Message> {
        // Resolve elevation once per frame; the result threads through
        // `main_content` into each view's `*ViewData.elevated` which
        // `BaseSlotListLayoutConfig.elevated` then carries into
        // `horizontal_layout`. Both the elevated and non-elevated top-nav
        // branches below produce the same `Stack[base, nav_overlay]`
        // shape — see the branch comment for why.
        let elevated_extent = self.elevated_artwork_extent();

        // Optional radio metadata mapping
        let (radio_name, radio_url, icy_artist, icy_title) = match &self.active_playback {
            crate::state::ActivePlayback::Radio(state) => (
                Some(state.station.name.as_str()),
                Some(state.station.stream_url.as_str()),
                state.icy_artist.as_deref(),
                state.icy_title.as_deref(),
            ),
            crate::state::ActivePlayback::Queue => (None, None, None, None),
        };

        let has_queue = !self.library.queue_songs.is_empty();

        // Mode-gated mini-player artwork handle — see
        // `Nokkvi::mini_player_artwork()` for the resolution rules. Gated on
        // `TrackInfoDisplay::MiniPlayer` inside the method so other strip
        // modes short-circuit before walking the queue.
        let mini_player_artwork: Option<iced::widget::image::Handle> = self.mini_player_artwork();

        let player_bar_data = widgets::PlayerBarViewData {
            playback_position: self.playback.position,
            playback_duration: self.playback.duration,
            playback_playing: self.playback.playing,
            playback_paused: self.playback.paused,
            volume: self.playback.volume,
            has_queue,
            is_radio: self.active_playback.is_radio(),
            is_random_mode: self.modes.random,
            is_repeat_mode: self.modes.repeat,
            is_repeat_queue_mode: self.modes.repeat_queue,
            is_consume_mode: self.modes.consume,
            eq_enabled: self.playback.eq_state.is_enabled(),
            sound_effects_enabled: self.sfx.enabled,
            sfx_volume: self.sfx.volume,
            crossfade_enabled: self.engine.crossfade_enabled,
            bit_perfect: self.engine.bit_perfect,
            visualization_mode: self.engine.visualization_mode,
            window_width: self.window.width,
            // MiniPlayer remaps per the width-driven regime: the wide
            // three-section layout passes the mode-cull count through (modes
            // expand + cull individually like the normal bar), while the compact
            // layout folds every mode into one permanent kebab. Every other mode
            // uses the raw width-driven layout. `compute_layout` still owns
            // `self.player_bar_layout` — `effective_player_bar_layout` is a
            // render-only override.
            layout: crate::widgets::player_bar::effective_player_bar_layout(self.player_bar_layout),
            is_light_mode: crate::theme::is_light_mode(),
            // For radio playback the station name lives on `radio_name` and
            // ICY values fill the title/artist slots — empty when no ICY has
            // arrived yet (vs. echoing the station name into slot 2). For
            // queue playback the fields carry the song metadata directly.
            track_title: if self.active_playback.is_radio() {
                icy_title.unwrap_or_default().to_string()
            } else {
                self.playback.title.clone()
            },
            track_artist: if self.active_playback.is_radio() {
                icy_artist.unwrap_or_default().to_string()
            } else {
                self.playback.artist.clone()
            },
            track_album: if self.active_playback.is_radio() {
                String::new()
            } else {
                self.playback.album.clone()
            },
            radio_name: radio_name.map(|s| s.to_string()),
            // Codec / sample-rate / bitrate for the MiniPlayer capsule end-caps
            // (same playback source the track info strip reads).
            format_suffix: self.playback.format_suffix.clone(),
            sample_rate: self.playback.sample_rate,
            bitrate: self.playback.bitrate,
            bit_perfect_status: self.playback.bit_perfect_status,
            bit_perfect_holder: self.playback.bit_perfect_holder.clone(),
            artwork_handle: mini_player_artwork,
            hamburger_open: matches!(
                self.open_menu,
                Some(crate::app_message::OpenMenu::Hamburger)
            ),
            player_modes_open: matches!(
                self.open_menu,
                Some(crate::app_message::OpenMenu::PlayerModes)
            ),
        };

        // Shared strip data — borrows playback state, no clones needed.
        let strip_data = widgets::track_info_strip::TrackInfoStripData {
            title: &self.playback.title,
            artist: &self.playback.artist,
            album: &self.playback.album,
            format_suffix: &self.playback.format_suffix,
            sample_rate: self.playback.sample_rate,
            bit_perfect_status: self.playback.bit_perfect_status,
            bit_perfect_holder: self.playback.bit_perfect_holder.as_deref(),
            bitrate: self.playback.bitrate,
            radio_name,
            radio_url,
            icy_artist,
            icy_title,
        };

        // Build the player bar info strip if PlayerBar mode is active
        let player_strip: Option<Element<'_, widgets::PlayerBarMessage>> =
            if crate::theme::show_player_bar_strip() {
                Some(widgets::track_info_strip::track_info_strip_with_separator(
                    &strip_data,
                    Some(widgets::PlayerBarMessage::StripClicked),
                ))
            } else {
                None
            };

        // Wrap player bar strip in context menu for right-click actions (if not radio)
        let player_strip: Option<Element<'_, widgets::PlayerBarMessage>> =
            player_strip.map(|strip| {
                widgets::context_menu::wrap_strip_context_menu(
                    strip,
                    radio_name.is_some(),
                    !self.settings.local_music_path.is_empty(),
                    self.is_current_track_starred(),
                    strip_context_state(&self.open_menu),
                    widgets::PlayerBarMessage::StripContextAction,
                    widgets::PlayerBarMessage::SetOpenMenu,
                )
            });

        // Base layout:
        //   Top mode:  nav_bar  + content + player_bar
        //   Side mode: row[sidebar, column[strip?, content, player_bar]]
        //              (sidebar runs the full window height — strip + player
        //              are pushed RIGHT of the sidebar to match the flat
        //              redesign mockups)
        //   None mode: [strip?] + content + player_bar  (no sidebar)

        // Helper: build the optional top-area metadata strip as a single
        // Element. Returns `None` when the strip is hidden.
        //
        // `with_separator_above = false` → `TopBar` styling: bare strip with a
        // 1 px separator BELOW (visually divides strip from the content under
        // it). Used by side-nav (inside the right column) and none-nav (top of
        // the outer column).
        //
        // `with_separator_above = true` → `TopBarUnder` styling: 1 px separator
        // ABOVE the strip (matches the player-bar variant). Used by top-nav,
        // where the separator divides the nav row from the strip beneath it.
        let build_top_strip = |with_separator_above: bool| -> Option<Element<'_, Message>> {
            let visible = if with_separator_above {
                crate::theme::show_top_bar_under_strip()
            } else {
                crate::theme::show_top_bar_strip()
            };
            if !visible {
                return None;
            }
            let strip = if with_separator_above {
                widgets::track_info_strip::track_info_strip_with_separator(
                    &strip_data,
                    Some(Message::StripClicked),
                )
            } else {
                widgets::track_info_strip::track_info_strip(
                    &strip_data,
                    Some(Message::StripClicked),
                )
            };
            let wrapped: Element<'_, Message> = widgets::context_menu::wrap_strip_context_menu(
                strip,
                radio_name.is_some(),
                !self.settings.local_music_path.is_empty(),
                self.is_current_track_starred(),
                strip_context_state(&self.open_menu),
                Message::StripContextAction,
                Message::SetOpenMenu,
            );
            if with_separator_above {
                Some(wrapped)
            } else {
                Some(column![wrapped, crate::theme::horizontal_separator::<Message>(1.0),].into())
            }
        };

        let base_layer: Element<'_, Message> = if crate::theme::is_side_nav()
            || crate::theme::is_none_nav()
        {
            let mut outer = iced::widget::Column::new();

            if crate::theme::is_side_nav() {
                // Settings has no NavView counterpart; the sidebar treats it
                // as Queue (`settings_open` flag below highlights it instead).
                let side_nav_view: widgets::NavView =
                    Option::<widgets::NavView>::from(self.current_view)
                        .unwrap_or(widgets::NavView::Queue);
                // Mirror the top-nav library state into the side-nav so
                // the footer trigger + popover see the same source of
                // truth (shared via `library_filter_view_data`).
                let lib = self.library_filter_view_data();
                let side_data = widgets::SideNavBarData {
                    current_view: side_nav_view,
                    settings_open: self.current_view == View::Settings,
                    editor_session_active: self.playlist_editor.is_some(),
                    editor_active: matches!(self.current_view, View::PlaylistEditor),
                    library_count: lib.count,
                    active_library_count: lib.active_count,
                    library_selector_open: lib.popover_open,
                    library_selector_bounds: lib.trigger_bounds,
                    library_rows: lib.rows,
                    hamburger_open: matches!(
                        self.open_menu,
                        Some(crate::app_message::OpenMenu::Hamburger)
                    ),
                    is_light_mode: crate::theme::is_light_mode(),
                };
                // Side-nav mode: sidebar runs the FULL window height; the
                // top-bar strip, content, and player bar all live in the
                // right column so the sidebar is the visual leftmost band
                // across every row of chrome (matches the flat-redesign
                // side-nav mockups).
                let mut right_col = iced::widget::Column::new();
                if let Some(strip_el) = build_top_strip(false) {
                    right_col = right_col.push(strip_el);
                }
                right_col = right_col.push(self.main_content(false));
                right_col = right_col.push(
                    widgets::player_bar(&player_bar_data, player_strip).map(Message::PlayerBar),
                );
                // Fill the window height so the player bar sits flush at the
                // window's bottom edge (no gap below it).
                let right_col = right_col.height(Length::Fill);

                outer = outer.push(
                    iced::widget::row![
                        widgets::side_nav_bar(side_data).map(map_nav_bar_message),
                        right_col,
                    ]
                    .height(Length::Fill),
                );
            } else {
                // None mode: no sidebar — strip (if any), content, player
                // bar all span the full window width.
                if let Some(strip_el) = build_top_strip(false) {
                    outer = outer.push(strip_el);
                }
                outer = outer.push(self.main_content(false));
                outer = outer.push(
                    widgets::player_bar(&player_bar_data, player_strip).map(Message::PlayerBar),
                );
            }

            // Fill the window height so the player bar sits flush at the bottom.
            outer.height(Length::Fill).into()
        } else {
            // Top-nav layout — always wrap in `Stack` with the same column
            // shape underneath, even when elevation is off. Switching the
            // root widget type between Column (non-elevated) and Stack
            // (elevated) would tear down `text_input` focus and any other
            // stateful widgets every time elevation flipped — Ctrl+E to
            // open the browsing panel, navigating to an ineligible view,
            // a window resize crossing the Auto-mode threshold. See
            // CLAUDE.md "Render output" gotcha and gotchas.md:38.
            //
            // The outer `Space` reserves the nav-bar's vertical band:
            //   - non-elevated → `NAV_BAR_HEIGHT` so `main_content` is
            //     pushed below the nav band (same layout as before)
            //   - elevated → `0.0` so `main_content` extends to the top
            //     of the window, letting the artwork pane reach y=0
            //
            // `nav_visual_width` is the horizontal extent the nav-bar
            // occupies — full window width when not elevated, only the
            // slot-list area when elevated (the artwork pane underneath
            // shows through to the right of the nav).
            let (outer_space_height, nav_visual_width) =
                if let Some(artwork_extent) = elevated_extent {
                    (0.0, (self.content_pane_width() - artwork_extent).max(0.0))
                } else {
                    // Use the live nav-bar height (32 flat / 44 rounded)
                    // — the legacy `slot_list::NAV_BAR_HEIGHT` const is
                    // pinned at 32 and lets the rounded-mode nav overlay
                    // into the view header by 12 px, eating its top
                    // margin and pushing the header pill flush against
                    // the bottom of the nav bar.
                    (crate::theme::nav_bar_height(), self.window.width)
                };
            let is_elevated = elevated_extent.is_some();

            // `TopBarUnder` mode in top-nav: insert the player-bar-styled
            // strip between the nav-band Space and `main_content` so it
            // renders directly beneath the nav row and pushes the main
            // content down by the strip's height. The nav overlay sits
            // ABOVE the Space; the strip occupies the next column slot,
            // so nav → strip → content stacks naturally.
            let top_under_strip = build_top_strip(true);

            let mut base_col = iced::widget::Column::new().push(
                iced::widget::Space::new()
                    .width(Length::Fill)
                    .height(Length::Fixed(outer_space_height)),
            );
            if let Some(strip_el) = top_under_strip {
                base_col = base_col.push(strip_el);
            }
            base_col = base_col.push(self.main_content(is_elevated));
            base_col = base_col
                .push(widgets::player_bar(&player_bar_data, player_strip).map(Message::PlayerBar));
            // Fill the window so `main_content` (Length::Fill) expands and pins
            // the player bar flush to the window's bottom edge (no gap below it).
            let base = base_col.height(Length::Fill);

            let nav_overlay = column![
                container(self.navigation_bar(nav_visual_width))
                    .width(Length::Fixed(nav_visual_width))
                    .height(Length::Shrink),
                iced::widget::Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill);

            Stack::new().push(base).push(nav_overlay).into()
        };

        // Create stack with base layer
        let mut stack = Stack::new().push(base_layer);

        // Add visualizer as overlay if enabled
        use nokkvi_data::types::player_settings::VisualizationMode;
        if self.engine.visualization_mode != VisualizationMode::Off
            && let Some(ref viz) = self.visualizer
        {
            // Set mode based on current visualization_mode state
            let widget_mode = match self.engine.visualization_mode {
                VisualizationMode::Lines => widgets::visualizer::VisualizationMode::Lines,
                _ => widgets::visualizer::VisualizationMode::Bars,
            };
            let viz_with_mode = viz
                .clone()
                .mode(widget_mode)
                .window_height(self.window.height)
                .width(self.window.width);

            // Visualizer height scales with window (configurable via config.toml, min 80px)
            // Read height_percent from shared config (hot-reloadable). Sizing logic lives
            // in `widgets::visualizer::visualizer_area_height` so the boat-tick handler
            // can derive the same value without duplicating the curve.
            let cfg = self.visualizer_config.read();
            let height_percent = cfg.height_percent;
            let viz_opacity = cfg.opacity;
            let lines_mirror = cfg.lines.mirror;
            drop(cfg);

            // In side-nav mode the sidebar is the full-height leftmost
            // band; the visualizer (and boat) overlay must start to its
            // RIGHT, not at x=0, or the bars/lines bleed under the icons.
            let side_nav_inset = if crate::theme::is_side_nav() {
                crate::widgets::side_nav_bar::side_nav_total_width()
            } else {
                0.0
            };
            let visualizer_width = (self.window.width - side_nav_inset).max(0.0);

            let visualizer_height = widgets::visualizer::visualizer_area_height(
                visualizer_width,
                self.window.height,
                height_percent,
            );
            let spacer_height =
                (self.window.height - widgets::player_bar::player_bar_height() - visualizer_height)
                    .max(0.0);
            let visualizer_inner = column![
                container(iced::widget::Space::new()).height(Length::Fixed(spacer_height)),
                container(viz_with_mode.view())
                    .width(Length::Fill)
                    .height(Length::Fixed(visualizer_height))
            ]
            .width(Length::Fill)
            .height(Length::Fill);
            let visualizer_overlay = iced::widget::row![
                iced::widget::Space::new().width(Length::Fixed(side_nav_inset)),
                visualizer_inner,
            ]
            .width(Length::Fill)
            .height(Length::Fill);

            stack = stack.push(visualizer_overlay);

            // Surfing-boat overlay (lines mode only). Mirrors the spacer
            // shape above so the boat overlay's pixel coordinate space lines
            // up with the visualizer area. `boat_overlay()` returns a
            // fixed-size, self-clipping container, so we don't need an
            // outer wrapper here. Insets the same `side_nav_inset` so the
            // boat sails over the visualizer, not the sidebar.
            if self.boat.visible && self.engine.visualization_mode == VisualizationMode::Lines {
                let boat_inner = column![
                    container(iced::widget::Space::new()).height(Length::Fixed(spacer_height)),
                    crate::widgets::boat::boat_overlay::<Message>(
                        &self.boat,
                        visualizer_width,
                        visualizer_height,
                        viz_opacity,
                        lines_mirror,
                    ),
                ]
                .width(Length::Fill)
                .height(Length::Fill);
                let boat_overlay_col = iced::widget::row![
                    iced::widget::Space::new().width(Length::Fixed(side_nav_inset)),
                    boat_inner,
                ]
                .width(Length::Fill)
                .height(Length::Fill);

                stack = stack.push(boat_overlay_col);
            }
        }

        stack.into()
    }

    /// Wrap a base view with global overlays (modals, toasts, dialogs)
    fn wrap_with_global_overlays<'a>(&'a self, base: Element<'a, Message>) -> Element<'a, Message> {
        let mut stack = Stack::new().push(base);

        // Add text input dialog overlay (if visible)
        if let Some(dialog_overlay) =
            crate::widgets::text_input_dialog::text_input_dialog_overlay(&self.text_input_dialog)
        {
            stack = stack.push(dialog_overlay.map(Message::TextInputDialog));
        }

        // Add info modal overlay (if visible)
        if let Some(info_overlay) = crate::widgets::info_modal::info_modal_overlay(&self.info_modal)
        {
            stack = stack.push(info_overlay.map(Message::InfoModal));
        }

        // Add about modal overlay (if visible)
        if let Some(about_overlay) = crate::widgets::about_modal::about_modal_overlay(
            &self.about_modal,
            crate::widgets::about_modal::AboutViewData {
                server_url: &self.login_page.server_url,
                username: &self.login_page.username,
                server_version: self.server_version.as_deref(),
            },
        ) {
            stack = stack.push(about_overlay.map(Message::AboutModal));
        }

        // When EQ is disabled, show flat gains in the UI so sliders read 0 —
        // avoids the misleading appearance of active boosts. Real gains are
        // preserved in EqState and restore visually when re-enabled.
        let eq_enabled = self.playback.eq_state.is_enabled();
        let eq_gains = if eq_enabled {
            let mut gains = [0.0; 10];
            for (i, g) in gains.iter_mut().enumerate() {
                *g = self.playback.eq_state.get_band_gain(i);
            }
            gains
        } else {
            [0.0; 10]
        };
        if let Some(eq_overlay) = crate::widgets::eq_modal_overlay(
            self.eq_modal.open,
            eq_enabled,
            eq_gains,
            &self.eq_modal.custom_presets,
            self.eq_modal.save_mode,
            &self.eq_modal.save_name,
        ) {
            stack = stack.push(eq_overlay.map(Message::EqModal));
        }

        // Add default-playlist picker overlay (if open)
        if let Some(picker_state) = &self.default_playlist_picker {
            let picker_overlay =
                crate::widgets::default_playlist_picker::default_playlist_picker_overlay(
                    picker_state,
                    self.window.height,
                    &self.artwork.playlist.mini.snapshot,
                );
            stack = stack.push(picker_overlay.map(Message::DefaultPlaylistPicker));
        }

        // Add toast status bar overlay (if any active toast)
        if let Some(toast) = self.toast.current() {
            // Toast icon prefix based on level
            let h_align = if toast.right_aligned {
                iced::alignment::Horizontal::Right
            } else {
                iced::alignment::Horizontal::Left
            };
            let toast_text = iced::widget::text(&toast.message)
                .color(crate::theme::toast_level_color(toast.level))
                .font(crate::theme::ui_font())
                .size(14)
                .width(Length::Fill)
                .align_x(h_align);

            // Status bar at bottom of content area.
            // On Home screen, leave room for player bar (~56px).
            // On Login screen, player bar is not visible.
            let bottom_padding = if self.screen == Screen::Home {
                widgets::player_bar::player_bar_height()
            } else {
                12.0 // Just a bit of margin from bottom
            };

            let toast_bar = container(
                container(toast_text)
                    .padding([4, 12])
                    .style(|_theme: &iced::Theme| container::Style {
                        background: Some(crate::theme::bg0_hard().into()),
                        border: iced::Border {
                            color: crate::theme::bg3(),
                            width: 1.0,
                            radius: crate::theme::ui_border_radius(),
                        },
                        ..Default::default()
                    })
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(iced::Padding {
                top: 0.0,
                right: 0.0,
                bottom: bottom_padding,
                left: 0.0,
            });

            stack = stack.push(toast_bar);
        }

        // Add floating drag indicator during cross-pane drag — renders a copy
        // of the centered browsing slot at the cursor position.
        if let Some(ref drag) = self.cross_pane_drag.active {
            let slot_element = self.render_drag_slot();

            // Position the slot near the cursor. Use a width that matches
            // the browser pane slot width, and a fixed row height.
            let slot_width = (self.window.width * 0.42).min(600.0);
            let slot_height = 64.0_f32;

            let offset_x = 12.0_f32;
            let offset_y = -(slot_height / 2.0); // Center vertically on cursor
            let pad_left =
                (drag.cursor.x + offset_x).clamp(0.0, self.window.width - slot_width - 20.0);
            let pad_top = (drag.cursor.y + offset_y).clamp(0.0, self.window.height - slot_height);

            let drag_overlay = container(
                container(slot_element)
                    .width(Length::Fixed(slot_width))
                    .height(Length::Fixed(slot_height)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(iced::Padding {
                top: pad_top,
                left: pad_left,
                right: 0.0,
                bottom: 0.0,
            });

            stack = stack.push(drag_overlay);
            // The drop indicator is rendered inside the queue's own slot
            // list area (see `slot_list_view_with_drag`) using the slot
            // index recorded by per-slot `mouse_area::on_enter`, so its
            // y-position is in the slot list's coordinate space — no
            // chrome reconstruction needed at the window level.
        }

        stack.into()
    }

    /// Visual slot index for the cross-pane drag's drop indicator, or
    /// `None` if no drag is active or the cursor is not over any queue
    /// slot. Read by [`crate::views::QueueViewData::drop_indicator_slot`].
    fn cross_pane_drop_indicator_slot(&self) -> Option<usize> {
        self.cross_pane_drag.active.as_ref()?;
        // While editing, the LEFT pane is the playlist editor, so the drop
        // indicator tracks the editor's own hovered slot (its slot-list state
        // is independent of the live queue's). Otherwise read the queue pane.
        let slot_list = match self.playlist_editor.as_ref() {
            Some(editor) => &editor.common.slot_list,
            None => &self.queue_page.common.slot_list,
        };
        slot_list.hovered_slot.map(|h| h.slot_index())
    }

    /// Resolve the active playlist's strip cover handle: the collage's first
    /// tile, falling back to the mini cover. `None` when no playlist is active
    /// or its artwork hasn't been cached yet — the strip omits the cover then.
    /// Reuses the Playlists view's `CollageArtworkCache`, so a playlist the
    /// user has browsed already has its cover warm.
    fn active_playlist_strip_cover(&self) -> Option<&iced::widget::image::Handle> {
        let ctx = self.active_playlist_info.as_ref()?;
        self.artwork
            .playlist
            .collage
            .snapshot
            .get(&ctx.id)
            .and_then(|tiles| tiles.first())
            .or_else(|| self.artwork.playlist.mini.snapshot.get(&ctx.id))
    }

    /// Resolve the active playlist's strip cover as a 2×2 quad from the
    /// FROZEN `strip_quad_album_ids` snapshot (taken when the playlist
    /// context was entered, so the tiles are the playlist's first ≤4 distinct
    /// album covers regardless of later queue mutations), each tile served by
    /// the album-id-keyed 80px `album_art` cache. `None` when no playlist is
    /// active, the snapshot spans fewer than 2 distinct albums, or any tile
    /// is still cold — the strip then falls back to the single
    /// [`Self::active_playlist_strip_cover`] exactly as before.
    pub(crate) fn active_playlist_strip_quad(&self) -> Option<Vec<&iced::widget::image::Handle>> {
        use crate::services::collage_artwork::resolve_quad_handles;

        self.active_playlist_info.as_ref()?;
        resolve_quad_handles(&self.strip_quad_album_ids, &self.artwork.album_art.snapshot)
    }

    // -------------------------------------------------------------------------
    // Per-view ViewData builders
    //
    // Each library view (Albums, Artists, Songs, Genres) renders twice — once
    // as the main pane and once inside the browsing panel — and the only
    // differences between the two construction sites are the
    // `(window_width, window_height, in_browsing_panel, stable_viewport)`
    // tuple. Folding `column_dropdown_state(...)` into the helper keeps both
    // branches limited to a one-line call.
    //
    // Queue is built the same way (`build_queue_view_data`); it differs only
    // in `window_width` and `elevated` between the split-pane and single-view
    // branches. Playlists and Similar render in only one branch and stay
    // inline at the call site.
    // -------------------------------------------------------------------------

    /// Build `AlbumsViewData` from current app state. Shared by the main pane
    /// and the browsing-panel split-view branch.
    fn build_albums_view_data(
        &self,
        window_width: f32,
        window_height: f32,
        in_browsing_panel: bool,
        stable_viewport: bool,
        elevated: bool,
    ) -> views::AlbumsViewData<'_> {
        let (column_dropdown_open, column_dropdown_trigger_bounds) =
            column_dropdown_state(&self.open_menu, View::Albums);
        views::AlbumsViewData {
            albums: &self.library.albums,
            album_art: &self.artwork.album_art.snapshot,
            large_artwork: &self.artwork.large_artwork.snapshot,
            window_width,
            window_height,
            scale_factor: self.window.scale_factor,
            modifiers: self.window.keyboard_modifiers,
            total_album_count: self.library.counts.albums,
            loading: self.library.albums.is_loading(),
            stable_viewport,
            in_browsing_panel,
            elevated,
            overlay: views::OverlayMenuViewData {
                column_dropdown_open,
                column_dropdown_trigger_bounds,
                open_menu: self.open_menu.as_ref(),
            },
        }
    }

    /// Build `ArtistsViewData` from current app state. Shared by the main pane
    /// and the browsing-panel split-view branch.
    fn build_artists_view_data(
        &self,
        window_width: f32,
        window_height: f32,
        in_browsing_panel: bool,
        stable_viewport: bool,
        elevated: bool,
    ) -> views::ArtistsViewData<'_> {
        let (column_dropdown_open, column_dropdown_trigger_bounds) =
            column_dropdown_state(&self.open_menu, View::Artists);
        views::ArtistsViewData {
            artists: &self.library.artists,
            // Reuse album art cache for artist images.
            artist_art: &self.artwork.album_art.snapshot,
            album_art: &self.artwork.album_art.snapshot,
            large_artwork: &self.artwork.large_artwork.snapshot,
            window_width,
            window_height,
            scale_factor: self.window.scale_factor,
            modifiers: self.window.keyboard_modifiers,
            total_artist_count: self.library.counts.artists,
            loading: self.library.artists.is_loading(),
            stable_viewport,
            in_browsing_panel,
            elevated,
            overlay: views::OverlayMenuViewData {
                column_dropdown_open,
                column_dropdown_trigger_bounds,
                open_menu: self.open_menu.as_ref(),
            },
        }
    }

    /// Build `SongsViewData` from current app state. Shared by the main pane
    /// and the browsing-panel split-view branch.
    fn build_songs_view_data(
        &self,
        window_width: f32,
        window_height: f32,
        in_browsing_panel: bool,
        stable_viewport: bool,
        elevated: bool,
    ) -> views::SongsViewData<'_> {
        let (column_dropdown_open, column_dropdown_trigger_bounds) =
            column_dropdown_state(&self.open_menu, View::Songs);
        views::SongsViewData {
            songs: &self.library.songs,
            album_art: &self.artwork.album_art.snapshot,
            large_artwork: &self.artwork.large_artwork.snapshot,
            window_width,
            window_height,
            scale_factor: self.window.scale_factor,
            modifiers: self.window.keyboard_modifiers,
            total_song_count: self.library.counts.songs,
            loading: self.library.songs.is_loading(),
            stable_viewport,
            in_browsing_panel,
            elevated,
            overlay: views::OverlayMenuViewData {
                column_dropdown_open,
                column_dropdown_trigger_bounds,
                open_menu: self.open_menu.as_ref(),
            },
        }
    }

    /// Build `GenresViewData` from current app state. Shared by the main pane
    /// and the browsing-panel split-view branch.
    fn build_genres_view_data(
        &self,
        window_width: f32,
        window_height: f32,
        in_browsing_panel: bool,
        stable_viewport: bool,
        elevated: bool,
    ) -> views::GenresViewData<'_> {
        let (column_dropdown_open, column_dropdown_trigger_bounds) =
            column_dropdown_state(&self.open_menu, View::Genres);
        views::GenresViewData {
            genres: &self.library.genres,
            genre_artwork: &self.artwork.genre.mini.snapshot,
            genre_collage_artwork: &self.artwork.genre.collage.snapshot,
            album_art: &self.artwork.album_art.snapshot,
            window_width,
            window_height,
            scale_factor: self.window.scale_factor,
            modifiers: self.window.keyboard_modifiers,
            total_genre_count: self.library.counts.genres,
            loading: self.library.genres.is_loading(),
            stable_viewport,
            in_browsing_panel,
            elevated,
            overlay: views::OverlayMenuViewData {
                column_dropdown_open,
                column_dropdown_trigger_bounds,
                open_menu: self.open_menu.as_ref(),
            },
        }
    }

    /// Build `QueueViewData` from current app state. Shared by the
    /// browsing-panel split-view branch and the normal single-view branch.
    /// The two call sites differ only in `window_width` and `elevated`, so
    /// those are the parameters; everything else is read off `&self`.
    pub(crate) fn build_queue_view_data(
        &self,
        window_width: f32,
        elevated: bool,
    ) -> views::QueueViewData<'_> {
        let (column_dropdown_open, column_dropdown_trigger_bounds) =
            column_dropdown_state(&self.open_menu, View::Queue);
        views::QueueViewData {
            queue_songs: self.filter_queue_songs(),
            album_art: &self.artwork.album_art.snapshot,
            large_artwork: &self.artwork.large_artwork.snapshot,
            window_width,
            window_height: self.window.height,
            scale_factor: self.window.scale_factor,
            modifiers: self.window.keyboard_modifiers,
            current_playing_song_id: self.scrobble.current_song_id.clone(),
            current_playing_entry_id: self.last_queue_current_entry_id,
            is_playing: self.playback.playing && !self.playback.paused,
            total_queue_count: self
                .library
                .queue_loading_target
                .unwrap_or(self.library.queue_songs.len()),
            stable_viewport: self.settings.stable_viewport,
            elevated,
            playlist_context_info: self.active_playlist_info.clone(),
            playlist_strip_expanded: self.queue_page.playlist_strip_expanded,
            playlist_cover: self.active_playlist_strip_cover(),
            playlist_quad: self.active_playlist_strip_quad(),
            overlay: views::OverlayMenuViewData {
                column_dropdown_open,
                column_dropdown_trigger_bounds,
                open_menu: self.open_menu.as_ref(),
            },
            show_default_playlist_chip: self.settings.queue_show_default_playlist,
            default_playlist_name: &self.settings.default_playlist_name,
            drop_indicator_slot: self.cross_pane_drop_indicator_slot(),
        }
    }

    // -------------------------------------------------------------------------
    // Navigation Bar: Delegate to nav_bar component
    // -------------------------------------------------------------------------

    /// Navigation bar - delegates to nav_bar component with playback data.
    ///
    /// `effective_width` is the horizontal extent the nav-bar actually
    /// occupies. In the regular column-stacked layout this matches the
    /// window width; in the artwork-elevated layout the nav bar only spans
    /// the slot-list area to the left of the artwork pane, so callers pass
    /// `content_pane_width - artwork_extent` instead. The nav bar uses this
    /// width to drive its responsive collapse breakpoints.
    fn navigation_bar(&self, effective_width: f32) -> Element<'_, Message> {
        // Convert app::View to widgets::NavView for the component. Settings
        // is not a nav-bar column — fall back to Queue (ignored when
        // `settings_open` is set, which highlights the settings icon instead).
        let settings_open = matches!(self.current_view, View::Settings);
        let current_nav_view: widgets::NavView =
            Option::<widgets::NavView>::from(self.current_view).unwrap_or(widgets::NavView::Queue);

        // Create NavBarViewData with current playback state or radio overrides
        let (track_title, track_artist, track_album) =
            self.active_playback.nav_metadata(&self.playback);

        let (radio_name, radio_url, icy_artist, icy_title) = match &self.active_playback {
            crate::state::ActivePlayback::Radio(state) => (
                Some(state.station.name.clone()),
                Some(state.station.stream_url.clone()),
                state.icy_artist.clone(),
                state.icy_title.clone(),
            ),
            crate::state::ActivePlayback::Queue => (None, None, None, None),
        };

        // Library-filter chrome state — shared with the side-nav builder
        // via `library_filter_view_data`. See that method for the
        // "all-checked" convention and sort order.
        let lib = self.library_filter_view_data();
        let library_count = lib.count;
        let active_library_count = lib.active_count;
        let library_rows = lib.rows;
        let library_selector_open = lib.popover_open;
        let library_selector_bounds = lib.trigger_bounds;

        // One lookup feeds both the open flag and the menu position.
        let (strip_context_open, strip_context_position) = strip_context_state(&self.open_menu);

        let nav_bar_data = widgets::NavBarViewData {
            current_view: current_nav_view,
            editor_session_active: self.playlist_editor.is_some(),
            editor_active: matches!(self.current_view, View::PlaylistEditor),
            track_title,
            track_artist,
            track_album,
            is_playing: self.playback.has_track() || self.active_playback.is_radio(),
            format_suffix: self.playback.format_suffix.clone(),
            sample_rate_khz: self.playback.sample_rate as f32 / 1000.0,
            bit_perfect_status: self.playback.bit_perfect_status,
            bit_perfect_holder: self.playback.bit_perfect_holder.clone(),
            bitrate_kbps: self.playback.bitrate,
            window_width: effective_width,
            is_light_mode: crate::theme::is_light_mode(),
            settings_open,
            local_music_path: self.settings.local_music_path.clone(),
            is_current_starred: self.is_current_track_starred(),
            radio_name,
            radio_url,
            icy_artist,
            icy_title,
            hamburger_open: matches!(
                self.open_menu,
                Some(crate::app_message::OpenMenu::Hamburger)
            ),
            strip_context_open,
            strip_context_position,
            library_count,
            active_library_count,
            library_selector_open,
            library_selector_bounds,
            library_rows,
        };

        // Use the nav_bar component, mapping NavBarMessage to app Message
        widgets::nav_bar(nav_bar_data).map(map_nav_bar_message)
    }

    /// Main content area - dispatches to current view's page
    fn main_content(&self, elevated: bool) -> Element<'_, Message> {
        // Borrow the pre-computed large_artwork snapshot (refreshed after each LRU mutation).
        // This avoids re-creating the HashMap on every render frame.
        let large_artwork = &self.artwork.large_artwork.snapshot;

        // =====================================================================
        // Split-view layout for playlist edit mode or browsing panel toggle
        // =====================================================================
        // The editor renders its own split (its own buffer + an add-songs
        // browser); the non-edit queue split powers similar-songs / the
        // browse-toggle. They are mutually exclusive on `current_view`, and the
        // queue split is suppressed while a session is active so the editor's
        // browser never leaks into the Queue tab when the user peeks at it.
        let in_editor = self.current_view == View::PlaylistEditor;
        if self.browsing_panel.is_some()
            && (in_editor || (self.current_view == View::Queue && self.playlist_editor.is_none()))
        {
            use iced::widget::{column as col, row as r};

            // LEFT pane: the editor's own buffer on the editor view, else the
            // live queue. Each branch builds only its own view data.
            let queue_pane: Element<'_, Message> = match self.playlist_editor.as_ref() {
                Some(editor) if in_editor => {
                    let dirty = editor.edit.is_dirty(&self.editor_song_ids())
                        || editor.edit.has_metadata_changes();
                    let editor_data = views::EditorViewData {
                        songs: std::borrow::Cow::Borrowed(&editor.songs),
                        album_art: &self.artwork.album_art.snapshot,
                        large_artwork,
                        window_width: self.content_pane_width() * QUEUE_PANE_FRACTION,
                        window_height: self.window.height,
                        modifiers: self.window.keyboard_modifiers,
                        total_count: editor.songs.len(),
                        name: editor.edit.playlist_name.clone(),
                        comment: editor.edit.playlist_comment.clone(),
                        public: editor.edit.playlist_public,
                        dirty,
                        drop_indicator_slot: self.cross_pane_drop_indicator_slot(),
                        open_menu: self.open_menu.as_ref(),
                    };
                    editor.view(editor_data).map(Message::Editor)
                }
                _ => {
                    let queue_view_data = self.build_queue_view_data(
                        self.content_pane_width() * QUEUE_PANE_FRACTION,
                        false,
                    );
                    self.queue_page.view(queue_view_data).map(Message::Queue)
                }
            };
            let queue_focused = self.pane_focus == crate::state::PaneFocus::Queue;

            // Shared pane border style: accent + thick when focused, bg3 + thin otherwise
            let pane_border_style =
                |focused: bool| -> Box<dyn Fn(&iced::Theme) -> container::Style> {
                    let border_color = if focused {
                        crate::theme::accent()
                    } else {
                        crate::theme::bg3()
                    };
                    let border_width = if focused { 2.0 } else { 1.0 };
                    Box::new(move |_theme| container::Style {
                        border: iced::Border {
                            color: border_color,
                            width: border_width,
                            radius: crate::theme::ui_border_radius(),
                        },
                        ..Default::default()
                    })
                };

            let queue_container = container(queue_pane)
                .width(Length::FillPortion(QUEUE_PANE_PORTION))
                .height(Length::Fill)
                .style(if self.cross_pane_drag.active.is_some() {
                    // Drop target highlight during active drag
                    let accent = crate::theme::accent_bright();
                    Box::new(move |_theme: &iced::Theme| container::Style {
                        border: iced::Border {
                            color: accent,
                            width: 3.0,
                            radius: crate::theme::ui_border_radius(),
                        },
                        background: Some(iced::Color { a: 0.05, ..accent }.into()),
                        ..Default::default()
                    }) as Box<dyn Fn(&iced::Theme) -> container::Style>
                } else {
                    pane_border_style(queue_focused)
                });

            // --- RIGHT PANE: Browsing panel ---
            let browser_focused = self.pane_focus == crate::state::PaneFocus::Browser;

            let browser_content: Element<'_, Message> = if let Some(ref panel) = self.browsing_panel
            {
                let similar_label = self.similar_songs.as_ref().map(|s| s.label.as_str());
                let is_editing = self.playlist_editor.is_some();
                let tab_bar = panel
                    .tab_bar(similar_label, is_editing)
                    .map(Message::BrowsingPanel);

                // The tab bar eats into available height — subtract it so the
                // slot list slot calculation doesn't overflow the last slot.
                use crate::widgets::slot_list::TAB_BAR_HEIGHT;
                let browser_height = self.window.height - TAB_BAR_HEIGHT;

                // Delegate to the active view's existing page
                let view_content: Element<'_, Message> = match panel.active_view {
                    views::BrowsingView::Albums => {
                        // Browser pane: stable_viewport hardcoded `true`
                        // (click to highlight, not play); `BROWSER_PANE_FRACTION`
                        // width portion of the content pane; `in_browsing_panel = true`
                        // suppresses the "Center on Playing" header button.
                        let view_data = self.build_albums_view_data(
                            self.content_pane_width() * BROWSER_PANE_FRACTION,
                            browser_height,
                            true,
                            true,
                            false,
                        );
                        self.albums_page.view(view_data).map(Message::Albums)
                    }
                    views::BrowsingView::Songs => {
                        let view_data = self.build_songs_view_data(
                            self.content_pane_width() * BROWSER_PANE_FRACTION,
                            browser_height,
                            true,
                            true,
                            false,
                        );
                        self.songs_page.view(view_data).map(Message::Songs)
                    }
                    views::BrowsingView::Artists => {
                        let view_data = self.build_artists_view_data(
                            self.content_pane_width() * BROWSER_PANE_FRACTION,
                            browser_height,
                            true,
                            true,
                            false,
                        );
                        self.artists_page.view(view_data).map(Message::Artists)
                    }
                    views::BrowsingView::Genres => {
                        let view_data = self.build_genres_view_data(
                            self.content_pane_width() * BROWSER_PANE_FRACTION,
                            browser_height,
                            true,
                            true,
                            false,
                        );
                        self.genres_page.view(view_data).map(Message::Genres)
                    }
                    views::BrowsingView::Similar => {
                        let (songs, label, loading) = match self.similar_songs.as_ref() {
                            Some(s) => (s.songs.as_slice(), s.label.as_str(), s.loading),
                            None => (&[][..], "", false),
                        };
                        let (column_dropdown_open, column_dropdown_trigger_bounds) =
                            similar_column_dropdown_state(&self.open_menu);
                        let view_data = views::SimilarViewData {
                            songs,
                            album_art: &self.artwork.album_art.snapshot,
                            large_artwork,
                            window_width: self.content_pane_width() * BROWSER_PANE_FRACTION,
                            window_height: browser_height,
                            scale_factor: self.window.scale_factor,
                            modifiers: self.window.keyboard_modifiers,
                            label,
                            loading,
                            elevated: false,
                            overlay: views::OverlayMenuViewData {
                                column_dropdown_open,
                                column_dropdown_trigger_bounds,
                                open_menu: self.open_menu.as_ref(),
                            },
                        };
                        self.similar_page.view(view_data).map(Message::Similar)
                    }
                };

                col![tab_bar, view_content].into()
            } else {
                container(iced::widget::text("No library browser"))
                    .center(Length::Fill)
                    .into()
            };

            let browser_container = container(browser_content)
                .width(Length::FillPortion(BROWSER_PANE_PORTION))
                .height(Length::Fill)
                .style(pane_border_style(browser_focused));

            return r![queue_container, browser_container]
                .height(Length::Fill)
                .into();
        }

        // =====================================================================
        // Normal single-view layout
        // =====================================================================
        match self.current_view {
            // The editor renders through its own split block above this match
            // (see the `current_view == View::PlaylistEditor` branch). This arm
            // is a defensive fallback for the unreachable case where a session
            // was cleared mid-render.
            View::PlaylistEditor => container(
                iced::widget::Space::new()
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .into(),
            View::Albums => {
                let view_data = self.build_albums_view_data(
                    self.content_pane_width(),
                    self.window.height,
                    false,
                    self.settings.stable_viewport,
                    elevated,
                );
                self.albums_page.view(view_data).map(Message::Albums)
            }
            View::Queue => {
                let view_data = self.build_queue_view_data(self.content_pane_width(), elevated);
                self.queue_page.view(view_data).map(Message::Queue)
            }
            View::Artists => {
                let view_data = self.build_artists_view_data(
                    self.content_pane_width(),
                    self.window.height,
                    false,
                    self.settings.stable_viewport,
                    elevated,
                );
                self.artists_page.view(view_data).map(Message::Artists)
            }
            View::Songs => {
                let view_data = self.build_songs_view_data(
                    self.content_pane_width(),
                    self.window.height,
                    false,
                    self.settings.stable_viewport,
                    elevated,
                );
                self.songs_page.view(view_data).map(Message::Songs)
            }
            View::Genres => {
                let view_data = self.build_genres_view_data(
                    self.content_pane_width(),
                    self.window.height,
                    false,
                    self.settings.stable_viewport,
                    elevated,
                );
                self.genres_page.view(view_data).map(Message::Genres)
            }
            View::Playlists => {
                let (column_dropdown_open, column_dropdown_trigger_bounds) =
                    column_dropdown_state(&self.open_menu, View::Playlists);
                let view_data = views::PlaylistsViewData {
                    playlists: &self.library.playlists,
                    playlist_artwork: &self.artwork.playlist.mini.snapshot,
                    playlist_collage_artwork: &self.artwork.playlist.collage.snapshot,
                    album_art: &self.artwork.album_art.snapshot,
                    window_width: self.content_pane_width(),
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                    modifiers: self.window.keyboard_modifiers,
                    total_playlist_count: self.library.counts.playlists,
                    loading: self.library.playlists.is_loading(),
                    stable_viewport: self.settings.stable_viewport,
                    elevated,
                    default_playlist_name: &self.settings.default_playlist_name,
                    overlay: views::OverlayMenuViewData {
                        column_dropdown_open,
                        column_dropdown_trigger_bounds,
                        open_menu: self.open_menu.as_ref(),
                    },
                };
                self.playlists_page.view(view_data).map(Message::Playlists)
            }
            View::Settings => self
                .settings_page
                .view(self.window.width, self.window.height)
                .map(Message::Settings),
            View::Radios => {
                let filtered_stations = self.filter_radio_stations();
                let view_data = views::RadiosViewData {
                    stations: filtered_stations,
                    window_width: self.content_pane_width(),
                    window_height: self.window.height,
                    scale_factor: self.window.scale_factor,
                    loading: false, // TODO: add loading state for radio stations
                    total_station_count: self.library.radio_stations.len(),
                    stable_viewport: self.settings.stable_viewport,
                    elevated,
                    modifiers: self.window.keyboard_modifiers,
                    open_menu: self.open_menu.as_ref(),
                    current_playing_station_id: self
                        .active_playback
                        .radio_station()
                        .map(|s| s.id.as_str()),
                };
                self.radios_page.view(view_data).map(Message::Radios)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `View` variant must convert to either `Some(NavView)` or `None`
    /// (the contextual, no-permanent-tab views: Settings and PlaylistEditor)
    /// — the table doubles as a length-anchor: adding a `View` variant without
    /// updating the conversion fails this test, not just at compile time.
    #[test]
    fn view_to_nav_view_covers_every_variant() {
        for &v in View::ALL {
            let nav: Option<widgets::NavView> = v.into();
            match v {
                View::Settings | View::PlaylistEditor => {
                    assert!(nav.is_none(), "{v:?} is contextual — no NavView");
                }
                _ => assert!(nav.is_some(), "{v:?} must map to a NavView"),
            }
        }
    }

    /// Round-trip: every `NavView` variant → `View` → `Option<NavView>`
    /// returns the original. Pins that the two maps are inverses for the
    /// 7 overlapping variants.
    #[test]
    fn nav_view_to_view_round_trips() {
        for &nav in widgets::NavView::ALL {
            let v: View = nav.into();
            let back: Option<widgets::NavView> = v.into();
            assert_eq!(back, Some(nav), "round-trip failed for {nav:?}");
        }
    }

    /// Spot-check each name-paired conversion direction to keep the renaming
    /// table honest (catch e.g. a stray Albums → Artists transposition).
    #[test]
    fn nav_view_to_view_pairs_by_name() {
        assert_eq!(View::from(widgets::NavView::Queue), View::Queue);
        assert_eq!(View::from(widgets::NavView::Albums), View::Albums);
        assert_eq!(View::from(widgets::NavView::Artists), View::Artists);
        assert_eq!(View::from(widgets::NavView::Genres), View::Genres);
        assert_eq!(View::from(widgets::NavView::Songs), View::Songs);
        assert_eq!(View::from(widgets::NavView::Playlists), View::Playlists);
        assert_eq!(View::from(widgets::NavView::Radios), View::Radios);
    }

    #[test]
    fn view_to_nav_view_pairs_by_name() {
        assert_eq!(
            Option::<widgets::NavView>::from(View::Queue),
            Some(widgets::NavView::Queue)
        );
        assert_eq!(
            Option::<widgets::NavView>::from(View::Albums),
            Some(widgets::NavView::Albums)
        );
        assert_eq!(
            Option::<widgets::NavView>::from(View::Artists),
            Some(widgets::NavView::Artists)
        );
        assert_eq!(
            Option::<widgets::NavView>::from(View::Genres),
            Some(widgets::NavView::Genres)
        );
        assert_eq!(
            Option::<widgets::NavView>::from(View::Songs),
            Some(widgets::NavView::Songs)
        );
        assert_eq!(
            Option::<widgets::NavView>::from(View::Playlists),
            Some(widgets::NavView::Playlists)
        );
        assert_eq!(
            Option::<widgets::NavView>::from(View::Radios),
            Some(widgets::NavView::Radios)
        );
        assert_eq!(Option::<widgets::NavView>::from(View::Settings), None);
    }

    /// The split-pane slot-math fractions are derived from the `FillPortion`
    /// weights, and must stay bit-identical to the `0.55` / `0.45` literals
    /// they replaced — otherwise a portion retune that forgets to keep the
    /// fraction in step would silently shift pixels off-ratio. This is a
    /// permanent drift guard, not a runtime behavior test.
    #[test]
    fn split_pane_fractions_match_fill_portions() {
        assert_eq!(QUEUE_PANE_FRACTION.to_bits(), 0.55f32.to_bits());
        assert_eq!(BROWSER_PANE_FRACTION.to_bits(), 0.45f32.to_bits());
        assert_eq!(QUEUE_PANE_PORTION + BROWSER_PANE_PORTION, 100);
    }
}
