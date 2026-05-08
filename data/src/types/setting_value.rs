//! Typed setting values shared across the dispatcher, the settings UI items,
//! and the config writer.
//!
//! `SettingValue` is the iced-free runtime representation of a single setting's
//! value. The settings UI parses it into widget chrome (sliders, toggles,
//! enum cycles); the dispatcher pattern-matches on the variant to invoke the
//! right typed `SettingsManager::set_*` method; the TOML writer dispatches on
//! the variant to emit the appropriate scalar.

/// Determine the number of meaningful decimal places in a step value.
/// e.g. 0.1 → 1, 0.01 → 2, 0.005 → 3
fn decimal_places(step: f64) -> usize {
    let s = format!("{step}");
    s.find('.')
        .map_or(0, |dot| s[dot + 1..].trim_end_matches('0').len().max(1))
}

/// A typed setting value with metadata for rendering and editing.
#[derive(Debug, Clone)]
pub enum SettingValue {
    /// Floating-point with range and step.
    Float {
        val: f64,
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    /// Integer with range and step.
    Int {
        val: i64,
        min: i64,
        max: i64,
        step: i64,
        unit: &'static str,
    },
    /// Boolean toggle.
    Bool(bool),
    /// Enum-like string with a fixed set of options.
    Enum {
        val: String,
        options: Vec<&'static str>,
    },
    /// Hex color string (e.g. "#458588").
    HexColor(String),
    /// Array of hex color strings (gradient).
    ColorArray(Vec<String>),
    /// Read-only text (for display only, e.g. server URL).
    Text(String),
    /// Hotkey binding display (key combo string).
    Hotkey(String),
    /// Multi-select toggle badges — each badge independently toggleable.
    /// Vec of (display_label, setting_key, enabled).
    ToggleSet(Vec<(String, String, bool)>),
}

impl SettingValue {
    /// Human-readable display of the current value.
    pub fn display(&self) -> String {
        match self {
            SettingValue::Float {
                val, unit, step, ..
            } => {
                if *unit == "%" {
                    format!("{:.0}{}", val * 100.0, unit)
                } else {
                    let precision = decimal_places(*step);
                    format!(
                        "{:.prec$}{}",
                        val,
                        if unit.is_empty() { "" } else { unit },
                        prec = precision
                    )
                }
            }
            SettingValue::Int { val, unit, .. } => {
                format!("{}{}", val, if unit.is_empty() { "" } else { unit })
            }
            SettingValue::Bool(v) => {
                if *v {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            SettingValue::Enum { val, .. } => val.clone(),
            SettingValue::HexColor(hex) => hex.clone(),
            SettingValue::ColorArray(colors) => format!("{} colors", colors.len()),
            SettingValue::Text(t) => t.clone(),
            SettingValue::Hotkey(combo) => combo.clone(),
            SettingValue::ToggleSet(items) => {
                let enabled: Vec<_> = items
                    .iter()
                    .filter(|(_, _, on)| *on)
                    .map(|(label, _, _)| label.as_str())
                    .collect();
                if enabled.is_empty() {
                    "None".to_string()
                } else {
                    enabled.join(", ")
                }
            }
        }
    }

    /// Increment the value (Right arrow in edit mode).
    pub fn increment(&self) -> Option<SettingValue> {
        match self {
            SettingValue::Float {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val + step).min(*max);
                Some(SettingValue::Float {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Int {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val + step).min(*max);
                Some(SettingValue::Int {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Bool(v) => Some(SettingValue::Bool(!v)),
            SettingValue::Enum { val, options } => {
                if options.is_empty() {
                    return None;
                }
                let current_idx = options.iter().position(|o| o == val).unwrap_or(0);
                let next_idx = (current_idx + 1) % options.len();
                Some(SettingValue::Enum {
                    val: options[next_idx].to_string(),
                    options: options.clone(),
                })
            }
            _ => None,
        }
    }

    /// Decrement the value (Left arrow in edit mode).
    pub fn decrement(&self) -> Option<SettingValue> {
        match self {
            SettingValue::Float {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val - step).max(*min);
                Some(SettingValue::Float {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Int {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val - step).max(*min);
                Some(SettingValue::Int {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Bool(v) => Some(SettingValue::Bool(!v)),
            SettingValue::Enum { val, options } => {
                if options.is_empty() {
                    return None;
                }
                let current_idx = options.iter().position(|o| o == val).unwrap_or(0);
                let prev_idx = if current_idx == 0 {
                    options.len() - 1
                } else {
                    current_idx - 1
                };
                Some(SettingValue::Enum {
                    val: options[prev_idx].to_string(),
                    options: options.clone(),
                })
            }
            _ => None,
        }
    }

    /// Whether this value type supports inline increment/decrement editing.
    pub fn is_editable(&self) -> bool {
        matches!(
            self,
            SettingValue::Float { .. }
                | SettingValue::Int { .. }
                | SettingValue::Bool(_)
                | SettingValue::Enum { .. }
        )
    }

    /// Whether this value type is a numeric step value (Int/Float) that benefits
    /// from showing chevron arrow hints.
    pub fn is_incrementable(&self) -> bool {
        matches!(self, SettingValue::Float { .. } | SettingValue::Int { .. })
    }

    /// Parse a new value from a string representation.
    pub fn parse_from_str(&self, s: &str) -> Option<SettingValue> {
        match self {
            SettingValue::Bool(_) => match s {
                "true" | "On" => Some(SettingValue::Bool(true)),
                "false" | "Off" => Some(SettingValue::Bool(false)),
                _ => None,
            },
            SettingValue::Enum { options, .. } => {
                if options.contains(&s) {
                    Some(SettingValue::Enum {
                        val: s.to_string(),
                        options: options.clone(),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
