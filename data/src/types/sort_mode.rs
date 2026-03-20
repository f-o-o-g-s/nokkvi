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
