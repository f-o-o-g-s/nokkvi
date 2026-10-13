//! Per-session queues: the live queue belongs to one server and user.
//!
//! The queue is two redb blobs under fixed keys (`queue_order`,
//! `queue_songs`) and the "Playing From" playlist sits in the user settings,
//! so before this module a login to another server restored the previous
//! server's queue: rows whose songs, covers and playlist do not exist there,
//! and the new server's queue overwrote the old one's only copy.
//!
//! The live keys now carry an owner stamp (`queue_owner`, a [`QueueOwner`]
//! key). [`claim`] runs after every login, before anything reads the queue:
//! the same owner keeps the live queue; another owner parks it under its own
//! slot keys and moves the incoming owner's slot (or an empty queue) in, all
//! in one redb write. A queue saved before the stamp existed has no owner;
//! [`legacy_sample`] names a few of its song ids so the caller can ask the
//! server whether they exist there ([`LegacyVerdict`]).

use anyhow::Result;
use tracing::warn;

use super::{KEY_QUEUE_ORDER, KEY_QUEUE_SONGS};
use crate::{
    services::{
        state_storage::StateStorage,
        storage_keys::{QUEUE_OWNER, QUEUE_SLOT_PREFIX},
    },
    types::{active_playlist::ActivePlaylistRecord, queue::Queue},
};

/// Separates the parts of an owner key and of a slot key. A unit separator
/// never appears in a URL or a Navidrome username, so `server + user` pairs
/// cannot collide the way a `/` join of `http://x/a` + `b` and `http://x` +
/// `a/b` would.
const SEP: char = '\u{1f}';

/// The session a queue belongs to: the server it was built from and the user
/// logged into it. The same identity keys the stored session's owner check
/// (`credentials::SessionOwner`) and nokkvi's API-key player on each server
/// (`services::api_key`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueOwner {
    key: String,
    label: String,
}

impl QueueOwner {
    /// `server_url` is the resolved URL the session connected to; it is
    /// normalized (scheme and host lowercased by the URL parser, trailing `/`
    /// dropped) so a re-typed URL maps to the same owner. Navidrome usernames
    /// are case-insensitive, so the username is lowercased.
    pub fn new(server_url: &str, username: &str) -> Self {
        let trimmed = server_url.trim();
        let server =
            url::Url::parse(trimmed).map_or_else(|_| trimmed.to_owned(), |u| u.as_str().to_owned());
        let server = server.trim_end_matches('/').to_owned();
        let user = username.trim().to_lowercase();
        Self {
            key: format!("{server}{SEP}{user}"),
            label: format!("{user}@{server}"),
        }
    }

    /// `user@server`, for logs.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The normalized `server + user` key, for maps stored per session owner.
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
}

/// One stored part of a parked queue.
#[derive(Debug, Clone, Copy)]
enum SlotPart {
    Order,
    Songs,
    Playlist,
}

impl SlotPart {
    const fn name(self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Songs => "songs",
            Self::Playlist => "playlist",
        }
    }
}

fn slot_key(owner_key: &str, part: SlotPart) -> String {
    format!("{QUEUE_SLOT_PREFIX}{SEP}{owner_key}{SEP}{}", part.name())
}

/// Whether an unowned (pre-stamp) live queue belongs to the session claiming
/// it, as the caller found out from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyVerdict {
    /// Its songs exist on this server: keep it and stamp this owner.
    Belongs,
    /// None of the sampled songs exist here: another server's queue.
    Foreign,
    /// The server could not be asked. Change nothing and stamp nothing, so
    /// the next login asks again.
    Unknown,
}

/// What [`claim`] did to the live queue.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueClaim {
    /// The live queue (and the stored "Playing From" playlist) stay as they
    /// are.
    Kept,
    /// The live keys now hold another queue: reload the queue from storage
    /// and store `playlist` as the "Playing From" context. `restored` is
    /// whether a parked queue came back (`false`: started empty).
    Replaced {
        playlist: Option<ActivePlaylistRecord>,
        restored: bool,
    },
}

/// What [`legacy_sample`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacySample {
    /// The live queue has an owner stamp: [`claim`] needs no verdict.
    Owned,
    /// No stamp and no rows: nothing to ask about.
    Empty,
    /// No stamp: ask the server whether any of these song ids exist (the
    /// playing row first, then the first row).
    Ask(Vec<String>),
}

/// The song ids to check before claiming an unowned live queue.
pub fn legacy_sample(storage: &StateStorage) -> Result<LegacySample> {
    if live_owner(storage)?.is_some() {
        return Ok(LegacySample::Owned);
    }
    // An undecodable order blob is a queue `QueueManager::new` would start
    // empty anyway.
    let queue = match storage.load_binary::<Queue>(KEY_QUEUE_ORDER) {
        Ok(Some(queue)) => queue,
        Ok(None) => return Ok(LegacySample::Empty),
        Err(e) => {
            warn!(" [QUEUE] Unreadable queue order while sampling it: {e}");
            return Ok(LegacySample::Empty);
        }
    };
    let playing = queue
        .current_index()
        .and_then(|idx| queue.rows.get(idx))
        .map(|row| row.song_id.clone());
    let first = queue.rows.first().map(|row| row.song_id.clone());
    let mut ids: Vec<String> = playing.into_iter().collect();
    if let Some(first) = first
        && !ids.contains(&first)
    {
        ids.push(first);
    }
    Ok(if ids.is_empty() {
        LegacySample::Empty
    } else {
        LegacySample::Ask(ids)
    })
}

/// Claim the live queue for `owner` (see the module docs). `current_playlist`
/// is the "Playing From" context stored now, parked with the queue it belongs
/// to; `legacy` is only read for an unowned live queue.
pub fn claim(
    storage: &StateStorage,
    owner: &QueueOwner,
    current_playlist: Option<ActivePlaylistRecord>,
    legacy: LegacyVerdict,
) -> Result<QueueClaim> {
    let mut puts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut removes: Vec<String> = Vec::new();

    match live_owner(storage)? {
        Some(live) if live == owner.key => return Ok(QueueClaim::Kept),
        // Another session's queue: park it, with the playlist it plays from.
        Some(live) => {
            for (live_key, part) in [
                (KEY_QUEUE_ORDER, SlotPart::Order),
                (KEY_QUEUE_SONGS, SlotPart::Songs),
            ] {
                match storage.load_raw(live_key)? {
                    Some(bytes) => puts.push((slot_key(&live, part), bytes)),
                    None => removes.push(slot_key(&live, part)),
                }
            }
            match &current_playlist {
                Some(record) => {
                    puts.push((
                        slot_key(&live, SlotPart::Playlist),
                        serde_json::to_vec(record)?,
                    ));
                }
                None => removes.push(slot_key(&live, SlotPart::Playlist)),
            }
        }
        None => match legacy {
            LegacyVerdict::Belongs => {
                storage.save(QUEUE_OWNER, &owner.key)?;
                return Ok(QueueClaim::Kept);
            }
            LegacyVerdict::Unknown => return Ok(QueueClaim::Kept),
            // Another server's queue, and nothing says which: drop it below.
            LegacyVerdict::Foreign => {}
        },
    }

    // Move this owner's parked queue into the live keys, or start empty.
    let mut restored = false;
    for (live_key, part) in [
        (KEY_QUEUE_ORDER, SlotPart::Order),
        (KEY_QUEUE_SONGS, SlotPart::Songs),
    ] {
        let key = slot_key(&owner.key, part);
        match storage.load_raw(&key)? {
            Some(bytes) => {
                restored |= matches!(part, SlotPart::Order);
                puts.push((live_key.to_owned(), bytes));
            }
            None => removes.push(live_key.to_owned()),
        }
        removes.push(key);
    }
    let playlist_key = slot_key(&owner.key, SlotPart::Playlist);
    let playlist = match storage.load_raw(&playlist_key)? {
        Some(bytes) => match serde_json::from_slice(&bytes) {
            Ok(record) => Some(record),
            Err(e) => {
                warn!(" [QUEUE] Unreadable parked playlist context, dropping it: {e}");
                None
            }
        },
        None => None,
    };
    removes.push(playlist_key);
    puts.push((QUEUE_OWNER.to_owned(), serde_json::to_vec(&owner.key)?));

    let puts: Vec<(&str, &[u8])> = puts
        .iter()
        .map(|(key, bytes)| (key.as_str(), bytes.as_slice()))
        .collect();
    let removes: Vec<&str> = removes.iter().map(String::as_str).collect();
    storage.write_batch(&puts, &removes)?;

    Ok(QueueClaim::Replaced { playlist, restored })
}

/// The owner key stamped on the live queue, if any.
fn live_owner(storage: &StateStorage) -> Result<Option<String>> {
    storage.load::<String>(QUEUE_OWNER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::queue::{QueueManager, tests::make_test_song};

    fn storage() -> (StateStorage, tempfile::TempDir) {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let storage = StateStorage::new(temp.path().join("slots.redb")).expect("storage");
        (storage, temp)
    }

    /// Save `ids` as the live queue, playing the first row.
    fn save_live_queue(storage: &StateStorage, ids: &[&str]) {
        let mut qm = QueueManager::new(storage.clone()).expect("queue manager");
        let songs = ids.iter().map(|id| make_test_song(id)).collect();
        let _ = qm.set_queue(songs, Some(0));
    }

    fn live_ids(storage: &StateStorage) -> Vec<String> {
        QueueManager::new(storage.clone())
            .expect("queue manager")
            .song_ids_snapshot()
    }

    fn record(id: &str) -> ActivePlaylistRecord {
        ActivePlaylistRecord {
            id: id.to_owned(),
            name: format!("Playlist {id}"),
            ..Default::default()
        }
    }

    fn local() -> QueueOwner {
        QueueOwner::new("http://localhost:4533", "foogs")
    }

    fn demo() -> QueueOwner {
        QueueOwner::new("https://demo.navidrome.org", "demo")
    }

    fn stamp(storage: &StateStorage, owner: &QueueOwner) {
        assert_eq!(
            claim(storage, owner, None, LegacyVerdict::Belongs).expect("claim"),
            QueueClaim::Kept
        );
    }

    #[test]
    fn owner_normalizes_trailing_slash_host_case_and_username_case() {
        assert_eq!(
            QueueOwner::new("HTTP://LocalHost:4533/", "Foogs"),
            QueueOwner::new("http://localhost:4533", "foogs")
        );
        assert_ne!(local(), demo());
    }

    /// The bug: logging into another server showed the previous server's
    /// queue. Switching parks it and starts the new session empty;
    /// switching back brings it back, playlist context included.
    #[test]
    fn switching_servers_parks_the_queue_and_switching_back_restores_it() {
        let (storage, _temp) = storage();
        save_live_queue(&storage, &["l1", "l2"]);
        stamp(&storage, &local());

        let outcome = claim(
            &storage,
            &demo(),
            Some(record("pl-local")),
            LegacyVerdict::Unknown,
        )
        .expect("claim demo");
        assert_eq!(
            outcome,
            QueueClaim::Replaced {
                playlist: None,
                restored: false
            },
            "a first visit to the demo starts empty, with no Playing From"
        );
        assert!(live_ids(&storage).is_empty());

        save_live_queue(&storage, &["d1"]);
        let outcome = claim(
            &storage,
            &local(),
            Some(record("pl-demo")),
            LegacyVerdict::Unknown,
        )
        .expect("claim local");
        assert_eq!(
            outcome,
            QueueClaim::Replaced {
                playlist: Some(record("pl-local")),
                restored: true
            },
            "the local queue's Playing From comes back with it"
        );
        assert_eq!(live_ids(&storage), ["l1", "l2"]);

        let outcome = claim(
            &storage,
            &demo(),
            Some(record("pl-local")),
            LegacyVerdict::Unknown,
        )
        .expect("claim demo again");
        assert_eq!(
            outcome,
            QueueClaim::Replaced {
                playlist: Some(record("pl-demo")),
                restored: true
            }
        );
        assert_eq!(live_ids(&storage), ["d1"]);
    }

    #[test]
    fn the_same_session_keeps_its_queue() {
        let (storage, _temp) = storage();
        save_live_queue(&storage, &["l1"]);
        stamp(&storage, &local());

        let outcome = claim(
            &storage,
            &local(),
            Some(record("pl")),
            LegacyVerdict::Foreign,
        )
        .expect("claim");
        assert_eq!(
            outcome,
            QueueClaim::Kept,
            "a stamped queue never needs a verdict"
        );
        assert_eq!(live_ids(&storage), ["l1"]);
    }

    #[test]
    fn an_unowned_queue_whose_songs_exist_here_is_kept_and_stamped() {
        let (storage, _temp) = storage();
        save_live_queue(&storage, &["l1"]);
        assert_eq!(
            legacy_sample(&storage).expect("sample"),
            LegacySample::Ask(vec!["l1".to_owned()])
        );

        assert_eq!(
            claim(&storage, &local(), None, LegacyVerdict::Belongs).expect("claim"),
            QueueClaim::Kept
        );
        assert_eq!(live_ids(&storage), ["l1"]);
        assert_eq!(
            legacy_sample(&storage).expect("sample"),
            LegacySample::Owned,
            "stamped: never asked again"
        );
    }

    /// The owner's install today: the demo server's queue sits unowned in
    /// the live keys while they log into localhost.
    #[test]
    fn an_unowned_queue_from_another_server_is_dropped() {
        let (storage, _temp) = storage();
        save_live_queue(&storage, &["d1", "d2"]);

        let outcome = claim(
            &storage,
            &local(),
            Some(record("pl-demo")),
            LegacyVerdict::Foreign,
        )
        .expect("claim");
        assert_eq!(
            outcome,
            QueueClaim::Replaced {
                playlist: None,
                restored: false
            }
        );
        assert!(live_ids(&storage).is_empty());
        assert_eq!(
            legacy_sample(&storage).expect("sample"),
            LegacySample::Owned
        );
    }

    #[test]
    fn an_unowned_queue_the_server_could_not_judge_is_left_alone() {
        let (storage, _temp) = storage();
        save_live_queue(&storage, &["x1"]);

        assert_eq!(
            claim(&storage, &local(), None, LegacyVerdict::Unknown).expect("claim"),
            QueueClaim::Kept
        );
        assert_eq!(live_ids(&storage), ["x1"]);
        assert_eq!(
            legacy_sample(&storage).expect("sample"),
            LegacySample::Ask(vec!["x1".to_owned()]),
            "still unowned, so the next login asks again"
        );
    }

    #[test]
    fn the_sample_names_the_playing_row_then_the_first_row() {
        let (storage, _temp) = storage();
        let mut qm = QueueManager::new(storage.clone()).expect("queue manager");
        let songs = ["a", "b", "c"]
            .iter()
            .map(|id| make_test_song(id))
            .collect();
        let _ = qm.set_queue(songs, Some(2));

        assert_eq!(
            legacy_sample(&storage).expect("sample"),
            LegacySample::Ask(vec!["c".to_owned(), "a".to_owned()])
        );
    }

    #[test]
    fn an_unowned_empty_queue_needs_no_question() {
        let (storage, _temp) = storage();
        assert_eq!(
            legacy_sample(&storage).expect("sample"),
            LegacySample::Empty
        );
    }
}
