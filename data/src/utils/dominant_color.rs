//! Dominant color extraction from album artwork using color-thief.
//!
//! Provides a blocking function that decodes image bytes and returns the
//! dominant color as an `(u8, u8, u8)` tuple. The UI crate converts this
//! to an `iced::Color`.

/// Extract the dominant color from raw image bytes.
///
/// Uses `image::load_from_memory` to decode, then feeds the raw pixel buffer
/// to `color_thief::get_palette()`. Returns the first (most dominant) palette
/// entry as an `(r, g, b)` tuple.
///
/// Returns `None` if decoding fails or the image has no pixels.
pub fn extract_dominant_color(image_bytes: &[u8]) -> Option<(u8, u8, u8)> {
    let img = image::load_from_memory(image_bytes).ok()?;
    let rgba = img.to_rgba8();
    let pixels = rgba.as_raw();

    // color_thief expects &[u8] in RGB(A) layout with a specified ColorFormat
    let palette = color_thief::get_palette(pixels, color_thief::ColorFormat::Rgba, 5, 10).ok()?;

    let dominant = palette.first()?;
    Some((dominant.r, dominant.g, dominant.b))
}

/// Determine whether a color is perceptually "dark" using relative luminance.
///
/// Uses the standard sRGB luminance formula (BT.709).
/// Returns `true` if the color is dark (white text should be used).
// W3C relative luminance requires linearizing sRGB channels
fn linearize(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn is_dark_color(r: u8, g: u8, b: u8) -> bool {
    let luminance = 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b);

    // W3C suggests swapping to dark text if background luminance > 0.179
    luminance <= 0.179
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_color_detection() {
        assert!(is_dark_color(0, 0, 0)); // Black is dark
        assert!(is_dark_color(50, 20, 30)); // Deep maroon is dark
        assert!(!is_dark_color(255, 255, 255)); // White is not dark
        assert!(!is_dark_color(200, 200, 200)); // Light grey is not dark

        // Bright orange-red (like the Nina Kraviz album) is NOT dark
        assert!(!is_dark_color(240, 80, 40));
    }
}
