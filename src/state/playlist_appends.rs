//! Adds to a playlist that are running or waiting their turn.
//!
//! An add checks the playlist's tracks first (`getPlaylist`), then writes
//! only when none of the batch is there yet. Two adds to the same playlist
//! running at once would both pass the check before either write lands, so
//! the song went in twice. One add per playlist runs at a time; a second one
//! waits here and runs its own check once the first has finished, so that
//! check sees the first add.

use std::collections::{HashMap, HashSet, VecDeque};

use nokkvi_data::utils::dedupe::{already_present, without_present};

use super::StaleDropGen;

/// An add that waits for the running add to the same playlist to finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAppend {
    pub playlist_name: String,
    pub song_ids: Vec<String>,
    pub policy: DuplicatePolicy,
    /// The failure toast's verb phrase (`Failed to {error_ctx}: …`).
    pub error_ctx: &'static str,
}

/// What an add does about songs the playlist already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicatePolicy {
    /// Read the playlist's tracks first; with any of the batch already
    /// there, write nothing and ask (the add-conflict confirm). Quick-add
    /// and the picker's Add.
    Ask,
    /// Read the playlist's tracks first and add only the songs not there
    /// yet: the confirm's "Add" with its box checked. The read happens at
    /// add time, not when the confirm opened, so a song another add put
    /// there while the confirm was open is skipped too.
    Skip,
    /// Add every song, repeats included: the confirm's "Add anyway", or its
    /// "Add" with the box unchecked.
    AddAll,
}

impl DuplicatePolicy {
    /// Whether the add reads the playlist's tracks before it writes.
    pub fn checks(self) -> bool {
        !matches!(self, Self::AddAll)
    }
}

/// What an add writes once the playlist's tracks are known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppendPlan {
    /// Write `to_add` (possibly nothing); `skipped` songs of the batch were
    /// already there.
    Write { to_add: Vec<String>, skipped: usize },
    /// Write nothing and ask: `present` are the batch's ids already there,
    /// each once.
    Ask { present: Vec<String> },
}

/// Decide what an add of `song_ids` writes to a playlist holding `existing`.
pub(crate) fn plan_append(
    song_ids: &[String],
    existing: &HashSet<String>,
    policy: DuplicatePolicy,
) -> AppendPlan {
    match policy {
        DuplicatePolicy::AddAll => AppendPlan::Write {
            to_add: song_ids.to_vec(),
            skipped: 0,
        },
        DuplicatePolicy::Ask => {
            let present = already_present(song_ids, existing);
            if present.is_empty() {
                AppendPlan::Write {
                    to_add: song_ids.to_vec(),
                    skipped: 0,
                }
            } else {
                AppendPlan::Ask { present }
            }
        }
        DuplicatePolicy::Skip => {
            let present = already_present(song_ids, existing);
            let to_add = without_present(song_ids, &present);
            AppendPlan::Write {
                skipped: song_ids.len() - to_add.len(),
                to_add,
            }
        }
    }
}

/// The adds running (at most one per playlist id) and the adds waiting for
/// them, keyed by playlist id.
#[derive(Debug, Default)]
pub struct PlaylistAppendQueue {
    /// Playlist id → the ticket of the add running against it.
    running: HashMap<String, u64>,
    /// Playlist id → adds that arrived while one was running, in order.
    waiting: HashMap<String, VecDeque<PendingAppend>>,
    /// Mints tickets. Only counts up, across logout too: an add running at
    /// logout still finishes after it, and its ticket must not match an add
    /// the next session started (see [`StaleDropGen`]).
    tickets: StaleDropGen,
}

impl PlaylistAppendQueue {
    /// Ask to start an add to `playlist_id`. Returns the ticket the add
    /// hands back through [`Self::finish`] when it may run now, or `None`
    /// when another add to that playlist is running: this one is queued and
    /// [`Self::finish`] hands it back once that add is done.
    pub fn start(&mut self, playlist_id: &str, add: PendingAppend) -> Option<u64> {
        if self.running.contains_key(playlist_id) {
            self.waiting
                .entry(playlist_id.to_string())
                .or_default()
                .push_back(add);
            return None;
        }
        let ticket = self.tickets.bump();
        self.running.insert(playlist_id.to_string(), ticket);
        Some(ticket)
    }

    /// The add holding `ticket` against `playlist_id` has finished, whatever
    /// its outcome. Frees the playlist and returns the next waiting add, which
    /// the caller starts through [`Self::start`]. A ticket that no longer
    /// matches (the add outlived a logout) frees nothing and returns `None`.
    pub fn finish(&mut self, playlist_id: &str, ticket: u64) -> Option<PendingAppend> {
        if self.running.get(playlist_id) != Some(&ticket) {
            return None;
        }
        self.running.remove(playlist_id);
        let queue = self.waiting.get_mut(playlist_id)?;
        let next = queue.pop_front();
        if queue.is_empty() {
            self.waiting.remove(playlist_id);
        }
        next
    }

    /// The ticket of the add running against `playlist_id`, if any.
    pub fn running_ticket(&self, playlist_id: &str) -> Option<u64> {
        self.running.get(playlist_id).copied()
    }

    /// The adds waiting for `playlist_id`, oldest first.
    pub fn waiting(&self, playlist_id: &str) -> Vec<&PendingAppend> {
        self.waiting
            .get(playlist_id)
            .map(|queue| queue.iter().collect())
            .unwrap_or_default()
    }

    /// Logout: forget every running and waiting add (the waiting ones belong
    /// to a session that is gone). The ticket counter carries forward, so a
    /// running add that finishes after the reset frees nothing.
    pub fn reset_for_session(&mut self) {
        self.running.clear();
        self.waiting.clear();
        self.tickets = self.tickets.carried_forward();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(name: &str) -> PendingAppend {
        PendingAppend {
            playlist_name: name.to_string(),
            song_ids: vec!["s1".to_string()],
            policy: DuplicatePolicy::Ask,
            error_ctx: "add songs to playlist",
        }
    }

    fn ids<const N: usize>(ids: [&str; N]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn existing<const N: usize>(ids: [&str; N]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    /// The confirm's skip reads the playlist at add time: a song another
    /// add put there while the confirm was open is skipped too.
    #[test]
    fn skip_rechecks_against_the_playlist_as_it_is_now() {
        let batch = ids(["a", "b", "c"]);
        // The confirm opened with only "a" there; "b" landed since.
        assert_eq!(
            plan_append(&batch, &existing(["a", "b"]), DuplicatePolicy::Skip),
            AppendPlan::Write {
                to_add: ids(["c"]),
                skipped: 2,
            }
        );
        // Everything is there by now: nothing is written, nothing doubled.
        assert_eq!(
            plan_append(&batch, &existing(["a", "b", "c"]), DuplicatePolicy::Skip),
            AppendPlan::Write {
                to_add: Vec::new(),
                skipped: 3,
            }
        );
        // Repeats inside the batch are the user's selection and stay.
        assert_eq!(
            plan_append(
                &ids(["a", "b", "b"]),
                &existing(["a"]),
                DuplicatePolicy::Skip
            ),
            AppendPlan::Write {
                to_add: ids(["b", "b"]),
                skipped: 1,
            }
        );
    }

    #[test]
    fn add_all_writes_everything_without_a_check() {
        assert!(!DuplicatePolicy::AddAll.checks());
        let batch = ids(["a", "a"]);
        assert_eq!(
            plan_append(&batch, &existing(["a"]), DuplicatePolicy::AddAll),
            AppendPlan::Write {
                to_add: batch.clone(),
                skipped: 0,
            }
        );
    }

    #[test]
    fn ask_writes_when_nothing_is_there_and_asks_otherwise() {
        assert!(DuplicatePolicy::Ask.checks());
        let batch = ids(["a", "b", "a"]);
        assert_eq!(
            plan_append(&batch, &existing([]), DuplicatePolicy::Ask),
            AppendPlan::Write {
                to_add: batch.clone(),
                skipped: 0,
            }
        );
        assert_eq!(
            plan_append(&batch, &existing(["a", "z"]), DuplicatePolicy::Ask),
            AppendPlan::Ask {
                present: ids(["a"])
            }
        );
    }

    #[test]
    fn a_second_add_to_a_busy_playlist_waits_for_the_first() {
        let mut queue = PlaylistAppendQueue::default();
        let first = queue.start("pl1", add("Mix")).expect("the first add runs");
        assert_eq!(queue.start("pl1", add("Mix")), None, "the second waits");
        assert_eq!(queue.waiting("pl1").len(), 1);

        let next = queue
            .finish("pl1", first)
            .expect("the waiting add comes back");
        assert_eq!(next, add("Mix"));
        assert_eq!(queue.running_ticket("pl1"), None, "the playlist is free");
        assert!(queue.waiting("pl1").is_empty());

        let second = queue.start("pl1", next).expect("it now runs");
        assert_ne!(second, first, "each add gets its own ticket");
    }

    #[test]
    fn adds_to_different_playlists_run_side_by_side() {
        let mut queue = PlaylistAppendQueue::default();
        assert!(queue.start("pl1", add("A")).is_some());
        assert!(queue.start("pl2", add("B")).is_some());
        assert!(queue.waiting("pl1").is_empty() && queue.waiting("pl2").is_empty());
    }

    #[test]
    fn waiting_adds_come_back_in_order() {
        let mut queue = PlaylistAppendQueue::default();
        let first = queue.start("pl1", add("one")).expect("runs");
        assert_eq!(queue.start("pl1", add("two")), None);
        assert_eq!(queue.start("pl1", add("three")), None);

        let next = queue.finish("pl1", first).expect("two comes back");
        assert_eq!(next.playlist_name, "two");
        let second = queue.start("pl1", next).expect("runs");
        let next = queue.finish("pl1", second).expect("three comes back");
        assert_eq!(next.playlist_name, "three");
    }

    #[test]
    fn a_stale_ticket_frees_nothing() {
        let mut queue = PlaylistAppendQueue::default();
        let before_logout = queue.start("pl1", add("Mix")).expect("runs");
        assert_eq!(queue.start("pl1", add("Mix")), None);

        queue.reset_for_session();
        assert_eq!(queue.running_ticket("pl1"), None);
        assert!(queue.waiting("pl1").is_empty(), "waiting adds are dropped");

        let after_login = queue.start("pl1", add("Mix")).expect("runs");
        assert_ne!(after_login, before_logout, "tickets never repeat");
        assert_eq!(queue.finish("pl1", before_logout), None);
        assert_eq!(
            queue.running_ticket("pl1"),
            Some(after_login),
            "the pre-logout add must not free the new session's add"
        );
    }
}
