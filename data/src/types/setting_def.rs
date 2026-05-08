//! Settings macro foundation — `define_settings!` plus its supporting types.
//!
//! Each settings tab declares its keys via [`define_settings!`] in
//! `data/src/services/settings_tables/<tab>.rs`. The macro emits four
//! artifacts per tab:
//!
//! - `pub const TAB_<TAB>_SETTINGS: &[SettingDef]` — table of declared keys
//!   for future items-builder migration.
//! - `pub fn dispatch_<tab>_tab_setting(key, value, mgr) -> Option<Result<()>>`
//!   — sync persistence dispatcher. Returns `None` for keys not declared in
//!   this tab; `Some(Ok(()))` on success; `Some(Err(_))` on type mismatch or
//!   setter failure. Caller chains all three tab dispatchers; a `None` from
//!   every tab means the key is still owned by the legacy hand-written
//!   `match key.as_str()` arm in the UI crate.
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
//! `shell.settings().set_X(v).await` calls.

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
/// and `read` are all declared (no silent omission).
///
/// # Example
///
/// ```ignore
/// nokkvi_data::define_settings! {
///     tab: nokkvi_data::types::setting_def::Tab::General,
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
#[macro_export]
macro_rules! define_settings {
    (
        tab: $tab:expr,
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
                    read: |$rsrc:ident, $rout:ident| $rbody:expr $(,)?
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
        ) -> ::core::option::Option<::anyhow::Result<()>> {
            $(
                if key == $key {
                    return ::core::option::Option::Some(
                        $crate::define_settings_dispatch_arm!(
                            value, $vtype,
                            |$smgr, $sval: $sty| $sbody,
                            mgr, $key
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
    };
}

/// Internal helper used by [`define_settings!`] to extract the typed inner
/// payload from a [`SettingValue`] variant before invoking the setter
/// closure. Type mismatches yield `Err(anyhow::Error)`.
#[macro_export]
#[doc(hidden)]
macro_rules! define_settings_dispatch_arm {
    ($value:ident, Bool, |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr, $mgr:ident, $key:expr) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Bool(v) => {
                let $smgr = $mgr;
                let $sval: $sty = v;
                $sbody
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Bool, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Int, |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr, $mgr:ident, $key:expr) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Int { val, .. } => {
                let $smgr = $mgr;
                let $sval: $sty = val;
                $sbody
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Int, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Float, |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr, $mgr:ident, $key:expr) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Float { val, .. } => {
                let $smgr = $mgr;
                let $sval: $sty = val;
                $sbody
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Float, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Enum, |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr, $mgr:ident, $key:expr) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Enum { val, .. } => {
                let $smgr = $mgr;
                let $sval: $sty = val;
                $sbody
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Enum, got {:?}",
                $key,
                other
            )),
        }
    }};
    ($value:ident, Text, |$smgr:ident, $sval:ident : $sty:ty| $sbody:expr, $mgr:ident, $key:expr) => {{
        match $value {
            $crate::types::setting_value::SettingValue::Text(s) => {
                let $smgr = $mgr;
                let $sval: $sty = s;
                $sbody
            }
            other => ::core::result::Result::Err(::anyhow::anyhow!(
                "type mismatch for setting {}: expected Text, got {:?}",
                $key,
                other
            )),
        }
    }};
}
