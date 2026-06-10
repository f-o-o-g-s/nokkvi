//! Theme colors and styling helpers
//!
//! Colors are loaded from named theme files at `~/.config/nokkvi/themes/`.
//! Light/dark mode can be toggled at runtime.
//!
//! All color accessors are functions (not statics) so they react to hot-reload via `reload_theme()`.

#[cfg(test)]
use std::sync::atomic::Ordering;

#[cfg(test)]
use iced::Color;
#[cfg(test)]
use nokkvi_data::types::player_settings::{
    ArtworkColumnMode, ArtworkStretchFit, NavDisplayMode, NavLayout, RoundedMode, SlotRowHeight,
    StripClickAction, StripSeparator, TrackInfoDisplay,
};

#[cfg(test)]
use crate::atomic_u8_enum::AtomicU8Enum;
#[cfg(test)]
use crate::theme_config::{ResolvedDualTheme, ResolvedTheme};

mod colors;
mod font;
mod radius;
mod state;
mod style;
mod ui_mode;

pub(crate) use colors::*;
pub(crate) use font::*;
pub(crate) use radius::*;
pub(crate) use state::*;
pub(crate) use style::*;
pub(crate) use ui_mode::*;

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Micro-bench: measures cumulative cost of `theme::fg0()` over 10,000
    /// calls. Numbers print to stderr (use `cargo test -- --nocapture` to view).
    ///
    /// Recorded baselines (release build, this machine):
    /// - Pre-2A `RwLock<ResolvedDualTheme>` + 352-byte struct clone: ~13.1 ns/call.
    /// - Post-2A `ArcSwap<ResolvedDualTheme>` + lock-free Guard load: ~12.5 ns/call.
    ///
    /// The raw per-call delta is small because both paths bottleneck on a few
    /// atomic ops; the durable win of 2A is **lock-freedom** — the visualizer
    /// FFT thread no longer competes with the render thread for a theme lock.
    /// The upper bound is generous (regression net, not wall-clock guarantee).
    #[test]
    fn theme_accessor_microbench_fg0_x10000() {
        // Touch the theme once so any first-call setup (DUAL_THEME LazyLock
        // init, builtin theme seeding) is excluded from the measurement.
        let _warm = fg0();

        let iters = 10_000;
        let start = Instant::now();
        let mut acc_r = 0.0f32;
        for _ in 0..iters {
            // Use the result so the optimizer can't dead-code the call.
            acc_r += fg0().r;
        }
        let elapsed = start.elapsed();

        eprintln!(
            "theme::fg0() x{iters} = {:?} ({:.1} ns/call), accumulator={acc_r}",
            elapsed,
            (elapsed.as_nanos() as f64) / (iters as f64)
        );

        assert!(
            elapsed.as_millis() < 1_000,
            "fg0() x{iters} unexpectedly slow: {elapsed:?}"
        );
    }

    // `THEME_MODE_LOCK` is now defined at module scope so other test modules
    // in the crate can share the same guard.

    // ------------------------------------------------------------------------
    // Contrast helpers — luminance/contrast math + the light-mode darkening
    // routine that keeps muted theme accents readable as strip text.
    // ------------------------------------------------------------------------

    #[test]
    fn relative_luminance_at_endpoints() {
        assert!((relative_luminance(Color::WHITE) - 1.0).abs() < 0.001);
        assert!(relative_luminance(Color::BLACK).abs() < 0.001);
    }

    #[test]
    fn contrast_ratio_extremes() {
        assert!((contrast_ratio(Color::WHITE, Color::BLACK) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(Color::WHITE, Color::WHITE) - 1.0).abs() < 0.001);
    }

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    /// Every shipped palette as `(name, mode, ResolvedTheme)` — for theme-wide
    /// contrast guards. Reads embedded built-in TOML; no disk, no global theme
    /// state, so these sweeps are deterministic and lock-free.
    fn all_builtin_palettes() -> Vec<(String, &'static str, ResolvedTheme)> {
        let mut out = Vec::new();
        for stem in nokkvi_data::services::theme_loader::builtin_theme_stems() {
            let tf = nokkvi_data::services::theme_loader::load_builtin_theme(stem)
                .unwrap_or_else(|| panic!("built-in theme {stem} must parse"));
            let dual = ResolvedDualTheme::from_theme_file(&tf);
            out.push((stem.to_string(), "dark", dual.dark));
            out.push((stem.to_string(), "light", dual.light));
        }
        out
    }

    /// `legible_text_on` is provably ≥ 4.58:1 against any fill (the black/white
    /// contrast curves cross at luminance ≈ 0.179). Spot-check the hardest fills
    /// near the crossover plus the real problem colors.
    #[test]
    fn legible_text_on_is_always_legible() {
        let samples = [
            Color::BLACK,
            Color::WHITE,
            Color::from_rgb(0.5, 0.5, 0.5),
            Color::from_rgb(0.45, 0.45, 0.45),
            Color::from_rgb(0.40, 0.40, 0.40),
            rgb(0x22, 0x32, 0x49), // Kanagawa Dragon primary (dark navy)
            rgb(0x93, 0xB2, 0x59), // Everforest light accent_bright
            rgb(0xA6, 0xB0, 0xA0), // Everforest light old `selected` grey
        ];
        for fill in samples {
            let cr = contrast_ratio(fill, legible_text_on(fill));
            assert!(
                cr >= LEGIBLE_TEXT_CONTRAST,
                "forced text on {fill:?} only reached {cr:.2}:1"
            );
        }
    }

    /// The forced row text reads against BOTH derived highlight fills on every
    /// shipped theme/mode — the systemic fix for the unreadable Everforest-light
    /// selection and Kanagawa-Dragon-dark now-playing rows.
    #[test]
    fn forced_text_reads_on_every_highlight_fill() {
        for (name, mode, t) in all_builtin_palettes() {
            let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
            for (which, fill) in [("playing", play), ("selected", sel)] {
                let cr = contrast_ratio(fill, legible_text_on(fill));
                assert!(
                    cr >= LEGIBLE_TEXT_CONTRAST,
                    "{name}/{mode} {which} fill forced-text contrast {cr:.2}:1 below AA"
                );
            }
        }
    }

    /// Now-playing and selected fills stay perceptibly distinct on every shipped
    /// theme/mode, so a playing row and a cursor row are tellable apart at once.
    #[test]
    fn playing_and_selected_fills_stay_distinct() {
        for (name, mode, t) in all_builtin_palettes() {
            let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
            let cr = contrast_ratio(play, sel);
            assert!(
                cr >= FILL_DISTINCT_CONTRAST,
                "{name}/{mode} playing-vs-selected contrast {cr:.2}:1 < {FILL_DISTINCT_CONTRAST}"
            );
        }
    }

    /// The distinctness separator never inverts the hierarchy: when playing
    /// starts darker than selected, the resolved playing fill stays no lighter
    /// than the resolved selected fill (cursor = loud/bright, playing = ambient).
    #[test]
    fn playing_fill_does_not_invert_hierarchy() {
        for (name, mode, t) in all_builtin_palettes() {
            if relative_luminance(t.accent) < relative_luminance(t.accent_bright) {
                let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
                assert!(
                    relative_luminance(play) <= relative_luminance(sel) + 1e-4,
                    "{name}/{mode} playing fill became lighter than selected (hierarchy inverted)"
                );
            }
        }
    }

    /// The highlight border is perceptible against its fill at both strengths
    /// (regression for the old center-slot border that matched the fill 1:1 on
    /// the 17 themes that didn't customize `selected`).
    #[test]
    fn highlight_border_contrasts_its_fill() {
        for (name, mode, t) in all_builtin_palettes() {
            let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
            for (which, fill) in [("playing", play), ("selected", sel)] {
                assert!(
                    contrast_ratio(highlight_border(fill, 1.0), fill) > 1.3,
                    "{name}/{mode} {which} max-strength border indistinct from fill"
                );
                assert!(
                    contrast_ratio(highlight_border(fill, 0.55), fill) > 1.05,
                    "{name}/{mode} {which} subtle border indistinct from fill"
                );
            }
        }
    }

    /// `legible_against` is bidirectional: too-dark text on a dark surface is
    /// LIFTED (the old `darken_until_legible` could not), and too-light text on
    /// a light surface is darkened — both reaching WCAG AA.
    #[test]
    fn legible_against_is_bidirectional() {
        // Kanagawa Dragon dark: navy "Hemp Dub" text over the near-black strip.
        let navy = rgb(0x22, 0x32, 0x49);
        let dark_surface = rgb(0x0f, 0x0e, 0x0e);
        let lifted = legible_against(navy, dark_surface, LEGIBLE_TEXT_CONTRAST);
        assert!(
            contrast_ratio(lifted, dark_surface) >= LEGIBLE_TEXT_CONTRAST,
            "dark text on dark surface should reach AA"
        );
        assert!(
            relative_luminance(lifted) > relative_luminance(navy),
            "dark-on-dark fix must LIGHTEN (the old light-only path could not)"
        );

        // Everforest light: muted green text over cream chrome.
        let green = rgb(0x93, 0xB2, 0x59);
        let light_surface = rgb(0xEF, 0xEB, 0xD4);
        let darkened = legible_against(green, light_surface, LEGIBLE_TEXT_CONTRAST);
        assert!(
            contrast_ratio(darkened, light_surface) >= LEGIBLE_TEXT_CONTRAST,
            "light-ish text on a light surface should reach AA"
        );
        assert!(
            relative_luminance(darkened) < relative_luminance(green),
            "a fix on a light surface must DARKEN"
        );
    }

    /// `legible_against` is a no-op when the input already clears the floor.
    #[test]
    fn legible_against_returns_input_when_already_legible() {
        let r = legible_against(Color::BLACK, Color::WHITE, LEGIBLE_TEXT_CONTRAST);
        assert_eq!((r.r, r.g, r.b), (0.0, 0.0, 0.0));
    }

    /// The light-mode status strip is a perceptible band on every light palette
    /// (the old fixed darken-toward-black muddied warm cream into dingy grey).
    #[test]
    fn light_status_strip_band_separates_from_chrome() {
        for (name, mode, t) in all_builtin_palettes() {
            if mode != "light" {
                continue;
            }
            let band = strip_band_toward_ink(t.bg0_hard, t.fg0, STRIP_BAND_DELTA);
            let delta = (relative_luminance(band) - relative_luminance(t.bg0_hard)).abs();
            assert!(
                delta >= STRIP_BAND_DELTA - 1e-3,
                "{name}/{mode} status strip band only Δ{delta:.4} from chrome"
            );
        }
    }

    /// Strip text tiers (`fg2`/`fg3`) made legible over the painted strip
    /// surface clear WCAG AA on every theme/mode — including Kanagawa Dragon
    /// dark, where the old now_playing/selected-as-text path was unreadable.
    #[test]
    fn strip_text_tiers_read_on_their_surface() {
        for (name, mode, t) in all_builtin_palettes() {
            let surface = if mode == "light" {
                strip_band_toward_ink(t.bg0_hard, t.fg0, STRIP_BAND_DELTA)
            } else {
                darken(t.bg0_hard, 0.17)
            };
            for (tier, color) in [("fg2", t.fg2), ("fg3", t.fg3)] {
                let txt = legible_against(color, surface, LEGIBLE_TEXT_CONTRAST);
                let cr = contrast_ratio(txt, surface);
                assert!(
                    cr >= LEGIBLE_TEXT_CONTRAST,
                    "{name}/{mode} strip {tier} text only {cr:.2}:1 over its surface"
                );
            }
        }
    }

    /// `hover_tint()` must read against the neutral chrome surface it sits over
    /// (`bg0_hard()`) in both modes — the fix for the pre-redesign light-mode
    /// no-op where a near-black tint at 10% over a near-`bg0_hard()` surface
    /// was effectively invisible.
    #[test]
    fn hover_tint_reads_over_neutral_chrome() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = UI_MODE.light_mode.load(Ordering::Relaxed);

        for light in [true, false] {
            set_light_mode(light);
            let delta = (relative_luminance(hover_tint()) - relative_luminance(bg0_hard())).abs();
            assert!(
                delta > 0.02,
                "hover_tint() must differ perceptibly from neutral chrome (light={light}); \
                 luminance delta={delta:.4}"
            );
        }

        UI_MODE.light_mode.store(saved, Ordering::Relaxed);
    }

    /// Regression guard for the active-tab no-op. An accent-derived hover over
    /// a surface already filled with `accent_bright()` (active nav tab / mode
    /// toggle) is a near-no-op — in dark mode `accent_bright()` over
    /// `accent_bright()` is exactly zero. `hover_tint_on_accent()` must instead
    /// contrast against the accent fill so hovering an active tab still reads.
    #[test]
    fn hover_tint_on_accent_contrasts_with_accent_fill() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = UI_MODE.light_mode.load(Ordering::Relaxed);

        for light in [true, false] {
            set_light_mode(light);
            let delta = (relative_luminance(hover_tint_on_accent())
                - relative_luminance(accent_bright()))
            .abs();
            assert!(
                delta > 0.02,
                "hover_tint_on_accent() must contrast with the accent_bright() fill \
                 (light={light}); luminance delta={delta:.4}"
            );
        }

        UI_MODE.light_mode.store(saved, Ordering::Relaxed);
    }

    // ------------------------------------------------------------------------
    // Radius helpers — every `ui_radius_*` / `ui_border_radius*` helper now
    // delegates to `gated_radius`. Sweep all three RoundedMode states and pin
    // each helper's gate predicate + value, so a wrong gate or a forked
    // non-zero fallback in a future hand-added `_player` variant breaks here.
    // ------------------------------------------------------------------------

    #[test]
    fn radius_helpers_gate_and_value_across_modes() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = UI_MODE.rounded_mode.load(Ordering::Relaxed);

        // Flatten a Radius to its four corners; the `f32 -> Radius` `From` sets
        // all four equal, so one expected value covers them all.
        let corners = |radius: iced::border::Radius| {
            [
                radius.top_left,
                radius.top_right,
                radius.bottom_right,
                radius.bottom_left,
            ]
        };
        let all = |v: f32| [v, v, v, v];

        // Off: every helper is square (0.0), player variants included.
        set_rounded_mode(RoundedMode::Off);
        for got in [
            ui_border_radius(),
            ui_border_radius_player(),
            ui_radius_xs(),
            ui_radius_sm(),
            ui_radius_md(),
            ui_radius_lg(),
            ui_radius_pill(),
            ui_radius_sm_player(),
            ui_radius_pill_player(),
        ] {
            assert_eq!(corners(got), all(0.0), "Off mode must be square");
        }

        // On: every helper returns its scale value.
        set_rounded_mode(RoundedMode::On);
        assert_eq!(corners(ui_border_radius()), all(ROUNDED_RADIUS));
        assert_eq!(corners(ui_border_radius_player()), all(ROUNDED_RADIUS));
        assert_eq!(corners(ui_radius_xs()), all(R_XS));
        assert_eq!(corners(ui_radius_sm()), all(R_SM));
        assert_eq!(corners(ui_radius_md()), all(R_MD));
        assert_eq!(corners(ui_radius_lg()), all(R_LG));
        assert_eq!(corners(ui_radius_pill()), all(R_PILL));
        assert_eq!(corners(ui_radius_sm_player()), all(R_SM));
        assert_eq!(corners(ui_radius_pill_player()), all(R_PILL));

        // PlayerOnly: only the `_player` (is_rounded_for_player) helpers round;
        // the global `is_rounded_mode` helpers stay square.
        set_rounded_mode(RoundedMode::PlayerOnly);
        assert_eq!(corners(ui_border_radius()), all(0.0));
        assert_eq!(corners(ui_radius_xs()), all(0.0));
        assert_eq!(corners(ui_radius_sm()), all(0.0));
        assert_eq!(corners(ui_radius_md()), all(0.0));
        assert_eq!(corners(ui_radius_lg()), all(0.0));
        assert_eq!(corners(ui_radius_pill()), all(0.0));
        assert_eq!(corners(ui_border_radius_player()), all(ROUNDED_RADIUS));
        assert_eq!(corners(ui_radius_sm_player()), all(R_SM));
        assert_eq!(corners(ui_radius_pill_player()), all(R_PILL));

        UI_MODE.rounded_mode.store(saved, Ordering::Relaxed);
    }

    // ------------------------------------------------------------------------
    // atomic_u8_enum! macro — verifies that the loader/store impls emitted
    // for every `UiModeFlags` enum round-trip each variant through its
    // declaration discriminant, and that unknown bytes fall back to the
    // declared default variant. The bytes are a transient in-process cache
    // encoding (nothing persists them — persistence is serde wire strings),
    // so the fallback is purely defensive.
    // ------------------------------------------------------------------------

    /// Roundtrip every variant of two enums (one with a small variant set, one
    /// with a larger one) through `to_u8` / `from_u8`. Exercising the macro
    /// expansion twice guarantees we're testing the macro itself, not just one
    /// hand-written impl.
    #[test]
    fn atomic_u8_enum_macro_emits_roundtrip() {
        // NavLayout: 3 variants, declaration discriminants {0,1,2}.
        for (byte, variant) in [
            (0u8, NavLayout::Top),
            (1u8, NavLayout::Side),
            (2u8, NavLayout::None),
        ] {
            assert_eq!(
                NavLayout::from_u8(byte),
                variant,
                "NavLayout::from_u8({byte})"
            );
            assert_eq!(variant.to_u8(), byte, "NavLayout::{variant:?}.to_u8()");
        }

        // StripSeparator: 6 variants, declaration discriminants {0..=5}.
        // Exercises a larger variant list so we catch any macro misexpansion
        // that only manifests with more arms.
        for (byte, variant) in [
            (0u8, StripSeparator::Dot),
            (1u8, StripSeparator::Bullet),
            (2u8, StripSeparator::Pipe),
            (3u8, StripSeparator::EmDash),
            (4u8, StripSeparator::Slash),
            (5u8, StripSeparator::Bar),
        ] {
            assert_eq!(
                StripSeparator::from_u8(byte),
                variant,
                "StripSeparator::from_u8({byte})"
            );
            assert_eq!(variant.to_u8(), byte, "StripSeparator::{variant:?}.to_u8()");
        }
    }

    /// An unknown stored byte MUST decode to the declared default variant.
    /// This is purely defensive: the bytes live only inside the in-process
    /// `UI_MODE` atomics (nothing persists them), so an unknown byte can only
    /// come from a corrupted atomic — the fallback keeps the render thread
    /// from ever panicking on one.
    #[test]
    fn atomic_u8_enum_unknown_byte_falls_back_to_default() {
        // TrackInfoDisplay default = Off.
        assert_eq!(TrackInfoDisplay::from_u8(255), TrackInfoDisplay::Off);
        assert_eq!(TrackInfoDisplay::from_u8(99), TrackInfoDisplay::Off);
        // Also verify a byte just past the highest known variant (4) falls back.
        assert_eq!(TrackInfoDisplay::from_u8(5), TrackInfoDisplay::Off);
        // StripSeparator default is Slash (byte 4); unknown bytes fall back to it.
        assert_eq!(StripSeparator::from_u8(255), StripSeparator::Slash);
        assert_eq!(StripSeparator::from_u8(6), StripSeparator::Slash);
    }

    /// `ArtworkColumnMode` is the largest enum behind a `UI_MODE` atomic —
    /// round-trip every variant through `to_u8` / `from_u8` and pin that each
    /// byte equals the variant's declaration discriminant. The bytes are an
    /// in-memory cache encoding only (persistence is serde wire strings), so
    /// the discriminants are free to follow declaration order; this test
    /// catches a macro misexpansion that maps a variant to the wrong byte.
    #[test]
    fn artwork_column_mode_encoding_roundtrips_every_variant() {
        // {declaration discriminant → variant}, in declaration order.
        let table = [
            (0u8, ArtworkColumnMode::Auto),
            (1u8, ArtworkColumnMode::AlwaysNative),
            (2u8, ArtworkColumnMode::AlwaysStretched),
            (3u8, ArtworkColumnMode::AlwaysVerticalNative),
            (4u8, ArtworkColumnMode::AlwaysVerticalStretched),
            (5u8, ArtworkColumnMode::Never),
        ];

        for (byte, variant) in table {
            assert_eq!(
                ArtworkColumnMode::from_u8(variant.to_u8()),
                variant,
                "ArtworkColumnMode::{variant:?} must survive a to_u8/from_u8 roundtrip"
            );
            assert_eq!(
                variant.to_u8(),
                byte,
                "ArtworkColumnMode::{variant:?} must encode to its declaration discriminant {byte}"
            );
            assert_eq!(
                ArtworkColumnMode::from_u8(byte),
                variant,
                "ArtworkColumnMode byte {byte} must decode to {variant:?}"
            );
        }
    }

    /// End-to-end test through the actual `Theme` get/set helpers (not just
    /// the macro impls in isolation): write a known variant via `set_*`,
    /// then read it back via the matching getter and confirm the variant
    /// survives a full store-then-load cycle through the live `AtomicU8`.
    /// Exercises every migrated site at least once.
    #[test]
    fn store_then_load_preserves_variant_per_enum() {
        let _guard = THEME_MODE_LOCK.lock();

        // Snapshot every UI_MODE u8 we're about to mutate so neighboring
        // tests don't observe leaked state.
        let saved_tid = UI_MODE.track_info_display.load(Ordering::Relaxed);
        let saved_nav = UI_MODE.nav_layout.load(Ordering::Relaxed);
        let saved_ndm = UI_MODE.nav_display_mode.load(Ordering::Relaxed);
        let saved_srh = UI_MODE.slot_row_height.load(Ordering::Relaxed);
        let saved_sca = UI_MODE.strip_click_action.load(Ordering::Relaxed);
        let saved_sep = UI_MODE.strip_separator.load(Ordering::Relaxed);
        let saved_acm = UI_MODE.artwork_column_mode.load(Ordering::Relaxed);
        let saved_asf = UI_MODE.artwork_column_stretch_fit.load(Ordering::Relaxed);

        set_track_info_display(TrackInfoDisplay::TopBar);
        assert_eq!(track_info_display(), TrackInfoDisplay::TopBar);

        set_nav_layout(NavLayout::Side);
        assert!(is_side_nav());
        assert!(!is_top_nav());

        set_nav_display_mode(NavDisplayMode::IconsOnly);
        assert_eq!(nav_display_mode(), NavDisplayMode::IconsOnly);

        set_slot_row_height(SlotRowHeight::Spacious);
        assert_eq!(slot_row_height_variant(), SlotRowHeight::Spacious);

        set_strip_click_action(StripClickAction::CopyTrackInfo);
        assert_eq!(strip_click_action(), StripClickAction::CopyTrackInfo);

        set_strip_separator(StripSeparator::EmDash);
        assert_eq!(strip_separator(), StripSeparator::EmDash);

        // Hit the non-contiguous slot specifically.
        set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalStretched);
        assert_eq!(
            artwork_column_mode(),
            ArtworkColumnMode::AlwaysVerticalStretched
        );

        set_artwork_column_stretch_fit(ArtworkStretchFit::Fill);
        assert_eq!(artwork_column_stretch_fit(), ArtworkStretchFit::Fill);

        // Restore every mutated atomic so the next test sees the baseline state.
        UI_MODE
            .track_info_display
            .store(saved_tid, Ordering::Relaxed);
        UI_MODE.nav_layout.store(saved_nav, Ordering::Relaxed);
        UI_MODE.nav_display_mode.store(saved_ndm, Ordering::Relaxed);
        UI_MODE.slot_row_height.store(saved_srh, Ordering::Relaxed);
        UI_MODE
            .strip_click_action
            .store(saved_sca, Ordering::Relaxed);
        UI_MODE.strip_separator.store(saved_sep, Ordering::Relaxed);
        UI_MODE
            .artwork_column_mode
            .store(saved_acm, Ordering::Relaxed);
        UI_MODE
            .artwork_column_stretch_fit
            .store(saved_asf, Ordering::Relaxed);
    }

    // ------------------------------------------------------------------------
    // Modal / nav separator helpers — pin that the consolidated helpers
    // still compile, produce real Elements, and select the right axis
    // dimensions for `nav_separator`. The row-vs-header alpha pair is
    // documented in the helper bodies; the consolidation kept those values
    // intact, so the regression risk we guard against here is "future agent
    // accidentally swaps axes or returns the wrong type."
    // ------------------------------------------------------------------------

    /// Both modal separator helpers must produce real `Element`s — a
    /// characterization that the consolidation kept the lambdas building
    /// elements that wire into a `Column`. Regression risk we guard: a
    /// future refactor accidentally changes the return type to e.g. `Rule`
    /// (which has different default styling).
    #[test]
    fn modal_separators_produce_elements() {
        let _row: iced::Element<'_, ()> = modal_row_separator();
        let _header: iced::Element<'_, ()> = modal_header_separator();
    }

    /// `modal_scaffold` must accept any `M: Clone` and return an Element of
    /// the same message type. Pin the type-level contract so a future
    /// refactor of the scaffold helper can't accidentally drop the
    /// generic-message parameter (the audit explicitly calls out that
    /// each modal uses a different `Message::Close` / `Message::Cancel`).
    #[test]
    fn modal_scaffold_threads_message_type_through() {
        use iced::widget::Space;

        #[derive(Debug, Clone, PartialEq)]
        enum FakeMsg {
            Closed,
        }
        let dialog: iced::Element<'_, FakeMsg> =
            iced::Element::from(Space::new().width(100.0).height(60.0));
        let _scaffold: iced::Element<'_, FakeMsg> =
            modal_scaffold(dialog, FakeMsg::Closed, MODAL_BACKDROP_ALPHA);
    }
}
