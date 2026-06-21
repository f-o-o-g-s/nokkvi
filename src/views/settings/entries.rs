//! Settings entry building and filtering — pure functions for constructing and
//! filtering the `SettingsEntry` lists from config data.
//!
//! The persistent two-pane settings shell drives the detail-pane entries from
//! `SettingsPage::active_category`. Sidebar entries are `SettingsTab::ALL`
//! directly; the detail entries come from `build_category_sections`. Cross-tab
//! search ([`SettingsPage::search_all_entries`]) flattens results from every
//! tab into one list.
//!
//! ## Search ranking
//!
//! Search is fuzzy and relevance-ranked rather than a plain substring filter.
//! Each item is scored across its label, curated synonyms (see
//! `utils::setting_keywords`), subtitle, and category via the dependency-free
//! `utils::fuzzy` scorer, plus a weaker baseline when its section header or tab
//! name matches. Sections float up by their best item's score (headers still
//! group their items), so an exact label hit outranks an incidental
//! section-name match instead of being buried. Match highlighting is computed
//! separately at render time (`view::render_detail_pane`) from the live query,
//! so the `SettingItem` type carries no transient search state.

use std::cmp::Reverse;

use nokkvi_data::utils::{fuzzy, setting_keywords};

use super::{
    SettingsPage, SettingsTab, SettingsViewData, items,
    items::{SettingItem, SettingValue, SettingsEntry},
};

// ── Field weights ───────────────────────────────────────────────────────────
// Each field class occupies a tier; within a tier the fuzzy score breaks ties.
// The fuzzy score is unbounded (it grows with match length), so it is clamped
// below the gap to the next tier (see `tier`) — that makes the ordering
// guarantee real: a strong low-tier match can never outrank a higher tier.
const LABEL_BASE: i32 = 1000;
const KEYWORD_BASE: i32 = 760;
/// Tier for a Hotkeys-tab row matched by its key binding, so e.g. "ctrl" or
/// "space" surfaces the shortcuts bound to them.
const BINDING_BASE: i32 = 660;
const SUBTITLE_BASE: i32 = 560;
const CATEGORY_BASE: i32 = 360;
/// Baseline applied to every item in a section whose header label matches.
const HEADER_CONTEXT_BASE: i32 = 180;
/// Baseline applied to every item in a tab whose name matches.
const TAB_CONTEXT_BASE: i32 = 80;

/// Combine a tier `base` with a fuzzy `score`, clamping the score strictly
/// below the gap to `next_base` so a long low-tier match can never reach the
/// next tier up. `next_base == i32::MAX` leaves the top tier (label) uncapped.
fn tier(base: i32, next_base: i32, score: i32) -> i32 {
    base + score.min(next_base.saturating_sub(base) - 1)
}

struct ScoredItem {
    item: SettingItem,
    score: i32,
}

struct ScoredSection {
    header: Option<SettingsEntry>,
    items: Vec<ScoredItem>,
    score: i32,
}

impl SettingsPage {
    // ========================================================================
    // Category Sections (all sections always inline)
    // ========================================================================

    /// Build the detail-pane entries for a single category. Headers + Items
    /// are interleaved in section order; the renderer treats headers as
    /// non-interactive separators.
    pub(super) fn build_category_sections(
        tab: SettingsTab,
        data: &SettingsViewData,
    ) -> Vec<SettingsEntry> {
        // build_tab_entries already returns Header + Item sequences in section order
        Self::build_tab_entries(tab, data)
    }

    // ========================================================================
    // Tab Entry Builders (unchanged — delegates to items_*.rs)
    // ========================================================================

    /// Build entries for a single tab
    pub(super) fn build_tab_entries(
        tab: SettingsTab,
        data: &SettingsViewData,
    ) -> Vec<SettingsEntry> {
        match tab {
            SettingsTab::Visualizer => items::build_visualizer_items(
                &data.visualizer_config,
                &data.theme_file,
                &data.active_theme_stem,
            ),
            SettingsTab::Theme => items::build_theme_items(
                &data.theme_file,
                data.rounded_mode,
                data.opacity_gradient,
                data.is_light_mode,
            ),
            SettingsTab::General => items::build_general_items(&data.general),
            SettingsTab::Interface => items::build_interface_items(&data.interface),
            SettingsTab::Playback => items::build_playback_items(&data.playback),
            SettingsTab::Hotkeys => items::build_hotkeys_items(&data.hotkey_config),
        }
    }

    // ========================================================================
    // Search (cross-tab, fuzzy + ranked)
    // ========================================================================

    /// Build entries from ALL tabs (for cross-tab search)
    fn build_all_entries(data: &SettingsViewData) -> Vec<SettingsEntry> {
        let mut all = Vec::new();
        for tab in SettingsTab::ALL {
            all.extend(Self::build_tab_entries(*tab, data));
        }
        all
    }

    /// Search across all tabs, fuzzy-matched and relevance-ranked.
    ///
    /// Every tab's sections are scored and merged into one list ordered by
    /// section relevance (most-relevant section first), so a precise hit in one
    /// tab can outrank a whole tab pulled in only by a tab-name match. An empty
    /// query returns the natural (unranked) order of all entries.
    pub(super) fn search_all_entries(data: &SettingsViewData, query: &str) -> Vec<SettingsEntry> {
        if query.trim().is_empty() {
            return Self::build_all_entries(data);
        }
        let nq = query.trim().to_lowercase();
        let mut sections = Vec::new();
        for tab in SettingsTab::ALL {
            let tab_score =
                fuzzy::fuzzy_score(tab.label(), &nq).filter(|s| fuzzy::is_strong_score(*s, &nq));
            let entries = Self::build_tab_entries(*tab, data);
            sections.extend(score_sections(entries, &nq, tab_score));
        }
        // Stable sort preserves tab/section source order for equal scores.
        sections.sort_by_key(|s| Reverse(s.score));
        flatten(sections)
    }

    /// Fuzzy-filter and rank a flat entry list (one header/item sequence),
    /// exercising the same scoring/ranking core as [`Self::search_all_entries`]
    /// without needing a full `SettingsViewData`. Test-only entry point; an
    /// empty query returns the input unchanged.
    #[cfg(test)]
    pub(super) fn filter_by_search(entries: &[SettingsEntry], query: &str) -> Vec<SettingsEntry> {
        if query.trim().is_empty() {
            return entries.to_vec();
        }
        let nq = query.trim().to_lowercase();
        let mut sections = score_sections(entries.to_vec(), &nq, None);
        sections.sort_by_key(|s| Reverse(s.score));
        flatten(sections)
    }
}

/// Score every item in `entries`, grouped into sections by their header.
///
/// `nq` is the already-lowercased query. `tab_score`, when present, is the
/// (strong) fuzzy score of the owning tab's name and seeds a weak baseline for
/// every item in the tab. A section is emitted only if at least one of its
/// items scores (directly, via synonym, or via header/tab context).
fn score_sections(
    entries: Vec<SettingsEntry>,
    nq: &str,
    tab_score: Option<i32>,
) -> Vec<ScoredSection> {
    let mut sections: Vec<ScoredSection> = Vec::new();
    let mut cur_header: Option<SettingsEntry> = None;
    let mut cur_header_score: Option<i32> = None;
    let mut cur_items: Vec<ScoredItem> = Vec::new();

    for entry in entries {
        match entry {
            SettingsEntry::Header { label, icon } => {
                flush_section(&mut sections, cur_header.take(), &mut cur_items);
                cur_header_score =
                    fuzzy::fuzzy_score(label, nq).filter(|s| fuzzy::is_strong_score(*s, nq));
                cur_header = Some(SettingsEntry::Header { label, icon });
            }
            SettingsEntry::Item(item) => {
                if let Some(scored) = score_item(item, nq, cur_header_score, tab_score) {
                    cur_items.push(scored);
                }
            }
        }
    }
    flush_section(&mut sections, cur_header, &mut cur_items);
    sections
}

/// Score one item across its own fields plus section/tab context. Returns
/// `None` when nothing matched. (Highlight spans are recomputed at render time
/// from the live query, so the lean `SettingItem` type carries no transient
/// match state.)
fn score_item(
    item: SettingItem,
    nq: &str,
    header_score: Option<i32>,
    tab_score: Option<i32>,
) -> Option<ScoredItem> {
    let mut best = i32::MIN;
    if let Some(s) = fuzzy::fuzzy_score(&item.label, nq).filter(|s| fuzzy::is_strong_score(*s, nq))
    {
        best = best.max(tier(LABEL_BASE, i32::MAX, s));
    }
    for kw in setting_keywords::keywords_for(&item.key) {
        if let Some(s) = fuzzy::fuzzy_score(kw, nq).filter(|s| fuzzy::is_strong_score(*s, nq)) {
            best = best.max(tier(KEYWORD_BASE, LABEL_BASE, s));
        }
    }
    // Hotkey rows are searchable by their current key binding ("ctrl", "space").
    if let SettingValue::Hotkey(binding) = &item.value
        && let Some(s) = fuzzy::fuzzy_score(binding, nq).filter(|s| fuzzy::is_strong_score(*s, nq))
    {
        best = best.max(tier(BINDING_BASE, KEYWORD_BASE, s));
    }
    if let Some(sub) = item.subtitle.as_deref()
        && let Some(s) = fuzzy::fuzzy_score(sub, nq).filter(|s| fuzzy::is_strong_score(*s, nq))
    {
        best = best.max(tier(SUBTITLE_BASE, BINDING_BASE, s));
    }
    if let Some(s) =
        fuzzy::fuzzy_score(item.category, nq).filter(|s| fuzzy::is_strong_score(*s, nq))
    {
        best = best.max(tier(CATEGORY_BASE, SUBTITLE_BASE, s));
    }
    // Weak baseline from a matching section header or tab name.
    if let Some(cx) = [
        header_score.map(|s| tier(HEADER_CONTEXT_BASE, CATEGORY_BASE, s)),
        tab_score.map(|s| tier(TAB_CONTEXT_BASE, HEADER_CONTEXT_BASE, s)),
    ]
    .into_iter()
    .flatten()
    .max()
    {
        best = best.max(cx);
    }

    if best == i32::MIN {
        return None;
    }
    Some(ScoredItem { item, score: best })
}

/// Push the accumulated `items` as a section (scored by its best item) and
/// reset the buffer. Empty sections are dropped.
fn flush_section(
    sections: &mut Vec<ScoredSection>,
    header: Option<SettingsEntry>,
    items: &mut Vec<ScoredItem>,
) {
    if items.is_empty() {
        return;
    }
    let buffered = std::mem::take(items);
    let score = buffered.iter().map(|i| i.score).max().unwrap_or(i32::MIN);
    sections.push(ScoredSection {
        header,
        items: buffered,
        score,
    });
}

/// Flatten ranked sections into a render-ready entry list: each section's
/// header (if any) followed by its items ordered by score (descending, stable).
fn flatten(sections: Vec<ScoredSection>) -> Vec<SettingsEntry> {
    let mut out = Vec::new();
    for mut sec in sections {
        sec.items.sort_by_key(|s| Reverse(s.score));
        if let Some(h) = sec.header {
            out.push(h);
        }
        out.extend(sec.items.into_iter().map(|si| SettingsEntry::Item(si.item)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &'static str, label: &str, category: &'static str) -> SettingsEntry {
        items::SettingItem::bool_val(items::SettingMeta::new(key, label, category), false, false)
    }

    fn header(label: &'static str) -> SettingsEntry {
        SettingsEntry::Header {
            label,
            icon: "assets/icons/cog.svg",
        }
    }

    fn labels(entries: &[SettingsEntry]) -> Vec<String> {
        entries
            .iter()
            .filter_map(|e| match e {
                SettingsEntry::Item(it) => Some(it.label.clone()),
                SettingsEntry::Header { .. } => None,
            })
            .collect()
    }

    fn keys(entries: &[SettingsEntry]) -> Vec<String> {
        entries
            .iter()
            .filter_map(|e| match e {
                SettingsEntry::Item(it) => Some(it.key.to_string()),
                SettingsEntry::Header { .. } => None,
            })
            .collect()
    }

    #[test]
    fn synonym_query_finds_row_without_the_word() {
        let entries = vec![
            header("Volume Normalization"),
            item(
                "general.volume_normalization",
                "Volume Normalization",
                "Volume Normalization",
            ),
        ];
        // "loudness" appears nowhere in the visible text; only the synonym table
        // links it to this row.
        let out = labels(&SettingsPage::filter_by_search(&entries, "loudness"));
        assert!(
            out.contains(&"Volume Normalization".to_string()),
            "got {out:?}"
        );
    }

    #[test]
    fn dropped_letter_typo_still_matches() {
        let entries = vec![
            header("Transitions"),
            item(
                "general.crossfade_duration",
                "Crossfade Duration",
                "Transitions",
            ),
        ];
        // "crssfade" drops the 'o' — a subsequence, not a substring, so plain
        // `.contains()` would miss it.
        let out = labels(&SettingsPage::filter_by_search(&entries, "crssfade"));
        assert!(
            out.contains(&"Crossfade Duration".to_string()),
            "got {out:?}"
        );
    }

    #[test]
    fn exact_label_match_outranks_section_context() {
        let entries = vec![
            header("Colors"),
            item("dark.bg", "Background", "Colors"),
            item("dark.border", "Chrome Border Color", "Colors"),
        ];
        // "color" matches the section header (pulling both rows in as context)
        // AND the second row's label directly — the direct label hit must rank
        // first, not stay in source order.
        let out = labels(&SettingsPage::filter_by_search(&entries, "color"));
        assert_eq!(
            out.first(),
            Some(&"Chrome Border Color".to_string()),
            "got {out:?}"
        );
    }

    #[test]
    fn label_highlight_spans_match_the_renderer_call() {
        // The renderer recomputes highlight ranges from the live query via this
        // exact call; lock the byte ranges it produces for a label hit.
        let m = fuzzy::fuzzy_match("Volume Normalization", "volume").expect("label hit");
        assert!(fuzzy::is_strong(&m, "volume"));
        assert_eq!(m.ranges, vec![(0, 6)]);
    }

    #[test]
    fn empty_query_is_unchanged_passthrough() {
        let entries = vec![
            header("A"),
            item("k.one", "One", "A"),
            item("k.two", "Two", "A"),
        ];
        let out = SettingsPage::filter_by_search(&entries, "");
        assert_eq!(labels(&out), vec!["One".to_string(), "Two".to_string()]);
        assert_eq!(out.len(), entries.len());
    }

    #[test]
    fn no_match_returns_empty() {
        let entries = vec![header("A"), item("k.one", "One", "A")];
        assert!(SettingsPage::filter_by_search(&entries, "zzzzq").is_empty());
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let entries = vec![
            header("Volume Normalization"),
            item(
                "general.volume_normalization",
                "Volume Normalization",
                "Volume Normalization",
            ),
        ];
        // A stray leading/trailing space must not make the needle fail to match.
        for q in ["  volume", "volume  ", "  loudness  "] {
            assert!(
                !SettingsPage::filter_by_search(&entries, q).is_empty(),
                "query {q:?} should still match"
            );
        }
    }

    #[test]
    fn tier_clamps_keep_classes_ordered() {
        // An unboundedly large lower-tier fuzzy score can never reach the tier
        // above it, so e.g. section/tab context cannot outrank a real field hit.
        assert!(tier(KEYWORD_BASE, LABEL_BASE, 10_000) < LABEL_BASE);
        assert!(tier(SUBTITLE_BASE, KEYWORD_BASE, 10_000) < KEYWORD_BASE);
        assert!(tier(CATEGORY_BASE, SUBTITLE_BASE, 10_000) < SUBTITLE_BASE);
        assert!(tier(HEADER_CONTEXT_BASE, CATEGORY_BASE, 10_000) < CATEGORY_BASE);
        assert!(tier(TAB_CONTEXT_BASE, HEADER_CONTEXT_BASE, 10_000) < HEADER_CONTEXT_BASE);
        // Small scores still pass through as an intra-tier tiebreak.
        assert_eq!(tier(LABEL_BASE, i32::MAX, 42), LABEL_BASE + 42);
        assert_eq!(tier(KEYWORD_BASE, LABEL_BASE, 5), KEYWORD_BASE + 5);
    }

    #[test]
    fn hotkey_rows_searchable_by_key_binding() {
        let entries = vec![
            header("Playback"),
            items::SettingItem::from_meta(
                items::SettingMeta::new("hotkey.toggle_play", "Play / Pause", "Playback"),
                SettingValue::Hotkey("Ctrl+Space".to_string()),
                SettingValue::Hotkey("Ctrl+Space".to_string()),
            ),
        ];
        // "space" and "ctrl" appear only in the binding, not the label/category.
        for q in ["space", "ctrl"] {
            let out = labels(&SettingsPage::filter_by_search(&entries, q));
            assert!(
                out.contains(&"Play / Pause".to_string()),
                "query {q:?} should find the hotkey, got {out:?}"
            );
        }
    }

    #[test]
    fn synonyms_fire_on_real_built_rows() {
        // End-to-end against the actual builder output (real keys/labels), not
        // synthetic fixtures: proves the keyword table keys line up with the
        // keys the UI actually emits.
        use nokkvi_data::types::theme_file::ThemeFile;
        let entries = items::build_visualizer_items(
            &crate::visualizer_config::VisualizerConfig::default(),
            &ThemeFile::default(),
            "everforest",
        );

        // "bass" appears in no label/subtitle — only the synonym table links it
        // to visualizer.lower_cutoff_freq.
        let bass = keys(&SettingsPage::filter_by_search(&entries, "bass"));
        assert!(
            bass.iter().any(|k| k == "visualizer.lower_cutoff_freq"),
            "‘bass’ should surface the lower-cutoff row, got {bass:?}"
        );

        // "smoothing" -> Noise Reduction via synonym.
        let smoothing = keys(&SettingsPage::filter_by_search(&entries, "smoothing"));
        assert!(
            smoothing.iter().any(|k| k == "visualizer.noise_reduction"),
            "‘smoothing’ should surface noise reduction, got {smoothing:?}"
        );
    }
}
