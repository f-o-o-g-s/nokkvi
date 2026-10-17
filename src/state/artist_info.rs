//! Session cache of the artist info nokkvi asked the server for
//! (`getArtistInfo2`), behind the Artists view's biography.

use std::num::NonZeroUsize;

use nokkvi_data::types::artist::ArtistInfo;

use super::centred_fetch::CentredFetch;

/// How many artists' info the session keeps. A biography is a few KB; an
/// evicted artist is simply asked for again when it is centred.
const ARTIST_INFO_CAPACITY: NonZeroUsize = NonZeroUsize::new(256).expect("non-zero");

/// The `getArtistInfo2` answers fetched this session, keyed by artist id.
///
/// Navidrome fetches an artist's biography from its agents only when a client
/// asks, so the Artists view asks once per artist id, when the artist has
/// stayed centred for a moment, and shows the answer where the native row's
/// `biography` is empty. The ask, cache and stale-drop rules are
/// [`CentredFetch`]'s.
pub(crate) type ArtistInfoState = CentredFetch<ArtistInfo>;

impl Default for ArtistInfoState {
    fn default() -> Self {
        Self::new(ARTIST_INFO_CAPACITY)
    }
}
