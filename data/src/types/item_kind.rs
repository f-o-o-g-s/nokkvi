//! Typed entity-kind discriminator for star/rating dispatch.
//!
//! Replaces the previous `item_type: &str` parameter that carried one of
//! `"album"`, `"artist"`, `"song"`. The drift-magic audit (§3) flagged the
//! string-keyed shape because `_ =>` fallbacks in `update/components.rs`
//! routed any unknown string (typo, future variant) silently to `Song`.
//!
//! Note: `Genre` is intentionally absent. Navidrome genres aren't
//! starrable or ratable today, and the rating-handler enumeration in
//! `src/update/hotkeys/star_rating.rs::get_center_item_info()` hard-codes
//! `SlotListEntry::Parent(_) => None` for the genre and playlist-parent
//! cases. `ItemKind::Playlist` is kept as a forward-compat slot since
//! the audit recommendation explicitly named it; today it's reachable
//! only via `BatchItem::Playlist` flattening, not via the star/rating
//! UI surface.
//!
//! Variant-count drift between `ItemKind` and `BatchItem` is locked by
//! `const _:` length anchors in this module — see the bottom of the file.

use crate::types::batch::BATCH_ITEM_VARIANT_COUNT;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ItemKind {
    Album,
    Artist,
    Song,
    Playlist,
}

impl ItemKind {
    /// Every `ItemKind` variant. Length-anchored — see the `const _:` lines.
    pub const ALL: &'static [ItemKind] = &[
        ItemKind::Album,
        ItemKind::Artist,
        ItemKind::Song,
        ItemKind::Playlist,
    ];

    /// Wire-format / log-label string. Stable: matches the prior
    /// `item_type: &str` literal at every site that fed the Subsonic
    /// API helpers (those helpers use the string only for error-message
    /// templating — the Subsonic `star`/`unstar`/`setRating` endpoints
    /// don't take a type discriminator on the wire).
    pub const fn api_str(self) -> &'static str {
        match self {
            ItemKind::Album => "album",
            ItemKind::Artist => "artist",
            ItemKind::Song => "song",
            ItemKind::Playlist => "playlist",
        }
    }
}

impl std::fmt::Display for ItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.api_str())
    }
}

// Length anchor: ItemKind has 4 variants; BatchItem has 5 (Genre is
// intentionally absent from ItemKind because Navidrome genres aren't
// starrable/ratable). Drift in either direction is a compile error.
const _: [(); 4 - ItemKind::ALL.len()] = [];
const _: [(); ItemKind::ALL.len() - 4] = [];
const _: [(); 5 - BATCH_ITEM_VARIANT_COUNT] = [];
const _: [(); BATCH_ITEM_VARIANT_COUNT - 5] = [];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_str_round_trip() {
        let pairs: Vec<(ItemKind, &str)> =
            ItemKind::ALL.iter().map(|k| (*k, k.api_str())).collect();
        assert_eq!(
            pairs,
            vec![
                (ItemKind::Album, "album"),
                (ItemKind::Artist, "artist"),
                (ItemKind::Song, "song"),
                (ItemKind::Playlist, "playlist"),
            ]
        );
    }

    #[test]
    fn display_matches_api_str() {
        for kind in ItemKind::ALL {
            assert_eq!(format!("{kind}"), kind.api_str());
        }
    }

    #[test]
    fn all_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for kind in ItemKind::ALL {
            assert!(seen.insert(*kind), "duplicate variant in ALL: {kind:?}");
        }
    }
}
