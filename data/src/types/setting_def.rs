//! Settings macro foundation — `define_settings!` plus its supporting types.
//!
//! Each settings tab declares its keys via [`define_settings!`] in
//! `data/src/services/settings_tables/<tab>.rs`. The macro emits four
//! artifacts per tab:
//!
//! - `pub const TAB_<TAB>_SETTINGS: &[SettingDef]` — table of declared keys
//!   for future items-builder migration.
//! - `pub fn dispatch_<tab>_tab_setting(key, value, mgr) -> Option<Result<SettingsSideEffect>>`
//!   — sync persistence dispatcher. Returns `None` for keys not declared in
//!   this tab; `Some(Ok(SettingsSideEffect::None))` on a setter-only success;
//!   `Some(Ok(SettingsSideEffect::<variant>(...)))` when the setting also
//!   declared an `on_dispatch:` hook (e.g. emit a toast, kick a follow-up
//!   load); `Some(Err(_))` on type mismatch or setter failure. Caller chains
//!   all three tab dispatchers; a `None` from every tab means the key is
//!   still owned by the legacy hand-written `match key.as_str()` arm in the
//!   UI crate.
//! - `pub fn apply_toml_<tab>_tab(ts, p)` — runs the per-setting
//!   `toml_apply` closures. Called from `apply_toml_settings_to_internal`.
//! - `pub fn dump_<tab>_tab_player_settings(src, out)` — runs the per-setting
//!   `read` closures, copying the redb-backed internal `PlayerSettings` into
//!   the UI-facing `PlayerSettings` consumed by `Message::PlayerSettingsLoaded`.
//!   Called from `SettingsManager::get_player_settings`.
//!
//! The dispatcher takes `&mut SettingsManager` (sync). The UI handler in
//! `update/settings.rs` locks the manager mutex inside an async task before
//! calling the dispatcher chain, mirroring the pattern of the existing
//! `shell.settings().set_X(v).await` calls. After the dispatcher returns,
//! the UI handler maps the `SettingsSideEffect` to a follow-up
//! `Task<Message>` (toast push, `LoadArtists`, light-mode atomic + Tick,
//! verbose-config writer chain) so the data crate never imports `iced`.

/// Which settings tab a key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Interface,
    Playback,
}

/// Static metadata about a single declared setting. The macro emits one
/// `SettingDef` per declared entry into a per-tab `&[SettingDef]` constant.
#[derive(Debug, Clone, Copy)]
pub struct SettingDef {
    /// Dotted TOML key path (e.g. `"general.stable_viewport"`).
    pub key: &'static str,
    /// Tab this setting belongs to.
    pub tab: Tab,
}

/// Declarative table of settings for a single tab.
///
/// See the module-level docs for the artifacts emitted. Adding a setting is a
/// single declarative entry; the compiler enforces that `setter`, `toml_apply`,
/// and `read` are all declared (no silent omission). `on_dispatch:` is
/// optional — entries that omit it default to
/// [`SettingsSideEffect::None`][crate::types::settings_side_effect::SettingsSideEffect::None].
///
/// `ui_meta:` is also optional. When supplied, the macro additionally emits
/// `$items_fn(data: &$data_type) -> Vec<SettingsEntry>` containing a row per
/// entry that has a `ui_meta:` cluster. Entries without `ui_meta:` (e.g.
/// `general.light_mode`, the queue-column-visibility booleans, the four
/// ToggleSet sub-keys) participate in dispatch/apply/dump as before but emit
/// no UI row — the items file in the UI crate stitches their UI elsewhere
/// (a `toggle_set` parent, a separate tab, the queue header, …).
///
/// # Example
///
/// ```ignore
/// nokkvi_data::define_settings! {
///     tab: nokkvi_data::types::setting_def::Tab::General,
///     data_type: nokkvi_data::types::settings_data::GeneralSettingsData,
///     items_fn: build_general_tab_settings_items,
///     settings_const: TAB_GENERAL_SETTINGS,
///     contains_fn: tab_general_contains,
///     dispatch_fn: dispatch_general_tab_setting,
///     apply_fn: apply_toml_general_tab,
///     dump_fn: dump_general_tab_player_settings,
///     settings: [
///         StableViewport {
///             key: "general.stable_viewport",
///             value_type: Bool,
///             setter: |mgr, v: bool| mgr.set_stable_viewport(v),
///             toml_apply: |ts, p| p.stable_viewport = ts.stable_viewport,
///             read: |src, out| out.stable_viewport = src.stable_viewport,
///             ui_meta: {
///                 label: "Stable Viewport",
///                 category: "Mouse Behavior",
///                 subtitle: Some("Click highlights in-place without scrolling"),
///                 default: true,
///                 read_field: |d| d.stable_viewport,
///             },
///         },
///         LightMode {
///             // No ui_meta — light_mode renders on the Theme tab, not General.
///             key: "general.light_mode",
///             value_type: Enum,
///             setter: |_mgr, _v: String| Ok(()),
///             toml_apply: |ts, p| p.light_mode = ts.light_mode,
///             read: |_src, _out| {},
///             on_dispatch: |v: String| SettingsSideEffect::SetLightModeAtomic(v == "Light"),
///         },
///     ]
/// }
/// ```
///
/// `value_type` selects how `SettingValue` is unpacked before being handed to
/// the setter. Supported variants today: `Bool`, `Int`, `Float`, `Enum`,
/// `Text`. The setter receives the inner payload typed as the closure
/// signature requests; type-mismatch at runtime returns
/// `Some(Err(anyhow::Error))`.
///
/// `read` carries the per-field cast/clone semantics needed to land the
/// redb-stored internal `PlayerSettings` value onto the UI-facing struct
/// (e.g. `out.scrobble_threshold = src.scrobble_threshold as f32` or
/// `out.start_view = src.start_view.clone()`).
///
/// `on_dispatch` (when supplied) receives the same unpacked payload — for
/// `Text` and `Enum` keys the macro hands it a clone so the setter can still
/// move the original — and returns a
/// [`SettingsSideEffect`][crate::types::settings_side_effect::SettingsSideEffect]
/// the UI handler runs after `handle_player_settings_loaded`.
///
/// `ui_meta:` carries the UI-row payload used by `$items_fn`. Field shape:
///
/// - `label: &'static str` — human-readable row label.
/// - `category: &'static str` — section header label (matches the surrounding
///   `SettingsEntry::Header.label` in the UI crate's items builder).
/// - `subtitle: Option<&'static str>` — description text shown in the footer.
/// - `default: <typed>` — default value the row resets to.
/// - `options: &[&'static str]` — required for `Enum` entries, a list of
///   selectable labels.
/// - `min`/`max`/`step`/`unit:` — required for `Int`/`Float` entries.
/// - `read_field: |d| <expr>` — closure body that reads the current value
///   from the per-tab `data_type`. The macro evaluates this expression with
///   `d` bound to the data borrow and passes the result to the right
///   `SettingItem::*` constructor based on `value_type`.
#[macro_export]
macro_rules! define_settings {
    (
        tab: $tab:expr,
        data_type: $data_type:ty,
        items_fn: $items_fn:ident,
        settings_const: $settings_const:ident,
        contains_fn: $contains_fn:ident,
        dispatch_fn: $dispatch_fn:ident,
        apply_fn: $apply_fn:ident,
        dump_fn: $dump_fn:ident,
        settings: [
            $(
                $variant:ident {
                    key: $key:literal,
                    value_type: $vtype:ident,
                    setter: |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr,
                    toml_apply: |$ats:ident, $ap:ident| $abody:expr,
                    read: |$rsrc:ident, $rout:ident| $rbody:expr
                    $(, on_dispatch: |$dval:ident : $dty:ty| $dbody:expr)?
                    $(, ui_meta: {
                        label: $label:literal,
                        category: $cat:literal,
                        subtitle: $sub:expr,
                        default: $default:expr
                        $(, options: $options:expr)?
                        $(, min: $min:expr, max: $max:expr, step: $step:expr, unit: $unit:literal)?
                        , read_field: |$rfd:ident| $rfbody:expr
                        $(,)?
                    })?
                    $(,)?
                }
            ),* $(,)?
        ] $(,)?
    ) => {
        #[allow(dead_code)]
        pub const $settings_const: &[$crate::types::setting_def::SettingDef] = &[
            $(
                $crate::types::setting_def::SettingDef {
                    key: $key,
                    tab: $tab,
                },
            )*
        ];

        pub fn $contains_fn(_key: &str) -> bool {
            const KEYS: &[&str] = &[$( $key, )*];
            KEYS.contains(&_key)
        }

        // Allows: empty-tab variants (no declared settings) leave `key`,
        // `value`, and `mgr` unbound — same goes for `ts` / `p` in
        // `$apply_fn` and `src` / `out` in `$dump_fn`. Once the per-tab
        // follow-ups land entries here, every binding is consumed by the
        // generated arms.
        #[allow(unused_variables)]
        pub fn $dispatch_fn(
            key: &str,
            value: $crate::types::setting_value::SettingValue,
            mgr: &mut $crate::services::settings::SettingsManager,
        ) -> ::core::option::Option<
            ::anyhow::Result<$crate::types::settings_side_effect::SettingsSideEffect>,
        > {
            $(
                if key == $key {
                    return ::core::option::Option::Some(
                        $crate::define_settings_dispatch_arm!(
                            value, $vtype,
                            |$smgr, $sval: $sty| $sbody,
                            mgr, $key
                            $(, on_dispatch: |$dval: $dty| $dbody)?
                        )
                    );
                }
            )*
            ::core::option::Option::None
        }

        #[allow(unused_variables)]
        pub fn $apply_fn(
            ts: &$crate::types::toml_settings::TomlSettings,
            p: &mut $crate::types::settings::PlayerSettings,
        ) {
            $(
                {
                    let $ats: &$crate::types::toml_settings::TomlSettings = ts;
                    let $ap: &mut $crate::types::settings::PlayerSettings = p;
                    $abody;
                }
            )*
        }

        #[allow(unused_variables)]
        pub fn $dump_fn(
            src: &$crate::types::settings::PlayerSettings,
            out: &mut $crate::types::player_settings::PlayerSettings,
        ) {
            $(
                {
                    let $rsrc: &$crate::types::settings::PlayerSettings = src;
                    let $rout: &mut $crate::types::player_settings::PlayerSettings = out;
                    $rbody;
                }
            )*
        }

        /// Macro-emitted flat-row builder. Returns one `SettingsEntry::Item`
        /// per entry that declared `ui_meta:`. Section headers, conditional
        /// rows, ToggleSet rows, and dialog sentinel rows stay hand-written
        /// in the UI crate's `items_<tab>.rs` builder, which interleaves them
        /// with the rows returned here.
        #[allow(dead_code, unused_variables, unused_mut)]
        pub fn $items_fn(
            data: &$data_type,
        ) -> ::std::vec::Vec<$crate::types::setting_item::SettingsEntry> {
            let mut out: ::std::vec::Vec<$crate::types::setting_item::SettingsEntry> =
                ::std::vec::Vec::new();
            $(
                $(
                    {
                        let $rfd: &$data_type = data;
                        let __value = $rfbody;
                        out.push($crate::define_settings_build_item_arm!(
                            $vtype,
                            $key, $label, $cat, $sub,
                            __value, $default
                            $(, options: $options)?
                            $(, min: $min, max: $max, step: $step, unit: $unit)?
                        ));
                    }
                )?
            )*
            out
        }
    };
}

/// Internal helper used by [`define_settings!`] to extract the typed inner
/// payload from a [`SettingValue`] variant before invoking the setter
/// closure. Type mismatches yield `Err(anyhow::Error)`.
///
/// When the entry declares an `on_dispatch:` hook, the unpacked payload is
/// also bound to the hook's parameter (cloned for `Text`/`Enum` so the
/// setter still owns the original) and the hook expression supplies the
/// returned [`SettingsSideEffect`][crate::types::settings_side_effect::SettingsSideEffect].
/// Otherwise the arm returns `SettingsSideEffect::None`.
#[macro_export]
#[doc(hidden)]
macro_rules! define_settings_dispatch_arm {
    ($value:ident, Bool,
     |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr,
     $mgr:ident, $key:expr
     $(, on_dispatch: |$dval:ident : $dty:ty| $dbody:expr)?
    ) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Bool(v) => {
                $( let $dval: $dty = v; )?
                let $smgr = $mgr;
                let $sval: $sty = v;
                let setter_result: ::anyhow::Result<()> = $sbody;
                setter_result.map(|()| {
                    #[allow(unused_mut, unused_assignments)]
                    let mut __effect =
                        $crate::types::settings_side_effect::SettingsSideEffect::None;
                    $( __effect = $dbody; )?
                    __effect
                })
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Bool, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Int,
     |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr,
     $mgr:ident, $key:expr
     $(, on_dispatch: |$dval:ident : $dty:ty| $dbody:expr)?
    ) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Int { val, .. } => {
                $( let $dval: $dty = val; )?
                let $smgr = $mgr;
                let $sval: $sty = val;
                let setter_result: ::anyhow::Result<()> = $sbody;
                setter_result.map(|()| {
                    #[allow(unused_mut, unused_assignments)]
                    let mut __effect =
                        $crate::types::settings_side_effect::SettingsSideEffect::None;
                    $( __effect = $dbody; )?
                    __effect
                })
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Int, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Float,
     |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr,
     $mgr:ident, $key:expr
     $(, on_dispatch: |$dval:ident : $dty:ty| $dbody:expr)?
    ) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Float { val, .. } => {
                $( let $dval: $dty = val; )?
                let $smgr = $mgr;
                let $sval: $sty = val;
                let setter_result: ::anyhow::Result<()> = $sbody;
                setter_result.map(|()| {
                    #[allow(unused_mut, unused_assignments)]
                    let mut __effect =
                        $crate::types::settings_side_effect::SettingsSideEffect::None;
                    $( __effect = $dbody; )?
                    __effect
                })
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Float, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Enum,
     |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr,
     $mgr:ident, $key:expr
     $(, on_dispatch: |$dval:ident : $dty:ty| $dbody:expr)?
    ) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Enum { val, .. } => {
                $( let $dval: $dty = val.clone(); )?
                let $smgr = $mgr;
                let $sval: $sty = val;
                let setter_result: ::anyhow::Result<()> = $sbody;
                setter_result.map(|()| {
                    #[allow(unused_mut, unused_assignments)]
                    let mut __effect =
                        $crate::types::settings_side_effect::SettingsSideEffect::None;
                    $( __effect = $dbody; )?
                    __effect
                })
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Enum, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Text,
     |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr,
     $mgr:ident, $key:expr
     $(, on_dispatch: |$dval:ident : $dty:ty| $dbody:expr)?
    ) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Text(s) => {
                $( let $dval: $dty = s.clone(); )?
                let $smgr = $mgr;
                let $sval: $sty = s;
                let setter_result: ::anyhow::Result<()> = $sbody;
                setter_result.map(|()| {
                    #[allow(unused_mut, unused_assignments)]
                    let mut __effect =
                        $crate::types::settings_side_effect::SettingsSideEffect::None;
                    $( __effect = $dbody; )?
                    __effect
                })
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Text, got {:?}",
                $key,
                other
            )),
        }
    }};
}

/// Internal helper used by [`define_settings!`] to build one
/// `SettingsEntry::Item` from a `ui_meta:` cluster. Dispatches on the
/// `value_type` ident so the right `SettingItem::*` constructor receives the
/// right typed payload.
///
/// Each arm constructs a `SettingMeta` literal (parallel to the UI-crate
/// `SettingMeta::new(...).with_subtitle(...)` builder used by the
/// hand-written `items_<tab>.rs` files) and forwards `(val, default, [knobs])`
/// to the matching constructor. `Enum` arms additionally clone the `options`
/// slice into a `Vec` since `enum_val` takes ownership of the option list.
#[macro_export]
#[doc(hidden)]
macro_rules! define_settings_build_item_arm {
    (Bool, $key:expr, $label:expr, $cat:expr, $sub:expr, $val:expr, $default:expr) => {
        $crate::types::setting_item::SettingItem::bool_val(
            $crate::types::setting_item::SettingMeta {
                key: ::std::borrow::Cow::Borrowed($key),
                label: $label,
                category: $cat,
                subtitle: ($sub).map(::std::borrow::Cow::Borrowed),
            },
            $val,
            $default,
        )
    };
    (Int, $key:expr, $label:expr, $cat:expr, $sub:expr, $val:expr, $default:expr,
     min: $min:expr, max: $max:expr, step: $step:expr, unit: $unit:literal) => {
        $crate::types::setting_item::SettingItem::int(
            $crate::types::setting_item::SettingMeta {
                key: ::std::borrow::Cow::Borrowed($key),
                label: $label,
                category: $cat,
                subtitle: ($sub).map(::std::borrow::Cow::Borrowed),
            },
            $val,
            $default,
            $min,
            $max,
            $step,
            $unit,
        )
    };
    (Float, $key:expr, $label:expr, $cat:expr, $sub:expr, $val:expr, $default:expr,
     min: $min:expr, max: $max:expr, step: $step:expr, unit: $unit:literal) => {
        $crate::types::setting_item::SettingItem::float(
            $crate::types::setting_item::SettingMeta {
                key: ::std::borrow::Cow::Borrowed($key),
                label: $label,
                category: $cat,
                subtitle: ($sub).map(::std::borrow::Cow::Borrowed),
            },
            $val,
            $default,
            $min,
            $max,
            $step,
            $unit,
        )
    };
    (Enum, $key:expr, $label:expr, $cat:expr, $sub:expr, $val:expr, $default:expr,
     options: $options:expr) => {
        $crate::types::setting_item::SettingItem::enum_val(
            $crate::types::setting_item::SettingMeta {
                key: ::std::borrow::Cow::Borrowed($key),
                label: $label,
                category: $cat,
                subtitle: ($sub).map(::std::borrow::Cow::Borrowed),
            },
            $val,
            $default,
            ($options).to_vec(),
        )
    };
    (Text, $key:expr, $label:expr, $cat:expr, $sub:expr, $val:expr, $default:expr) => {
        $crate::types::setting_item::SettingItem::text(
            $crate::types::setting_item::SettingMeta {
                key: ::std::borrow::Cow::Borrowed($key),
                label: $label,
                category: $cat,
                subtitle: ($sub).map(::std::borrow::Cow::Borrowed),
            },
            $val,
            $default,
        )
    };
}

/// Declarative table of view-column-visibility booleans for one slot-list view
/// (Albums / Artists / Songs / Genres / Playlists / Similar / Queue). Emits
/// three free functions per invocation:
///
/// - `$apply_fn(ts, p)` — copy `ts.field → p.field` for each declared field,
///   moving the column toggle from TOML onto the redb-backed internal
///   `PlayerSettings`.
/// - `$dump_fn(src, out)` — copy `src.field → out.field` for each declared
///   field, moving the column toggle from the redb-backed internal
///   `PlayerSettings` onto the UI-facing `PlayerSettings` consumed by
///   `Message::PlayerSettingsLoaded`.
/// - `$write_fn(ps, ts)` — copy `ps.field → ts.field` for each declared field,
///   moving the column toggle from the UI-facing `PlayerSettings` onto the
///   TOML representation written to `config.toml`.
///
/// Companion to `define_view_columns!` (UI crate, `src/views/mod.rs`): the
/// UI macro owns the column enum + visibility struct + `ColumnPersist` impl
/// (UI types), this macro owns the TOML wire copies (data types). The two
/// invocations share the same column-set per view; a parity test in this
/// crate's test module asserts the column counts match so a column added on
/// one side without the other surfaces as a test failure.
///
/// All declared fields must be `bool` on `TomlSettings`, the redb-backed
/// internal `PlayerSettings`, and the UI-facing `PlayerSettings` — i.e. the
/// per-view-column toggles. The macro relies on identical field names across
/// all three types, which is the case today for every `<view>_show_<column>`
/// field.
///
/// # Example
///
/// ```ignore
/// nokkvi_data::define_view_column_toml_helpers! {
///     view: Albums,
///     apply_fn: apply_toml_albums_columns,
///     dump_fn: dump_albums_columns_to_player,
///     write_fn: write_albums_columns_to_toml,
///     fields: [
///         albums_show_select,
///         albums_show_index,
///         albums_show_thumbnail,
///         albums_show_stars,
///         albums_show_songcount,
///         albums_show_plays,
///         albums_show_love,
///     ],
/// }
/// ```
#[macro_export]
macro_rules! define_view_column_toml_helpers {
    (
        view: $view:ident,
        apply_fn: $apply_fn:ident,
        dump_fn: $dump_fn:ident,
        write_fn: $write_fn:ident,
        fields: [ $( $field:ident ),* $(,)? ] $(,)?
    ) => {
        /// Macro-emitted: copy declared view-column-visibility fields from
        /// `TomlSettings` onto the redb-backed internal `PlayerSettings`.
        #[allow(dead_code)]
        pub fn $apply_fn(
            ts: &$crate::types::toml_settings::TomlSettings,
            p: &mut $crate::types::settings::PlayerSettings,
        ) {
            $(
                p.$field = ts.$field;
            )*
        }

        /// Macro-emitted: copy declared view-column-visibility fields from
        /// the redb-backed internal `PlayerSettings` onto the UI-facing
        /// `PlayerSettings`.
        #[allow(dead_code)]
        pub fn $dump_fn(
            src: &$crate::types::settings::PlayerSettings,
            out: &mut $crate::types::player_settings::PlayerSettings,
        ) {
            $(
                out.$field = src.$field;
            )*
        }

        /// Macro-emitted: copy declared view-column-visibility fields from
        /// the UI-facing `PlayerSettings` onto `TomlSettings` (for writing
        /// back to `config.toml`).
        #[allow(dead_code)]
        pub fn $write_fn(
            ps: &$crate::types::player_settings::PlayerSettings,
            ts: &mut $crate::types::toml_settings::TomlSettings,
        ) {
            $(
                ts.$field = ps.$field;
            )*
        }
    };
}
