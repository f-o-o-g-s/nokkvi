//! Playlists ViewModel - UI-specific view data for playlists

use crate::types::playlist::Playlist;

/// UI-specific view data for playlists
/// UI-projected data
#[derive(Debug, Clone)]
pub struct PlaylistUIViewData {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub duration: f32,
    pub song_count: u32,
    pub owner_name: String,
    pub public: bool,
    pub updated_at: String,
    /// Album IDs for 3x3 collage (up to 9 albums)
    pub artwork_album_ids: Vec<String>,
}

impl From<Playlist> for PlaylistUIViewData {
    fn from(playlist: Playlist) -> Self {
        Self {
            id: playlist.id,
            name: playlist.name,
            comment: playlist.comment,
            duration: playlist.duration,
            song_count: playlist.song_count,
            owner_name: playlist.owner_name,
            public: playlist.public,
            updated_at: playlist.updated_at,
            artwork_album_ids: Vec::new(),
        }
    }
}

impl From<&Playlist> for PlaylistUIViewData {
    fn from(playlist: &Playlist) -> Self {
        Self {
            id: playlist.id.clone(),
            name: playlist.name.clone(),
            comment: playlist.comment.clone(),
            duration: playlist.duration,
            song_count: playlist.song_count,
            owner_name: playlist.owner_name.clone(),
            public: playlist.public,
            updated_at: playlist.updated_at.clone(),
            artwork_album_ids: Vec::new(),
        }
    }
}

impl crate::utils::search::Searchable for PlaylistUIViewData {
    fn searchable_fields(&self) -> Vec<&str> {
        vec![&self.name, &self.comment]
    }
}

impl crate::types::collage_artwork::CollageArtworkItem for PlaylistUIViewData {
    fn id(&self) -> &str {
        &self.id
    }

    fn artwork_album_ids(&self) -> &[String] {
        &self.artwork_album_ids
    }
}
