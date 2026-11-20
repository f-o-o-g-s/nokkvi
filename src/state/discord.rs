//! Discord Rich Presence state: the presence worker, what it was last told,
//! and the public cover link of each album heard this session.

use std::collections::HashMap;

use nokkvi_data::services::discord_rpc::{
    DiscordRpc, Presence, PresenceKey, effective_app_id, presence_changed,
};

/// Albums whose cover link is remembered before the cache starts over.
const MAX_COVERS: usize = 256;

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
    /// or the request is still on its way). Present means asked.
    covers: HashMap<String, Option<String>>,
    /// The server answered with a link Discord can't reach (a LAN or tailnet
    /// address). Every album's link is built the same way, so asking again
    /// only costs the server work (a first `getAlbumInfo2` makes Navidrome
    /// broadcast a refresh to every client): no more asks this session.
    links_private: bool,
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
    pub(crate) fn claim_cover(&mut self, album_id: &str) -> bool {
        if self.links_private || self.covers.contains_key(album_id) {
            return false;
        }
        if self.covers.len() >= MAX_COVERS {
            self.covers.clear();
        }
        self.covers.insert(album_id.to_owned(), None);
        true
    }

    /// Record the answer for `album_id`. `link_private`: the server gave a
    /// link Discord can't reach, so stop asking ([`Self::links_private`]).
    pub(crate) fn set_cover(&mut self, album_id: String, url: Option<String>, link_private: bool) {
        self.covers.insert(album_id, url);
        self.links_private |= link_private;
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
        assert!(state.claim_cover("al1"));
        state.set_cover("al1".into(), None, true);
        assert!(
            !state.claim_cover("al2"),
            "every album's link is just as private"
        );
        state.stop();
        assert!(
            state.claim_cover("al2"),
            "a new session may have another server"
        );
    }

    #[test]
    fn a_cover_is_asked_for_once_and_remembered() {
        let mut state = DiscordState::default();
        assert!(state.claim_cover("al1"));
        assert!(
            !state.claim_cover("al1"),
            "a request on its way is not repeated"
        );
        assert_eq!(state.cover_of("al1"), None);
        state.set_cover(
            "al1".into(),
            Some("https://m.example/share/img/t".into()),
            false,
        );
        assert_eq!(state.cover_of("al1"), Some("https://m.example/share/img/t"));
        assert!(!state.claim_cover("al1"));
    }

    #[test]
    fn stopping_forgets_the_covers() {
        let mut state = DiscordState::default();
        state.claim_cover("al1");
        state.stop();
        assert!(state.claim_cover("al1"));
    }
}
