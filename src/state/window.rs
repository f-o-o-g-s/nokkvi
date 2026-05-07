//! Window dimensions, scale factor, EQ-modal flags, and tracked modifiers.

/// Window dimensions and scale factor
#[derive(Debug, Clone)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub scale_factor: f32,
    /// Whether the EQ modal overlay is currently visible.
    pub eq_modal_open: bool,
    /// Whether the EQ modal is in "save preset" mode (showing name input).
    pub eq_save_mode: bool,
    /// Text input content for the preset name being saved.
    pub eq_save_name: String,
    /// Cached custom EQ presets (loaded from redb, kept in sync on save/delete).
    pub custom_eq_presets: Vec<nokkvi_data::audio::eq::CustomEqPreset>,
    /// Global keyboard modifiers tracked for mouse interaction (e.g. shift-clicking)
    pub keyboard_modifiers: iced::keyboard::Modifiers,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            scale_factor: 1.0,
            eq_modal_open: false,
            eq_save_mode: false,
            eq_save_name: String::new(),
            custom_eq_presets: Vec::new(),
            keyboard_modifiers: iced::keyboard::Modifiers::default(),
        }
    }
}
