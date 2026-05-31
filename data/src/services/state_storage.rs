use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use redb::{Database, ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};

const STATE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("queue");

/// Key under which the user's active multi-library (music folder)
/// selection is persisted. Value is a `HashSet<i32>` (bincode). An
/// empty set is the explicit "no filter" / "show all libraries"
/// default — the same shape the brand-new state reports.
pub const ACTIVE_LIBRARY_IDS_KEY: &str = "active_library_ids";

/// Redb-based storage backend for application state persistence
///
/// This provides ACID transaction guarantees and better performance
/// than JSON file storage for frequent updates.
///
/// Wraps `Database` in `Arc` so the same underlying DB can be shared
/// across multiple managers (e.g. `QueueManager` and `SettingsManager`).
#[derive(Clone)]
pub struct StateStorage {
    db: Arc<Database>,
}

impl StateStorage {
    /// Create or open a state storage database
    pub fn new(path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Database::create(path)?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Save data as JSON (small/debuggable payloads like queue order)
    pub fn save<T: Serialize>(&self, key: &str, data: &T) -> Result<()> {
        let serialized = serde_json::to_vec(data)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(STATE_TABLE)?;
            table.insert(key, serialized.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Load data from JSON
    pub fn load<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        let read_txn = self.db.begin_read()?;

        // If table doesn't exist yet, return None (empty queue)
        let table = match read_txn.open_table(STATE_TABLE) {
            Ok(table) => table,
            Err(_) => return Ok(None),
        };

        match table.get(key)? {
            Some(value) => {
                let bytes: &[u8] = value.value();
                let data = serde_json::from_slice(bytes)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Save data as bincode (large payloads like song pool — ~3× faster, ~3× smaller).
    /// Uses native bincode Encode trait (not serde) to avoid serde-attribute
    /// incompatibilities like `#[serde(untagged)]`.
    pub fn save_binary<T: bincode_next::Encode>(&self, key: &str, data: &T) -> Result<()> {
        let serialized = bincode_next::encode_to_vec(data, bincode_next::config::standard())
            .map_err(|e| anyhow::anyhow!("bincode encode: {e}"))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(STATE_TABLE)?;
            table.insert(key, serialized.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Atomically persist several bincode-encoded payloads in a SINGLE redb
    /// write transaction. Either every entry commits together or none does —
    /// a crash/kill mid-batch can never leave a torn (some-new, some-stale)
    /// snapshot on disk.
    ///
    /// Callers encode each value to `Vec<u8>` (via
    /// `bincode_next::encode_to_vec(.., bincode_next::config::standard())`,
    /// matching [`Self::save_binary`]'s on-disk format) and pass the
    /// `(key, bytes)` pairs. Used by `QueueManager::save_all` to commit the
    /// queue ORDER and SONG-POOL blobs together so a reload always sees a
    /// consistent pair.
    pub fn save_binary_batch(&self, entries: &[(&str, &[u8])]) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(STATE_TABLE)?;
            for (key, bytes) in entries {
                table.insert(*key, *bytes)?;
            }
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Load data from bincode (native Decode trait).
    pub fn load_binary<T: bincode_next::Decode<()>>(&self, key: &str) -> Result<Option<T>> {
        let read_txn = self.db.begin_read()?;

        let table = match read_txn.open_table(STATE_TABLE) {
            Ok(table) => table,
            Err(_) => return Ok(None),
        };

        match table.get(key)? {
            Some(value) => {
                let bytes: &[u8] = value.value();
                let (data, _) = bincode_next::decode_from_slice::<T, _>(
                    bytes,
                    bincode_next::config::standard(),
                )
                .map_err(|e| anyhow::anyhow!("bincode decode: {e}"))?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(
        Debug, Clone, PartialEq, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode,
    )]
    struct TestQueue {
        songs: Vec<String>,
        index: usize,
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_queue.redb");

        // Clean up any existing test db
        let _ = std::fs::remove_file(&db_path);

        let storage = StateStorage::new(db_path.clone()).unwrap();

        let test_queue = TestQueue {
            songs: vec!["song1".to_string(), "song2".to_string()],
            index: 1,
        };

        // Save
        storage.save("current_queue", &test_queue).unwrap();

        // Load
        let loaded: Option<TestQueue> = storage.load("current_queue").unwrap();
        assert_eq!(loaded, Some(test_queue));

        // Clean up
        std::fs::remove_file(db_path).unwrap();
    }

    #[test]
    fn test_binary_round_trip() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_binary_rt.redb");
        let _ = std::fs::remove_file(&db_path);

        let storage = StateStorage::new(db_path.clone()).unwrap();

        let data = TestQueue {
            songs: vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            index: 2,
        };

        // Save as bincode
        storage.save_binary("pool", &data).unwrap();

        // Load via load_binary
        let loaded: Option<TestQueue> = storage.load_binary("pool").unwrap();
        assert_eq!(loaded, Some(data));

        std::fs::remove_file(db_path).unwrap();
    }

    #[test]
    fn save_binary_batch_is_atomic_round_trip() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_binary_batch.redb");
        let _ = std::fs::remove_file(&db_path);

        let storage = StateStorage::new(db_path.clone()).unwrap();

        // ORDER references ids {a, b, c}; POOL contains exactly {a, b, c}.
        let order = TestQueue {
            songs: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            index: 0,
        };
        let pool = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        let order_bytes =
            bincode_next::encode_to_vec(&order, bincode_next::config::standard()).unwrap();
        let pool_bytes =
            bincode_next::encode_to_vec(&pool, bincode_next::config::standard()).unwrap();

        storage
            .save_binary_batch(&[
                ("batch_order", order_bytes.as_slice()),
                ("batch_pool", pool_bytes.as_slice()),
            ])
            .unwrap();

        // Reopen the SAME path with a fresh StateStorage.
        drop(storage);
        let storage = StateStorage::new(db_path.clone()).unwrap();

        let loaded_order: TestQueue = storage.load_binary("batch_order").unwrap().unwrap();
        let loaded_pool: Vec<String> = storage.load_binary("batch_pool").unwrap().unwrap();

        // (1) Every id referenced by the loaded order resolves in the loaded pool.
        for id in &loaded_order.songs {
            assert!(
                loaded_pool.contains(id),
                "dangling reference: {id} not in pool",
            );
        }
        // (2) Both loaded blobs equal what was written.
        assert_eq!(loaded_order, order);
        assert_eq!(loaded_pool, pool);

        std::fs::remove_file(db_path).unwrap();
    }
}
