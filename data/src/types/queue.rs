use serde::{Deserialize, Serialize};

use crate::types::{queue_sort_mode::QueueSortMode, sort_mode::SortMode};

/// Sort preferences for a view (sort mode + ascending/descending)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortPreferences {
    pub sort_mode: SortMode,
    pub sort_ascending: bool,
}

impl SortPreferences {
    pub fn new(sort_mode: SortMode, sort_ascending: bool) -> Self {
        Self {
            sort_mode,
            sort_ascending,
        }
    }
}

/// Queue-specific sort preferences (uses QueueSortMode instead of SortMode)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSortPreferences {
    pub sort_mode: QueueSortMode,
    pub sort_ascending: bool,
}

impl QueueSortPreferences {
    pub fn new(sort_mode: QueueSortMode, sort_ascending: bool) -> Self {
        Self {
            sort_mode,
            sort_ascending,
        }
    }
}

/// One physical queue row: the song's id plus a per-row identity.
///
/// `entry_id` is RUNTIME-ONLY — never serialized (see the custom
/// `Encode`/`Decode` on [`Queue`]). Two rows sharing a `song_id` (duplicate
/// adds, "Play Next" of an already-queued song) get distinct `entry_id`s so
/// per-row removal/drag targets exactly one row. Ids reseed `0..len` on
/// every load and are handed out monotonically by
/// `QueueManager::next_entry_id` afterwards.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueRow {
    pub song_id: String,
    pub entry_id: u64,
}

/// A permutation of `0..len` mapping play-order position → row index.
///
/// The inner Vec is PRIVATE: every constructor and mutator preserves the
/// permutation invariant, so an out-of-range or duplicated index cannot be
/// constructed through this API. The one documented exception is
/// [`PlayOrder::from_raw_unvalidated`] on the bincode decode path — a
/// persisted blob may be arbitrary bytes, and `reconcile_loaded_queue`
/// validates/repairs it on load before any consumer sees it (rejecting at
/// decode time would turn a repairable order into a dropped queue).
///
/// Reads go through `Deref<Target = [usize]>` (indexing, `len`, `get`,
/// `iter`, …) — the `PagedBuffer` pattern; there is deliberately no
/// `DerefMut`.
///
/// Wire format: encodes exactly like the inner `Vec<usize>` (locked by
/// `queue_bincode_golden_bytes`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayOrder(Vec<usize>);

impl PlayOrder {
    /// The identity permutation `[0, 1, …, len-1]` (shuffle off).
    pub fn identity(len: usize) -> Self {
        Self((0..len).collect())
    }

    /// Wrap raw indices WITHOUT validating the permutation invariant.
    /// Decode/repair path ONLY (see the type docs) — every other
    /// construction goes through the validated constructors.
    pub(crate) fn from_raw_unvalidated(raw: Vec<usize>) -> Self {
        Self(raw)
    }

    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }

    /// Fisher-Yates shuffle with the honest-shuffle anchor semantics the
    /// owner accepted as correct: when `anchor` names the current play-order
    /// position, that row moves to `order[0]` ("already played") and only
    /// the tail is shuffled; with no anchor the whole order shuffles.
    /// Returns the new cursor position (`Some(0)` when anchored). A 0/1-row
    /// order is untouched and the cursor passes through unchanged.
    pub(crate) fn shuffle_anchored<R: rand::RngExt>(
        &mut self,
        anchor: Option<usize>,
        rng: &mut R,
    ) -> Option<usize> {
        use rand::seq::SliceRandom;
        if self.0.len() <= 1 {
            return anchor;
        }
        match anchor {
            Some(cur) => {
                // Move current to front, shuffle the rest. The final
                // write-back is defensive (swap already placed it).
                let cur_row = self.0[cur];
                self.0.swap(0, cur);
                self.0[1..].shuffle(rng);
                self.0[0] = cur_row;
                Some(0)
            }
            None => {
                self.0.shuffle(rng);
                None
            }
        }
    }

    /// `true` when `slice` is exactly a full permutation of `0..len` (no
    /// out-of-range entry, no duplicate, correct length). The single
    /// validator shared by the newtype's own mutators and the load-repair
    /// path (`reconcile_loaded_queue`).
    pub(crate) fn is_full_permutation(slice: &[usize], len: usize) -> bool {
        slice.len() == len && {
            let mut seen = vec![false; len];
            slice
                .iter()
                .all(|&o| o < len && !std::mem::replace(&mut seen[o], true))
        }
    }

    /// Replace the permutation with `candidate` if it is a FULL permutation
    /// of `0..len`; returns `false` (leaving self untouched) otherwise so
    /// the caller can fall back to identity. Used by the move-under-shuffle
    /// splice that reproduces a captured play sequence.
    pub(crate) fn splice_from_play_sequence(&mut self, candidate: Vec<usize>, len: usize) -> bool {
        let valid = Self::is_full_permutation(&candidate, len);
        if valid {
            self.0 = candidate;
        }
        valid
    }

    /// Remove the entry pointing at `removed_row` (returning its play-order
    /// position, `None` if absent) and shift every index above `removed_row`
    /// down by one — the row vector just lost that slot. The shift runs even
    /// when the entry is absent, mirroring the historical repair behavior.
    pub(crate) fn remove_row(&mut self, removed_row: usize) -> Option<usize> {
        let order_pos = self.0.iter().position(|&o| o == removed_row);
        if let Some(pos) = order_pos {
            self.0.remove(pos);
        }
        for entry in &mut self.0 {
            if *entry > removed_row {
                *entry -= 1;
            }
        }
        order_pos
    }

    /// Append entries for freshly-pushed rows. `new_indices` must be
    /// `old_rows_len..new_rows_len`. With `shuffled_after: Some(pos)` each
    /// new index lands at a random play-order slot at or after `pos` (the
    /// "upcoming" portion under shuffle); with `None` they extend in order.
    pub(crate) fn extend_rows<R: rand::RngExt>(
        &mut self,
        new_indices: std::ops::Range<usize>,
        shuffled_after: Option<usize>,
        rng: &mut R,
    ) {
        debug_assert_eq!(
            new_indices.start,
            self.0.len(),
            "extend_rows must be fed the fresh tail of the row vector"
        );
        // Appending fresh tail indices is exactly an insert_rows at the old
        // length: no existing index shifts, and the same random-slot /
        // sequential placement applies. One implementation, one
        // randomization semantic.
        let count = new_indices.len();
        let _ = self.insert_rows(new_indices.start, count, shuffled_after, rng);
    }

    /// Handle a row-vector insertion of `count` rows at `insert_pos`:
    /// shift existing indices >= `insert_pos` up, then insert the new
    /// indices — at random slots at or after `shuffled_after` when `Some`
    /// (shuffle mode), or at the matching sequential position when `None`.
    /// Returns the play-order positions the inserts landed at, in
    /// application order, so the caller can replay cursor adjustments.
    pub(crate) fn insert_rows<R: rand::RngExt>(
        &mut self,
        insert_pos: usize,
        count: usize,
        shuffled_after: Option<usize>,
        rng: &mut R,
    ) -> Vec<usize> {
        for entry in &mut self.0 {
            if *entry >= insert_pos {
                *entry += count;
            }
        }
        let mut applied = Vec::with_capacity(count);
        match shuffled_after {
            Some(insert_after) => {
                for i in 0..count {
                    let insert_at = if insert_after < self.0.len() {
                        rng.random_range(insert_after..=self.0.len())
                    } else {
                        self.0.len()
                    };
                    self.0.insert(insert_at, insert_pos + i);
                    applied.push(insert_at);
                }
            }
            None => {
                let order_insert = self
                    .0
                    .iter()
                    .position(|&o| o >= insert_pos + count)
                    .unwrap_or(self.0.len());
                for i in 0..count {
                    self.0.insert(order_insert + i, insert_pos + i);
                    applied.push(order_insert + i);
                }
            }
        }
        applied
    }

    /// Test-only escape hatch for corrupting the permutation, so sentinel
    /// tests can prove the guards bite. Unreachable from production code.
    #[cfg(test)]
    pub(crate) fn corrupt_push_for_test(&mut self, value: usize) {
        self.0.push(value);
    }
}

impl std::ops::Deref for PlayOrder {
    type Target = [usize];
    fn deref(&self) -> &[usize] {
        &self.0
    }
}

impl PartialEq<Vec<usize>> for PlayOrder {
    fn eq(&self, other: &Vec<usize>) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
impl From<Vec<usize>> for PlayOrder {
    fn from(raw: Vec<usize>) -> Self {
        Self(raw)
    }
}

impl bincode_next::Encode for PlayOrder {
    fn encode<E: bincode_next::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode_next::error::EncodeError> {
        self.0.encode(encoder)
    }
}

impl<C> bincode_next::Decode<C> for PlayOrder {
    fn decode<D: bincode_next::de::Decoder<Context = C>>(
        decoder: &mut D,
    ) -> Result<Self, bincode_next::error::DecodeError> {
        // Raw wrap — validated by reconcile_loaded_queue on load (type docs).
        Ok(Self::from_raw_unvalidated(bincode_next::Decode::decode(
            decoder,
        )?))
    }
}

/// The playback queue — lightweight ordering and mode state.
///
/// Song data lives in `SongPool`; this struct holds only the ordered list of
/// rows (song id + runtime `entry_id`), the play-order cursor (from which
/// the physical playback index derives), and mode flags. Serialization cost
/// is proportional to the number of IDs × UUID length (~100 KB at 12k
/// tracks) rather than full `Song` structs (~5 MB).
///
/// The `order` array maps play-order positions to `rows` indices.
/// When shuffle is off, `order` is identity `[0, 1, 2, …]`.
/// When shuffle is on, `order` is Fisher-Yates shuffled.
/// `current_order` tracks position within `order`.
/// `queued` holds the order-index of the pre-buffered next song (for gapless/crossfade).
///
/// ## Wire format (frozen)
///
/// Persisted as bincode under the `queue_order` redb key. The hand-written
/// `Encode`/`Decode` below keep the byte layout IDENTICAL to the historical
/// derive on the old 8-field struct (`song_ids: Vec<String>` first, then the
/// remaining 7 fields in declaration order) — `entry_id` never reaches the
/// wire. Locked by `queue_bincode_golden_bytes`; a diff there is a
/// saved-queue data-loss bug to fix HERE, never by re-blessing the constant.
#[derive(Debug, Clone, PartialEq)]
pub struct Queue {
    pub rows: Vec<QueueRow>,
    /// Position in the `order` array (NOT in `rows`). The PHYSICAL playhead
    /// is derived: [`Queue::current_index`] returns `order[current_order]`,
    /// so the two can no longer disagree (I3 structural). Wire slot #2
    /// still carries the derived index for byte-compat (see Encode/Decode).
    pub current_order: Option<usize>,
    /// Maps play-order → `rows` index. Identity when shuffle is off.
    pub order: PlayOrder,
    /// Order-index of the pre-buffered next song (gapless/crossfade prep).
    /// Set by `peek_next_song()`, consumed by `PeekedQueue::transition()`.
    pub queued: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub consume: bool,
}

impl bincode_next::Encode for Queue {
    fn encode<E: bincode_next::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode_next::error::EncodeError> {
        // Field #1: ONLY the song ids, emitted as the old `song_ids`
        // leading Vec<String> field — length prefix then each id, exactly
        // what Vec's Encode produces. `entry_id` is runtime-only.
        (self.rows.len() as u64).encode(encoder)?;
        for row in &self.rows {
            row.song_id.encode(encoder)?;
        }
        // Wire slot #2: the DERIVED physical index. Valid saved states
        // satisfy order[current_order] == current_index, so this emits the
        // exact bytes the historical stored field carried.
        self.current_index().encode(encoder)?;
        self.current_order.encode(encoder)?;
        self.order.encode(encoder)?;
        self.queued.encode(encoder)?;
        self.shuffle.encode(encoder)?;
        self.repeat.encode(encoder)?;
        self.consume.encode(encoder)?;
        Ok(())
    }
}

impl<C> bincode_next::Decode<C> for Queue {
    fn decode<D: bincode_next::de::Decoder<Context = C>>(
        decoder: &mut D,
    ) -> Result<Self, bincode_next::error::DecodeError> {
        let song_ids: Vec<String> = bincode_next::Decode::decode(decoder)?;
        // Placeholder entry_ids: QueueManager::new reseeds them 0..len AFTER
        // reconcile_loaded_queue prunes rows, so these never leak.
        let rows = song_ids
            .into_iter()
            .enumerate()
            .map(|(i, song_id)| QueueRow {
                song_id,
                entry_id: i as u64,
            })
            .collect();
        // Wire slot #2 is the legacy stored physical index. The playhead
        // truth is now order[current_order]; for valid saves the two agree
        // (I3 held at save time), so the slot is read and discarded.
        // reconcile_loaded_queue re-derives and repairs on load.
        let _legacy_current_index: Option<usize> = bincode_next::Decode::decode(decoder)?;
        Ok(Queue {
            rows,
            current_order: bincode_next::Decode::decode(decoder)?,
            order: PlayOrder::decode(decoder)?,
            queued: bincode_next::Decode::decode(decoder)?,
            shuffle: bincode_next::Decode::decode(decoder)?,
            repeat: bincode_next::Decode::decode(decoder)?,
            consume: bincode_next::Decode::decode(decoder)?,
        })
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    bincode_next::Encode,
    bincode_next::Decode,
)]
pub enum RepeatMode {
    None,
    Track,
    Playlist,
}

impl Queue {
    /// The physical row index of the playing song, DERIVED from the
    /// play-order cursor: `order[current_order]`. There is no independently
    /// writable stored index to disagree with the cursor, so the I3
    /// coupling (`order[current_order] == current_index`) is
    /// unrepresentable-when-broken.
    pub fn current_index(&self) -> Option<usize> {
        self.current_order
            .and_then(|co| self.order.get(co).copied())
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            current_order: None,
            order: PlayOrder::identity(0),
            queued: None,
            shuffle: false,
            repeat: RepeatMode::None,
            consume: false,
        }
    }
}

/// Where a multi-row drag-reorder should land. The target is named by the
/// `entry_id` of the row to insert above (drift-immune across optimistic UI
/// mutations) rather than a raw index that may have shifted in the window
/// between UI dispatch and backend ack.
#[derive(Debug, Clone, Copy)]
pub enum MoveBatchTarget {
    /// Insert the moved rows immediately above the row with this `entry_id`.
    /// If the entry_id is itself among the moved rows, the move resolves
    /// against the entry's current position before the descending-removal
    /// pass — i.e. the moved block lands where that entry sat.
    AboveEntry(u64),
    /// Append the moved rows at the end of the queue.
    End,
}

#[cfg(test)]
mod play_order_tests {
    use proptest::prelude::*;
    use rand::{RngExt, SeedableRng, rngs::StdRng, seq::SliceRandom};

    use super::*;

    /// Literal port of the pre-PlayOrder `shuffle_order` body (order.rs @
    /// M4) — the owner-blessed honest-shuffle distribution. The newtype's
    /// `shuffle_anchored` must reproduce it byte-for-byte for the same RNG
    /// stream.
    fn legacy_shuffle_anchored(
        order: &mut [usize],
        anchor: Option<usize>,
        rng: &mut StdRng,
    ) -> Option<usize> {
        if order.len() <= 1 {
            return anchor;
        }
        match anchor {
            Some(cur) => {
                let cur_row = order[cur];
                order.swap(0, cur);
                order[1..].shuffle(rng);
                order[0] = cur_row;
                Some(0)
            }
            None => {
                order.shuffle(rng);
                None
            }
        }
    }

    #[test]
    fn shuffle_anchored_matches_legacy_distribution() {
        for seed in 0..64u64 {
            for anchor in [None, Some(0), Some(3), Some(7)] {
                let base: Vec<usize> = (0..8).collect();
                let mut legacy = base.clone();
                let legacy_cur =
                    legacy_shuffle_anchored(&mut legacy, anchor, &mut StdRng::seed_from_u64(seed));

                let mut po = PlayOrder::from_raw_unvalidated(base);
                let new_cur = po.shuffle_anchored(anchor, &mut StdRng::seed_from_u64(seed));

                assert_eq!(
                    po.as_slice(),
                    legacy.as_slice(),
                    "diverged from legacy shuffle at seed {seed}, anchor {anchor:?}"
                );
                assert_eq!(new_cur, legacy_cur, "cursor semantics diverged");
                if let Some(a) = anchor {
                    // base is the identity permutation, so the row at the
                    // anchored position is `a` itself.
                    assert_eq!(po[0], a, "anchored row must sit at order[0]");
                }
            }
        }
        // 0/1-length orders are untouched; the cursor passes through.
        let mut po = PlayOrder::identity(1);
        assert_eq!(
            po.shuffle_anchored(Some(0), &mut StdRng::seed_from_u64(1)),
            Some(0)
        );
        assert_eq!(po.as_slice(), &[0]);
        let mut po = PlayOrder::identity(0);
        assert_eq!(
            po.shuffle_anchored(None, &mut StdRng::seed_from_u64(1)),
            None
        );
    }

    proptest! {
        /// Random sequences of every PlayOrder mutator always leave a full
        /// permutation of 0..len — the structural I2 guarantee.
        #[test]
        fn play_order_ops_preserve_permutation(
            ops in proptest::collection::vec(0u8..5, 1..40),
            seed in any::<u64>(),
        ) {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut len = 6usize;
            let mut po = PlayOrder::identity(len);
            for op in ops {
                match op {
                    0 => {
                        let anchor = if len > 0 && rng.random_range(0..2u8) == 0 {
                            Some(rng.random_range(0..len))
                        } else {
                            None
                        };
                        let _ = po.shuffle_anchored(anchor, &mut rng);
                    }
                    1 => {
                        if len > 0 {
                            let row = rng.random_range(0..len);
                            let removed = po.remove_row(row);
                            prop_assert!(removed.is_some(), "every row is in the order");
                            len -= 1;
                        }
                    }
                    2 => {
                        let count = rng.random_range(1..4usize);
                        let shuffled_after = (rng.random_range(0..2u8) == 0)
                            .then(|| rng.random_range(0..=len));
                        po.extend_rows(len..len + count, shuffled_after, &mut rng);
                        len += count;
                    }
                    3 => {
                        let count = rng.random_range(1..4usize);
                        let pos = rng.random_range(0..=len);
                        let shuffled_after = (rng.random_range(0..2u8) == 0)
                            .then(|| rng.random_range(0..=len));
                        let applied = po.insert_rows(pos, count, shuffled_after, &mut rng);
                        prop_assert_eq!(applied.len(), count);
                        len += count;
                    }
                    _ => {
                        if rng.random_range(0..2u8) == 0 {
                            let mut cand: Vec<usize> = (0..len).collect();
                            cand.shuffle(&mut rng);
                            prop_assert!(po.splice_from_play_sequence(cand, len));
                        } else {
                            // Junk candidates are rejected and self is untouched.
                            let before = po.to_vec();
                            let junk = vec![0usize; len + 1];
                            prop_assert!(!po.splice_from_play_sequence(junk, len));
                            prop_assert_eq!(po.as_slice(), before.as_slice());
                        }
                    }
                }
                prop_assert_eq!(po.len(), len);
                let mut seen = vec![false; len];
                for &o in po.iter() {
                    prop_assert!(o < len, "index {} out of range {}", o, len);
                    prop_assert!(!seen[o], "index {} duplicated", o);
                    seen[o] = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    /// The canonical wire fixture: exercises every field with non-default
    /// values (shuffled order, split playhead positions, all flags).
    /// `order[current_order] == current_index` (1 == order[0]) so the fixture
    /// is a VALID saved state — required for the Phase-2 derived-slot encode
    /// to reproduce these exact bytes.
    fn golden_fixture_queue() -> Queue {
        Queue {
            // entry_ids 0..len match the Decode placeholders so the
            // decode(GOLDEN) == fixture assertion covers them too.
            rows: ["a", "b", "c"]
                .iter()
                .enumerate()
                .map(|(i, id)| QueueRow {
                    song_id: (*id).to_string(),
                    entry_id: i as u64,
                })
                .collect(),
            // derived current_index() == order[0] == 1 (the legacy
            // fixture's stored Some(1)) — wire slot #2 unchanged.
            current_order: Some(0),
            order: vec![1, 0, 2].into(),
            queued: None,
            shuffle: true,
            repeat: RepeatMode::Playlist,
            consume: true,
        }
    }

    /// GOLDEN captured from the pre-refactor encoder (derived
    /// `bincode_next::Encode` on the 8-field `Queue`, `config::standard()`,
    /// main @ 03eb6ea7). NEVER regenerate — a diff here is a WIRE BREAK to
    /// fix in code, not a constant to re-bless. A stale `app.redb` whose
    /// `queue_order` blob stops decoding silently degrades the user's saved
    /// queue to empty on next launch; this constant is the only tripwire.
    ///
    /// Layout (field declaration order): song_ids as Vec<String>,
    /// current_index, current_order, order as Vec<usize>, queued, shuffle,
    /// repeat, consume.
    const GOLDEN: &[u8] = &[
        3, 1, 97, 1, 98, 1, 99, // song_ids: ["a", "b", "c"]
        1, 1, // current_index: Some(1)
        1, 0, // current_order: Some(0)
        3, 1, 0, 2, // order: [1, 0, 2]
        0, // queued: None
        1, // shuffle: true
        2, // repeat: Playlist
        1, // consume: true
    ];

    /// W1/W2 wire lock: the encoder must reproduce the pre-refactor bytes
    /// EXACTLY, and the pre-refactor bytes must decode back to the same
    /// logical queue — across BOTH refactor phases (`QueueRow` collapse and
    /// the derived playhead). `entry_id` must never gain a byte here.
    #[test]
    fn queue_bincode_golden_bytes() {
        let q = golden_fixture_queue();

        let encoded = bincode_next::encode_to_vec(&q, bincode_next::config::standard())
            .expect("encode golden queue");
        assert_eq!(
            encoded, GOLDEN,
            "Queue encode drifted from the pre-refactor wire bytes — this is \
             a saved-queue data-loss bug; fix the encoder, never the constant"
        );

        let (decoded, consumed): (Queue, usize) =
            bincode_next::decode_from_slice(GOLDEN, bincode_next::config::standard())
                .expect("decode golden bytes");
        assert_eq!(consumed, GOLDEN.len(), "decode must consume every byte");
        assert_eq!(
            decoded, q,
            "pre-refactor bytes no longer decode to the same logical queue"
        );
    }

    /// Companion lock with ASYMMETRIC boolean flags (`shuffle=true`,
    /// `consume=false`). The main fixture has `shuffle == consume == true`,
    /// which a hypothetical simultaneous shuffle↔consume transposition in
    /// BOTH Encode and Decode would survive; this fixture breaks that
    /// symmetry so a flag swap cannot slip past the wire lock.
    #[test]
    fn queue_bincode_golden_bytes_asymmetric_flags() {
        const GOLDEN_ASYM: &[u8] = &[
            1, 1, 120, // song_ids: ["x"]
            0,   // current_index: None
            0,   // current_order: None
            1, 0, // order: [0]
            0, // queued: None
            1, // shuffle: true
            0, // repeat: None
            0, // consume: false
        ];
        let q = Queue {
            rows: vec![QueueRow {
                song_id: "x".to_string(),
                entry_id: 0,
            }],
            current_order: None,
            order: vec![0].into(),
            queued: None,
            shuffle: true,
            repeat: RepeatMode::None,
            consume: false,
        };
        let encoded = bincode_next::encode_to_vec(&q, bincode_next::config::standard())
            .expect("encode asymmetric fixture");
        assert_eq!(encoded, GOLDEN_ASYM);
        let (decoded, consumed): (Queue, usize) =
            bincode_next::decode_from_slice(GOLDEN_ASYM, bincode_next::config::standard())
                .expect("decode asymmetric fixture");
        assert_eq!(consumed, GOLDEN_ASYM.len());
        assert_eq!(decoded, q);
    }
}
