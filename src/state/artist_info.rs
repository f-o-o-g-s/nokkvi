//! Session cache of the artist info nokkvi asked the server for
//! (`getArtistInfo2`), behind the Artists view's biography.

use std::{collections::HashSet, num::NonZeroUsize};

use nokkvi_data::types::artist::ArtistInfo;

use super::{StaleDropGen, snapshotted_lru::SnapshottedLru};

/// How many artists' info the session keeps. A biography is a few KB; an
/// evicted artist is simply asked for again when it is centred.
const ARTIST_INFO_CAPACITY: NonZeroUsize = NonZeroUsize::new(256).expect("non-zero");

/// The `getArtistInfo2` answers fetched this session, keyed by artist id.
///
/// Navidrome fetches an artist's biography from its agents only when a client
/// asks, so the Artists view asks once per artist id, when the artist has
/// stayed centred for a moment, and shows the answer where the native row's
/// `biography` is empty. A result is cached under the id it was asked for, so one that
/// lands after the user moved on can never show under another artist; it is
/// kept for when they come back. Only results from before a logout are
/// dropped, through the session `generation`.
pub struct ArtistInfoState {
    /// Fetched info per artist id. The view borrows `cache.snapshot`.
    pub cache: SnapshottedLru<String, ArtistInfo>,
    /// Artist ids with a fetch in flight, so re-centring one does not ask
    /// again before the first answer lands.
    pending: HashSet<String>,
    /// Session counter: a result captured under an older value came from a
    /// session since reset, and is dropped.
    generation: StaleDropGen,
    /// The artist centred last, while its settle timer runs. Scrolling past
    /// a row replaces it, so only an artist that stays centred is asked
    /// about: a first ask makes the server run its agents for that artist.
    settling: Option<String>,
}

impl Default for ArtistInfoState {
    fn default() -> Self {
        Self {
            cache: SnapshottedLru::new(ARTIST_INFO_CAPACITY),
            pending: HashSet::new(),
            generation: StaleDropGen::default(),
            settling: None,
        }
    }
}

impl ArtistInfoState {
    /// The Artists view centred `artist_id`. Returns whether to start a
    /// settle timer for it: not when its info is cached or already asked
    /// for. Either way it supersedes the previous artist's timer.
    pub(crate) fn centre(&mut self, artist_id: &str) -> bool {
        let wanted = !artist_id.is_empty()
            && !self.cache.contains(&artist_id.to_string())
            && !self.pending.contains(artist_id);
        self.settling = wanted.then(|| artist_id.to_string());
        wanted
    }

    /// A settle timer fired for `artist_id`: `true` when nothing was centred
    /// since, so the artist is worth asking about.
    pub(crate) fn settled(&mut self, artist_id: &str) -> bool {
        if self.settling.as_deref() == Some(artist_id) {
            self.settling = None;
            true
        } else {
            false
        }
    }

    /// Claim a fetch for `artist_id`: `None` when its info is cached or a
    /// fetch is already in flight, else marks it in flight and returns the
    /// generation the result must carry back.
    pub(crate) fn begin(&mut self, artist_id: &str) -> Option<u64> {
        if artist_id.is_empty()
            || self.cache.contains(&artist_id.to_string())
            || !self.pending.insert(artist_id.to_string())
        {
            return None;
        }
        Some(self.generation.current())
    }

    /// The session generation a fetch dispatched now would carry.
    #[cfg(test)]
    pub(crate) fn session_generation(&self) -> u64 {
        self.generation.current()
    }

    /// Whether a fetch for `artist_id` is in flight.
    #[cfg(test)]
    pub(crate) fn is_pending(&self, artist_id: &str) -> bool {
        self.pending.contains(artist_id)
    }

    /// Land a fetch's outcome: `Some(info)` is cached, `None` (a failure)
    /// only releases the id so the next centring asks again. Returns `false`
    /// and changes nothing when the result is from a session since reset.
    pub(crate) fn finish(
        &mut self,
        generation: u64,
        artist_id: &str,
        info: Option<ArtistInfo>,
    ) -> bool {
        if !self.generation.accepts(generation) {
            return false;
        }
        self.pending.remove(artist_id);
        if let Some(info) = info {
            self.cache.put(artist_id.to_string(), info);
        }
        true
    }

    /// Logout: the cached info belonged to the old server. The generation is
    /// carried forward so a fetch still in flight is dropped when it lands.
    pub(crate) fn reset_for_session(&mut self) {
        *self = Self {
            generation: self.generation.carried_forward(),
            ..Self::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(bio: &str) -> ArtistInfo {
        ArtistInfo {
            biography: Some(bio.to_string()),
            ..ArtistInfo::default()
        }
    }

    #[test]
    fn asks_once_per_artist_until_the_answer_lands_and_then_not_at_all() {
        let mut state = ArtistInfoState::default();
        let generation = state.begin("a1").expect("first centring asks");
        assert!(state.begin("a1").is_none(), "no second ask while in flight");
        assert!(state.finish(generation, "a1", Some(info("bio"))));
        assert!(!state.is_pending("a1"));
        assert!(
            state.begin("a1").is_none(),
            "a cached artist is not asked again"
        );
        assert_eq!(
            state
                .cache
                .peek(&"a1".to_string())
                .and_then(|i| i.biography.as_deref()),
            Some("bio")
        );
    }

    #[test]
    fn a_failure_releases_the_artist_so_the_next_centring_retries() {
        let mut state = ArtistInfoState::default();
        let generation = state.begin("a1").expect("asks");
        assert!(state.finish(generation, "a1", None));
        assert!(!state.cache.contains(&"a1".to_string()));
        assert!(state.begin("a1").is_some(), "a failure is not remembered");
    }

    #[test]
    fn a_result_from_before_logout_is_dropped() {
        let mut state = ArtistInfoState::default();
        let before = state.begin("a1").expect("asks");
        state.reset_for_session();
        let after = state.begin("a1").expect("the new session asks again");
        assert!(!state.finish(before, "a1", Some(info("old server"))));
        assert!(
            state.is_pending("a1"),
            "the stale result must not free the new ask"
        );
        assert!(state.cache.is_empty());
        assert!(state.finish(after, "a1", Some(info("new server"))));
    }

    #[test]
    fn only_the_artist_still_centred_when_its_timer_fires_is_asked_for() {
        let mut state = ArtistInfoState::default();
        assert!(state.centre("a1"));
        assert!(state.centre("a2"), "scrolling on supersedes a1");
        assert!(!state.settled("a1"), "a1 was scrolled past");
        assert!(state.settled("a2"));
        assert!(!state.settled("a2"), "one timer, one ask");
    }

    #[test]
    fn a_cached_artist_starts_no_timer_and_cancels_the_last_one() {
        let mut state = ArtistInfoState::default();
        let generation = state.begin("cached").expect("asks");
        state.finish(generation, "cached", Some(info("bio")));
        assert!(state.centre("a1"));
        assert!(!state.centre("cached"));
        assert!(!state.settled("a1"), "a1 is no longer centred");
    }

    #[test]
    fn an_empty_id_is_never_asked_for() {
        assert!(ArtistInfoState::default().begin("").is_none());
    }
}
