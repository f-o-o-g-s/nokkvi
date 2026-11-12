//! Session cache of the album info nokkvi asked the server for
//! (`getAlbumInfo2`), behind the Albums view's album notes.

use std::num::NonZeroUsize;

use nokkvi_data::types::album::AlbumInfo;

use super::centred_fetch::CentredFetch;

/// How many albums' info the session keeps. An answer is a link and at most
/// a few hundred characters of notes; an evicted album is simply asked for
/// again when it is centred.
const ALBUM_INFO_CAPACITY: NonZeroUsize = NonZeroUsize::new(512).expect("non-zero");

/// The `getAlbumInfo2` answers fetched this session, keyed by album id.
///
/// Navidrome fetches an album's notes from its agents only when a client
/// asks, so the Albums view asks once per album id, when the album has stayed
/// centred for a moment, and shows the answer where the native row's notes
/// are empty. The ask, cache and stale-drop rules are [`CentredFetch`]'s.
pub(crate) type AlbumInfoState = CentredFetch<AlbumInfo>;

impl Default for AlbumInfoState {
    fn default() -> Self {
        Self::new(ALBUM_INFO_CAPACITY)
    }
}
