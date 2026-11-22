//! Discord Rich Presence state: the presence worker, what it was last told,
//! and the public cover link of each album heard this session.

use std::collections::HashMap;

use nokkvi_data::services::discord_rpc::{
    DiscordRpc, Presence, PresenceKey, effective_app_id, presence_changed,
};

/// Albums whose cover link is remembered before the cache starts over.
const MAX_COVERS: usize = 256;

/// How long after a failed cover request the next one may go out (the MPRIS
/// cover's `RETRY_FAILED_AFTER` is the same minute).
const COVER_RETRY_AFTER_SECS: i64 = 60;

/// What the server said when asked for an album's public cover link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverAnswer {
    /// A link Discord can show.
    Link(String),
    /// The server has no link for this album.
    NoLink,
    /// A link Discord can't reach (a LAN or tailnet address).
    PrivateLink,
    /// The request failed (a timeout, a server error).
    Failed,
}

/// The running worker (with the application id it was started for), the
/// presence last handed to it, and the cover links.
#[derive(Default)]
pub(crate) struct DiscordState {
    worker: Option<(String, DiscordRpc)>,
    sent: Option<Option<PresenceKey>>,
    /// The queue song the last tick described and its album, so the queue is
    /// searched once per song and not every tick. Only a hit is kept: right
    /// after a new play the tick can run before the queue list reloads.
    song: Option<(String, String)>,
    /// Album id -> the vetted public cover link (`None`: the server has none,
    /// or the request is still on its way). Present means asked; a failed
    /// request takes its album out again.
    covers: HashMap<String, Option<String>>,
    /// The server answered with a link Discord can't reach (a LAN or tailnet
    /// address). Every album's link is built the same way, so asking again
    /// only costs the server work (a first `getAlbumInfo2` makes Navidrome
    /// broadcast a refresh to every client): no more asks this session.
    links_private: bool,
    /// The unix second before which no cover is asked for. Set when a request
    /// fails (a timeout, a server error): the album stays unasked, and a
    /// server having trouble is not sent a request per tick or per song.
    cover_retry_at: Option<i64>,
}

impl DiscordState {
    /// The album the tick's song belongs to, looked up through `find` only
    /// when the song changed.
    pub(crate) fn album_of(
        &mut self,
        song_id: &str,
        find: impl FnOnce() -> Option<String>,
    ) -> Option<&str> {
        if self.song.as_ref().is_none_or(|(id, _)| id != song_id) {
            self.song = Some((song_id.to_owned(), find()?));
        }
        self.song.as_ref().map(|(_, album)| album.as_str())
    }

    /// The cover link known for `album_id`.
    pub(crate) fn cover_of(&self, album_id: &str) -> Option<&str> {
        self.covers.get(album_id)?.as_deref()
    }

    /// Mark `album_id` as asked; `true` when the caller should send the request.
    pub(crate) fn claim_cover(&mut self, album_id: &str, now_unix: i64) -> bool {
        if self.links_private
            || self.covers.contains_key(album_id)
            || self.cover_retry_at.is_some_and(|at| now_unix < at)
        {
            return false;
        }
        if self.covers.len() >= MAX_COVERS {
            self.covers.clear();
        }
        self.covers.insert(album_id.to_owned(), None);
        true
    }

    /// Record the server's `answer` for `album_id`, which arrived at `now_unix`.
    /// A failure is no answer: the album goes back to unasked, and nothing is
    /// asked for [`COVER_RETRY_AFTER_SECS`] (remembered like "no link", one
    /// timeout left the album on nokkvi's icon until logout).
    pub(crate) fn set_cover(&mut self, album_id: String, answer: CoverAnswer, now_unix: i64) {
        match answer {
            CoverAnswer::Link(url) => {
                self.covers.insert(album_id, Some(url));
            }
            CoverAnswer::NoLink => {
                self.covers.insert(album_id, None);
            }
            CoverAnswer::PrivateLink => {
                self.covers.insert(album_id, None);
                self.links_private = true;
            }
            CoverAnswer::Failed => {
                self.covers.remove(&album_id);
                self.cover_retry_at = Some(now_unix.saturating_add(COVER_RETRY_AFTER_SECS));
            }
        }
    }

    /// Bring Discord in line with `presence`. Turning presence off stops the
    /// worker (which clears the activity as it ends); a changed application
    /// (`override_id`, empty for nokkvi's own) restarts it. Work only reaches
    /// the worker when what Discord shows would change.
    pub(crate) fn sync(
        &mut self,
        enabled: bool,
        override_id: &str,
        presence: Option<Presence>,
        now_unix: i64,
    ) {
        if !enabled {
            self.worker = None;
            self.sent = None;
            return;
        }
        let app_id = effective_app_id(override_id);
        if self.worker.as_ref().is_none_or(|(id, _)| id != app_id) {
            self.worker = Some((app_id.to_owned(), DiscordRpc::start(app_id.to_owned())));
            self.sent = None;
        }
        let key = presence.as_ref().map(|p| PresenceKey::of(p, now_unix));
        if !presence_changed(self.sent.as_ref(), &key) {
            return;
        }
        if let Some((_, worker)) = self.worker.as_ref() {
            worker.set(presence, now_unix);
        }
        self.sent = Some(key);
    }

    /// Whether a worker is running (presence is on).
    pub(crate) fn is_running(&self) -> bool {
        self.worker.is_some()
    }

    /// Stop the worker (logout): the activity clears as its thread ends, and
    /// the cover links go with the session.
    pub(crate) fn stop(&mut self) {
        self.worker = None;
        self.sent = None;
        self.song = None;
        self.covers.clear();
        self.links_private = false;
        self.cover_retry_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_album_is_looked_up_once_per_song() {
        let mut state = DiscordState::default();
        let mut lookups = 0;
        for _ in 0..3 {
            assert_eq!(
                state.album_of("s1", || {
                    lookups += 1;
                    Some("al1".into())
                }),
                Some("al1")
            );
        }
        assert_eq!(lookups, 1);
        assert_eq!(state.album_of("s2", || None), None);
    }

    #[test]
    fn a_song_not_in_the_queue_yet_is_looked_up_again() {
        let mut state = DiscordState::default();
        assert_eq!(state.album_of("s1", || None), None);
        assert_eq!(state.album_of("s1", || Some("al1".into())), Some("al1"));
    }

    #[test]
    fn a_private_link_stops_the_asking_until_the_session_ends() {
        let mut state = DiscordState::default();
        assert!(state.claim_cover("al1", 0));
        state.set_cover("al1".into(), CoverAnswer::PrivateLink, 0);
        assert!(
            !state.claim_cover("al2", 0),
            "every album's link is just as private"
        );
        state.stop();
        assert!(
            state.claim_cover("al2", 0),
            "a new session may have another server"
        );
    }

    #[test]
    fn a_cover_is_asked_for_once_and_remembered() {
        let mut state = DiscordState::default();
        assert!(state.claim_cover("al1", 0));
        assert!(
            !state.claim_cover("al1", 0),
            "a request on its way is not repeated"
        );
        assert_eq!(state.cover_of("al1"), None);
        state.set_cover(
            "al1".into(),
            CoverAnswer::Link("https://m.example/share/img/t".into()),
            0,
        );
        assert_eq!(state.cover_of("al1"), Some("https://m.example/share/img/t"));
        assert!(!state.claim_cover("al1", 0));
    }

    #[test]
    fn stopping_forgets_the_covers() {
        let mut state = DiscordState::default();
        state.claim_cover("al1", 0);
        state.stop();
        assert!(state.claim_cover("al1", 0));
    }

    /// One timeout used to leave the album on nokkvi's icon until logout: the
    /// failure was remembered like "the server has no link".
    #[test]
    fn a_failed_lookup_is_asked_again_once_the_wait_is_over() {
        let mut state = DiscordState::default();
        assert!(state.claim_cover("al1", 1_000));
        state.set_cover("al1".into(), CoverAnswer::Failed, 1_002);
        assert!(
            !state.claim_cover("al1", 1_003),
            "a failing server is not asked again on the next tick"
        );
        assert!(
            !state.claim_cover("al2", 1_003),
            "nor about another album: the wait covers every ask"
        );
        assert!(!state.claim_cover("al1", 1_002 + COVER_RETRY_AFTER_SECS - 1));
        assert!(
            state.claim_cover("al1", 1_002 + COVER_RETRY_AFTER_SECS),
            "the album is asked about again after the wait"
        );
        state.set_cover(
            "al1".into(),
            CoverAnswer::Link("https://m.example/share/img/t".into()),
            1_100,
        );
        assert_eq!(state.cover_of("al1"), Some("https://m.example/share/img/t"));
    }

    /// An album the server has no link for stays asked: only a failure retries.
    #[test]
    fn an_album_without_a_link_is_never_asked_again() {
        let mut state = DiscordState::default();
        assert!(state.claim_cover("al1", 1_000));
        state.set_cover("al1".into(), CoverAnswer::NoLink, 1_001);
        assert!(!state.claim_cover("al1", 1_001 + 10 * COVER_RETRY_AFTER_SECS));
        assert!(
            state.claim_cover("al2", 1_002),
            "and it holds no other album back"
        );
    }

    #[test]
    fn stopping_ends_a_failed_lookups_wait() {
        let mut state = DiscordState::default();
        assert!(state.claim_cover("al1", 1_000));
        state.set_cover("al1".into(), CoverAnswer::Failed, 1_001);
        state.stop();
        assert!(
            state.claim_cover("al1", 1_002),
            "a new session may have another server"
        );
    }
}
