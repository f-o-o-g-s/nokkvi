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

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SortMode::RecentlyAdded => write!(f, "Recently Added"),
            SortMode::RecentlyPlayed => write!(f, "Recently Played"),
            SortMode::MostPlayed => write!(f, "Most Played"),
            SortMode::Favorited => write!(f, "Favorited"),
            SortMode::Random => write!(f, "Random"),
            SortMode::Name => write!(f, "Name"),
            SortMode::AlbumArtist => write!(f, "Album Artist"),
            SortMode::Artist => write!(f, "Artist"),
            SortMode::ReleaseYear => write!(f, "Release Year"),
            SortMode::SongCount => write!(f, "Song Count"),
            SortMode::AlbumCount => write!(f, "Album Count"),
            SortMode::Duration => write!(f, "Duration"),
            SortMode::Rating => write!(f, "Rating"),
            SortMode::Genre => write!(f, "Genre"),
            SortMode::Title => write!(f, "Title"),
            SortMode::Album => write!(f, "Album"),
            SortMode::Bpm => write!(f, "BPM"),
            SortMode::Channels => write!(f, "Channels"),
            SortMode::Comment => write!(f, "Comment"),
            SortMode::UpdatedAt => write!(f, "Updated At"),
        }
    }
}

impl SortMode {
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
        match self {
            SortMode::RecentlyAdded => "recently_added",
            SortMode::RecentlyPlayed => "recently_played",
            SortMode::MostPlayed => "most_played",
            SortMode::Favorited => "favorited",
            SortMode::Random => "random",
            SortMode::Name => "name",
            SortMode::AlbumArtist => "album_artist",
            SortMode::Artist => "artist",
            SortMode::ReleaseYear => "release_year",
            SortMode::SongCount => "song_count",
            SortMode::AlbumCount => "album_count",
            SortMode::Duration => "duration",
            SortMode::Rating => "rating",
            SortMode::Genre => "genre",
            SortMode::Title => "title",
            SortMode::Album => "album",
            SortMode::Bpm => "bpm",
            SortMode::Channels => "channels",
            SortMode::Comment => "comment",
            SortMode::UpdatedAt => "updated_at",
        }
    }

    /// Parse from a snake_case TOML key string. Falls back to `Name` for unknown values.
    pub fn from_toml_key(s: &str) -> SortMode {
        match s {
            "recently_added" => SortMode::RecentlyAdded,
            "recently_played" => SortMode::RecentlyPlayed,
            "most_played" => SortMode::MostPlayed,
            "favorited" => SortMode::Favorited,
            "random" => SortMode::Random,
            "name" => SortMode::Name,
            "album_artist" => SortMode::AlbumArtist,
            "artist" => SortMode::Artist,
            "release_year" => SortMode::ReleaseYear,
            "song_count" => SortMode::SongCount,
            "album_count" => SortMode::AlbumCount,
            "duration" => SortMode::Duration,
            "rating" => SortMode::Rating,
            "genre" => SortMode::Genre,
            "title" => SortMode::Title,
            "album" => SortMode::Album,
            "bpm" => SortMode::Bpm,
            "channels" => SortMode::Channels,
            "comment" => SortMode::Comment,
            "updated_at" => SortMode::UpdatedAt,
            _ => SortMode::Name,
        }
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
