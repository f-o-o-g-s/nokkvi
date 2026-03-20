//! Shared scaling utilities for dynamic UI sizing
//!
//! These helpers calculate appropriate sizes based on window dimensions and scale factor.
//! Used by slot list views to maintain consistent proportions across pages.

/// Calculate font size based on row height
///
/// # Arguments
/// * `base_size` - Base font size at reference height
/// * `row_height` - Current row height based on window size
/// * `scale_factor` - Display scale factor
///
/// # Returns
/// Scaled font size clamped to min/max bounds
pub fn calculate_font_size(base_size: f32, row_height: f32, scale_factor: f32) -> f32 {
    let reference_height = 60.0;
    let height_scale_factor = row_height / reference_height;
    let final_size = base_size * height_scale_factor;
    let (min_size, max_size) = (8.0, 24.0);

    let logical_size = final_size / scale_factor;
    logical_size.clamp(min_size, max_size).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_size_at_reference_height() {
        let size = calculate_font_size(16.0, 60.0, 1.0);
        assert_eq!(size, 16.0);
    }

    #[test]
    fn test_font_size_clamped_at_max() {
        let size = calculate_font_size(30.0, 60.0, 1.0);
        assert_eq!(size, 24.0); // Capped at max
    }

    #[test]
    fn test_scale_factor_affects_size() {
        let normal = calculate_font_size(16.0, 60.0, 1.0);
        let scaled = calculate_font_size(16.0, 60.0, 2.0);
        assert!(scaled < normal); // Higher scale factor = smaller logical size
    }
}
