//! Pure helpers behind Autoplay: a playback mode that keeps the music going
//! by appending songs like the last ones played when the queue runs low.
//!
//! The UI's playback tick asks [`autoplay_wants_fill`] (or, once the queue
//! has run out, [`autoplay_wants_resume`]) whether to ask for a batch; the
//! backend verb (`AppService::autoplay_top_up`) picks seeds with
//! [`pick_autoplay_seeds`], resolves them through the instant-mix ladder
//! with the Autoplay plan (which ends in [`cap_per_artist`]) and re-checks
//! the same rule before it appends. Everything here is pure.

use std::collections::{HashMap, HashSet};

use crate::types::{
    instant_mix::{artist_key, is_rejected},
    song::Song,
};

/// Autoplay adds a batch once at most this many songs are left after the
/// playing one in play order.
pub const AUTOPLAY_LOW_WATERMARK: usize = 1;

/// How many songs one Autoplay batch adds.
pub const AUTOPLAY_BATCH: usize = 10;

/// How many seed songs one batch is built from: the last distinct songs of
/// the listening trail.
pub const AUTOPLAY_SEEDS: usize = 3;

/// Most songs by one artist in a batch, relaxed only when the batch would
/// otherwise come up short ([`cap_per_artist`]).
pub const AUTOPLAY_MAX_PER_ARTIST: usize = 2;

/// Seconds the UI waits before it asks again after a batch found nothing or
/// failed.
pub const AUTOPLAY_RETRY_SECS: u64 = 30;

/// How many times the UI asks again for one current song after a batch
/// found nothing or failed; after that it waits for the next song.
pub const AUTOPLAY_MAX_RETRIES: u8 = 3;

/// Whether Autoplay wants a batch now: it is on, Repeat is off, a queue row
/// is current (`runway` is `Some`) and at most [`AUTOPLAY_LOW_WATERMARK`]
/// songs follow it in play order. `runway` is the count of songs after the
/// current one in play order (`QueueManager::runway`).
pub fn autoplay_wants_fill(enabled: bool, repeat_off: bool, runway: Option<usize>) -> bool {
    enabled && repeat_off && runway.is_some_and(|left| left <= AUTOPLAY_LOW_WATERMARK)
}

/// Whether Autoplay wants a batch after the queue ran out: it is on, Repeat
/// is off and no queue row is current (the queue ended and nothing was
/// started since). The batch then plays from its first song.
pub fn autoplay_wants_resume(enabled: bool, repeat_off: bool, runway: Option<usize>) -> bool {
    enabled && repeat_off && runway.is_none()
}

/// What one `AppService::autoplay_top_up` call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoplayOutcome {
    /// No batch was wanted, at the start or any more by the time the songs
    /// arrived (the runway is long, Repeat is on, the mode was turned off, a
    /// row is current again after a run-out, or there was nothing to seed
    /// from). Nothing was added; not a failure.
    NotNeeded,
    /// A batch was wanted and nothing new was found (not even random
    /// songs), or everything found was queued meanwhile. Nothing was added.
    NothingFound,
    /// `count` songs were appended as Autoplay rows, the first being the row
    /// `first_entry_id`. The verb never plays: after a run-out the UI plays
    /// that row itself.
    Added { count: usize, first_entry_id: u64 },
}

/// One song of the listening trail Autoplay picks its seeds from.
#[derive(Debug, Clone, Copy)]
pub struct TrailSong<'a> {
    pub song: &'a Song,
    /// Whether Autoplay added this song's row (false for a song the user
    /// chose).
    pub autoplay: bool,
}

/// Up to `count` distinct seed songs from the END of `trail` (oldest first:
/// the play history, then the play order from the current song to the
/// tail), tail first. Never a song rated exactly one star (the
/// `pick_instant_mix_seeds` rule). When every seed picked is a song
/// Autoplay added and the trail holds a usable song the user chose, the
/// oldest seed gives way to the most recent such song, so a long session
/// stays tied to what the user picked. An empty trail gives no seeds.
pub fn pick_autoplay_seeds(trail: &[TrailSong<'_>], count: usize) -> Vec<Song> {
    if count == 0 {
        return Vec::new();
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut picked: Vec<TrailSong<'_>> = Vec::with_capacity(count);
    for entry in trail.iter().rev() {
        if picked.len() == count {
            break;
        }
        if !is_rejected(entry.song) && seen.insert(entry.song.id.as_str()) {
            picked.push(*entry);
        }
    }
    if !picked.is_empty() && picked.iter().all(|seed| seed.autoplay) {
        let anchor = trail.iter().rev().find(|entry| {
            !entry.autoplay && !is_rejected(entry.song) && !seen.contains(entry.song.id.as_str())
        });
        if let (Some(anchor), Some(oldest)) = (anchor, picked.last_mut()) {
            *oldest = *anchor;
        }
    }
    picked.into_iter().map(|seed| seed.song.clone()).collect()
}

/// At most `want` of `songs`, in their order, with at most `per_artist`
/// songs by any one artist (the instant mix's artist key: the id, else the
/// name). When that leaves fewer than `want`, the songs the cap skipped
/// fill up to `want`, earliest first, so a batch is only short when
/// `songs` is.
pub fn cap_per_artist(songs: Vec<Song>, per_artist: usize, want: usize) -> Vec<Song> {
    let mut kept = vec![false; songs.len()];
    let mut taken = 0;
    {
        let mut per: HashMap<&str, usize> = HashMap::new();
        for (i, song) in songs.iter().enumerate() {
            let count = per.entry(artist_key(song)).or_default();
            if *count < per_artist {
                *count += 1;
                kept[i] = true;
                taken += 1;
            }
        }
    }
    for flag in &mut kept {
        if taken >= want {
            break;
        }
        if !*flag {
            *flag = true;
            taken += 1;
        }
    }
    songs
        .into_iter()
        .zip(kept)
        .filter_map(|(song, keep)| keep.then_some(song))
        .take(want)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by(id: &str, artist: &str) -> Song {
        let mut s = Song::test_default(id, id);
        s.artist = artist.to_owned();
        s.artist_id = Some(format!("ar-{artist}"));
        s
    }

    fn ids(list: &[Song]) -> Vec<&str> {
        list.iter().map(|s| s.id.as_str()).collect()
    }

    fn trail<'a>(songs: &'a [(Song, bool)]) -> Vec<TrailSong<'a>> {
        songs
            .iter()
            .map(|(song, autoplay)| TrailSong {
                song,
                autoplay: *autoplay,
            })
            .collect()
    }

    fn user(id: &str) -> (Song, bool) {
        (Song::test_default(id, id), false)
    }

    fn auto(id: &str) -> (Song, bool) {
        (Song::test_default(id, id), true)
    }

    #[test]
    fn wants_fill_truth_table() {
        let w = AUTOPLAY_LOW_WATERMARK;
        assert!(autoplay_wants_fill(true, true, Some(0)));
        assert!(autoplay_wants_fill(true, true, Some(w)));
        assert!(!autoplay_wants_fill(true, true, Some(w + 1)), "runway long");
        assert!(!autoplay_wants_fill(true, true, None), "no current row");
        assert!(!autoplay_wants_fill(false, true, Some(0)), "off");
        assert!(!autoplay_wants_fill(true, false, Some(0)), "repeat on");
    }

    #[test]
    fn wants_resume_only_with_no_current_row() {
        assert!(autoplay_wants_resume(true, true, None));
        assert!(!autoplay_wants_resume(true, true, Some(0)));
        assert!(!autoplay_wants_resume(false, true, None));
        assert!(!autoplay_wants_resume(true, false, None));
    }

    #[test]
    fn seeds_come_from_the_tail_distinct_and_newest_first() {
        let songs = [user("a"), user("b"), user("c"), user("b"), user("d")];
        let out = pick_autoplay_seeds(&trail(&songs), 3);
        assert_eq!(ids(&out), vec!["d", "b", "c"], "b counted once");
    }

    #[test]
    fn seeds_skip_one_star_songs() {
        let mut one = Song::test_default("x", "x");
        one.rating = Some(1);
        let songs = [user("a"), user("b"), (one, false), user("c")];
        let out = pick_autoplay_seeds(&trail(&songs), 3);
        assert_eq!(ids(&out), vec!["c", "b", "a"]);
    }

    #[test]
    fn seeds_from_a_short_or_empty_trail() {
        assert!(pick_autoplay_seeds(&[], 3).is_empty());
        let songs = [user("a")];
        assert_eq!(ids(&pick_autoplay_seeds(&trail(&songs), 3)), vec!["a"]);
        assert!(pick_autoplay_seeds(&trail(&songs), 0).is_empty());
    }

    #[test]
    fn an_all_autoplay_pick_gives_its_oldest_seed_to_the_last_user_song() {
        let songs = [
            user("u1"),
            user("u2"),
            auto("a1"),
            auto("a2"),
            auto("a3"),
            auto("a4"),
        ];
        let out = pick_autoplay_seeds(&trail(&songs), 3);
        assert_eq!(ids(&out), vec!["a4", "a3", "u2"]);
    }

    #[test]
    fn the_anchor_rule_stays_out_when_a_user_song_is_picked_or_none_exists() {
        let songs = [user("u1"), auto("a1"), user("u2"), auto("a2"), auto("a3")];
        let out = pick_autoplay_seeds(&trail(&songs), 3);
        assert_eq!(ids(&out), vec!["a3", "a2", "u2"], "u2 is picked anyway");

        let songs = [auto("a1"), auto("a2"), auto("a3"), auto("a4")];
        let out = pick_autoplay_seeds(&trail(&songs), 3);
        assert_eq!(
            ids(&out),
            vec!["a4", "a3", "a2"],
            "no user song to anchor on"
        );

        let mut rejected = Song::test_default("u1", "u1");
        rejected.rating = Some(1);
        let songs = [(rejected, false), auto("a1"), auto("a2"), auto("a3")];
        let out = pick_autoplay_seeds(&trail(&songs), 3);
        assert_eq!(
            ids(&out),
            vec!["a3", "a2", "a1"],
            "a one-star anchor is no anchor"
        );
    }

    #[test]
    fn the_cap_keeps_two_per_artist_in_order() {
        let songs = vec![
            by("a1", "A"),
            by("a2", "A"),
            by("a3", "A"),
            by("b1", "B"),
            by("a4", "A"),
            by("c1", "C"),
        ];
        let out = cap_per_artist(songs, 2, 4);
        assert_eq!(ids(&out), vec!["a1", "a2", "b1", "c1"]);
    }

    #[test]
    fn the_cap_refills_from_skipped_songs_only_to_reach_the_batch() {
        let songs = vec![
            by("a1", "A"),
            by("a2", "A"),
            by("a3", "A"),
            by("b1", "B"),
            by("a4", "A"),
        ];
        let out = cap_per_artist(songs.clone(), 2, 4);
        assert_eq!(
            ids(&out),
            vec!["a1", "a2", "a3", "b1"],
            "a3 refills, in its own place"
        );
        let out = cap_per_artist(songs, 2, 10);
        assert_eq!(ids(&out).len(), 5, "everything when the batch wants more");
    }

    #[test]
    fn the_cap_cuts_to_the_batch_and_handles_nothing() {
        let songs: Vec<Song> = (0..20)
            .map(|i| by(&format!("s{i}"), &format!("{i}")))
            .collect();
        assert_eq!(cap_per_artist(songs, 2, 10).len(), 10);
        assert!(cap_per_artist(Vec::new(), 2, 10).is_empty());
    }

    #[test]
    fn the_cap_reads_the_name_when_there_is_no_artist_id() {
        let mut x1 = Song::test_default("x1", "x1");
        x1.artist = "X".into();
        let mut x2 = x1.clone();
        x2.id = "x2".into();
        let mut x3 = x1.clone();
        x3.id = "x3".into();
        let out = cap_per_artist(vec![x1, x2, x3, by("y", "Y")], 2, 3);
        assert_eq!(ids(&out), vec!["x1", "x2", "y"]);
    }
}
