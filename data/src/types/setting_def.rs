//! Settings macro foundation — `define_settings!` plus its supporting types.
//!
//! Each settings tab declares its keys via [`define_settings!`] in
//! `data/src/services/settings_tables/<tab>.rs`. The macro emits five
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
//!   UI crate. The manager type is parametric — call sites pass `mgr_type:`
//!   explicitly so the macro doesn't hardcode `SettingsManager`.
//! - `pub fn apply_toml_<tab>_tab(ts, p)` — runs the per-setting
//!   `toml_apply` closures. Called from `apply_toml_settings_to_internal`.
//! - `pub fn dump_<tab>_tab_player_settings(src, out)` — runs the per-setting
//!   `read` closures, copying the redb-backed `PersistedPlayerSettings` into
//!   the UI-facing `LivePlayerSettings` consumed by
//!   `Message::PlayerSettingsLoaded`. Called from
//!   `SettingsManager::get_player_settings`.
//! - `pub fn write_<tab>_tab_toml(ps, ts)` — runs the per-setting `write`
//!   closures, copying the UI-facing `LivePlayerSettings` back onto
//!   `TomlSettings` for serialization to `config.toml`. Inverse of
//!   `apply_toml_<tab>_tab`. Called from `TomlSettings::from_player_settings`.
//!
//! The dispatcher takes `&mut <mgr_type>` (sync). The UI handler in
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
    /// Dormant until the M3 visualizer table lands — no
    /// `settings_tables/visualizer.rs` invocation references it yet.
    Visualizer,
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
/// `read`, and `write` are all declared (no silent omission). `on_dispatch:` is
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
///     mgr_type: nokkvi_data::services::settings::SettingsManager,
///     items_fn: build_general_tab_settings_items,
///     settings_const: TAB_GENERAL_SETTINGS,
///     contains_fn: tab_general_contains,
///     dispatch_fn: dispatch_general_tab_setting,
///     apply_fn: apply_toml_general_tab,
///     dump_fn: dump_general_tab_player_settings,
///     write_fn: write_general_tab_toml,
///     settings: [
///         StableViewport {
///             key: "general.stable_viewport",
///             value_type: Bool,
///             setter: |mgr, v: bool| mgr.set_stable_viewport(v),
///             toml_apply: |ts, p| p.stable_viewport = ts.stable_viewport,
///             read: |src, out| out.stable_viewport = src.stable_viewport,
///             write: |ps, ts| ts.stable_viewport = ps.stable_viewport,
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
///             // UI-PS has no light_mode field — write is a no-op. The
///             // on-disk truth is maintained separately by the
///             // `SetLightModeAtomic` side-effect handler in the UI crate
///             // via a targeted toml_edit write that does not go through
///             // `from_player_settings`.
///             read: |_src, _out| {},
///             write: |_ps, _ts| {},
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
/// redb-stored `PersistedPlayerSettings` value onto the UI-facing
/// `LivePlayerSettings` struct (e.g.
/// `out.scrobble_threshold = src.scrobble_threshold as f32` or
/// `out.start_view = src.start_view.clone()`).
///
/// `write` carries the per-field cast/clone semantics needed to land the
/// UI-facing `LivePlayerSettings` value onto `TomlSettings` for serialization
/// — the inverse of `read`. Field types are usually identical between
/// `LivePlayerSettings` and `TomlSettings` (both f32 for `scrobble_threshold`,
/// both `String` for `start_view`), so most `write` closures are a simple
/// assignment of the matching field name. Entries whose UI-facing struct
/// lacks the field (only `light_mode` today) declare a `|_ps, _ts| {}`
/// no-op.
///
/// `on_dispatch` (when supplied) receives the same unpacked payload — for
/// `Text` and `Enum` keys the macro hands it a clone so the setter can still
/// move the original — and returns a
/// [`SettingsSideEffect`][crate::types::settings_side_effect::SettingsSideEffect]
/// the UI handler runs after `handle_player_settings_loaded`.
///
/// An optional trailing `copy_only_const: <NAME>, copy_only: [ ... ]` section
/// (after `settings: [...]`) declares entries that participate ONLY in the
/// emitted `apply`/`dump`/`write` copy functions — no setter, no dispatch
/// arm, no containment hit, no UI row. Each copy-only entry declares `key`,
/// `toml_apply`, `read`, and `write` (same closure shapes as a full entry;
/// bodies may be non-scalar — array assigns, `Vec` clones). Their keys are
/// published in `pub const <NAME>: &[&str]` so structural sentinel tests can
/// assert a residual field is macro-owned, and are deliberately EXCLUDED
/// from the `SettingDef` table, `contains_fn`, `dispatch_fn`, and
/// `items_fn`.
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
        mgr_type: $mgr_type:ty,
        items_fn: $items_fn:ident,
        settings_const: $settings_const:ident,
        contains_fn: $contains_fn:ident,
        dispatch_fn: $dispatch_fn:ident,
        apply_fn: $apply_fn:ident,
        dump_fn: $dump_fn:ident,
        write_fn: $write_fn:ident,
        settings: [
            $(
                $variant:ident {
                    key: $key:literal,
                    value_type: $vtype:ident,
                    setter: |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr,
                    toml_apply: |$ats:ident, $ap:ident| $abody:expr,
                    read: |$rsrc:ident, $rout:ident| $rbody:expr,
                    write: |$wps:ident, $wts:ident| $wbody:expr
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
        ]
        $(,
            copy_only_const: $copy_only_const:ident,
            copy_only: [
                $(
                    $cvariant:ident {
                        key: $ckey:literal,
                        toml_apply: |$cats:ident, $cap:ident| $cabody:expr,
                        read: |$crsrc:ident, $crout:ident| $crbody:expr,
                        write: |$cwps:ident, $cwts:ident| $cwbody:expr
                        $(,)?
                    }
                ),* $(,)?
            ]
        )?
        $(,
            view_columns: {
                fields: [ $( $vc_field:ident ),* $(,)? ]
                $(, assert_exhaustive: $vc_cov_fn:ident)?
                $(,)?
            }
        )?
        $(,)?
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

        $(
            /// Macro-emitted registry of this tab's copy-only keys. These
            /// entries run apply/dump/write copy closures ONLY — they have no
            /// setter, no dispatch arm, no `ui_meta` row, and never appear in
            /// the `TAB_<TAB>_SETTINGS` table. The const exists so structural
            /// sentinel tests can assert a residual field is macro-owned
            /// without exposing it to dispatch.
            #[allow(dead_code)]
            pub const $copy_only_const: &[&str] = &[ $( $ckey, )* ];
        )?

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
            mgr: &mut $mgr_type,
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
            p: &mut $crate::types::settings::PersistedPlayerSettings,
        ) {
            $(
                {
                    let $ats: &$crate::types::toml_settings::TomlSettings = ts;
                    let $ap: &mut $crate::types::settings::PersistedPlayerSettings = p;
                    $abody;
                }
            )*
            $( $(
                {
                    let $cats: &$crate::types::toml_settings::TomlSettings = ts;
                    let $cap: &mut $crate::types::settings::PersistedPlayerSettings = p;
                    $cabody;
                }
            )* )?
            $( $(
                p.view_columns.$vc_field = ts.view_columns.$vc_field;
            )* )?
        }

        #[allow(unused_variables)]
        pub fn $dump_fn(
            src: &$crate::types::settings::PersistedPlayerSettings,
            out: &mut $crate::types::player_settings::LivePlayerSettings,
        ) {
            $(
                {
                    let $rsrc: &$crate::types::settings::PersistedPlayerSettings = src;
                    let $rout: &mut $crate::types::player_settings::LivePlayerSettings = out;
                    $rbody;
                }
            )*
            $( $(
                {
                    let $crsrc: &$crate::types::settings::PersistedPlayerSettings = src;
                    let $crout: &mut $crate::types::player_settings::LivePlayerSettings = out;
                    $crbody;
                }
            )* )?
            $( $(
                out.view_columns.$vc_field = src.view_columns.$vc_field;
            )* )?
        }

        /// Macro-emitted writer — copies the per-tab declared fields from the
        /// UI-facing `LivePlayerSettings` onto `TomlSettings` for
        /// serialization back to `config.toml`. Inverse of `$apply_fn`
        /// (TOML→`PersistedPlayerSettings`) and `$dump_fn`
        /// (`PersistedPlayerSettings`→`LivePlayerSettings`). Entries whose
        /// UI-facing struct does not carry the field (e.g. `light_mode`,
        /// which lives only on the redb-backed `PersistedPlayerSettings`)
        /// declare a no-op `write:` closure so the per-tab function still
        /// claims the key even though the wire copy is a no-op.
        #[allow(unused_variables)]
        pub fn $write_fn(
            ps: &$crate::types::player_settings::LivePlayerSettings,
            ts: &mut $crate::types::toml_settings::TomlSettings,
        ) {
            $(
                {
                    let $wps: &$crate::types::player_settings::LivePlayerSettings = ps;
                    let $wts: &mut $crate::types::toml_settings::TomlSettings = ts;
                    $wbody;
                }
            )*
            $( $(
                {
                    let $cwps: &$crate::types::player_settings::LivePlayerSettings = ps;
                    let $cwts: &mut $crate::types::toml_settings::TomlSettings = ts;
                    $cwbody;
                }
            )* )?
            $( $(
                ts.view_columns.$vc_field = ps.view_columns.$vc_field;
            )* )?
        }

        $(
            $crate::define_settings_view_columns_cov! {
                cov_fn: [ $( $vc_cov_fn )? ],
                fields: [ $( $vc_field ),* ]
            }
        )?

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

/// Internal helper used by [`define_settings!`] to emit the optional
/// compile-time coverage guard for the `view_columns:` clause. When an
/// `assert_exhaustive:` ident is supplied, emits a never-called function that
/// destructures [`ViewColumns`][crate::types::view_columns::ViewColumns] with
/// NO `..` rest pattern, so any field not declared in the `fields:` list
/// fails the build with E0027 (pattern does not mention field). The function
/// body IS the check.
#[macro_export]
#[doc(hidden)]
macro_rules! define_settings_view_columns_cov {
    ( cov_fn: [], fields: [ $( $field:ident ),* $(,)? ] ) => {};
    ( cov_fn: [ $cov_fn:ident ], fields: [ $( $field:ident ),* $(,)? ] ) => {
        #[allow(dead_code, clippy::needless_pass_by_value)]
        fn $cov_fn(v: $crate::types::view_columns::ViewColumns) {
            let $crate::types::view_columns::ViewColumns {
                $( $field: _, )*
            } = v;
        }
    };
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

#[cfg(test)]
mod tests {
    use super::Tab;

    /// `Tab::Visualizer` exists as a dormant M0 capability (consumed by the
    /// M3 visualizer table). It must be a distinct variant.
    #[test]
    fn tab_visualizer_variant_is_distinct() {
        assert_ne!(Tab::Visualizer, Tab::General);
    }

    /// Stand-in for `SettingsManager` to prove `define_settings!` is
    /// genuinely parametric on its `mgr_type:` argument. If the macro ever
    /// regresses to hardcoding `SettingsManager`, this module fails to
    /// compile (the macro would try to call `MockMgr::set_*` on a
    /// `SettingsManager` borrow).
    pub struct MockMgr {
        pub last_flag: bool,
    }

    impl MockMgr {
        pub fn set_flag(&mut self, v: bool) -> ::anyhow::Result<()> {
            self.last_flag = v;
            Ok(())
        }
    }

    crate::define_settings! {
        tab: crate::types::setting_def::Tab::General,
        data_type: crate::types::settings_data::GeneralSettingsData,
        mgr_type: MockMgr,
        items_fn: mock_items_fn,
        settings_const: MOCK_SETTINGS_TABLE,
        contains_fn: mock_contains,
        dispatch_fn: mock_dispatch,
        apply_fn: mock_apply_toml,
        dump_fn: mock_dump_ps,
        write_fn: mock_write_toml,
        settings: [
            MockFlag {
                key: "mock.flag",
                value_type: Bool,
                setter: |mgr, v: bool| mgr.set_flag(v),
                toml_apply: |_ts, _p| {},
                read: |_src, _out| {},
                write: |_ps, _ts| {},
            },
        ],
        copy_only_const: MOCK_COPY_ONLY_KEYS,
        copy_only: [
            // String body — the font_family shape.
            CopyOnlyFont {
                key: "mock.copy_font",
                toml_apply: |ts, p| p.font_family = ts.font_family.clone(),
                read: |src, out| out.font_family = src.font_family.clone(),
                write: |ps, ts| ts.font_family = ps.font_family.clone(),
            },
            // Copy-array body — the eq_gains [f32; 10] shape.
            CopyOnlyEq {
                key: "mock.copy_eq_gains",
                toml_apply: |ts, p| p.eq_gains = ts.eq_gains,
                read: |src, out| out.eq_gains = src.eq_gains,
                write: |ps, ts| ts.eq_gains = ps.eq_gains,
            },
            // Vec-clone body — the custom_eq_presets shape.
            CopyOnlyPresets {
                key: "mock.copy_presets",
                toml_apply: |ts, p| p.custom_eq_presets = ts.custom_eq_presets.clone(),
                read: |src, out| out.custom_eq_presets = src.custom_eq_presets.clone(),
                write: |ps, ts| ts.custom_eq_presets = ps.custom_eq_presets.clone(),
            },
        ]
    }

    // Second invocation proving the `view_columns:` clause in isolation —
    // empty settings list, two declared column fields, no assert_exhaustive
    // (the consolidated 50-field invocation in settings_tables/columns.rs
    // owns the exhaustive destructure).
    crate::define_settings! {
        tab: crate::types::setting_def::Tab::General,
        data_type: crate::types::settings_data::GeneralSettingsData,
        mgr_type: MockMgr,
        items_fn: mock_columns_items_fn,
        settings_const: MOCK_COLUMNS_TABLE,
        contains_fn: mock_columns_contains,
        dispatch_fn: mock_columns_dispatch,
        apply_fn: mock_columns_apply_toml,
        dump_fn: mock_columns_dump_ps,
        write_fn: mock_columns_write_toml,
        settings: [],
        view_columns: {
            fields: [queue_show_select, queue_show_stars],
        }
    }

    /// The `view_columns:` clause emits `view_columns.<field>` copy steps in
    /// all three emitted functions for exactly the declared fields.
    #[test]
    fn macro_view_columns_clause_copies_declared_fields() {
        // apply: TOML → Persisted.
        let mut ts = crate::types::toml_settings::TomlSettings::default();
        ts.view_columns.queue_show_select = true; // default false
        ts.view_columns.queue_show_stars = false; // default true
        ts.view_columns.queue_show_album = false; // default true — NOT declared
        let mut p = crate::types::settings::PersistedPlayerSettings::default();
        mock_columns_apply_toml(&ts, &mut p);
        assert!(p.view_columns.queue_show_select);
        assert!(!p.view_columns.queue_show_stars);
        assert!(
            p.view_columns.queue_show_album,
            "undeclared field must NOT be copied by the clause"
        );

        // dump: Persisted → Live.
        let mut out = crate::types::player_settings::LivePlayerSettings::default();
        mock_columns_dump_ps(&p, &mut out);
        assert!(out.view_columns.queue_show_select);
        assert!(!out.view_columns.queue_show_stars);

        // write: Live → TOML.
        let mut ts2 = crate::types::toml_settings::TomlSettings::default();
        mock_columns_write_toml(&out, &mut ts2);
        assert!(ts2.view_columns.queue_show_select);
        assert!(!ts2.view_columns.queue_show_stars);

        // view_columns fields are copy-steps only — never dispatchable and
        // never claimed by the containment helper.
        let mut mgr = MockMgr { last_flag: false };
        for key in ["queue_show_select", "queue_show_stars"] {
            assert!(!mock_columns_contains(key));
            assert!(
                mock_columns_dispatch(
                    key,
                    crate::types::setting_value::SettingValue::Bool(true),
                    &mut mgr,
                )
                .is_none()
            );
        }
    }

    /// The macro is parametric on the manager type: instantiated above with
    /// `mgr_type: MockMgr`, the generated `mock_dispatch` accepts
    /// `&mut MockMgr` and forwards the unpacked Bool to `MockMgr::set_flag`.
    #[test]
    fn define_settings_macro_accepts_alternate_mgr_type() {
        let mut mgr = MockMgr { last_flag: false };
        let result = mock_dispatch(
            "mock.flag",
            crate::types::setting_value::SettingValue::Bool(true),
            &mut mgr,
        );
        assert!(
            result.is_some(),
            "mock_dispatch must claim mock.flag (containment check)"
        );
        let effect = result.unwrap().expect("setter must succeed");
        assert!(
            matches!(
                effect,
                crate::types::settings_side_effect::SettingsSideEffect::None
            ),
            "setter-only entry must return SettingsSideEffect::None"
        );
        assert!(
            mgr.last_flag,
            "MockMgr::set_flag must have been invoked with v=true"
        );

        // Containment helper and miss path also verified for completeness.
        assert!(mock_contains("mock.flag"));
        assert!(!mock_contains("mock.unknown"));
        let miss = mock_dispatch(
            "mock.unknown",
            crate::types::setting_value::SettingValue::Bool(true),
            &mut mgr,
        );
        assert!(miss.is_none(), "unknown key must return None");
    }

    /// `copy_only` entries run their closure bodies in the emitted apply /
    /// dump / write functions — including non-scalar bodies (`[f32; 10]`
    /// plain assign, `Vec` clone) that `SettingValue` has no variant for.
    #[test]
    fn copy_only_runs_in_apply_dump_write() {
        let preset = crate::audio::eq::CustomEqPreset {
            name: "mock preset".to_string(),
            gains: [2.0; 10],
        };

        // toml_apply: TomlSettings → PersistedPlayerSettings.
        let mut ts = crate::types::toml_settings::TomlSettings::default();
        ts.font_family = "MockFont".to_string();
        ts.eq_gains = [1.5; 10];
        ts.custom_eq_presets = vec![preset.clone()];
        let mut p = crate::types::settings::PersistedPlayerSettings::default();
        mock_apply_toml(&ts, &mut p);
        assert_eq!(p.font_family, "MockFont");
        assert_eq!(p.eq_gains, [1.5; 10]);
        assert_eq!(p.custom_eq_presets.len(), 1);
        assert_eq!(p.custom_eq_presets[0].name, "mock preset");

        // read: PersistedPlayerSettings → LivePlayerSettings.
        let mut out = crate::types::player_settings::LivePlayerSettings::default();
        mock_dump_ps(&p, &mut out);
        assert_eq!(out.font_family, "MockFont");
        assert_eq!(out.eq_gains, [1.5; 10]);
        assert_eq!(out.custom_eq_presets.len(), 1);

        // write: LivePlayerSettings → TomlSettings.
        let mut ts2 = crate::types::toml_settings::TomlSettings::default();
        mock_write_toml(&out, &mut ts2);
        assert_eq!(ts2.font_family, "MockFont");
        assert_eq!(ts2.eq_gains, [1.5; 10]);
        assert_eq!(ts2.custom_eq_presets.len(), 1);
        assert_eq!(ts2.custom_eq_presets[0].gains, [2.0; 10]);
    }

    /// `copy_only` keys must be invisible to dispatch and the containment
    /// helper — they have no setter, so a dispatch hit would be a phantom.
    #[test]
    fn copy_only_excluded_from_dispatch_and_contains() {
        let mut mgr = MockMgr { last_flag: false };
        for key in ["mock.copy_font", "mock.copy_eq_gains", "mock.copy_presets"] {
            assert!(!mock_contains(key), "contains must reject copy-only {key}");
            let miss = mock_dispatch(
                key,
                crate::types::setting_value::SettingValue::Bool(true),
                &mut mgr,
            );
            assert!(miss.is_none(), "dispatch must not claim copy-only {key}");
        }
    }

    /// `copy_only` entries carry no `ui_meta`, so the items builder must not
    /// emit rows for them (no phantom settings rows in the GUI).
    #[test]
    fn copy_only_omitted_from_items() {
        let data = crate::types::settings_data::GeneralSettingsData::default();
        let items = mock_items_fn(&data);
        assert!(
            items.is_empty(),
            "no entry declares ui_meta, so items must stay empty even with copy_only present"
        );
    }

    /// The copy-only registry publishes exactly the declared keys, and none
    /// of them leak into the dispatchable settings table.
    #[test]
    fn copy_only_keys_appear_in_copy_only_registry() {
        assert_eq!(
            MOCK_COPY_ONLY_KEYS,
            &["mock.copy_font", "mock.copy_eq_gains", "mock.copy_presets"]
        );
        for key in MOCK_COPY_ONLY_KEYS {
            assert!(
                !MOCK_SETTINGS_TABLE.iter().any(|d| d.key == *key),
                "copy-only {key} must not appear in TAB settings table"
            );
        }
    }

    /// Smoke test for the other three macro-emitted functions on the same
    /// `MockMgr` invocation. Each has a no-op closure body (this entry
    /// declares `toml_apply`, `read`, and `write` as `|_, _| {}`), so the
    /// test only proves that the macro still emits the functions and that
    /// they accept the renamed `PersistedPlayerSettings` /
    /// `LivePlayerSettings` types in their signatures.
    #[test]
    fn define_settings_macro_emits_apply_dump_write_functions() {
        let ts = crate::types::toml_settings::TomlSettings::default();
        let mut p = crate::types::settings::PersistedPlayerSettings::default();
        mock_apply_toml(&ts, &mut p);

        let src = crate::types::settings::PersistedPlayerSettings::default();
        let mut out = crate::types::player_settings::LivePlayerSettings::default();
        mock_dump_ps(&src, &mut out);

        let ps = crate::types::player_settings::LivePlayerSettings::default();
        let mut ts2 = crate::types::toml_settings::TomlSettings::default();
        mock_write_toml(&ps, &mut ts2);

        // Nothing to assert — the entry's read/write closures are no-ops.
        // The body of this test is the signature check itself; if the
        // macro had used the old `PlayerSettings` names anywhere, this
        // wouldn't compile.
    }
}
