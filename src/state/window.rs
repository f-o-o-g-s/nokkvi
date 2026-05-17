//! Window dimensions, scale factor, and tracked modifiers.

/// Window dimensions and scale factor
#[derive(Debug, Clone)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub scale_factor: f32,
    /// Global keyboard modifiers tracked for mouse interaction (e.g. shift-clicking)
    pub keyboard_modifiers: iced::keyboard::Modifiers,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            scale_factor: 1.0,
            keyboard_modifiers: iced::keyboard::Modifiers::default(),
        }
    }
}
