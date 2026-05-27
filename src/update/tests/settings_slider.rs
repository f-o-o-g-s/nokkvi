//! Tests for the draggable settings slider message handler.
//!
//! Covers `SettingsMessage::EditSetFraction(f32)` — the message emitted by
//! the slider widget on click + drag. The handler should:
//! - Auto-enter edit mode on the focused row (mirrors `EditLeft` / `EditRight`).
//! - Quantize the fraction to the row's `step` and clamp to `[min, max]` via
//!   [`SettingValue::set_fraction`].
//! - Mutate the cached entry so the UI reflects the change immediately.
//! - Return a `WriteConfig` / `WriteGeneralSetting` action so the value
//!   persists.
//!
//! [`SettingValue::set_fraction`]: nokkvi_data::types::setting_value::SettingValue::set_fraction

use nokkvi_data::types::setting_value::SettingValue;

use crate::{
    test_helpers::*,
    views::settings::{
        SettingsAction, SettingsMessage, SettingsTab,
        items::{SettingItem, SettingsEntry},
    },
};

/// Focus the first `Item` in the active category whose value matches
/// `predicate`. Returns the cached index, or panics if no such row exists.
fn focus_first_matching(
    page: &mut crate::views::SettingsPage,
    predicate: impl Fn(&SettingItem) -> bool,
) -> usize {
    let idx = page
        .cached_entries
        .iter()
        .position(|e| match e {
            SettingsEntry::Item(item) => predicate(item),
            SettingsEntry::Header { .. } => false,
        })
        .expect("expected at least one matching Item in cached_entries");
    let total = page.cached_entries.len();
    page.slot_list.set_offset(idx, total);
    idx
}

#[test]
fn edit_set_fraction_at_midpoint_writes_quantized_int() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // Visualizer tab has the well-known Int settings ("Lower Cutoff Freq",
    // 20–1000 step 10; "Max Bar Count", 16–2048 step 8). We target the
    // first Int row so the test stays stable if Float rows reorder.
    page.active_category = SettingsTab::Visualizer;
    page.refresh_entries(&data);
    let idx = focus_first_matching(&mut page, |item| {
        matches!(item.value, SettingValue::Int { .. })
    });

    let action = page.update(SettingsMessage::EditSetFraction(0.5), &data);

    // Cached entry now sits at min + 0.5*(max-min), snapped to step.
    let SettingsEntry::Item(item) = &page.cached_entries[idx] else {
        panic!("entry at focused index was not an Item");
    };
    let SettingValue::Int {
        val,
        min,
        max,
        step,
        ..
    } = item.value
    else {
        panic!("focused row is not Int");
    };
    let span = max - min;
    let expected_raw = min as f64 + 0.5 * span as f64;
    let expected = ((expected_raw - min as f64) / step as f64).round() as i64 * step + min;
    assert_eq!(val, expected, "midpoint should snap to nearest step");
    assert!((min..=max).contains(&val));

    // Drag should produce a write action (not None).
    assert!(
        !matches!(action, SettingsAction::None),
        "EditSetFraction must produce a write action, got {action:?}"
    );

    // editing_index is set by auto_enter_edit_if_needed.
    assert_eq!(page.editing_index, Some(idx));
}

#[test]
fn edit_set_fraction_clamps_below_min() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.active_category = SettingsTab::Visualizer;
    page.refresh_entries(&data);
    let idx = focus_first_matching(&mut page, |item| {
        matches!(item.value, SettingValue::Float { .. })
    });

    let _ = page.update(SettingsMessage::EditSetFraction(-2.0), &data);

    let SettingsEntry::Item(item) = &page.cached_entries[idx] else {
        panic!();
    };
    let SettingValue::Float { val, min, .. } = item.value else {
        panic!();
    };
    assert!(
        (val - min).abs() < 1e-9,
        "negative fraction should clamp to min"
    );
}

#[test]
fn edit_set_fraction_clamps_above_max() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.active_category = SettingsTab::Visualizer;
    page.refresh_entries(&data);
    let idx = focus_first_matching(&mut page, |item| {
        matches!(item.value, SettingValue::Float { .. })
    });

    let _ = page.update(SettingsMessage::EditSetFraction(2.0), &data);

    let SettingsEntry::Item(item) = &page.cached_entries[idx] else {
        panic!();
    };
    let SettingValue::Float { val, max, .. } = item.value else {
        panic!();
    };
    assert!(
        (val - max).abs() < 1e-9,
        "out-of-range fraction should clamp to max"
    );
}

#[test]
fn edit_set_fraction_auto_enters_edit_mode_like_edit_left() {
    // EditLeft / EditRight call auto_enter_edit_if_needed so the user can
    // drag without first pressing Enter. EditSetFraction must mirror that
    // contract.
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.active_category = SettingsTab::Visualizer;
    page.refresh_entries(&data);
    let idx = focus_first_matching(&mut page, |item| {
        matches!(item.value, SettingValue::Int { .. })
    });

    assert!(page.editing_index.is_none());
    let _ = page.update(SettingsMessage::EditSetFraction(0.25), &data);
    assert_eq!(page.editing_index, Some(idx));
}
