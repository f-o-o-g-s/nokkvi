//! Shared test helpers for asserting settings-entry structure.
//!
//! Compiled only for tests (`#[cfg(test)] pub(crate) mod test_support;` in
//! `views/settings/mod.rs`). The helpers assert section membership by KEY,
//! so a failure names the missing/extra/reordered key (and its section)
//! instead of reporting a drifted bare item count.

use super::items::SettingsEntry;

/// Extract all TOML key paths from settings entries (headers skipped).
pub(crate) fn extract_keys(entries: &[SettingsEntry]) -> Vec<&str> {
    entries
        .iter()
        .filter_map(|e| match e {
            SettingsEntry::Item(item) => Some(item.key.as_ref()),
            SettingsEntry::Header { .. } => None,
        })
        .collect()
}

/// All section header labels in display order.
pub(crate) fn header_labels(entries: &[SettingsEntry]) -> Vec<&'static str> {
    entries
        .iter()
        .filter_map(|e| match e {
            SettingsEntry::Header { label, .. } => Some(*label),
            SettingsEntry::Item(_) => None,
        })
        .collect()
}

/// Locate the slice of entries belonging to a given header (header
/// inclusive, up to but excluding the next header). Panics naming the
/// missing header.
fn section_slice<'a>(entries: &'a [SettingsEntry], header_label: &str) -> &'a [SettingsEntry] {
    let start = entries
        .iter()
        .position(|e| matches!(e, SettingsEntry::Header { label, .. } if *label == header_label))
        .unwrap_or_else(|| panic!("missing header {header_label}"));
    let after = entries[start + 1..]
        .iter()
        .position(|e| matches!(e, SettingsEntry::Header { .. }))
        .map_or(entries.len(), |i| start + 1 + i);
    &entries[start..after]
}

/// Assert the section under `header_label` carries exactly the `expected`
/// item keys in order. A failure names the section and shows both key lists,
/// so the diverging key is identifiable at a glance.
pub(crate) fn assert_section_keys(
    entries: &[SettingsEntry],
    header_label: &str,
    expected: &[&str],
) {
    let section = section_slice(entries, header_label);
    let keys = extract_keys(&section[1..]);
    assert_eq!(
        keys, expected,
        "section '{header_label}': item keys diverge from the expected ordered set",
    );
}
