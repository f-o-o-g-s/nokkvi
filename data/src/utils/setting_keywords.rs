//! Hidden synonym / alias index for settings search.
//!
//! Substring and fuzzy matching only help when the user types letters that are
//! actually present in a row's label / category / subtitle. They do nothing for
//! *vocabulary mismatch* — a user typing the domain term they know (`loudness`,
//! `replaygain`, `systray`, `dark mode`) when the label uses a different word
//! (`Volume Normalization`, `Show Tray Icon`, `Theme Mode`). That empty-result
//! miss is the worst search outcome: the user concludes the setting doesn't
//! exist.
//!
//! [`keywords_for`] maps a row's config key to a curated list of alias terms
//! that are matched (via the same fuzzy scorer as the visible fields) but never
//! rendered. Keeping the whole synonym set in one keyed table — rather than a
//! field scattered across the item builders — leaves the `define_settings!`
//! macro and every `SettingMeta` literal untouched, works uniformly for
//! macro-generated and hand-built rows, and degrades gracefully: a renamed key
//! simply stops resolving aliases (no breakage, just a dud synonym).

/// Declare the alias table once, emitting both the [`keywords_for`] lookup and
/// the enumerable [`TABLE_KEYS`] list.
///
/// A plain `match` cannot be enumerated at runtime, so the "is this entry still
/// live?" direction had no way to be tested and four entries rotted unnoticed.
/// Deriving both from one list closes that: an entry cannot be in the lookup
/// but missing from the audit list.
macro_rules! keyword_table {
    ($( $($key:literal)|+ => $terms:expr ),* $(,)?) => {
        /// Curated alias terms for a settings row, keyed by its
        /// (palette-normalized) config key. Returns `&[]` for rows without
        /// aliases.
        ///
        /// Terms are lowercase and intentionally add vocabulary the
        /// label/subtitle lacks — acronyms, domain synonyms, and common
        /// alternate names — rather than repeating words the visible text
        /// already carries.
        ///
        /// **Every row the settings UI renders must resolve here** (or via
        /// [`family_keywords_for`]); `every_settings_row_has_search_keywords`
        /// in `src/views/settings/entries.rs` fails the build otherwise. Keys
        /// must also stay live — `no_dead_keyword_table_keys` rejects an entry
        /// whose row no longer exists.
        pub fn keywords_for(key: &str) -> &'static [&'static str] {
            match normalize(key) {
                $( $($key)|+ => $terms, )*
                _ => &[],
            }
        }

        /// Every key the alias table declares, for coverage auditing. Order
        /// follows the declaration, and palette-prefixed rows appear in their
        /// normalized (prefix-stripped) form.
        pub const TABLE_KEYS: &[&str] = &[ $( $($key),+ ),* ];
    };
}

/// Strip a leading `dark.` / `light.` palette segment so both theme palettes
/// share one alias list. Non-palette keys (e.g. `general.light_mode`) are
/// returned unchanged — only a literal `dark.` / `light.` *prefix* is removed.
fn normalize(key: &str) -> &str {
    key.strip_prefix("dark.")
        .or_else(|| key.strip_prefix("light."))
        .unwrap_or(key)
}

/// Alias terms shared by a whole *family* of keys, so vocabulary that applies
/// uniformly is declared once instead of copied onto every row.
///
/// Kept separate from [`keywords_for`] because a `match` arm returns one slice:
/// a per-key arm and a family arm cannot be concatenated there without either
/// duplicating the shared terms into all 53 hotkey arms or losing the per-key
/// ones. [`all_keywords_for`] is what search iterates.
pub fn family_keywords_for(key: &str) -> &'static [&'static str] {
    if key.starts_with("hotkey.") || key == "__restore_all_hotkeys" {
        // Nothing on a hotkey row's label, subtitle or category carries the
        // words a user actually types to find the tab.
        return &["shortcut", "keybind", "key binding", "keyboard", "hotkey"];
    }
    &[]
}

/// Every alias term for a row: its own plus its family's. This is the search
/// path's entry point — prefer it over [`keywords_for`].
pub fn all_keywords_for(key: &str) -> impl Iterator<Item = &'static str> {
    keywords_for(key)
        .iter()
        .chain(family_keywords_for(key))
        .copied()
}

// The alias table. Four entries rotted here before this was auditable: two when
// the inline theme colour editors were removed, two when their fields were
// folded into a ToggleSet whose row carries a sentinel key instead.
keyword_table! {
        // ── Hotkeys · Views ──────────────────────────────────────────────────
        // (every hotkey row also inherits `family_keywords_for`)
        "__restore_all_hotkeys" => &["reset", "defaults", "revert", "factory"],
        "hotkey.switch_to_queue"
        | "hotkey.switch_to_albums"
        | "hotkey.switch_to_artists"
        | "hotkey.switch_to_songs"
        | "hotkey.switch_to_genres"
        | "hotkey.switch_to_playlists"
        | "hotkey.switch_to_radios"
        | "hotkey.switch_to_harbour"
        | "hotkey.switch_to_settings" => &["go to", "jump to", "open view", "navigate", "switch"],

        // ── Hotkeys · Playback ───────────────────────────────────────────────
        "hotkey.toggle_play" => &["pause", "resume", "start", "stop", "space"],
        "hotkey.toggle_random" => &["shuffle", "randomize", "mix"],
        "hotkey.toggle_repeat" => &["loop", "again", "replay"],
        "hotkey.toggle_consume" => &["remove after play", "burn", "pop"],
        "hotkey.toggle_sfx" => &["sound effects", "clicks", "audio feedback", "beeps"],
        "hotkey.cycle_vis" => &["spectrum", "bars", "lines", "scope", "waveform"],
        "hotkey.toggle_eq_modal" => &["eq", "bands", "tone", "treble", "bass"],
        "hotkey.open_trawl" => &["mix", "trawl", "crate", "builder", "blend", "anchor"],
        "hotkey.toggle_crossfade" => &["fade", "blend", "gapless"],
        "hotkey.toggle_lyrics" => &["karaoke", "synced", "words", "subtitles"],
        "hotkey.toggle_bit_perfect" => &["lossless", "audiophile", "hi-res", "passthrough"],

        // ── Hotkeys · Navigation ─────────────────────────────────────────────
        "hotkey.slot_list_up" | "hotkey.slot_list_down" => {
            &["scroll", "move cursor", "navigate", "arrow"]
        },
        "hotkey.activate" => &["enter", "play", "open", "select"],
        "hotkey.expand_center" => &["open", "unfold", "drill down", "show tracks"],
        "hotkey.shuffle_play" => &["random", "randomize", "mix"],
        "hotkey.toggle_browsing_panel" => &["split view", "side by side", "browser", "panel"],
        "hotkey.center_playing" => &["scroll to", "jump to current", "find playing", "locate"],
        "hotkey.focus_search" => &["find", "filter", "query", "type"],
        "hotkey.settings_category_next" | "hotkey.settings_category_prev" => {
            &["settings tab", "sidebar", "category"]
        },

        // ── Hotkeys · Item Actions ───────────────────────────────────────────
        "hotkey.toggle_star" => &["heart", "favorite", "loved", "like"],
        "hotkey.add_to_queue" => &["enqueue", "append", "play later"],
        "hotkey.remove_from_queue" => &["delete", "drop", "unqueue"],
        "hotkey.clear_queue" => &["empty", "wipe", "reset queue"],
        "hotkey.increase_rating" | "hotkey.decrease_rating" => &["stars", "rate"],
        "hotkey.get_info" => &["properties", "details", "metadata", "tags"],
        "hotkey.find_similar" => &["related", "more like this", "recommendations", "discover"],
        "hotkey.find_top_songs" => &["best of", "popular", "greatest hits", "most played"],
        "hotkey.move_track_up" | "hotkey.move_track_down" => &["reorder", "rearrange", "shift"],
        "hotkey.save_queue_as_playlist" => &["export", "store", "create playlist"],
        "hotkey.trawl_save_as_playlist" => &["mix", "crate", "export", "create playlist"],
        "hotkey.new_smart_playlist" => &["nsp", "rules", "dynamic playlist", "auto playlist"],
        "hotkey.edit_centered_playlist" => &["rename", "modify", "change playlist"],

        // ── Hotkeys · Sort & View / Settings Edit ────────────────────────────
        "hotkey.cycle_view_left" | "hotkey.cycle_view_right" => {
            &["sort", "ordering", "arrange"]
        },
        "hotkey.toggle_sort_order" => &["ascending", "descending", "reverse", "flip"],
        "hotkey.refresh_view" => &["reload", "rescan", "update", "sync"],
        "hotkey.roulette" => &["random pick", "slot machine", "surprise me", "spin"],
        "hotkey.edit_up" | "hotkey.edit_down" => {
            &["change value", "adjust", "increment", "decrement"]
        },

        // ── General · Library / Display / Behavior ───────────────────────────
        "general.library_page_size" => &["pagination", "batch size", "fetch count"],
        "general.artwork_resolution" => {
            &["cover art", "album art", "image quality", "thumbnail size"]
        },
        "general.show_album_artists_only" => &["compilation", "featured", "guest artists"],
        "general.start_view" => &["home", "startup", "landing page", "default page"],
        "general.enter_behavior" => &["return key", "double click", "activate"],
        "general.enter_shuffle" => &["random", "shuffle play", "randomize", "mix"],
        "general.auto_follow_playing" => &["autoscroll", "jump to current", "center on track"],
        "general.stable_viewport" => &["no scroll", "click to select", "anchor", "in place"],
        "general.suppress_library_refresh_toasts" => {
            &["notification", "rescan", "popup", "hide toast"]
        },

        // ── General · Window & Tray / Advanced / Account ─────────────────────
        "general.show_tray_icon" => &["systray", "notification area", "status indicator"],
        "general.close_to_tray" => &["minimize to tray", "background", "hide window"],
        "general.local_music_path" => &["folder", "directory", "library location"],
        "general.verbose_config" => &["full config", "explicit", "dump defaults"],
        "general.server_url" => &["host", "address", "navidrome", "endpoint", "connection"],
        "general.username" => &["login", "account", "user", "credentials"],
        "__action_logout" => &["log out", "sign off", "switch account", "disconnect"],

        // ── Interface · Navigation / Slot List / Player Bar / Font ───────────
        "general.nav_layout" => &["sidebar", "menu position", "tabs"],
        "general.nav_display_mode" => &["labels", "glyphs", "tab style"],
        "general.slot_row_height" => &["compact", "spacing", "list density"],
        "general.horizontal_volume" => &["volume layout", "slider orientation"],
        "general.autohide_toolbar" => &["collapse toolbar", "hide search bar", "hide sort bar"],
        "general.autohide_collapsed_appearance" => {
            &["hairline", "strip", "collapsed toolbar", "sliver"]
        },
        "general.autohide_toolbar_height" => &["reveal zone", "strip height", "toolbar size"],
        "general.autohide_toolbar_grip" => &["handle", "grab bar", "pull tab", "toolbar grip"],
        "general.scrollbar_visibility" => &["scroll bar", "gutter", "handle", "thumb", "track"],
        "general.icon_set" => &["phosphor", "lucide", "glyphs", "symbols", "iconography"],
        "general.track_info_display" => &["now playing", "song info", "metadata"],
        "__toggle_mini_player_controls" => {
            &["mini player", "transport", "buttons", "volume", "shuffle repeat"]
        },
        // Salvaged from `general.strip_show_format_info`, which is a field
        // INSIDE this ToggleSet — search only ever sees the parent row's key.
        "__toggle_strip_fields" => {
            &["codec", "bitrate", "quality", "flac", "sample rate", "now playing fields"]
        },
        "general.strip_show_labels" => &["captions", "field names", "prefix"],
        // Salvaged from `general.albums_artwork_overlay` (same ToggleSet story).
        "__toggle_artwork_overlays" => {
            &["caption", "label on cover", "title on art", "hover text"]
        },
        "general.artwork_auto_max_pct" => &["cover size", "artwork size", "max width", "panel"],
        "general.artwork_vertical_height_pct" => &["cover size", "artwork height", "panel"],
        "general.strip_separator" => &["delimiter", "divider"],
        "general.artwork_column_mode" => &["cover art", "album art panel", "sidebar art"],
        "general.artwork_column_stretch_fit" => &["crop", "fill", "aspect ratio", "cover"],
        "general.slot_text_links" => &[
            "hyperlink",
            "clickable text",
            "jump to artist",
            "jump to album",
        ],
        // `general.albums_artwork_overlay` and `general.strip_show_format_info`
        // used to be keyed here, but both are fields inside a ToggleSet row —
        // search only ever sees the parent sentinel key, so the entries were
        // unreachable. Their vocabulary now lives on `__toggle_artwork_overlays`
        // and `__toggle_strip_fields` above.
        "general.strip_merged_mode" => &["combined", "single line", "joined"],
        "general.strip_click_action" => &["tap", "on click", "copy info"],
        "font_family" => &["typeface", "typography"],

        // ── Playback · Transitions ───────────────────────────────────────────
        "general.crossfade_enabled" => &["fade", "blend", "gapless"],
        "general.lyrics_enabled" => &["lyrics", "karaoke", "synced", "subtitles", "words"],
        "general.lyrics_fetch_online" => &["lrclib", "lyrics download", "internet", "fetch"],
        "general.lyrics_backdrop_blur" => &["blur", "frost", "frosted", "backdrop", "cover blur"],
        "general.crossfade_duration" => &["fade time", "fade length"],
        "general.crossfade_curve" => &["equal power", "constant gain", "fade shape", "fade curve"],
        "general.crossfade_min_track" => &["short tracks", "interlude", "skit", "minimum length"],
        "general.crossfade_album_gapless" => &[
            "segue",
            "attacca",
            "live album",
            "seamless",
            "album continuity",
        ],

        // ── Playback · Fading ────────────────────────────────────────────────
        "general.smooth_track_starts" => &["declick", "de-click", "onset ramp", "pop", "click"],
        "general.fade_on_pause" => &["pause fade", "resume fade", "soft pause", "click"],
        "general.fade_pause_ms" => &["pause fade length", "ramp"],
        "general.fade_on_stop" => &["stop fade", "soft stop", "ease out", "click"],
        "general.fade_stop_ms" => &["stop fade length", "ramp"],
        "general.fade_radio_transitions" => {
            &["radio fade", "station switch", "soft switch", "click"]
        },
        "general.fade_on_skip" => &[
            "skip fade",
            "next fade",
            "manual crossfade",
            "fade to next",
            "soft skip",
        ],
        "general.fade_skip_secs" => &["skip fade length", "overlap", "blend"],
        "general.skip_silence" => &[
            "silence",
            "silent intro",
            "silent outro",
            "trim",
            "dead air",
            "quiet",
        ],
        "general.crossfade_offset" => &["gap", "overlap", "spacing", "pause between tracks"],
        "general.crossfade_bar_snap" => &["bpm", "beat", "tempo", "bars", "beatmatch"],
        "general.bit_perfect" => &[
            "lossless",
            "audiophile",
            "hi-res",
            "exclusive mode",
            "passthrough",
            "hifi",
            "bit-exact",
            "no dsp",
        ],
        "general.rewind_on_previous" => &["restart track", "back button", "skip back"],

        // ── Playback · Volume Normalization ──────────────────────────────────
        "general.volume_normalization" => {
            &["loudness", "replaygain", "rg", "agc", "leveling", "gain"]
        },
        "general.normalization_level" => &["loudness target", "lufs", "gain"],
        "general.replay_gain_preamp_db" => &["rg", "preamp", "gain boost"],
        "general.replay_gain_fallback_db" => &["replaygain", "rg", "untagged", "default gain"],
        "general.replay_gain_fallback_to_agc" => &["replaygain", "rg", "automatic gain"],
        "general.replay_gain_prevent_clipping" => &["replaygain", "rg", "limiter", "distortion"],

        // ── Playback · Scrobbling / Rating ───────────────────────────────────
        "general.scrobbling_enabled" => &["last.fm", "lastfm", "listenbrainz", "play history"],
        "general.scrobble_threshold" => &["last.fm", "lastfm", "play count", "submit point"],
        "general.rating_reminder_enabled" => &["stars", "rate prompt"],
        "general.rating_change_notification_enabled" => &[
            "stars",
            "rating popup",
            "rate confirmation",
            "hotkey",
            "notify",
        ],
        "general.love_change_notification_enabled" => {
            &["heart", "favorite", "loved", "star", "hotkey", "notify"]
        },
        "general.rating_reminder_trigger" => &["stars", "when to remind", "rate prompt"],
        "general.rating_reminder_percent" => &["stars", "how far", "rate prompt", "progress"],

        // ── Playback · Radio Scrobbling ──────────────────────────────────────
        // The section's labels say "Radio", never the service names a user
        // types to find it.
        "general.radio_scrobbling_enabled" => &[
            "last.fm",
            "lastfm",
            "listenbrainz",
            "stream history",
            "internet radio",
        ],
        "general.radio_scrobble_threshold_secs" => {
            &["last.fm", "lastfm", "listenbrainz", "submit point", "how long"]
        },
        "general.radio_now_playing_enabled" => {
            &["last.fm", "lastfm", "listenbrainz", "now playing", "status"]
        },
        "__set_listenbrainz_token" => &["api key", "credentials", "user token", "scrobble"],
        "__verify_listenbrainz" => &["test", "check token", "validate", "scrobble"],
        "__set_lastfm_credentials" => &["api key", "secret", "credentials", "scrobble"],
        "__connect_lastfm" => &["authorize", "sign in", "link account", "scrobble"],
        "__disconnect_lastfm" => &["sign out", "unlink", "revoke", "scrobble"],
        "general.quick_add_to_playlist" => &["skip dialog", "one click add", "fast add"],
        "general.default_playlist_name" => {
            &["preferred playlist", "target playlist", "favorite list"]
        },
        "general.queue_show_default_playlist" => &["badge", "pill", "header chip"],

        // ── Theme · Mode / Display / Colors ──────────────────────────────────
        "general.light_mode" => &["dark mode", "light mode", "appearance", "color scheme"],
        "general.rounded_mode" => &["border radius", "square corners", "shape"],
        "general.opacity_gradient" => &["fade", "dim", "transparency"],
        // Theme COLORS moved out of the GUI into the theme TOML files, so the
        // picker is now the only row a colour search can land on. It inherits
        // the vocabulary the deleted `accent.primary` / `border` rows carried.
        "__theme_picker" => &[
            "colors",
            "palette",
            "accent",
            "highlight",
            "background",
            "border",
            "outline",
            "appearance",
            "skin",
        ],

        // ── Visualizer · Frame / Signal / Bars / Lines / Scope ───────────────
        "__restore_visualizer" => &["reset", "defaults", "revert", "factory"],
        "visualizer.height_percent" => &["size"],
        "visualizer.noise_reduction" => &["smoothing", "denoise"],
        "visualizer.lower_cutoff_freq" => &["bass", "low frequency", "highpass"],
        "visualizer.higher_cutoff_freq" => &["treble", "high frequency", "lowpass"],
        "visualizer.opacity" => &["transparency", "alpha", "fade"],
        "visualizer.bloom" => &["glow", "halo", "neon", "emissive", "shine"],
        "visualizer.bloom_intensity" => &["glow strength", "halo", "neon", "bloom amount"],
        "visualizer.beat_reactivity" => &["pump", "beat", "bass drop", "punch", "kick", "pulse"],
        "visualizer.bars.trails" | "visualizer.lines.trails" | "visualizer.scope.trails" => &[
            "motion blur",
            "persistence",
            "echo",
            "ghost",
            "comet",
            "afterimage",
        ],
        "visualizer.bars.echo" | "visualizer.lines.echo" | "visualizer.scope.echo" => &[
            "milkdrop",
            "feedback",
            "spiral",
            "tunnel",
            "psychedelic",
            "warp",
        ],
        "visualizer.bars.placement" | "visualizer.lines.placement" => &[
            "position",
            "location",
            "where",
            "bottom band",
            "player bar",
            "over cover",
            "cover art",
            "album art",
        ],
        "visualizer.crt" => &[
            "retro",
            "scanlines",
            "vignette",
            "grain",
            "vhs",
            "chromatic aberration",
            "film",
        ],
        "visualizer.auto_sensitivity" => &["agc", "auto gain", "normalize", "auto scale"],
        "visualizer.waves" => &["spline", "rolling hills", "catmull-rom"],
        "visualizer.waves_smoothing" => &["spline", "rolling hills", "wave amount", "catmull-rom"],
        "visualizer.monstercat" => &["spread", "falloff", "blur", "cava"],
        "visualizer.bars.led_bars" => &["vu meter", "segments", "blocks"],
        "visualizer.bars.led_segment_height" => &["block", "segment size", "vu"],
        "visualizer.bars.max_bars" => &["bands", "resolution", "number of bars"],
        "visualizer.bars.bar_spacing" => &["gap", "padding", "separation"],
        "visualizer.bars.bar_width_min" => &["thin bars", "narrow", "bar size"],
        "visualizer.bars.bar_width_max" => &["thick bars", "wide", "bar size"],
        "visualizer.bars.border_width" => &["outline", "stroke", "edge"],
        "visualizer.bars.peak_fade_time" => &["cap", "tip", "fade out", "decay"],
        "visualizer.bars.gradient_mode" => &["color mode", "shimmer", "energy", "coloring"],
        "visualizer.bars.gradient_orientation" => &["direction", "axis", "horizontal", "vertical"],
        "visualizer.bars.peak_mode" => &["cap", "tip", "falling peaks"],
        "visualizer.bars.peak_gradient_mode" => &["peak color", "cap color", "tip color"],
        "visualizer.bars.peak_hold_time" => &["cap", "tip", "dwell", "linger"],
        "visualizer.bars.peak_fall_speed" => &["cap", "tip", "drop rate", "gravity"],
        "visualizer.bars.peak_height_ratio" => &["cap", "tip", "marker size"],
        "visualizer.bars.bar_depth_3d" => &["3d", "perspective"],
        "visualizer.bars.flash_intensity" => {
            &["beat flash", "bloom", "pulse", "peak flash", "punch"]
        },
        "visualizer.bar_gradient_colors" => {
            &["palette", "color stops", "rainbow", "spectrum colors"]
        },
        // Bar Colors sections — one arm each serves the Dark and Light rows
        // (`normalize` strips the palette prefix).
        "visualizer.border_color" => &["outline", "stroke", "edge color"],
        "visualizer.border_opacity" => &["outline", "stroke", "alpha", "transparency"],
        "visualizer.led_border_opacity" => {
            &["vu", "segment", "outline", "alpha", "transparency"]
        },
        "visualizer.peak_gradient_colors" => &["cap", "tip", "palette", "color stops"],
        "visualizer.lines.point_count" => &["resolution", "detail", "samples", "vertices"],
        "visualizer.lines.line_thickness" => &["stroke", "weight", "width"],
        "visualizer.lines.outline_thickness" => &["stroke", "border", "edge"],
        "visualizer.lines.outline_opacity" => &["stroke", "border", "alpha", "transparency"],
        "visualizer.lines.animation_speed" => &["breathing", "motion", "tempo", "rate"],
        "visualizer.lines.gradient_mode" => &["color mode", "breathing", "rainbow", "coloring"],
        "visualizer.lines.fill_opacity" => &["area fill", "shade under", "filled curve"],
        "visualizer.lines.glow_intensity" => &["neon", "halo", "bloom", "glow", "luminous"],
        "visualizer.lines.style" => &["smooth", "angular", "curve", "interpolation"],
        "visualizer.lines.mirror" => &["symmetric", "oscilloscope", "reflect"],
        "visualizer.lines.boat" => &["surf", "rider", "easter egg"],
        "visualizer.scope.radius" => &["oscilloscope", "circular", "ring", "ring size", "diameter"],
        "visualizer.scope.sensitivity" => &["oscilloscope", "gain", "waveform swing", "amplitude"],
        "visualizer.scope.point_count" => &["resolution", "detail", "samples", "vertices"],
        "visualizer.scope.particles" => &["sparks", "dust", "embers", "particle field", "stars"],
        "visualizer.scope.particle_count" => &["sparks", "dust", "density", "number of particles"],
        "visualizer.scope.particle_speed" => &["sparks", "dust", "drift", "flow speed"],
        "visualizer.scope.glow_intensity" => &["neon", "halo", "bloom", "glow", "luminous"],
        "visualizer.scope.beam" => &["beam", "neon", "glow", "halo", "trace glow"],
        "visualizer.scope.style" => &["smooth", "angular", "curve", "interpolation"],
        // "Scope" is shorter than "oscilloscope", so the needle can never match
        // the label or section header — every Scope row has to carry the word.
        "visualizer.scope.line_thickness" => &["oscilloscope", "stroke", "weight", "width"],
        "visualizer.scope.fill_opacity" => &["oscilloscope", "area fill", "shade under", "alpha"],
        "visualizer.scope.outline_thickness" => {
            &["oscilloscope", "stroke", "border", "edge"]
        },
        "visualizer.scope.outline_opacity" => {
            &["oscilloscope", "stroke", "border", "alpha", "transparency"]
        },
        "visualizer.scope.gradient_mode" => {
            &["oscilloscope", "color mode", "rainbow", "coloring"]
        },
        "visualizer.scope.animation_speed" => &["oscilloscope", "motion", "tempo", "rate"],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_key_returns_aliases() {
        assert!(keywords_for("general.volume_normalization").contains(&"loudness"));
        assert!(keywords_for("general.scrobbling_enabled").contains(&"last.fm"));
    }

    #[test]
    fn unknown_key_is_empty() {
        assert!(keywords_for("general.does_not_exist").is_empty());
        assert!(keywords_for("").is_empty());
    }

    #[test]
    fn palette_prefix_is_normalized() {
        // Both theme palettes resolve to the same alias list — the Bar Colors
        // sections render one row per palette off a single entry here.
        assert_eq!(
            keywords_for("dark.visualizer.border_color"),
            keywords_for("light.visualizer.border_color")
        );
        assert!(keywords_for("dark.visualizer.border_color").contains(&"outline"));
        assert!(keywords_for("light.visualizer.peak_gradient_colors").contains(&"cap"));
    }

    #[test]
    fn light_mode_key_is_not_mis_stripped() {
        // `general.light_mode` must NOT be treated as a `light.` palette key.
        assert!(keywords_for("general.light_mode").contains(&"dark mode"));
    }

    #[test]
    fn visualizer_keys_resolve_without_palette_prefix() {
        assert!(keywords_for("visualizer.lower_cutoff_freq").contains(&"bass"));
        assert!(keywords_for("visualizer.higher_cutoff_freq").contains(&"treble"));
    }

    #[test]
    fn bit_perfect_key_resolves() {
        // The "Bit-Perfect Output" row's label/subtitle never say "lossless" or
        // "exclusive mode" — the audiophile vocabulary a searcher actually types.
        let kw = keywords_for("general.bit_perfect");
        assert!(kw.contains(&"lossless"));
        assert!(kw.contains(&"exclusive mode"));
    }

    #[test]
    fn second_batch_keys_resolve() {
        // Account rows + the logout sentinel pseudo-key.
        assert!(keywords_for("general.server_url").contains(&"navidrome"));
        assert!(keywords_for("__action_logout").contains(&"sign off"));
        // Per-palette visualizer color row, palette-normalized.
        assert!(keywords_for("dark.visualizer.bar_gradient_colors").contains(&"palette"));
        assert!(keywords_for("visualizer.bars.peak_mode").contains(&"cap"));
    }
}
