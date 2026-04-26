use serde::{Deserialize, Serialize};

/// Sort modes for album/artist sorting/filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    RecentlyAdded,
    RecentlyPlayed,
    MostPlayed,
    Favorited,
    Random,
    Name,
    AlbumArtist,
    Artist,
    ReleaseYear,
    SongCount,
    AlbumCount, // For artists view
    Duration,
    Rating,
    Genre,
    // Song-specific sort modes
    Title,     // For songs view - sort by title
    Album,     // For songs view - sort by album name
    Bpm,       // For songs view
    Channels,  // For songs view
    Comment,   // For songs view
    UpdatedAt, // For playlists view - sort by last updated
}

/// Compact per-variant metadata: human-readable display string + snake_case
/// TOML key. The `Display` impl, `to_toml_key`, and `from_toml_key` all read
/// from this single table — adding a variant is a one-row edit.
struct SortModeMeta {
    display: &'static str,
    toml_key: &'static str,
}

const TABLE: &[(SortMode, SortModeMeta)] = &[
    (
        SortMode::RecentlyAdded,
        SortModeMeta {
            display: "Recently Added",
            toml_key: "recently_added",
        },
    ),
    (
        SortMode::RecentlyPlayed,
        SortModeMeta {
            display: "Recently Played",
            toml_key: "recently_played",
        },
    ),
    (
        SortMode::MostPlayed,
        SortModeMeta {
            display: "Most Played",
            toml_key: "most_played",
        },
    ),
    (
        SortMode::Favorited,
        SortModeMeta {
            display: "Favorited",
            toml_key: "favorited",
        },
    ),
    (
        SortMode::Random,
        SortModeMeta {
            display: "Random",
            toml_key: "random",
        },
    ),
    (
        SortMode::Name,
        SortModeMeta {
            display: "Name",
            toml_key: "name",
        },
    ),
    (
        SortMode::AlbumArtist,
        SortModeMeta {
            display: "Album Artist",
            toml_key: "album_artist",
        },
    ),
    (
        SortMode::Artist,
        SortModeMeta {
            display: "Artist",
            toml_key: "artist",
        },
    ),
    (
        SortMode::ReleaseYear,
        SortModeMeta {
            display: "Release Year",
            toml_key: "release_year",
        },
    ),
    (
        SortMode::SongCount,
        SortModeMeta {
            display: "Song Count",
            toml_key: "song_count",
        },
    ),
    (
        SortMode::AlbumCount,
        SortModeMeta {
            display: "Album Count",
            toml_key: "album_count",
        },
    ),
    (
        SortMode::Duration,
        SortModeMeta {
            display: "Duration",
            toml_key: "duration",
        },
    ),
    (
        SortMode::Rating,
        SortModeMeta {
            display: "Rating",
            toml_key: "rating",
        },
    ),
    (
        SortMode::Genre,
        SortModeMeta {
            display: "Genre",
            toml_key: "genre",
        },
    ),
    (
        SortMode::Title,
        SortModeMeta {
            display: "Title",
            toml_key: "title",
        },
    ),
    (
        SortMode::Album,
        SortModeMeta {
            display: "Album",
            toml_key: "album",
        },
    ),
    (
        SortMode::Bpm,
        SortModeMeta {
            display: "BPM",
            toml_key: "bpm",
        },
    ),
    (
        SortMode::Channels,
        SortModeMeta {
            display: "Channels",
            toml_key: "channels",
        },
    ),
    (
        SortMode::Comment,
        SortModeMeta {
            display: "Comment",
            toml_key: "comment",
        },
    ),
    (
        SortMode::UpdatedAt,
        SortModeMeta {
            display: "Updated At",
            toml_key: "updated_at",
        },
    ),
];

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.meta().display)
    }
}

impl SortMode {
    fn meta(self) -> &'static SortModeMeta {
        TABLE
            .iter()
            .find_map(|(m, meta)| if *m == self { Some(meta) } else { None })
            .expect("every SortMode variant must have a TABLE row")
    }

    /// Sort mode options for Albums view
    pub const ALBUM_OPTIONS: &[SortMode] = &[
        SortMode::RecentlyAdded,
        SortMode::RecentlyPlayed,
        SortMode::MostPlayed,
        SortMode::Favorited,
        SortMode::Random,
        SortMode::Name,
        SortMode::AlbumArtist,
        SortMode::Artist,
        SortMode::ReleaseYear,
        SortMode::SongCount,
        SortMode::Duration,
        SortMode::Rating,
        SortMode::Genre,
    ];

    /// Sort mode options for Artists view
    pub const ARTIST_OPTIONS: &[SortMode] = &[
        SortMode::Name,
        SortMode::Favorited,
        SortMode::MostPlayed,
        SortMode::AlbumCount,
        SortMode::SongCount,
        SortMode::Rating,
        SortMode::Random,
    ];

    /// Sort mode options for Songs view
    pub const SONG_OPTIONS: &[SortMode] = &[
        SortMode::RecentlyAdded,
        SortMode::RecentlyPlayed,
        SortMode::MostPlayed,
        SortMode::Favorited,
        SortMode::Random,
        SortMode::Title,
        SortMode::Album,
        SortMode::Artist,
        SortMode::AlbumArtist,
        SortMode::ReleaseYear,
        SortMode::Duration,
        SortMode::Bpm,
        SortMode::Channels,
        SortMode::Genre,
        SortMode::Rating,
        SortMode::Comment,
    ];

    /// Sort mode options for Genres view
    pub const GENRE_OPTIONS: &[SortMode] = &[
        SortMode::Name,
        SortMode::AlbumCount,
        SortMode::SongCount,
        SortMode::Random,
    ];

    /// Sort mode options for Playlists view
    pub const PLAYLIST_OPTIONS: &[SortMode] = &[
        SortMode::Name,
        SortMode::SongCount,
        SortMode::Duration,
        SortMode::UpdatedAt,
        SortMode::Random,
    ];

    /// Convert to a snake_case TOML key string.
    pub fn to_toml_key(self) -> &'static str {
        self.meta().toml_key
    }

    /// Parse from a snake_case TOML key string. Falls back to `Name` for unknown values.
    pub fn from_toml_key(s: &str) -> SortMode {
        TABLE
            .iter()
            .find_map(|(m, meta)| if meta.toml_key == s { Some(*m) } else { None })
            .unwrap_or(SortMode::Name)
    }

    /// Cycle to next/previous sort mode in a list
    pub fn cycle(current: SortMode, options: &[SortMode], forward: bool) -> SortMode {
        let current_idx = options.iter().position(|t| *t == current).unwrap_or(0);
        let new_idx = if forward {
            (current_idx + 1) % options.len()
        } else {
            (current_idx + options.len() - 1) % options.len()
        };
        options[new_idx]
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Every SortMode variant. Used by exhaustive tests below.
    const ALL_VARIANTS: &[SortMode] = &[
        SortMode::RecentlyAdded,
        SortMode::RecentlyPlayed,
        SortMode::MostPlayed,
        SortMode::Favorited,
        SortMode::Random,
        SortMode::Name,
        SortMode::AlbumArtist,
        SortMode::Artist,
        SortMode::ReleaseYear,
        SortMode::SongCount,
        SortMode::AlbumCount,
        SortMode::Duration,
        SortMode::Rating,
        SortMode::Genre,
        SortMode::Title,
        SortMode::Album,
        SortMode::Bpm,
        SortMode::Channels,
        SortMode::Comment,
        SortMode::UpdatedAt,
    ];

    #[test]
    fn artist_options_includes_most_played() {
        assert!(SortMode::ARTIST_OPTIONS.contains(&SortMode::MostPlayed));
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

    proptest! {
        /// Round-trip: every variant survives `to_toml_key → from_toml_key`.
        #[test]
        fn toml_key_round_trip(variant in proptest::sample::select(ALL_VARIANTS)) {
            let key = variant.to_toml_key();
            let parsed = SortMode::from_toml_key(key);
            prop_assert_eq!(parsed, variant);
        }
    }
}
