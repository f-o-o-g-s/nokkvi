//! OpenSubsonic API keys for `/rest` (the `apiKeyAuthentication` extension).
//!
//! Navidrome (master since 2026-09-28, #6219) authenticates a `/rest` request
//! with `apiKey=<key>` alone; adding `u`, `p`, `t`, `s` or `jwt` is error 43,
//! an unknown key is 44. A key belongs to one player row: the CLIENT mints it
//! (`nds_` + 22 base62 characters, the shape the server enforces), registers
//! it through the native `/api/player` with the password login's JWT, and the
//! server keeps only its SHA-256. A keyed request is bound to that player, so
//! the player's transcoding, bit-rate cap and scrobble settings apply to it.
//!
//! nokkvi keeps one player per install for each session owner (server URL +
//! username), remembered in redb by id. Every password login puts a FRESH key
//! on it (`PUT` replaces the old one, which stops working at once), and so does
//! the first resume of a token session on a server that has started offering
//! keys. An explicit logout revokes the key; a session that expires keeps it
//! on the server until the next login replaces it. Server and user come from
//! [`QueueOwner`], so a key never goes to another server or user.
//!
//! The orchestration lives on [`crate::backend::auth::AuthGateway`]; this module
//! holds the parts that need no session: minting, the player records, the
//! settings a new player starts from, and the lock that orders key writes.

use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tracing::debug;

use crate::services::{
    api::players::{PlayerRow, PlayerSettings},
    auth::{CredentialParts, same_user},
    queue::slots::QueueOwner,
    state_storage::StateStorage,
};

/// The OpenSubsonic extension name a server advertises when it takes keys.
pub const API_KEY_EXTENSION: &str = "apiKeyAuthentication";

/// The client name nokkvi's player carries: the same `c=` every `/rest`
/// request sends.
pub(crate) const PLAYER_CLIENT: &str = "nokkvi";

const KEY_PREFIX: &str = "nds_";
const KEY_BODY_LEN: usize = 22;
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// A random byte maps to `BASE62[b % 62]` only below this bound (62 × 4):
/// `256 % 62 == 8`, so keeping the top 8 values would favour the first eight
/// characters.
const UNBIASED_BELOW: u8 = 248;
/// Draws of 32 bytes before minting gives up (each draw keeps ~31 bytes; the
/// key needs 22). Only a broken random source gets anywhere near it.
const MAX_DRAWS: usize = 16;

/// An OpenSubsonic API key. `Debug` never prints it and there is no
/// `Display`, so it cannot reach a log line by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

impl ApiKey {
    /// Mint a new key from the operating system's random source.
    pub fn mint() -> Result<Self> {
        use rand::TryRng;
        let mut rng = rand::rngs::SysRng;
        Self::from_random(|buf| {
            rng.try_fill_bytes(buf)
                .map_err(|e| anyhow!("the system random number generator failed: {e}"))
        })
    }

    /// Mint from `fill`, which writes random bytes into its buffer. Split
    /// out so the mapping and the bias rejection are testable.
    fn from_random(mut fill: impl FnMut(&mut [u8]) -> Result<()>) -> Result<Self> {
        let mut body = String::with_capacity(KEY_BODY_LEN);
        let mut buf = [0u8; 32];
        for _ in 0..MAX_DRAWS {
            fill(&mut buf)?;
            for &byte in &buf {
                if body.len() == KEY_BODY_LEN {
                    break;
                }
                if byte < UNBIASED_BELOW {
                    body.push(char::from(BASE62[usize::from(byte % 62)]));
                }
            }
            if body.len() == KEY_BODY_LEN {
                return Ok(Self(format!("{KEY_PREFIX}{body}")));
            }
        }
        Err(anyhow!("the random source kept returning unusable bytes"))
    }

    /// The key itself, for a registration body.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `/rest` credential that sends this key: `apiKey=<key>`, with no
    /// `u` (the server answers a key plus any other credential with error
    /// 43). The key is `nds_` plus base62, so it needs no encoding.
    pub fn credential(&self) -> String {
        format!("apiKey={}", self.0)
    }
}

/// Whether a `/rest` credential string sends an API key (`apiKey=<key>`)
/// rather than the token pair.
pub fn is_api_key_credential(credential: &str) -> bool {
    CredentialParts::parse(credential)
        .api_key
        .is_some_and(|key| !key.is_empty())
}

// ── The lock that orders key writes ─────────────────────────────────────────

/// Held across every key write (a login's register or rotate, a logout's
/// revoke). Process-wide because the writes come from different sessions:
/// a logout's revoke must land before the next login puts a new key on the
/// same player, or the revoke would kill the new key.
static KEY_WRITES: LazyLock<Arc<Mutex<()>>> = LazyLock::new(|| Arc::new(Mutex::new(())));

/// Take the key-write lock now, if it is free. A logout calls this
/// synchronously, before any later login can queue for the lock.
pub fn try_claim_key_writes() -> Option<OwnedMutexGuard<()>> {
    KEY_WRITES.clone().try_lock_owned().ok()
}

/// Wait for the key-write lock.
pub async fn claim_key_writes() -> OwnedMutexGuard<()> {
    KEY_WRITES.clone().lock_owned().await
}

/// Serializes the tests that take [`KEY_WRITES`]: the lock is process-wide,
/// so two such tests running in parallel would see each other's claim.
#[cfg(test)]
pub(crate) static KEY_WRITES_TEST_LOCK: Mutex<()> = Mutex::const_new(());

// ── Player records (redb) ───────────────────────────────────────────────────

/// The redb key of the player records.
const PLAYER_RECORDS_KEY: &str = crate::services::storage_keys::API_KEY_PLAYERS;

/// nokkvi's player on one server, for one user. `user_id` is the Navidrome
/// id of the user it was registered for (empty when the session didn't know
/// it), so a record never serves another user who later took the same name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PlayerRecord {
    player_id: String,
    #[serde(default)]
    user_id: String,
}

/// Session owner key ([`QueueOwner::key`]) → that owner's player.
type PlayerRecords = BTreeMap<String, PlayerRecord>;

/// The records. A failed read is an error, since writing over it would drop
/// every other owner's record; a value that doesn't parse reads as none, as
/// nothing in it can be kept (a lost record costs one extra player row on the
/// server, never a failed login).
fn load_records(storage: &StateStorage) -> Result<PlayerRecords> {
    let Some(raw) = storage
        .load_raw(PLAYER_RECORDS_KEY)
        .context("reading the API key player records")?
    else {
        return Ok(PlayerRecords::new());
    };
    Ok(serde_json::from_slice(&raw).unwrap_or_else(|e| {
        debug!(" [API KEY] Player records unreadable, starting over: {e}");
        PlayerRecords::new()
    }))
}

/// A player id is only ever a server-made id; anything else (a hand-edited
/// or damaged record) would end up in a URL path, so it reads as no record.
fn is_plausible_player_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The player nokkvi registered for `owner`, if it remembers one for the user
/// with id `user_id` (an empty id on either side matches). A failed read is
/// an error: taken as "no record", it would make a new player and leave the
/// recorded one's key live and unrevoked.
pub(crate) fn recorded_player_id(
    storage: &StateStorage,
    owner: &QueueOwner,
    user_id: &str,
) -> Result<Option<String>> {
    Ok(load_records(storage)?
        .get(owner.key())
        .filter(|record| {
            record.user_id.is_empty() || user_id.is_empty() || record.user_id == user_id
        })
        .map(|record| record.player_id.clone())
        .filter(|id| is_plausible_player_id(id)))
}

/// Remember `player_id` as `owner`'s player, registered for user `user_id`.
pub(crate) fn record_player_id(
    storage: &StateStorage,
    owner: &QueueOwner,
    player_id: &str,
    user_id: &str,
) -> Result<()> {
    let mut records = load_records(storage)?;
    records.insert(
        owner.key().to_owned(),
        PlayerRecord {
            player_id: player_id.to_owned(),
            user_id: user_id.to_owned(),
        },
    );
    storage
        .save(PLAYER_RECORDS_KEY, &records)
        .context("saving the API key player record")
}

/// Forget `owner`'s player (it is gone from the server, or not theirs).
pub(crate) fn forget_player_id(storage: &StateStorage, owner: &QueueOwner) -> Result<()> {
    let mut records = load_records(storage)?;
    if records.remove(owner.key()).is_some() {
        storage
            .save(PLAYER_RECORDS_KEY, &records)
            .context("saving the API key player records")?;
    }
    Ok(())
}

// ── A new player ────────────────────────────────────────────────────────────

/// The name nokkvi's player gets: `nokkvi (<hostname>)`, so one install's
/// player is told apart from another's in the web UI's Players list.
pub(crate) fn player_name() -> String {
    player_name_for(host_name().as_deref())
}

fn player_name_for(host: Option<&str>) -> String {
    host.map_or_else(
        || PLAYER_CLIENT.to_owned(),
        |host| format!("{PLAYER_CLIENT} ({host})"),
    )
}

/// This machine's hostname, or `None` when it can't be read.
fn host_name() -> Option<String> {
    let raw = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
        .ok()?;
    let host: String = raw.trim().chars().take(64).collect();
    (!host.is_empty()).then_some(host)
}

/// The settings a new player starts from: those of the player Navidrome
/// registered for nokkvi's token requests (client `nokkvi`, no key, and the
/// name `core/players.go` `Register` gives it, `nokkvi [<user agent>]`), the
/// most recently seen one this user owns, so a transcoding profile, bit-rate
/// cap or scrobble switch set there carries over to the keyed player. Another
/// install's keyed player whose key a logout revoked is that install's own and
/// is skipped. Without one, Navidrome's own defaults
/// ([`PlayerSettings::default`]).
pub(crate) fn settings_template(
    rows: &[PlayerRow],
    user_id: &str,
    username: &str,
) -> PlayerSettings {
    rows.iter()
        .filter(|row| {
            row.client == PLAYER_CLIENT
                && !row.has_api_key
                && row.name == format!("{} [{}]", row.client, row.user_agent)
        })
        .filter(|row| {
            // The id when both sides have one; an admin's list holds every
            // user's players, so an empty id must not match everything.
            if !user_id.is_empty() && !row.user_id.is_empty() {
                row.user_id == user_id
            } else {
                !username.is_empty() && same_user(&row.user_name, username)
            }
        })
        .max_by_key(|row| {
            chrono::DateTime::parse_from_rfc3339(&row.last_seen)
                .map_or(i64::MIN, |seen| seen.timestamp_millis())
        })
        .map_or_else(PlayerSettings::default, |row| PlayerSettings {
            transcoding_id: row.transcoding_id.clone(),
            max_bit_rate: row.max_bit_rate,
            report_real_path: row.report_real_path,
            scrobble_enabled: row.scrobble_enabled,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_shape_holds(key: &str) -> bool {
        key.len() == KEY_PREFIX.len() + KEY_BODY_LEN
            && key.starts_with(KEY_PREFIX)
            && key[KEY_PREFIX.len()..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
    }

    /// The server rejects any key not shaped `^nds_[0-9A-Za-z]{22}$`
    /// (`persistence/player_repository.go` `apiKeyFormat`).
    #[test]
    fn minted_keys_have_the_server_shape_and_differ() {
        let a = ApiKey::mint().expect("mint");
        let b = ApiKey::mint().expect("mint");
        assert!(key_shape_holds(a.as_str()), "{}", a.as_str());
        assert!(key_shape_holds(b.as_str()), "{}", b.as_str());
        assert_ne!(a, b);
    }

    /// Bytes from 248 up are dropped, not folded onto the first characters.
    #[test]
    fn biased_bytes_are_skipped() {
        let mut draws = 0;
        let key = ApiKey::from_random(|buf| {
            draws += 1;
            for (i, b) in buf.iter_mut().enumerate() {
                // Every other byte is in the biased range.
                *b = if i % 2 == 0 { 255 } else { 61 };
            }
            Ok(())
        })
        .expect("mints from the unbiased half");
        assert_eq!(key.as_str(), format!("nds_{}", "z".repeat(22)));
        assert_eq!(draws, 2, "16 usable bytes per draw, so two draws");
    }

    #[test]
    fn a_dead_random_source_fails_instead_of_looping() {
        assert!(
            ApiKey::from_random(|buf| {
                buf.fill(250);
                Ok(())
            })
            .is_err()
        );
        assert!(ApiKey::from_random(|_| Err(anyhow!("no entropy"))).is_err());
    }

    #[test]
    fn the_key_never_shows_in_debug() {
        let key = ApiKey::mint().expect("mint");
        let debug = format!("{key:?}");
        assert!(!debug.contains(key.as_str()));
        assert!(!debug.contains("nds_"));
    }

    #[test]
    fn credential_is_the_key_alone() {
        let key = ApiKey::mint().expect("mint");
        assert_eq!(key.credential(), format!("apiKey={}", key.as_str()));
        assert!(is_api_key_credential(&key.credential()));
    }

    #[test]
    fn credential_kind_is_read_from_the_keys() {
        assert!(is_api_key_credential("apiKey=nds_abc"));
        assert!(!is_api_key_credential("u=alice&s=salt&t=tok"));
        assert!(!is_api_key_credential("u=apiKey%3Dx&s=salt&t=tok"));
        assert!(!is_api_key_credential("apiKey="));
        assert!(!is_api_key_credential(""));
    }

    fn storage() -> (StateStorage, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let storage = StateStorage::new(dir.path().join("app.redb")).expect("storage");
        (storage, dir)
    }

    #[test]
    fn player_records_are_per_server_and_user() {
        let (storage, _dir) = storage();
        let alice_here = QueueOwner::new("https://music.example.com/", "Alice");
        let alice_there = QueueOwner::new("https://other.example.com", "alice");
        let bob_here = QueueOwner::new("https://music.example.com", "bob");

        record_player_id(&storage, &alice_here, "p-alice", "u-alice").expect("save");
        record_player_id(&storage, &bob_here, "p-bob", "u-bob").expect("save");

        // Same server (trailing slash dropped), same user (case-insensitive).
        let alice_again = QueueOwner::new("https://music.example.com", "alice");
        assert_eq!(
            recorded_player_id(&storage, &alice_again, "u-alice")
                .expect("read")
                .as_deref(),
            Some("p-alice")
        );
        assert_eq!(
            recorded_player_id(&storage, &alice_there, "u-alice").expect("read"),
            None
        );
        assert_eq!(
            recorded_player_id(&storage, &bob_here, "u-bob")
                .expect("read")
                .as_deref(),
            Some("p-bob")
        );

        forget_player_id(&storage, &alice_again).expect("forget");
        assert_eq!(
            recorded_player_id(&storage, &alice_here, "u-alice").expect("read"),
            None
        );
        assert_eq!(
            recorded_player_id(&storage, &bob_here, "u-bob")
                .expect("read")
                .as_deref(),
            Some("p-bob")
        );
    }

    #[test]
    fn a_damaged_record_reads_as_none() {
        let (storage, _dir) = storage();
        let owner = QueueOwner::new("https://music.example.com", "alice");
        record_player_id(&storage, &owner, "../../api/user", "").expect("save");
        assert_eq!(
            recorded_player_id(&storage, &owner, "").expect("read"),
            None
        );

        storage
            .save(PLAYER_RECORDS_KEY, &"not a map")
            .expect("save junk");
        assert_eq!(
            recorded_player_id(&storage, &owner, "").expect("read"),
            None
        );
        // A write over junk starts a fresh map instead of failing.
        record_player_id(&storage, &owner, "p1", "").expect("save over junk");
        assert_eq!(
            recorded_player_id(&storage, &owner, "")
                .expect("read")
                .as_deref(),
            Some("p1")
        );
    }

    /// A user who later took a recorded user's name never gets that user's
    /// player. A record or a session without an id (older records, a
    /// resumed token without a `uid`) still matches.
    #[test]
    fn a_record_never_serves_another_user_of_the_same_name() {
        let (storage, _dir) = storage();
        let owner = QueueOwner::new("https://music.example.com", "alice");
        record_player_id(&storage, &owner, "p1", "u-old").expect("save");

        assert_eq!(
            recorded_player_id(&storage, &owner, "u-new").expect("read"),
            None
        );
        assert_eq!(
            recorded_player_id(&storage, &owner, "u-old")
                .expect("read")
                .as_deref(),
            Some("p1")
        );
        assert_eq!(
            recorded_player_id(&storage, &owner, "")
                .expect("read")
                .as_deref(),
            Some("p1")
        );
        record_player_id(&storage, &owner, "p1", "").expect("save");
        assert_eq!(
            recorded_player_id(&storage, &owner, "u-new")
                .expect("read")
                .as_deref(),
            Some("p1")
        );
    }

    #[test]
    fn player_name_carries_the_host() {
        assert_eq!(player_name_for(Some("desk")), "nokkvi (desk)");
        assert_eq!(player_name_for(None), "nokkvi");
    }

    fn row(id: &str, user_id: &str, user: &str, seen: &str) -> PlayerRow {
        PlayerRow {
            id: id.into(),
            name: "nokkvi [nokkvi]".into(),
            client: "nokkvi".into(),
            user_agent: "nokkvi".into(),
            user_id: user_id.into(),
            user_name: user.into(),
            last_seen: seen.into(),
            ..PlayerRow::default()
        }
    }

    #[test]
    fn template_copies_the_users_latest_keyless_nokkvi_player() {
        let mut old = row("old", "u1", "alice", "2026-01-01T00:00:00Z");
        old.max_bit_rate = 96;
        let mut latest = row("latest", "u1", "alice", "2026-10-01T00:00:00+02:00");
        latest.max_bit_rate = 320;
        latest.transcoding_id = "opus".into();
        latest.scrobble_enabled = false;
        let mut keyed = row("keyed", "u1", "alice", "2026-10-11T00:00:00Z");
        keyed.has_api_key = true;
        keyed.max_bit_rate = 1;
        let mut other_user = row("bob", "u2", "bob", "2026-10-11T00:00:00Z");
        other_user.max_bit_rate = 2;
        let mut other_client = row("web", "u1", "alice", "2026-10-11T00:00:00Z");
        other_client.client = "NavidromeUI".into();
        other_client.max_bit_rate = 3;

        let rows = [old, latest, keyed, other_user, other_client];
        let template = settings_template(&rows, "u1", "alice");
        assert_eq!(
            template,
            PlayerSettings {
                transcoding_id: "opus".into(),
                max_bit_rate: 320,
                report_real_path: false,
                scrobble_enabled: false,
            }
        );
    }

    /// Another install's keyed player keeps its `nokkvi (<host>)` name after a
    /// logout revokes its key; its settings are that install's, not the ones
    /// Navidrome registered for nokkvi's token requests.
    #[test]
    fn template_skips_another_installs_revoked_player() {
        let mut auto = row("auto", "u1", "alice", "2026-10-01T00:00:00Z");
        auto.max_bit_rate = 128;
        let mut other_install = row("laptop", "u1", "alice", "2026-10-11T00:00:00Z");
        other_install.name = "nokkvi (laptop)".into();
        other_install.max_bit_rate = 64;
        let rows = [auto, other_install];
        assert_eq!(settings_template(&rows, "u1", "alice").max_bit_rate, 128);
    }

    /// With no user id (a resume whose JWT carried none) the row is matched
    /// by username, case-insensitively, and never by an empty name.
    #[test]
    fn template_falls_back_to_the_username() {
        let mut mine = row("mine", "u1", "Alice", "2026-10-01T00:00:00Z");
        mine.max_bit_rate = 128;
        let rows = [mine];
        assert_eq!(settings_template(&rows, "", "alice").max_bit_rate, 128);
        assert_eq!(settings_template(&rows, "", ""), PlayerSettings::default());
    }

    #[test]
    fn template_defaults_keep_scrobbling_on() {
        let template = settings_template(&[], "u1", "alice");
        assert_eq!(template, PlayerSettings::default());
        assert!(template.scrobble_enabled);
    }

    /// A logout's claim, taken synchronously, makes a later login's write wait.
    #[tokio::test]
    async fn a_held_claim_orders_the_next_writer() {
        let _serial = KEY_WRITES_TEST_LOCK.lock().await;
        let held = try_claim_key_writes().expect("free at start");
        assert!(try_claim_key_writes().is_none());
        let waiter = tokio::spawn(claim_key_writes());
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished(), "the next writer waits");
        drop(held);
        let _next = waiter.await.expect("the waiter gets the lock");
    }
}
