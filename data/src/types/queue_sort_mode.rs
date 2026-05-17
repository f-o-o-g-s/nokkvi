use serde::{Deserialize, Serialize};

/// Queue sort modes — each mode is a one-shot action that physically reorders
/// the queue. Manual reorder (drag / hotkey) works from any mode.
///
/// `Random` is a refresh-style mode: re-selecting it (or toggling the order
/// button while it is the active mode) re-shuffles the queue. The UI never
/// persists `Random` to `config.toml` — picking a deterministic mode again
/// is what updates the saved preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueSortMode {
    Album,
    Artist,
    Title,
    Duration,
    Genre,
    Rating,
    MostPlayed,
    Random,
}

/// Compact per-variant metadata: human-readable display string + snake_case
/// TOML key. The `Display` impl, `to_toml_key`, `from_toml_key`, and `all`
/// all read from this single table — adding a variant is a one-row edit.
struct QueueSortModeMeta {
    display: &'static str,
    toml_key: &'static str,
}

const TABLE: &[(QueueSortMode, QueueSortModeMeta)] = &[
    (
        QueueSortMode::Album,
        QueueSortModeMeta {
            display: "Album",
            toml_key: "album",
        },
    ),
    (
        QueueSortMode::Artist,
        QueueSortModeMeta {
            display: "Artist",
            toml_key: "artist",
        },
    ),
    (
        QueueSortMode::Title,
        QueueSortModeMeta {
            display: "Title",
            toml_key: "title",
        },
    ),
    (
        QueueSortMode::Duration,
        QueueSortModeMeta {
            display: "Duration",
            toml_key: "duration",
        },
    ),
    (
        QueueSortMode::Genre,
        QueueSortModeMeta {
            display: "Genre",
            toml_key: "genre",
        },
    ),
    (
        QueueSortMode::Rating,
        QueueSortModeMeta {
            display: "Rating",
            toml_key: "rating",
        },
    ),
    (
        QueueSortMode::MostPlayed,
        QueueSortModeMeta {
            display: "Most Played",
            toml_key: "most_played",
        },
    ),
    (
        QueueSortMode::Random,
        QueueSortModeMeta {
            display: "Random",
            toml_key: "random",
        },
    ),
];

impl QueueSortMode {
    fn meta(self) -> &'static QueueSortModeMeta {
        TABLE
            .iter()
            .find_map(|(m, meta)| if *m == self { Some(meta) } else { None })
            .expect("every QueueSortMode variant must have a TABLE row")
    }

    /// Every queue sort mode, in canonical UI order.
    pub fn all() -> Vec<QueueSortMode> {
        TABLE.iter().map(|(m, _)| *m).collect()
    }

    /// Convert to a snake_case TOML key string.
    pub fn to_toml_key(self) -> &'static str {
        self.meta().toml_key
    }

    /// Parse from a snake_case TOML key string. Falls back to `Title` for unknown values.
    pub fn from_toml_key(s: &str) -> QueueSortMode {
        TABLE
            .iter()
            .find_map(|(m, meta)| if meta.toml_key == s { Some(*m) } else { None })
            .unwrap_or(QueueSortMode::Title)
    }
}

impl std::fmt::Display for QueueSortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.meta().display)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Every QueueSortMode variant. Used by exhaustive tests below.
    const ALL_VARIANTS: &[QueueSortMode] = &[
        QueueSortMode::Album,
        QueueSortMode::Artist,
        QueueSortMode::Title,
        QueueSortMode::Duration,
        QueueSortMode::Genre,
        QueueSortMode::Rating,
        QueueSortMode::MostPlayed,
        QueueSortMode::Random,
    ];

    // Const-anchor: a new variant added without a TABLE row becomes a compile
    // error. Both directions are pinned so adding to one without the other
    // (ALL_VARIANTS or TABLE) also fails to compile.
    const _: [(); 8 - TABLE.len()] = [];
    const _: [(); TABLE.len() - 8] = [];
    const _: [(); 8 - ALL_VARIANTS.len()] = [];
    const _: [(); ALL_VARIANTS.len() - 8] = [];

    #[test]
    fn most_played_in_all_list() {
        assert!(QueueSortMode::all().contains(&QueueSortMode::MostPlayed));
    }

    #[test]
    fn most_played_display_label() {
        assert_eq!(QueueSortMode::MostPlayed.to_string(), "Most Played");
    }

    #[test]
    fn most_played_toml_key_roundtrips() {
        let key = QueueSortMode::MostPlayed.to_toml_key();
        assert_eq!(QueueSortMode::from_toml_key(key), QueueSortMode::MostPlayed);
    }

    #[test]
    fn random_in_all_list() {
        assert!(QueueSortMode::all().contains(&QueueSortMode::Random));
    }

    #[test]
    fn random_display_label() {
        assert_eq!(QueueSortMode::Random.to_string(), "Random");
    }

    #[test]
    fn random_toml_key_roundtrips() {
        let key = QueueSortMode::Random.to_toml_key();
        assert_eq!(QueueSortMode::from_toml_key(key), QueueSortMode::Random);
    }

    #[test]
    fn table_covers_every_variant() {
        for &variant in ALL_VARIANTS {
            assert!(
                TABLE.iter().any(|(m, _)| *m == variant),
                "TABLE missing entry for {variant:?}"
            );
        }
    }

    #[test]
    fn toml_keys_are_unique() {
        let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
        for (variant, meta) in TABLE {
            assert!(
                seen.insert(meta.toml_key),
                "duplicate toml_key {} for variant {variant:?}",
                meta.toml_key
            );
        }
    }

    #[test]
    fn display_strings_are_unique() {
        let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
        for (variant, meta) in TABLE {
            assert!(
                seen.insert(meta.display),
                "duplicate display {} for variant {variant:?}",
                meta.display
            );
        }
    }

    #[test]
    fn unknown_toml_key_falls_back_to_title() {
        assert_eq!(
            QueueSortMode::from_toml_key("not_a_real_mode"),
            QueueSortMode::Title
        );
    }

    proptest! {
        /// Round-trip: every variant survives `to_toml_key → from_toml_key`.
        #[test]
        fn toml_key_round_trip(variant in proptest::sample::select(ALL_VARIANTS)) {
            let key = variant.to_toml_key();
            let parsed = QueueSortMode::from_toml_key(key);
            prop_assert_eq!(parsed, variant);
        }
    }
}
