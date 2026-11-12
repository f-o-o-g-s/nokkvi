//! A session cache of answers asked for the row a view centres: one ask per
//! id, once the row has stayed centred for a moment.
//!
//! Three fetches run on this shape. Two are for the artist the Artists view
//! centres: the server's `getArtistInfo2` answer (`ArtistInfoState`) and the
//! ListenBrainz and Deezer similar artists (`ArtistRecsState`). The third is
//! the server's `getAlbumInfo2` answer for the album the Albums view centres
//! (`AlbumInfoState`). Each keeps its own copy, because they land, fail and
//! are cached on their own.

use std::{collections::HashSet, num::NonZeroUsize};

use super::{StaleDropGen, snapshotted_lru::SnapshottedLru};

/// How long a row must stay centred before its view asks the server about it
/// (`getArtistInfo2`, `getAlbumInfo2`). A first ask makes the server run its
/// metadata agents and broadcast a library refresh: too much to do for every
/// row a held arrow key scrolls past.
pub(crate) const CENTRE_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// Answers fetched this session for centred rows, keyed by the row's id (an
/// artist id or an album id).
///
/// The view asks once per id, when the row has stayed centred for a moment. A
/// result is cached under the id it was asked for, so one that lands after
/// the user moved on can never show under another row; it is kept for when
/// they come back. Only results from before a logout are dropped, through the
/// session `generation`.
pub struct CentredFetch<T> {
    /// Fetched answers per id. The view borrows `cache.snapshot`.
    pub cache: SnapshottedLru<String, T>,
    /// Ids with a fetch in flight, so re-centring one does not ask again
    /// before the first answer lands.
    pending: HashSet<String>,
    /// Session counter: a result captured under an older value came from a
    /// session since reset, and is dropped.
    generation: StaleDropGen,
    /// The row centred last, while its settle timer runs. Scrolling past a
    /// row replaces it, so only a row that stays centred is asked about: a
    /// first ask can make the server run its agents for it.
    settling: Option<String>,
    /// How many answers the cache keeps; an evicted row is asked about again
    /// when it is centred.
    capacity: NonZeroUsize,
}

impl<T: Clone> CentredFetch<T> {
    pub(crate) fn new(capacity: NonZeroUsize) -> Self {
        Self {
            cache: SnapshottedLru::new(capacity),
            pending: HashSet::new(),
            generation: StaleDropGen::default(),
            settling: None,
            capacity,
        }
    }

    /// The view centred the row `id`. Returns whether to start a settle
    /// timer for it: not when its answer is cached or already asked for.
    /// Either way it supersedes the previous row's timer.
    pub(crate) fn centre(&mut self, id: &str) -> bool {
        let wanted =
            !id.is_empty() && !self.cache.contains(&id.to_string()) && !self.pending.contains(id);
        self.settling = wanted.then(|| id.to_string());
        wanted
    }

    /// A settle timer fired for `id`: `true` when nothing was centred since,
    /// so the row is worth asking about.
    pub(crate) fn settled(&mut self, id: &str) -> bool {
        if self.settling.as_deref() == Some(id) {
            self.settling = None;
            true
        } else {
            false
        }
    }

    /// Claim a fetch for `id`: `None` when its answer is cached or a fetch is
    /// already in flight, else marks it in flight and returns the generation
    /// the result must carry back.
    pub(crate) fn begin(&mut self, id: &str) -> Option<u64> {
        if id.is_empty()
            || self.cache.contains(&id.to_string())
            || !self.pending.insert(id.to_string())
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

    /// Whether a fetch for `id` is in flight.
    pub(crate) fn is_pending(&self, id: &str) -> bool {
        self.pending.contains(id)
    }

    /// Whether `id`'s answer is cached.
    pub(crate) fn is_cached(&self, id: &str) -> bool {
        self.cache.contains(&id.to_string())
    }

    /// Land a fetch's outcome: `Some(answer)` is cached, `None` (a failure)
    /// only releases the id so the next centring asks again. Returns `false`
    /// and changes nothing when the result is from a session since reset.
    pub(crate) fn finish(&mut self, generation: u64, id: &str, answer: Option<T>) -> bool {
        if !self.generation.accepts(generation) {
            return false;
        }
        self.pending.remove(id);
        if let Some(answer) = answer {
            self.cache.put(id.to_string(), answer);
        }
        true
    }

    /// Record an answer for `id` that needed no fetch, such as "nothing to
    /// ask with". It is cached like a fetched one.
    pub(crate) fn settle_with(&mut self, id: &str, answer: T) {
        self.pending.remove(id);
        self.cache.put(id.to_string(), answer);
    }

    /// Logout: the cached answers belonged to the old server. The generation
    /// is carried forward so a fetch still in flight is dropped when it lands.
    pub(crate) fn reset_for_session(&mut self) {
        *self = Self {
            generation: self.generation.carried_forward(),
            ..Self::new(self.capacity)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch() -> CentredFetch<String> {
        CentredFetch::new(NonZeroUsize::new(8).expect("non-zero"))
    }

    #[test]
    fn asks_once_per_artist_until_the_answer_lands_and_then_not_at_all() {
        let mut state = fetch();
        let generation = state.begin("a1").expect("first centring asks");
        assert!(state.begin("a1").is_none(), "no second ask while in flight");
        assert!(state.finish(generation, "a1", Some("bio".to_string())));
        assert!(!state.is_pending("a1"));
        assert!(state.is_cached("a1"));
        assert!(
            state.begin("a1").is_none(),
            "a cached artist is not asked again"
        );
        assert_eq!(
            state.cache.peek(&"a1".to_string()).map(String::as_str),
            Some("bio")
        );
    }

    #[test]
    fn a_failure_releases_the_artist_so_the_next_centring_retries() {
        let mut state = fetch();
        let generation = state.begin("a1").expect("asks");
        assert!(state.finish(generation, "a1", None));
        assert!(!state.is_cached("a1"));
        assert!(state.begin("a1").is_some(), "a failure is not remembered");
    }

    #[test]
    fn a_result_from_before_logout_is_dropped() {
        let mut state = fetch();
        let before = state.begin("a1").expect("asks");
        state.reset_for_session();
        let after = state.begin("a1").expect("the new session asks again");
        assert!(!state.finish(before, "a1", Some("old server".to_string())));
        assert!(
            state.is_pending("a1"),
            "the stale result must not free the new ask"
        );
        assert!(state.cache.is_empty());
        assert!(state.finish(after, "a1", Some("new server".to_string())));
    }

    #[test]
    fn only_the_artist_still_centred_when_its_timer_fires_is_asked_for() {
        let mut state = fetch();
        assert!(state.centre("a1"));
        assert!(state.centre("a2"), "scrolling on supersedes a1");
        assert!(!state.settled("a1"), "a1 was scrolled past");
        assert!(state.settled("a2"));
        assert!(!state.settled("a2"), "one timer, one ask");
    }

    #[test]
    fn a_cached_artist_starts_no_timer_and_cancels_the_last_one() {
        let mut state = fetch();
        let generation = state.begin("cached").expect("asks");
        state.finish(generation, "cached", Some("bio".to_string()));
        assert!(state.centre("a1"));
        assert!(!state.centre("cached"));
        assert!(!state.settled("a1"), "a1 is no longer centred");
    }

    #[test]
    fn an_answer_settled_without_a_fetch_is_cached_like_a_fetched_one() {
        let mut state = fetch();
        state.settle_with("a1", "nothing to ask with".to_string());
        assert!(state.is_cached("a1"));
        assert!(!state.centre("a1"), "no timer for a settled artist");
        assert!(state.begin("a1").is_none());
    }

    #[test]
    fn an_empty_id_is_never_asked_for() {
        assert!(fetch().begin("").is_none());
    }

    /// The cache keeps its capacity across a logout, so the next session
    /// evicts at the same size.
    #[test]
    fn a_reset_keeps_the_capacity() {
        let mut state = CentredFetch::<String>::new(NonZeroUsize::new(2).expect("non-zero"));
        state.reset_for_session();
        for id in ["a", "b", "c"] {
            state.settle_with(id, id.to_string());
        }
        assert_eq!(state.cache.len(), 2);
        assert!(!state.is_cached("a"), "the oldest was evicted");
    }
}
