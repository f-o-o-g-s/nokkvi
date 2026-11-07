use std::cmp::Reverse;

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

/// The song fields a queue sort reads. A sort runs twice: the backend sorts
/// its rows by their pooled songs (`QueueManager::sort_queue`) and the UI
/// sorts the rows it shows at once, without waiting for the backend. Both go
/// through this trait and [`QueueSortMode::sort_key`], so a row lands in the
/// same place on both sides and the order the Queue view shows is the order
/// that plays. Each implementation reads the values the queue's projection
/// copies (`build_queue_song_ui_view_data`), defaults included.
pub trait QueueSortFields {
    fn sort_title(&self) -> &str;
    fn sort_artist(&self) -> &str;
    fn sort_album(&self) -> &str;
    /// Empty when the song has no genre.
    fn sort_genre(&self) -> &str;
    fn sort_duration_seconds(&self) -> u32;
    /// 0 when the song is unrated.
    fn sort_rating(&self) -> u32;
    /// 0 when the song was never played.
    fn sort_play_count(&self) -> u32;
}

/// What one song sorts by under a mode, in that mode's ascending order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueueSortValue {
    /// Title, artist, album or genre, lowercased: A to Z.
    Text(String),
    /// Duration in seconds: shortest first.
    Seconds(u32),
    /// Rating or play count: highest first, which is what these two modes
    /// call ascending.
    HighestFirst(Reverse<u32>),
    /// Random orders nothing, so every song ties.
    Unordered,
}

/// A song's place in one queue sort: the mode's value with the direction
/// folded in. Descending wraps the value in [`Reverse`] rather than reversing
/// an ascending sort's result, so a stable sort by this key keeps songs that
/// tie in their queue order in both directions. Reversing the result flips
/// every run of ties: the UI once did that while the backend kept them, and
/// a reload then moved the song just played from the top of an unrated block
/// to its bottom.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueueSortKey {
    Ascending(QueueSortValue),
    Descending(Reverse<QueueSortValue>),
}

impl QueueSortMode {
    fn sort_value<S: QueueSortFields + ?Sized>(self, song: &S) -> QueueSortValue {
        match self {
            Self::Title => QueueSortValue::Text(song.sort_title().to_lowercase()),
            Self::Artist => QueueSortValue::Text(song.sort_artist().to_lowercase()),
            Self::Album => QueueSortValue::Text(song.sort_album().to_lowercase()),
            Self::Genre => QueueSortValue::Text(song.sort_genre().to_lowercase()),
            Self::Duration => QueueSortValue::Seconds(song.sort_duration_seconds()),
            Self::Rating => QueueSortValue::HighestFirst(Reverse(song.sort_rating())),
            Self::MostPlayed => QueueSortValue::HighestFirst(Reverse(song.sort_play_count())),
            Self::Random => QueueSortValue::Unordered,
        }
    }

    /// The key `song` sorts by under this mode and direction. Every queue
    /// sort and sortedness check keys through here, so the UI and the backend
    /// can't order the same rows differently.
    pub fn sort_key<S: QueueSortFields + ?Sized>(self, ascending: bool, song: &S) -> QueueSortKey {
        let value = self.sort_value(song);
        if ascending {
            QueueSortKey::Ascending(value)
        } else {
            QueueSortKey::Descending(Reverse(value))
        }
    }

    /// Sort `songs` by this mode, stably: songs that tie keep their order in
    /// both directions. Each key is built once (`sort_by_cached_key`), so a
    /// text sort lowercases each song once. `Random` leaves the order alone;
    /// shuffling is the backend's job.
    pub fn sort_songs<S: QueueSortFields>(self, ascending: bool, songs: &mut [S]) {
        songs.sort_by_cached_key(|song| self.sort_key(ascending, song));
    }

    /// Whether `songs` are already in this mode's order. Fewer than two songs
    /// always are, and so is any order under `Random`, where every song ties.
    pub fn songs_are_sorted<S: QueueSortFields>(self, ascending: bool, songs: &[S]) -> bool {
        songs.is_sorted_by_key(|song| self.sort_key(ascending, song))
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

    /// A stand-in row for the sort: only the rating varies.
    struct Rated(&'static str, u32);

    impl QueueSortFields for Rated {
        fn sort_title(&self) -> &str {
            self.0
        }
        fn sort_artist(&self) -> &str {
            ""
        }
        fn sort_album(&self) -> &str {
            ""
        }
        fn sort_genre(&self) -> &str {
            ""
        }
        fn sort_duration_seconds(&self) -> u32 {
            0
        }
        fn sort_rating(&self) -> u32 {
            self.1
        }
        fn sort_play_count(&self) -> u32 {
            0
        }
    }

    /// Rating's ascending is highest first and its descending lowest first;
    /// either way the songs that tie keep their queue order, and the order a
    /// sort leaves passes the sortedness check for the same mode and
    /// direction (the check the Queue's "Unsorted" label runs).
    #[test]
    fn rating_sort_keeps_tied_songs_in_queue_order_both_ways() {
        let names = |rows: &[Rated]| rows.iter().map(|r| r.0).collect::<Vec<_>>();
        let mut rows = vec![Rated("a", 0), Rated("b", 3), Rated("c", 0), Rated("d", 0)];

        QueueSortMode::Rating.sort_songs(false, &mut rows);
        assert_eq!(names(&rows), ["a", "c", "d", "b"]);
        assert!(QueueSortMode::Rating.songs_are_sorted(false, &rows));
        assert!(!QueueSortMode::Rating.songs_are_sorted(true, &rows));

        QueueSortMode::Rating.sort_songs(true, &mut rows);
        assert_eq!(names(&rows), ["b", "a", "c", "d"]);
        assert!(QueueSortMode::Rating.songs_are_sorted(true, &rows));
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
