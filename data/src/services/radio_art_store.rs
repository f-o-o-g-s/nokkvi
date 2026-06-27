//! Bounded on-disk cache for remembered radio now-playing (ICY) artwork.
//!
//! Radio station lists are small (a few dozen), so rather than a separate cache
//! directory + file index (with all the missing-file / ext-sniffing drift that
//! invites), the image BYTES live directly in a single bincode blob under the
//! shared [`StateStorage`] redb (key [`RADIO_ART_INDEX`]), keyed by station id.
//! This is atomic (no file/index skew), bounded by BOTH entry count and total
//! bytes (LRU by fetch recency), and namespaced by server URL so switching
//! servers never surfaces another server's art.
//!
//! Only logo-LESS stations' last-played stream art is stored here; stations with
//! an admin-uploaded `coverArt` logo re-fetch that cheaply each session and are
//! never persisted (the UI gates logo stations out of both the capture trigger
//! and the persist in `update::radio_artwork`).

use std::collections::HashMap;

use anyhow::Result;
use bincode_next::{Decode, Encode};

use crate::services::{state_storage::StateStorage, storage_keys::RADIO_ART_INDEX};

/// Max number of stations whose art is retained on disk.
const MAX_ENTRIES: usize = 96;
/// Max total bytes retained on disk across all entries (belt-and-braces with
/// `MAX_ENTRIES` so an unusually large image can't blow up the blob).
const MAX_TOTAL_BYTES: usize = 48 * 1024 * 1024;

/// Pre-`_v2` redb key. A short-lived earlier build used this key; its blob is
/// merged forward into [`RADIO_ART_INDEX`] once (then deleted) so remembered
/// art isn't lost across the key change. See [`RadioArtStore::load_migrating`].
const LEGACY_RADIO_ART_INDEX: &str = "radio_art_index";

/// Serializes the whole-blob read-modify-write across every `RadioArtStore`
/// (each `new()` wraps the same shared redb). `load_binary` (a read txn) and
/// `save_binary` (a write txn) are two separate transactions, so without this a
/// concurrent `put` / `remove_station` / `load_migrating` could read a stale
/// blob and clobber another's entry (last-writer-wins). The lock is held only
/// for the brief synchronous RMW, never across an `.await`.
static BLOB_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`BLOB_WRITE_LOCK`], recovering the guard if a previous holder
/// panicked (a poisoned `()` lock carries no invalid state to protect against).
fn blob_write_guard() -> std::sync::MutexGuard<'static, ()> {
    BLOB_WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Encode, Decode)]
struct RadioArtEntry {
    /// The ICY `StreamUrl` the bytes were fetched from — the dedup key the UI
    /// compares against to avoid re-fetching the same now-playing image.
    source_url: String,
    /// The raw image bytes.
    bytes: Vec<u8>,
    /// Unix seconds when fetched — drives LRU-by-recency eviction.
    fetched_at_unix: u64,
}

#[derive(Default, Encode, Decode)]
struct RadioArtBlob {
    /// Server URL these entries belong to; a mismatch invalidates them all.
    server: String,
    /// `station_id -> entry`.
    entries: HashMap<String, RadioArtEntry>,
}

/// One remembered station-art record handed back to the UI for hydration.
pub struct RadioArtRecord {
    pub station_id: String,
    pub source_url: String,
    pub bytes: Vec<u8>,
}

/// Bounded on-disk store for remembered radio (ICY) artwork. Cheap to clone
/// (wraps the `Arc`-backed [`StateStorage`]).
#[derive(Clone)]
pub struct RadioArtStore {
    storage: StateStorage,
}

impl RadioArtStore {
    pub fn new(storage: StateStorage) -> Self {
        Self { storage }
    }

    fn load_blob_key(&self, key: &str) -> RadioArtBlob {
        self.storage
            .load_binary::<RadioArtBlob>(key)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    fn load_blob(&self) -> RadioArtBlob {
        self.load_blob_key(RADIO_ART_INDEX)
    }

    /// Load all remembered art for `server`. Returns empty when the stored blob
    /// belongs to a different server (no cross-server bleed).
    pub fn load_for_server(&self, server: &str) -> Vec<RadioArtRecord> {
        Self::blob_records(self.load_blob(), server)
    }

    /// One-time forward migration of the pre-`_v2` blob, then load. Merges any
    /// [`LEGACY_RADIO_ART_INDEX`] entries (for the same server) into the current
    /// blob without overwriting newer ones, persists, deletes the legacy key,
    /// and returns the merged records. Idempotent: once the legacy key is gone
    /// this is just [`Self::load_for_server`]. Returns the migrated count too,
    /// for logging.
    pub fn load_migrating(&self, server: &str) -> (Vec<RadioArtRecord>, usize) {
        let _guard = blob_write_guard();
        let legacy = self.load_blob_key(LEGACY_RADIO_ART_INDEX);
        let mut migrated = 0usize;
        // Only migrate + delete when the legacy blob is for THIS server — a
        // different-server legacy is left untouched (no cross-server data loss;
        // it migrates if/when that server is active again).
        if !legacy.entries.is_empty() && legacy.server == server {
            let mut current = self.load_blob();
            if current.server != server {
                current = RadioArtBlob {
                    server: server.to_string(),
                    entries: HashMap::new(),
                };
            }
            for (id, entry) in legacy.entries {
                if let std::collections::hash_map::Entry::Vacant(slot) = current.entries.entry(id) {
                    slot.insert(entry);
                    migrated += 1;
                }
            }
            Self::evict(&mut current);
            // Delete the legacy blob ONLY after the forward write is durable —
            // a failed save must leave it intact for a clean retry next launch.
            if self.storage.save_binary(RADIO_ART_INDEX, &current).is_ok() {
                let _ = self.storage.remove(LEGACY_RADIO_ART_INDEX);
            } else {
                // Nothing was durably migrated; report 0 so the caller's log
                // doesn't claim a migration that the retry next launch will redo.
                migrated = 0;
            }
        }
        (self.load_for_server(server), migrated)
    }

    fn blob_records(blob: RadioArtBlob, server: &str) -> Vec<RadioArtRecord> {
        if blob.server != server {
            return Vec::new();
        }
        blob.entries
            .into_iter()
            .map(|(station_id, e)| RadioArtRecord {
                station_id,
                source_url: e.source_url,
                bytes: e.bytes,
            })
            .collect()
    }

    /// Forget the remembered art for one station (e.g. a user "refresh artwork"
    /// to clear a stale/wrong thumbnail). No-op when absent or the server
    /// differs.
    pub fn remove_station(&self, server: &str, station_id: &str) -> Result<()> {
        let _guard = blob_write_guard();
        let mut blob = self.load_blob();
        if blob.server != server {
            return Ok(());
        }
        if blob.entries.remove(station_id).is_some() {
            self.storage.save_binary(RADIO_ART_INDEX, &blob)?;
        }
        Ok(())
    }

    /// Persist `bytes` as the remembered art for `station_id` on `server`,
    /// fetched from `source_url` at `now_unix`. Bounds the store by entry count
    /// and total bytes, evicting least-recently-fetched entries; switching
    /// servers resets the blob.
    pub fn put(
        &self,
        server: &str,
        station_id: &str,
        source_url: &str,
        bytes: &[u8],
        now_unix: u64,
    ) -> Result<()> {
        let _guard = blob_write_guard();
        let mut blob = self.load_blob();
        if blob.server != server {
            blob = RadioArtBlob {
                server: server.to_string(),
                entries: HashMap::new(),
            };
        }
        blob.entries.insert(
            station_id.to_string(),
            RadioArtEntry {
                source_url: source_url.to_string(),
                bytes: bytes.to_vec(),
                fetched_at_unix: now_unix,
            },
        );
        Self::evict(&mut blob);
        self.storage.save_binary(RADIO_ART_INDEX, &blob)
    }

    /// Evict least-recently-fetched entries until within BOTH the entry-count
    /// and total-byte budgets.
    fn evict(blob: &mut RadioArtBlob) {
        let mut total_bytes: usize = blob.entries.values().map(|e| e.bytes.len()).sum();
        if blob.entries.len() <= MAX_ENTRIES && total_bytes <= MAX_TOTAL_BYTES {
            return;
        }
        // Oldest-first eviction order.
        let mut by_age: Vec<(String, u64)> = blob
            .entries
            .iter()
            .map(|(k, e)| (k.clone(), e.fetched_at_unix))
            .collect();
        by_age.sort_by_key(|(_, ts)| *ts);

        for (id, _) in by_age {
            if blob.entries.len() <= MAX_ENTRIES && total_bytes <= MAX_TOTAL_BYTES {
                break;
            }
            if let Some(e) = blob.entries.remove(&id) {
                total_bytes = total_bytes.saturating_sub(e.bytes.len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (RadioArtStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("radio_art_test.redb");
        let storage = StateStorage::new(path).expect("redb open");
        (RadioArtStore::new(storage), dir)
    }

    #[test]
    fn round_trips_and_namespaces_by_server() {
        let (store, _d) = temp_store();
        store.put("srvA", "s1", "http://a/img", b"abc", 1).unwrap();

        let recs = store.load_for_server("srvA");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].station_id, "s1");
        assert_eq!(recs[0].source_url, "http://a/img");
        assert_eq!(recs[0].bytes, b"abc");

        // A different server must see nothing (no cross-server bleed).
        assert!(store.load_for_server("srvB").is_empty());
    }

    #[test]
    fn switching_server_resets_entries() {
        let (store, _d) = temp_store();
        store.put("srvA", "s1", "u", b"x", 1).unwrap();
        store.put("srvB", "s2", "u", b"y", 2).unwrap();

        assert!(
            store.load_for_server("srvA").is_empty(),
            "old server's entries must be dropped on switch"
        );
        let recs = store.load_for_server("srvB");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].station_id, "s2");
    }

    #[test]
    fn evicts_oldest_beyond_entry_cap() {
        let (store, _d) = temp_store();
        // Insert MAX_ENTRIES + 5 with increasing timestamps; the oldest 5 evict.
        for i in 0..(MAX_ENTRIES as u64 + 5) {
            store.put("srv", &format!("s{i}"), "u", b"z", i).unwrap();
        }
        let recs = store.load_for_server("srv");
        assert_eq!(recs.len(), MAX_ENTRIES);

        let ids: std::collections::HashSet<&str> =
            recs.iter().map(|r| r.station_id.as_str()).collect();
        assert!(!ids.contains("s0"), "oldest must be evicted");
        assert!(!ids.contains("s4"), "5th-oldest must be evicted");
        assert!(ids.contains("s5"), "6th-oldest must survive");
    }

    #[test]
    fn evicts_oldest_beyond_byte_budget() {
        let (store, _d) = temp_store();
        // 4 MiB entries (the per-image cap) so the BYTE budget — not the entry
        // cap (MAX_ENTRIES=96) — drives eviction with only a handful of entries.
        let img = vec![0u8; 4 * 1024 * 1024];
        let n = (MAX_TOTAL_BYTES / img.len()) as u64 + 2;
        assert!(
            (n as usize) < MAX_ENTRIES,
            "byte budget, not entry cap, must bind"
        );
        for i in 0..n {
            store.put("srv", &format!("s{i}"), "u", &img, i).unwrap();
        }
        let recs = store.load_for_server("srv");
        let total: usize = recs.iter().map(|r| r.bytes.len()).sum();
        assert!(total <= MAX_TOTAL_BYTES, "must end within the byte budget");
        assert!(
            recs.len() < n as usize,
            "byte budget must have evicted some"
        );
        let ids: std::collections::HashSet<&str> =
            recs.iter().map(|r| r.station_id.as_str()).collect();
        assert!(
            !ids.contains("s0"),
            "oldest must evict first under byte pressure"
        );
    }

    #[test]
    fn updating_a_station_does_not_grow_entry_count() {
        let (store, _d) = temp_store();
        store.put("srv", "s1", "url-v1", b"old", 1).unwrap();
        store.put("srv", "s1", "url-v2", b"new", 2).unwrap();

        let recs = store.load_for_server("srv");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].source_url, "url-v2");
        assert_eq!(recs[0].bytes, b"new");
    }

    fn seed_legacy(store: &RadioArtStore, server: &str, entries: &[(&str, &str, &[u8], u64)]) {
        let map = entries
            .iter()
            .map(|(id, url, bytes, ts)| {
                (
                    (*id).to_string(),
                    RadioArtEntry {
                        source_url: (*url).to_string(),
                        bytes: bytes.to_vec(),
                        fetched_at_unix: *ts,
                    },
                )
            })
            .collect();
        let blob = RadioArtBlob {
            server: server.to_string(),
            entries: map,
        };
        store
            .storage
            .save_binary(LEGACY_RADIO_ART_INDEX, &blob)
            .unwrap();
    }

    #[test]
    fn migrates_legacy_blob_forward_then_deletes_it() {
        let (store, _d) = temp_store();
        seed_legacy(
            &store,
            "srv",
            &[("s1", "u1", b"a", 1), ("s2", "u2", b"b", 2)],
        );

        let (recs, migrated) = store.load_migrating("srv");
        assert_eq!(migrated, 2);
        assert_eq!(recs.len(), 2);

        // Legacy key is gone, so a second migration is a no-op.
        let (recs2, migrated2) = store.load_migrating("srv");
        assert_eq!(migrated2, 0);
        assert_eq!(recs2.len(), 2);
    }

    #[test]
    fn migration_keeps_newer_current_entries() {
        let (store, _d) = temp_store();
        store.put("srv", "s1", "new-url", b"new", 10).unwrap(); // current (v2)
        seed_legacy(
            &store,
            "srv",
            &[("s1", "old", b"old", 1), ("s2", "u2", b"b", 2)],
        );

        let (recs, migrated) = store.load_migrating("srv");
        assert_eq!(migrated, 1, "only s2 is new; current s1 is preserved");
        let s1 = recs.iter().find(|r| r.station_id == "s1").unwrap();
        assert_eq!(
            s1.bytes, b"new",
            "legacy must not overwrite the newer entry"
        );
        assert!(recs.iter().any(|r| r.station_id == "s2"));
    }

    #[test]
    fn migration_skips_other_server_legacy() {
        let (store, _d) = temp_store();
        seed_legacy(&store, "OTHER", &[("s1", "u1", b"a", 1)]);
        let (recs, migrated) = store.load_migrating("srv");
        assert_eq!(migrated, 0);
        assert!(
            recs.is_empty(),
            "legacy from a different server isn't merged"
        );
    }

    #[test]
    fn remove_station_forgets_one_entry() {
        let (store, _d) = temp_store();
        store.put("srv", "s1", "u1", b"a", 1).unwrap();
        store.put("srv", "s2", "u2", b"b", 2).unwrap();

        store.remove_station("srv", "s1").unwrap();
        let recs = store.load_for_server("srv");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].station_id, "s2");

        // Wrong server is a no-op.
        store.remove_station("other", "s2").unwrap();
        assert_eq!(store.load_for_server("srv").len(), 1);
    }

    /// Manual diagnostic: dump a real `app.redb`'s radio-art blobs.
    /// `NOKKVI_REDB=/path/app.redb cargo test -p nokkvi-data dump_radio_art -- --ignored --nocapture`
    #[test]
    #[ignore = "manual diagnostic"]
    fn dump_radio_art_blob() {
        let Ok(path) = std::env::var("NOKKVI_REDB") else {
            eprintln!("set NOKKVI_REDB to the app.redb path");
            return;
        };
        let storage = StateStorage::new(std::path::PathBuf::from(path)).expect("open redb");
        for key in ["radio_art_index", "radio_art_index_v2"] {
            match storage.load_binary::<RadioArtBlob>(key) {
                Ok(Some(blob)) => {
                    let total: usize = blob.entries.values().map(|e| e.bytes.len()).sum();
                    eprintln!(
                        "[{key}] server={:?} entries={} total_bytes={}",
                        blob.server,
                        blob.entries.len(),
                        total
                    );
                    for (id, e) in blob.entries.iter().take(10) {
                        eprintln!("   {id} <- {} ({} bytes)", e.source_url, e.bytes.len());
                    }
                }
                Ok(None) => eprintln!("[{key}] (absent)"),
                Err(e) => eprintln!("[{key}] decode error: {e}"),
            }
        }
    }
}
