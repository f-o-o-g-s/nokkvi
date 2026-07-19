//! Uniform accessor traits for domain types.
//!
//! Every primary domain entity (Song, Album, Artist, Playlist, Genre,
//! RadioStation) exposes a `display_name()` and an `id()`. The two traits
//! defined here let generic callers project either uniformly without
//! caring which concrete type they have.
//!
//! Iced-free by design: only `&str` / `String` flow through these
//! signatures, keeping the `data/` crate's invariant intact.

use crate::types::{
    album::Album, artist::Artist, genre::Genre, playlist::Playlist, radio_station::RadioStation,
    song::Song,
};

/// Types with a human-readable display name (entity title for the UI).
pub trait Named {
    fn display_name(&self) -> &str;
}

/// Types with a stable string identifier (Navidrome ID).
pub trait HasId {
    fn id(&self) -> &str;
}

// ============================================================================
// Named impls — Song uses its `title`, the others use `name`.
// ============================================================================

impl Named for Song {
    fn display_name(&self) -> &str {
        &self.title
    }
}

impl Named for Album {
    fn display_name(&self) -> &str {
        &self.name
    }
}

impl Named for Artist {
    fn display_name(&self) -> &str {
        &self.name
    }
}

impl Named for Playlist {
    fn display_name(&self) -> &str {
        &self.name
    }
}

impl Named for Genre {
    fn display_name(&self) -> &str {
        &self.name
    }
}

impl Named for RadioStation {
    fn display_name(&self) -> &str {
        &self.name
    }
}

// ============================================================================
// HasId impls — all six expose `pub id: String`.
// ============================================================================

impl HasId for Song {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for Album {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for Artist {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for Playlist {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for Genre {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for RadioStation {
    fn id(&self) -> &str {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_returns_expected_display_name_for_song() {
        let song = Song::test_default("s1", "My Song Title");
        assert_eq!(<Song as Named>::display_name(&song), "My Song Title");
        assert_eq!(<Song as HasId>::id(&song), "s1");
    }

    #[test]
    fn named_returns_expected_display_name_for_album() {
        let album = Album {
            id: "a1".to_string(),
            name: "My Album".to_string(),
            album_artist: None,
            artist: None,
            album_artist_id: None,
            artist_id: None,
            cover_art: None,
            song_count: None,
            duration: None,
            max_year: None,
            year: None,
            genre: None,
            genres: None,
            created_at: None,
            play_date: None,
            play_count: None,
            library_id: None,
            library_path: None,
            library_name: None,
            date: None,
            min_year: None,
            max_original_year: None,
            min_original_year: None,
            original_date: None,
            release_date: None,
            compilation: None,
            comment: None,
            starred: None,
            starred_at: None,
            rating: None,
            updated_at: None,
            size: None,
            mbz_album_id: None,
            mbz_album_type: None,
            tags: None,
            participants: None,
            display_artist_cached: String::new(),
        };
        assert_eq!(<Album as Named>::display_name(&album), "My Album");
        assert_eq!(<Album as HasId>::id(&album), "a1");
    }

    #[test]
    fn named_returns_expected_display_name_for_artist() {
        let artist = Artist {
            id: "ar1".to_string(),
            name: "My Artist".to_string(),
            album_count: None,
            song_count: None,
            starred: None,
            starred_at: None,
            large_image_url: None,
            medium_image_url: None,
            small_image_url: None,
            play_count: None,
            play_date: None,
            size: None,
            mbz_artist_id: None,
            biography: None,
            similar_artists: None,
            external_url: None,
            external_info_updated_at: None,
            rating: None,
        };
        assert_eq!(<Artist as Named>::display_name(&artist), "My Artist");
        assert_eq!(<Artist as HasId>::id(&artist), "ar1");
    }

    #[test]
    fn named_returns_expected_display_name_for_playlist() {
        let playlist = Playlist {
            id: "p1".to_string(),
            name: "My Playlist".to_string(),
            comment: String::new(),
            duration: 0.0,
            size: 0,
            song_count: 0,
            owner_name: String::new(),
            owner_id: String::new(),
            public: false,
            created_at: String::new(),
            updated_at: String::new(),
            uploaded_image: None,
            external_image_url: None,
            rules: None,
            evaluated_at: None,
            path: String::new(),
            sync: false,
        };
        assert_eq!(<Playlist as Named>::display_name(&playlist), "My Playlist");
        assert_eq!(<Playlist as HasId>::id(&playlist), "p1");
    }

    #[test]
    fn named_returns_expected_display_name_for_genre() {
        let genre = Genre {
            id: "g1".to_string(),
            name: "Black Metal".to_string(),
            album_count: 0,
            song_count: 0,
        };
        assert_eq!(<Genre as Named>::display_name(&genre), "Black Metal");
        assert_eq!(<Genre as HasId>::id(&genre), "g1");
    }

    #[test]
    fn named_returns_expected_display_name_for_radio_station() {
        let radio = RadioStation {
            id: "r1".to_string(),
            name: "My Radio Station".to_string(),
            stream_url: "https://example.com/stream".to_string(),
            home_page_url: None,
            cover_art: None,
        };
        assert_eq!(
            <RadioStation as Named>::display_name(&radio),
            "My Radio Station"
        );
        assert_eq!(<RadioStation as HasId>::id(&radio), "r1");
    }

    #[test]
    fn is_starred_observes_song_starred_field() {
        // Song.starred is bool (never Option) — see deserialize_starred fn
        let mut song = Song::test_default("s1", "Title");
        song.starred = false;
        assert!(!song.is_starred());
        song.starred = true;
        assert!(song.is_starred());
    }

    #[test]
    fn is_starred_observes_album_and_artist_optional_fields() {
        // Album.starred is Option<bool>; None and Some(false) both map to false.
        let mut album = Album {
            id: "a".to_string(),
            name: "n".to_string(),
            album_artist: None,
            artist: None,
            album_artist_id: None,
            artist_id: None,
            cover_art: None,
            song_count: None,
            duration: None,
            max_year: None,
            year: None,
            genre: None,
            genres: None,
            created_at: None,
            play_date: None,
            play_count: None,
            library_id: None,
            library_path: None,
            library_name: None,
            date: None,
            min_year: None,
            max_original_year: None,
            min_original_year: None,
            original_date: None,
            release_date: None,
            compilation: None,
            comment: None,
            starred: None,
            starred_at: None,
            rating: None,
            updated_at: None,
            size: None,
            mbz_album_id: None,
            mbz_album_type: None,
            tags: None,
            participants: None,
            display_artist_cached: String::new(),
        };
        assert!(!album.is_starred()); // None → false
        album.starred = Some(false);
        assert!(!album.is_starred());
        album.starred = Some(true);
        assert!(album.is_starred());

        let mut artist = Artist {
            id: "a".to_string(),
            name: "n".to_string(),
            album_count: None,
            song_count: None,
            starred: None,
            starred_at: None,
            large_image_url: None,
            medium_image_url: None,
            small_image_url: None,
            play_count: None,
            play_date: None,
            size: None,
            mbz_artist_id: None,
            biography: None,
            similar_artists: None,
            external_url: None,
            external_info_updated_at: None,
            rating: None,
        };
        assert!(!artist.is_starred()); // None → false
        artist.starred = Some(false);
        assert!(!artist.is_starred());
        artist.starred = Some(true);
        assert!(artist.is_starred());
    }
}
