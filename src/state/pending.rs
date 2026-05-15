//! Pending find-and-expand state shared across album/artist/genre/song chains.

/// In-flight find-and-expand target. Set by the matching
/// `handle_navigate_and_expand_*` and consumed by
/// `try_resolve_pending_expand_*` once the target id appears in its
/// library buffer. Album/Artist load paginated; Genre is single-shot.
///
/// `for_browsing_pane = true` routes the final `FocusAndExpand` dispatch
/// into the browsing-panel's tab (split-view) instead of the top pane.
///
/// Mutually exclusive — at most one chain runs at a time. Starting a new
/// chain (or any user-driven view change) supersedes the previous one.
#[derive(Debug, Clone)]
pub enum PendingExpand {
    Album {
        album_id: String,
        for_browsing_pane: bool,
    },
    Artist {
        artist_id: String,
        for_browsing_pane: bool,
    },
    Genre {
        genre_id: String,
        for_browsing_pane: bool,
    },
    /// Songs aren't expandable, so this variant exists solely to support the
    /// CenterOnPlaying (Shift+C) fallback in the Songs view: clear search,
    /// paginate forward until the playing track appears, and center on it
    /// without dispatching a FocusAndExpand. The `pending_expand_center_only`
    /// flag is implicit for this variant.
    Song {
        song_id: String,
        for_browsing_pane: bool,
    },
}

impl PendingExpand {
    /// View where the chain renders its result. Used by `handle_switch_view`
    /// to decide whether navigating away should cancel the chain.
    pub fn host_view(&self) -> crate::View {
        match self {
            Self::Album {
                for_browsing_pane: true,
                ..
            }
            | Self::Artist {
                for_browsing_pane: true,
                ..
            }
            | Self::Genre {
                for_browsing_pane: true,
                ..
            }
            | Self::Song {
                for_browsing_pane: true,
                ..
            } => crate::View::Queue,
            Self::Album { .. } => crate::View::Albums,
            Self::Artist { .. } => crate::View::Artists,
            Self::Genre { .. } => crate::View::Genres,
            Self::Song { .. } => crate::View::Songs,
        }
    }

    /// Target id carried by this variant. Each variant's id field has a
    /// distinct name (`album_id`/`artist_id`/`genre_id`/`song_id`) — this
    /// accessor lets entity-agnostic call sites read the id without a
    /// per-variant match.
    pub fn entity_id(&self) -> &str {
        match self {
            Self::Album { album_id, .. } => album_id,
            Self::Artist { artist_id, .. } => artist_id,
            Self::Genre { genre_id, .. } => genre_id,
            Self::Song { song_id, .. } => song_id,
        }
    }

    /// The `Message::Load*` that fetches this entity's library. Used by
    /// the collapsed `start_center_on_playing_chain` to dispatch the right
    /// load without a per-variant match at the call site. Note: this is
    /// the *entity-load* message, distinct from `ViewPage::reload_message`
    /// (which is per-view and includes Playlists/Radios — entities that
    /// don't participate in the find-and-expand chain).
    pub fn load_message(&self) -> crate::app_message::Message {
        match self {
            Self::Album { .. } => crate::app_message::Message::LoadAlbums,
            Self::Artist { .. } => crate::app_message::Message::LoadArtists,
            Self::Genre { .. } => crate::app_message::Message::LoadGenres,
            Self::Song { .. } => crate::app_message::Message::LoadSongs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_message::Message;

    fn id(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn load_message_picks_loadalbums_for_album_variant() {
        let p = PendingExpand::Album {
            album_id: id("a1"),
            for_browsing_pane: false,
        };
        assert!(matches!(p.load_message(), Message::LoadAlbums));
    }

    #[test]
    fn load_message_picks_loadartists_for_artist_variant() {
        let p = PendingExpand::Artist {
            artist_id: id("ar1"),
            for_browsing_pane: true,
        };
        assert!(matches!(p.load_message(), Message::LoadArtists));
    }

    #[test]
    fn load_message_picks_loadgenres_for_genre_variant() {
        let p = PendingExpand::Genre {
            genre_id: id("Rock"),
            for_browsing_pane: false,
        };
        assert!(matches!(p.load_message(), Message::LoadGenres));
    }

    #[test]
    fn load_message_picks_loadsongs_for_song_variant() {
        let p = PendingExpand::Song {
            song_id: id("s9"),
            for_browsing_pane: false,
        };
        assert!(matches!(p.load_message(), Message::LoadSongs));
    }

    #[test]
    fn entity_id_returns_inner_id_for_every_variant() {
        assert_eq!(
            PendingExpand::Album {
                album_id: id("a1"),
                for_browsing_pane: false,
            }
            .entity_id(),
            "a1",
        );
        assert_eq!(
            PendingExpand::Artist {
                artist_id: id("ar2"),
                for_browsing_pane: false,
            }
            .entity_id(),
            "ar2",
        );
        assert_eq!(
            PendingExpand::Genre {
                genre_id: id("Jazz"),
                for_browsing_pane: false,
            }
            .entity_id(),
            "Jazz",
        );
        assert_eq!(
            PendingExpand::Song {
                song_id: id("s7"),
                for_browsing_pane: false,
            }
            .entity_id(),
            "s7",
        );
    }
}

/// Item to re-pin the highlight onto after `set_children` fires.
///
/// `try_resolve_pending_expand_*` sets this after dispatching
/// `FocusAndExpand`, naming the target id and which library to look it
/// up in. When the corresponding children-load message lands
/// (`TracksLoaded` for albums, `AlbumsLoaded` for artists/genres), the
/// handler re-runs `set_selected` on the target's flat-list index so the
/// highlight stays on the focused item rather than drifting to whatever
/// happens to live at the center slot.
///
/// Cleared on the same triggers as `pending_expand_*_target`.
#[derive(Debug, Clone)]
pub enum PendingTopPin {
    Album(String),
    Artist(String),
    Genre(String),
}
