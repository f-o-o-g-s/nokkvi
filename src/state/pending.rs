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
