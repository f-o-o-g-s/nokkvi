//! Default-playlist chip — pin button in the view header.
//!
//! Renders as the same `ICON_CELL_WIDTH × HEADER_HEIGHT` transparent icon
//! cell every other `view_header` icon button (sort/refresh/center/add) uses
//! — `view_header::header_icon_cell` is the shared chassis, so a future
//! tweak to the header-icon vocabulary reaches this chip automatically.
//! The tooltip carries the playlist name (or `(none) — click to set` when
//! no default is configured); click opens the picker overlay.
//!
//! Used by the Playlists view (always) and the Queue view (gated by the
//! `queue_show_default_playlist` setting).

use iced::Element;

use crate::widgets::view_header::header_icon_cell;

/// Render the default-playlist chip as a pin icon button.
///
/// `default_playlist_name` empty → "(none)" placeholder in the tooltip.
/// `on_press` fires when the chip is clicked (open the picker).
pub(crate) fn default_playlist_chip<'a, Message: Clone + 'a>(
    default_playlist_name: &str,
    on_press: Message,
) -> Element<'a, Message> {
    let tooltip_label = if default_playlist_name.is_empty() {
        "Default Playlist: (none) — click to set".to_string()
    } else {
        format!("Default Playlist: {default_playlist_name}")
    };
    header_icon_cell("assets/icons/pin.svg", &tooltip_label, on_press)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_renders_with_default_set() {
        let _: Element<'_, String> = default_playlist_chip("Workout", "click".to_string());
    }

    #[test]
    fn chip_renders_when_no_default() {
        let _: Element<'_, String> = default_playlist_chip("", "click".to_string());
    }
}
