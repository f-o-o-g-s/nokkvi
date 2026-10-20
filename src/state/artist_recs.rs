//! Session cache of the similar artists ListenBrainz and Deezer list for the artists
//! the Artists view centres, behind the panel's "Similar artists" row.

use std::num::NonZeroUsize;

use nokkvi_data::types::artist_recs::ArtistRec;

use super::centred_fetch::CentredFetch;

/// How many artists' lists the session keeps. A list is ten short rows; an
/// evicted artist is asked about again when it is centred.
const ARTIST_RECS_CAPACITY: NonZeroUsize = NonZeroUsize::new(256).expect("non-zero");

/// The similar-artist lists fetched this session, keyed by artist id, plus
/// the one artist whose ask waits on its MusicBrainz id.
///
/// The fetch needs the artist's MusicBrainz id, which the native row carries
/// when the library is tagged with one and the `getArtistInfo2` answer
/// otherwise supplies (Navidrome's agents look it up). An artist settled
/// before that answer lands waits in `awaiting_mbid`, and the info's landing
/// asks for it then. An artist with no id anywhere settles on an empty list,
/// so it is not asked about again this session.
pub struct ArtistRecsState {
    fetch: CentredFetch<Vec<ArtistRec>>,
    /// The settled artist whose MusicBrainz id the info fetch has yet to
    /// supply. One at a time: centring another artist supersedes it.
    awaiting_mbid: Option<String>,
}

impl Default for ArtistRecsState {
    fn default() -> Self {
        Self {
            fetch: CentredFetch::new(ARTIST_RECS_CAPACITY),
            awaiting_mbid: None,
        }
    }
}

impl std::ops::Deref for ArtistRecsState {
    type Target = CentredFetch<Vec<ArtistRec>>;

    fn deref(&self) -> &Self::Target {
        &self.fetch
    }
}

impl std::ops::DerefMut for ArtistRecsState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fetch
    }
}

impl ArtistRecsState {
    /// `artist_id` settled with no MusicBrainz id in hand while its info
    /// fetch is out: ask once that lands.
    pub(crate) fn wait_for_mbid(&mut self, artist_id: &str) {
        self.awaiting_mbid = Some(artist_id.to_string());
    }

    /// `artist_id`'s info fetch ended: `true` when its similar artists were
    /// waiting on it. The wait is over either way.
    pub(crate) fn take_awaiting(&mut self, artist_id: &str) -> bool {
        if self.awaiting_mbid.as_deref() == Some(artist_id) {
            self.awaiting_mbid = None;
            true
        } else {
            false
        }
    }

    /// Whether `artist_id`'s ask waits on its MusicBrainz id.
    #[cfg(test)]
    pub(crate) fn is_awaiting(&self, artist_id: &str) -> bool {
        self.awaiting_mbid.as_deref() == Some(artist_id)
    }

    /// Logout: the cached lists belonged to the old server's artists.
    pub(crate) fn reset_for_session(&mut self) {
        self.fetch.reset_for_session();
        self.awaiting_mbid = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_is_for_one_artist_and_ends_when_its_info_lands() {
        let mut state = ArtistRecsState::default();
        state.wait_for_mbid("a1");
        state.wait_for_mbid("a2");
        assert!(!state.is_awaiting("a1"), "a2 superseded a1");
        assert!(!state.take_awaiting("a1"));
        assert!(state.take_awaiting("a2"));
        assert!(!state.take_awaiting("a2"), "taken once");
    }

    #[test]
    fn a_logout_forgets_the_wait_and_the_lists() {
        let mut state = ArtistRecsState::default();
        state.wait_for_mbid("a1");
        state.settle_with("a2", Vec::new());
        state.reset_for_session();
        assert!(!state.is_awaiting("a1"));
        assert!(!state.is_cached("a2"));
    }
}
