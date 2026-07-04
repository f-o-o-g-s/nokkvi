//! Crossfade-vs-gapless transition policy (M4).
//!
//! One pure decision function owns the *metadata-level* "should this
//! transition blend or hard-join?" question, computed controller-side at
//! gapless-prep time (`PlaybackController::prepare_next_for_gapless`) — the
//! engine boundary carries no [`Song`] metadata, so the verdict is threaded
//! down as a per-transition suppress flag on the `CrossfadeCoordinator`.
//!
//! Scope note: the bit-perfect FORMAT gate is **not** re-derived here. Both
//! crossfade triggers gate on the renderer's `crossfade_blocked(current,
//! incoming)` with the same format pair (the load-bearing dual-trigger
//! agreement), and duplicating that decision from `Song` metadata could
//! disagree with the real decoded formats. `CrossfadePolicyCfg::format_blocked`
//! exists for callers that have *already* run that gate and want the unified
//! decision vocabulary; the prep path always passes `false`.

use tracing::debug;

use crate::{services::queue::TransitionReason, types::song::Song};

/// The policy verdict for one queue transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfadeDecision {
    /// Overlap-and-blend the outgoing track into the incoming one.
    Crossfade,
    /// Hard-join: the incoming track continues the same album sequentially
    /// (authored segue / attacca / live set), so play it gapless.
    GaplessAlbumContinuation,
    /// Hard-join: at least one of the two tracks is shorter than the user's
    /// minimum-track-length floor, so the pair plays gapless.
    GaplessTooShort,
    /// Hard-cut: the caller reports the format pair cannot blend (bit-perfect
    /// Strict, or a Relaxed cross-format change). Never produced on the
    /// prep path — see the module note.
    HardCutFormatBlocked,
}

impl CrossfadeDecision {
    /// Whether this verdict suppresses the crossfade arm/trigger for the
    /// transition (everything except [`CrossfadeDecision::Crossfade`]).
    pub fn suppresses_crossfade(self) -> bool {
        !matches!(self, CrossfadeDecision::Crossfade)
    }
}

/// Inputs to [`crossfade_decision`] that live outside the two songs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrossfadePolicyCfg {
    /// Minimum track length (seconds) below which transitions play gapless.
    /// 0 blends everything with a known duration.
    pub min_track_secs: u32,
    /// The opt-in album-continuity gate: sequential same-album tracks
    /// hard-join while shuffle, compilations, disc boundaries, and
    /// album-to-album changes still crossfade.
    pub album_continuity: bool,
    /// Caller-supplied per-transition input: `true` only when the caller has
    /// already established the format pair cannot blend. The gapless-prep
    /// path passes `false` — format gating stays owned by the dual
    /// `crossfade_blocked` sites downstream.
    pub format_blocked: bool,
}

/// Decide how the transition `current → next` should play, from track
/// metadata alone. Pure — no locks, no engine state. Logs the reason
/// whenever the verdict falls back from a crossfade.
pub fn crossfade_decision(
    current: &Song,
    next: &Song,
    reason: TransitionReason,
    cfg: &CrossfadePolicyCfg,
) -> CrossfadeDecision {
    let decision = decide(current, next, reason, cfg);
    if decision.suppresses_crossfade() {
        debug!(
            "🔀 [POLICY] Crossfade fall-back: {:?} ({} → {}, reason={})",
            decision, current.title, next.title, reason
        );
    }
    decision
}

fn decide(
    current: &Song,
    next: &Song,
    reason: TransitionReason,
    cfg: &CrossfadePolicyCfg,
) -> CrossfadeDecision {
    if cfg.format_blocked {
        return CrossfadeDecision::HardCutFormatBlocked;
    }
    // Too-short floor first: the physically harder gate, and it names the
    // real reason in the fall-back log even when the album gate would also
    // suppress. `Song.duration` is server metadata (seconds); the renderer's
    // arm gate re-checks with decoder-probed durations, so the two agree in
    // the same direction (both hard-join, never one blends while the other
    // refuses and orphans a stream).
    if current.duration.min(next.duration) < cfg.min_track_secs {
        return CrossfadeDecision::GaplessTooShort;
    }
    if cfg.album_continuity && is_album_continuation(current, next, reason) {
        return CrossfadeDecision::GaplessAlbumContinuation;
    }
    CrossfadeDecision::Crossfade
}

/// Whether `next` continues `current`'s album in authored order: the case
/// where a crossfade would smear an intended gapless segue (attacca, live
/// set, concept album). Shuffle landing on the sequential pair is chance,
/// not authored order; compilations and disc boundaries carry no authored
/// segue; missing album ids / track numbers can't prove continuity, so they
/// stay safe-to-crossfade.
fn is_album_continuation(current: &Song, next: &Song, reason: TransitionReason) -> bool {
    if reason == TransitionReason::Shuffle {
        return false;
    }
    let (Some(current_album), Some(next_album)) = (&current.album_id, &next.album_id) else {
        return false;
    };
    if current_album != next_album {
        return false;
    }
    if current.compilation == Some(true) || next.compilation == Some(true) {
        return false;
    }
    if is_various_artists(current) || is_various_artists(next) {
        return false;
    }
    // Disc boundary = physical-media break; a missing tag reads as disc 1
    // (single-disc rips usually carry no disc tag).
    if current.disc.unwrap_or(1) != next.disc.unwrap_or(1) {
        return false;
    }
    let (Some(current_track), Some(next_track)) = (current.track, next.track) else {
        return false;
    };
    next_track == current_track + 1
}

/// Bar-snapped crossfade duration (M8, opt-in "Snap Crossfade to Musical
/// Bars"): round the user's crossfade length to a whole number of 4/4 bars of
/// the OUTGOING track's tempo, so beats line up through the blend when the
/// two tracks are naturally at/near the same tempo. `None` when the outgoing
/// track carries no usable BPM tag (untagged or a real-world `Some(0)`, which
/// would divide by zero) — the caller then keeps the plain duration.
///
/// The snapped value is clamped near the user's setting to
/// `[max(CROSSFADE_DURATION_MIN_SECS·1000, user − bar), min(CROSSFADE_DURATION_MAX_SECS·1000, user + bar)]`
/// (ms). The upper cap matters: a slow track (e.g. 30 BPM → 8 s bars) at a
/// long user duration would otherwise snap past the slider's 12 s ceiling —
/// `arm_crossfade` clamps only at `shorter/2` (an upper bound tied to track
/// length, not the slider). Pure; computed controller-side at gapless prep
/// (the engine boundary carries no `Song` metadata — same as
/// [`crossfade_decision`]).
pub fn bar_snapped_crossfade_ms(user_ms: u64, outgoing_bpm: Option<u32>) -> Option<u64> {
    use crate::types::player_settings::{CROSSFADE_DURATION_MAX_SECS, CROSSFADE_DURATION_MIN_SECS};

    let bpm = u64::from(outgoing_bpm.filter(|b| *b > 0)?);
    // One 4/4 bar: 4 beats × 60_000 ms / bpm. An absurd tag (> 240_000 BPM)
    // degenerates to a 0 ms bar — treat as untagged.
    let bar_ms = 240_000 / bpm;
    if bar_ms == 0 {
        return None;
    }
    let bars = (user_ms + bar_ms / 2) / bar_ms; // round-to-nearest
    let snapped = bars * bar_ms;
    let lower = (u64::from(CROSSFADE_DURATION_MIN_SECS) * 1000).max(user_ms.saturating_sub(bar_ms));
    let upper = (u64::from(CROSSFADE_DURATION_MAX_SECS) * 1000).min(user_ms + bar_ms);
    // `lower <= upper` holds for any user_ms within the slider bounds (each
    // side is anchored at user_ms); the max() guards a hand-edited config.
    Some(snapped.clamp(lower, upper.max(lower)))
}

/// Untagged-compilation heuristic: a "Various Artists" album artist without
/// the compilation flag.
fn is_various_artists(song: &Song) -> bool {
    song.album_artist
        .as_deref()
        .is_some_and(|artist| artist.eq_ignore_ascii_case("various artists"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Song with the album-identity fields the policy reads.
    fn album_song(
        id: &str,
        album_id: Option<&str>,
        disc: Option<u32>,
        track: Option<u32>,
        duration_secs: u32,
    ) -> Song {
        let mut song = Song::test_default(id, id);
        song.album_id = album_id.map(str::to_string);
        song.disc = disc;
        song.track = track;
        song.duration = duration_secs;
        song
    }

    fn cfg(min_track_secs: u32, album_continuity: bool) -> CrossfadePolicyCfg {
        CrossfadePolicyCfg {
            min_track_secs,
            album_continuity,
            format_blocked: false,
        }
    }

    /// Sequential same-album tracks (same disc, track N → N+1) with the album
    /// gate ON hard-join: this is the authored segue/attacca/live-set case.
    #[test]
    fn sequential_same_album_is_gapless_continuation() {
        let current = album_song("a", Some("alb1"), Some(1), Some(2), 240);
        let next = album_song("b", Some("alb1"), Some(1), Some(3), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::GaplessAlbumContinuation,
        );
    }

    /// The album gate is OPT-IN: with it off, the identical sequential
    /// same-album pair still crossfades.
    #[test]
    fn album_gate_off_sequential_same_album_crossfades() {
        let current = album_song("a", Some("alb1"), Some(1), Some(2), 240);
        let next = album_song("b", Some("alb1"), Some(1), Some(3), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, false)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// Under SHUFFLE a same-album sequential pair is chance, not authored
    /// order — it crossfades even with the gate on.
    #[test]
    fn shuffle_same_album_sequential_still_crossfades() {
        let current = album_song("a", Some("alb1"), Some(1), Some(2), 240);
        let next = album_song("b", Some("alb1"), Some(1), Some(3), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Shuffle, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// A disc boundary is a physical-media break, not an authored segue —
    /// the gate does not hard-join across it.
    #[test]
    fn disc_boundary_crossfades() {
        let current = album_song("a", Some("alb1"), Some(1), Some(12), 240);
        let next = album_song("b", Some("alb1"), Some(2), Some(13), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// Missing disc tags on both sides mean "same (only) disc" — the
    /// continuation still holds (most single-disc rips carry no disc tag).
    #[test]
    fn missing_disc_tags_still_continue() {
        let current = album_song("a", Some("alb1"), None, Some(2), 240);
        let next = album_song("b", Some("alb1"), None, Some(3), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::GaplessAlbumContinuation,
        );
    }

    /// Compilations are different artists' tracks packaged together — no
    /// authored segues to protect; they crossfade.
    #[test]
    fn compilation_crossfades() {
        let mut current = album_song("a", Some("alb1"), Some(1), Some(2), 240);
        let mut next = album_song("b", Some("alb1"), Some(1), Some(3), 240);
        current.compilation = Some(true);
        next.compilation = Some(true);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// A "Various Artists" album-artist without the compilation flag is the
    /// classic untagged-compilation shape — also crossfades.
    #[test]
    fn various_artists_album_artist_crossfades() {
        let mut current = album_song("a", Some("alb1"), Some(1), Some(2), 240);
        let mut next = album_song("b", Some("alb1"), Some(1), Some(3), 240);
        current.album_artist = Some("Various Artists".to_string());
        next.album_artist = Some("Various Artists".to_string());
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// Non-sequential same-album jumps (track 3 → 7) are user curation, not
    /// authored order — crossfade.
    #[test]
    fn non_sequential_same_album_crossfades() {
        let current = album_song("a", Some("alb1"), Some(1), Some(3), 240);
        let next = album_song("b", Some("alb1"), Some(1), Some(7), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// Missing track numbers can't prove sequential order — safe-to-crossfade.
    #[test]
    fn missing_track_number_crossfades() {
        let current = album_song("a", Some("alb1"), Some(1), None, 240);
        let next = album_song("b", Some("alb1"), Some(1), Some(3), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// Missing album ids (both None) must NOT read as "same album".
    #[test]
    fn missing_album_ids_crossfade() {
        let current = album_song("a", None, Some(1), Some(2), 240);
        let next = album_song("b", None, Some(1), Some(3), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// Different albums crossfade even with the gate on.
    #[test]
    fn different_albums_crossfade() {
        let current = album_song("a", Some("alb1"), Some(1), Some(12), 240);
        let next = album_song("b", Some("alb2"), Some(1), Some(1), 240);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// A pair with the shorter side under the floor plays gapless — and the
    /// too-short verdict outranks the album gate (it would suppress anyway,
    /// but the log should name the real reason).
    #[test]
    fn too_short_is_gapless() {
        let current = album_song("a", Some("alb1"), Some(1), Some(2), 240);
        let next = album_song("b", Some("alb1"), Some(1), Some(3), 8);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, false)),
            CrossfadeDecision::GaplessTooShort,
        );
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(10, true)),
            CrossfadeDecision::GaplessTooShort,
        );
    }

    /// min_track_secs = 0 means "blend everything": even very short tracks
    /// pass the floor.
    #[test]
    fn zero_min_track_blends_short_tracks() {
        let current = album_song("a", Some("alb1"), Some(1), Some(3), 3);
        let next = album_song("b", Some("alb2"), Some(1), Some(1), 3);
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg(0, false)),
            CrossfadeDecision::Crossfade,
        );
    }

    /// A caller that already ran the format gate gets the hard-cut verdict,
    /// and it outranks every other input.
    #[test]
    fn format_blocked_outranks_everything() {
        let current = album_song("a", Some("alb1"), Some(1), Some(2), 8);
        let next = album_song("b", Some("alb1"), Some(1), Some(3), 8);
        let cfg = CrossfadePolicyCfg {
            min_track_secs: 10,
            album_continuity: true,
            format_blocked: true,
        };
        assert_eq!(
            crossfade_decision(&current, &next, TransitionReason::Next, &cfg),
            CrossfadeDecision::HardCutFormatBlocked,
        );
    }

    /// M8 bar-snap: no BPM tag (or a real-world `Some(0)` tag, which would
    /// divide by zero) means no snap — the caller keeps the plain duration.
    #[test]
    fn bar_snap_without_usable_bpm_is_none() {
        assert_eq!(bar_snapped_crossfade_ms(7_000, None), None);
        assert_eq!(bar_snapped_crossfade_ms(7_000, Some(0)), None);
    }

    /// M8 bar-snap: rounds to the nearest whole bar. 120 BPM ⇒ a 4/4 bar is
    /// 2 s; a 7 s duration rounds to 4 bars = 8 s (within the ±1-bar clamp).
    #[test]
    fn bar_snap_rounds_to_nearest_whole_bar() {
        assert_eq!(bar_snapped_crossfade_ms(7_000, Some(120)), Some(8_000));
        // 240 BPM ⇒ 1 s bars: 7 s is already exactly 7 bars — unchanged.
        assert_eq!(bar_snapped_crossfade_ms(7_000, Some(240)), Some(7_000));
        // 60 BPM ⇒ 4 s bars: 7 s rounds to 2 bars = 8 s.
        assert_eq!(bar_snapped_crossfade_ms(7_000, Some(60)), Some(8_000));
    }

    /// M8 bar-snap upper cap: 30 BPM ⇒ 8 s bars; 12 s rounds to 2 bars =
    /// 16 s, beyond the slider's 12 s ceiling — clamped back to 12 s
    /// (`min(CROSSFADE_DURATION_MAX_SECS·1000, user + bar)` floors at the
    /// ceiling first).
    #[test]
    fn bar_snap_clamps_to_duration_ceiling() {
        assert_eq!(bar_snapped_crossfade_ms(12_000, Some(30)), Some(12_000));
    }

    /// M8 bar-snap lower clamp: 40 BPM ⇒ 6 s bars; a 2 s duration rounds to
    /// 0 bars = 0 ms — clamped up to the 1 s slider floor
    /// (`max(CROSSFADE_DURATION_MIN_SECS·1000, user − bar)` saturates).
    #[test]
    fn bar_snap_clamps_to_duration_floor() {
        assert_eq!(bar_snapped_crossfade_ms(2_000, Some(40)), Some(1_000));
    }

    /// M8 bar-snap ±1-bar band: the snap never moves the duration more than
    /// one bar away from the user's setting.
    #[test]
    fn bar_snap_stays_within_one_bar_of_user_duration() {
        for bpm in [40u32, 60, 90, 120, 174, 240] {
            let bar_ms = 240_000 / u64::from(bpm);
            for user_ms in [1_000u64, 2_000, 5_000, 7_000, 12_000] {
                let snapped =
                    bar_snapped_crossfade_ms(user_ms, Some(bpm)).expect("bpm > 0 must snap");
                assert!(
                    snapped.abs_diff(user_ms) <= bar_ms,
                    "snap moved {user_ms}ms by more than one bar ({bar_ms}ms) at {bpm} BPM: {snapped}ms"
                );
                assert!((1_000..=12_000).contains(&snapped));
            }
        }
    }

    /// Only Crossfade does not suppress.
    #[test]
    fn suppresses_crossfade_matches_variants() {
        assert!(!CrossfadeDecision::Crossfade.suppresses_crossfade());
        assert!(CrossfadeDecision::GaplessAlbumContinuation.suppresses_crossfade());
        assert!(CrossfadeDecision::GaplessTooShort.suppresses_crossfade());
        assert!(CrossfadeDecision::HardCutFormatBlocked.suppresses_crossfade());
    }
}
