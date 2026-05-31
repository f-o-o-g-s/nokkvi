//! Tests for the version-aware artwork prefetch dedup gate (N17).
//!
//! Pins the invariant that a server-side cover change (a different
//! `updated_at`) makes an already-warmed `album_art` slot a genuine prefetch
//! MISS, so the passive mini-thumbnail path re-fetches the new cover instead
//! of serving the stale one for the rest of the session.

use std::collections::{HashMap, HashSet};

use crate::update::components::{passive_artwork_version, should_refetch};

/// Build a `(cached_ids, versions)` pair the way a prefetch tick would: the
/// version map is kept in lockstep with the `album_art` keys.
fn seeded() -> (Vec<String>, HashMap<String, Option<String>>) {
    let ids = vec!["al-1".to_string()];
    let mut versions = HashMap::new();
    versions.insert("al-1".to_string(), Some("v1".to_string()));
    (ids, versions)
}

#[test]
fn prefetch_dedup_treats_changed_updated_at_as_cache_miss() {
    let (ids, versions) = seeded();
    let cached: HashSet<&String> = ids.iter().collect();

    // Same version → already warm → NOT a refetch.
    assert!(
        !should_refetch(
            &cached,
            &versions,
            &"al-1".to_string(),
            &Some("v1".to_string())
        ),
        "unchanged cover must not re-fetch",
    );

    // Changed version → server cover changed → MUST refetch.
    assert!(
        should_refetch(
            &cached,
            &versions,
            &"al-1".to_string(),
            &Some("v2".to_string())
        ),
        "a changed updated_at must be treated as a cache miss",
    );
}

#[test]
fn prefetch_dedup_unknown_id_is_a_miss() {
    let (ids, versions) = seeded();
    let cached: HashSet<&String> = ids.iter().collect();

    assert!(
        should_refetch(
            &cached,
            &versions,
            &"al-unknown".to_string(),
            &Some("v1".to_string())
        ),
        "an id absent from album_art must always be a miss",
    );
}

#[test]
fn prefetch_dedup_evicted_handle_is_a_miss() {
    // Version recorded but the handle was evicted from album_art (capacity
    // skew): membership check, not just the version map, must catch it.
    let mut versions = HashMap::new();
    versions.insert("al-1".to_string(), Some("v1".to_string()));
    let cached: HashSet<&String> = HashSet::new(); // album_art is empty

    assert!(
        should_refetch(
            &cached,
            &versions,
            &"al-1".to_string(),
            &Some("v1".to_string())
        ),
        "a version hit whose album_art handle was evicted must re-fetch",
    );
}

/// F4: a single-album queue must not re-fetch the same 80px thumbnail every
/// scroll tick. The bug was that the album_id-keyed version map was fed the
/// PER-SONG `updated_at`, which differs across the album's tracks; across
/// prefetch batches the recorded version oscillated, `should_refetch` kept
/// returning true, `album_art` was re-put, and `Handle::from_bytes` minted a
/// fresh texture for identical bytes → flicker.
///
/// The fix feeds a constant `None` as the version at the passive surfaces
/// (queue / song-mini / similar / editor), so every track of one album maps to
/// the same recorded version and the gate is stable. This test pins both
/// halves: (1) the post-fix contract — `None` on both ticks never oscillates,
/// and (2) the pre-fix oscillation at the pure `should_refetch` level, proving
/// WHY the call sites had to stop passing per-song versions (its semantics are
/// intentionally left unchanged).
#[test]
fn passive_single_album_queue_does_not_oscillate() {
    let album = "al-1".to_string();
    // Two tracks of the same album carrying DIFFERENT per-song updated_at —
    // exactly what `Song.updated_at` does per mediafile. After F4 the passive
    // closures discard these in favour of `passive_artwork_version()`, so the
    // recorded album-keyed version is constant regardless of which track's row
    // drives a given prefetch tick.
    let song_a_updated = Some("2026-01-01".to_string());
    let song_b_updated = Some("2026-02-02".to_string());

    // The passive surfaces feed THIS as the version, not the per-song value.
    // It must be the same for every track so the album-keyed gate stays warm.
    let passive_a = passive_artwork_version(&song_a_updated);
    let passive_b = passive_artwork_version(&song_b_updated);
    assert_eq!(
        passive_a, passive_b,
        "passive surfaces must feed an album-coherent version, identical across \
         tracks of one album — otherwise the album-keyed gate oscillates",
    );

    // Tick 1 (cold): nothing cached → miss; the loaded handler records the
    // passive version for song A's row.
    let cold: HashSet<&String> = HashSet::new();
    let mut versions: HashMap<String, Option<String>> = HashMap::new();
    assert!(
        should_refetch(&cold, &versions, &album, &passive_a),
        "cold slot is a miss",
    );
    versions.insert(album.clone(), passive_a.clone());

    // Tick 2 (warm): the next same-album row (song B, different per-song
    // updated_at) feeds its passive version → must NOT re-fetch.
    let warm_ids = [album.clone()];
    let warm: HashSet<&String> = warm_ids.iter().collect();
    assert!(
        !should_refetch(&warm, &versions, &album, &passive_b),
        "a same-album row must not re-fetch once the album slot is version-warm",
    );

    // ── Pre-fix oscillation, documented at the pure gate level ──
    // Feeding the raw per-song versions (what the call sites USED to do)
    // re-fetches forever: tick 1 recorded song A's version, tick 2 arrives with
    // song B's. `should_refetch`'s own semantics are intentionally unchanged.
    let mut versions_song_a: HashMap<String, Option<String>> = HashMap::new();
    versions_song_a.insert(album.clone(), song_a_updated.clone());
    assert!(
        should_refetch(&warm, &versions_song_a, &album, &song_b_updated),
        "pre-fix: differing per-song updated_at oscillates the album-keyed gate \
         (the bug F4 removes at the call sites)",
    );
}
