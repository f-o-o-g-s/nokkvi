# ReplayGain Playback — Implementation Plan

## Status

Planning. Not yet started.

## Summary

Today's volume-leveling pipeline runs rodio's real-time AGC source over the
decoded stream. This plan keeps that stage but adds an alternative path: a
static, per-stream gain (`Source::amplify`) computed from the song's
`ReplayGain` tags before stream creation. The `volume_normalization: bool` /
`normalization_level: NormalizationLevel` pair widens to a single
`VolumeNormalizationMode` enum (`Off | Agc | ReplayGainTrack | ReplayGainAlbum`)
plus a pre-amp dB scalar, a fallback dB for un-tagged tracks, an opt-in
fallback-to-AGC, and a peak-aware clipping toggle.

`Song.replay_gain` is already populated everywhere it's constructed
(`data/src/types/song.rs:9-18`, populated in
`data/src/services/api/songs.rs` and `data/src/services/api/playlists.rs`),
so no API-layer changes are needed. The work is entirely
settings-schema, audio-chain plumbing, and UI.

Crossfade comes along for free — both faded streams are pre-leveled at
construction time, so the AGC convergence-delay artifact disappears.

## Premise corrections (verified during planning)

- **Persistence is JSON via serde**, not bincode framing as initially
  assumed. `state_storage.rs:35-46` writes `UserSettings` as
  `serde_json::to_vec`. Migration is therefore a serde concern.
- **Two streams per playing track:** the primary in `renderer.rs:210`
  (or `:381` on seek) and the crossfade in `renderer.rs:578`. Both must
  receive the correct per-track RG.
- **Gapless reuse and per-track gain conflict:** the gapless path
  (`renderer.rs:177-186`) reuses the existing stream when format
  matches, meaning a static `amplify` factor cannot change mid-stream.
  See §4 for handling.

## Reference clients (research summary)

Researched rmpc, Navidrome's WebUI, and Feishin to validate design
choices.

| Decision               | Navidrome WebUI    | Feishin                      | Chosen                        |
|------------------------|--------------------|------------------------------|-------------------------------|
| Mode enum shape        | `none/album/track` | `'no'/'album'/'track'`       | `Off/Agc/RG-track/RG-album`   |
| Default                | `none`             | `'no'`                       | `Off`                         |
| Pre-amp default        | 0 dB               | 0 dB                         | 0 dB                          |
| Pre-amp range          | -15 to +15, 0.5 step | unbounded                  | -15 to +15, 1 dB step         |
| Peak clamping          | always on          | toggle, default ON           | toggle, default ON            |
| Cross-fallback         | none (unity)       | smart: track↔album silent    | smart: track↔album silent     |
| Untagged fallback      | unity              | configurable dB + unity      | configurable dB + opt-in AGC  |
| Hotkey                 | none               | none                         | none (defer)                  |
| Crossfade interaction  | n/a                | per-stream pre-mix           | per-stream pre-mix            |

rmpc was not useful for direct prior art — it doesn't implement RG at
all, deferring to MPD's `replay_gain_mode` server option.

Navidrome converts R128 tags to ReplayGain dB during scan
(`reference-navidrome/model/metadata/map_mediafile.go:111-129` —
divide by 256, +5 dB shift). nokkvi receives normalized dB regardless
of the tagger used, so no client-side handling is needed. Worth one
sentence in the docs.

OpenSubsonic exposes additional `BaseGain` and `FallbackGain` fields
that nokkvi does not currently parse. Out of scope for v1.

## 1. Settings / state schema

### New types — `data/src/types/player_settings.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeNormalizationMode {
    #[default]
    Off,
    Agc,
    ReplayGainTrack,
    ReplayGainAlbum,
}

impl VolumeNormalizationMode {
    pub fn is_agc(self) -> bool { matches!(self, Self::Agc) }
    pub fn is_replay_gain(self) -> bool {
        matches!(self, Self::ReplayGainTrack | Self::ReplayGainAlbum)
    }
    pub fn prefers_album(self) -> bool { matches!(self, Self::ReplayGainAlbum) }
    pub fn as_label(self) -> &'static str { /* "Off" | "AGC" | "ReplayGain (Track)" | "ReplayGain (Album)" */ }
    pub fn from_label(s: &str) -> Self { /* inverse */ }
}
```

`NormalizationLevel` stays as-is (only meaningful when mode = `Agc`).

### `PlayerSettings` (`data/src/types/player_settings.rs:525`)

Replace:
```rust
pub volume_normalization: bool,
pub normalization_level: NormalizationLevel,
```
with:
```rust
pub volume_normalization: VolumeNormalizationMode,
pub normalization_level: NormalizationLevel,        // AGC-only target
pub replay_gain_preamp_db: f32,                     // default 0.0; UI clamp [-15, +15]
pub replay_gain_fallback_db: f32,                   // default 0.0; UI clamp [-15, +15]
pub replay_gain_fallback_to_agc: bool,              // default false
pub replay_gain_prevent_clipping: bool,             // default true
```

Update `PlayerSettings::default()` accordingly.

### Persisted struct: `UserSettings` (`data/src/types/settings.rs:98-101`) and `TomlSettings` (`data/src/types/toml_settings.rs:93-94`)

Mirror the same fields. Both use serde (JSON / TOML respectively).

### Migration

The on-disk shape today is `volume_normalization: bool` and
`normalization_level: "normal"`. A naive enum swap breaks deserialization
for users with existing redb state and `config.toml`. Approach:

1. In `UserSettings`, add a new key `volume_normalization_mode:
   VolumeNormalizationMode` (serde default `Off`). Keep the legacy field
   readable as `volume_normalization_legacy: Option<bool>` with
   `#[serde(default, rename = "volume_normalization")]`.
2. In `SettingsManager::new` (`data/src/services/settings.rs:50-110`),
   after the redb load, run a one-shot migration:
   - if `volume_normalization_legacy == Some(true)` and
     `volume_normalization_mode == Off`: set mode to `Agc`, clear the
     legacy field, save once.
3. Same idea for `TomlSettings`: both keys readable with
   `#[serde(default)]`; in `apply_toml_settings_to_internal`
   (`services/settings.rs:718`), prefer the new key, fall back to
   `legacy_bool ? Agc : Off`.
4. Log a one-shot warning when the legacy bool is observed during load
   so users editing TOML by hand aren't confused.
5. Migration code can be removed after one or two release cycles.

The simpler alternative — a custom `Deserialize` accepting both bool
and string — was rejected as more invasive and harder to evolve.

`PlayerSettings` (in-memory only) needs no migration; it's rebuilt
from `UserSettings` on every `get_player_settings()`.

## 2. Audio chain changes

### New `NormalizationConfig` — `data/src/audio/normalization.rs` (new file)

```rust
#[derive(Debug, Clone, Copy)]
pub enum NormalizationConfig {
    Off,
    Agc { target_level: f32 },
    /// Static linear gain factor (peak clamping already applied if enabled).
    Static(f32),
}

impl NormalizationConfig {
    pub fn off() -> Self { Self::Off }
    pub fn agc(target_level: f32) -> Self { Self::Agc { target_level } }
    pub fn r#static(linear: f32) -> Self { Self::Static(linear) }
}
```

Note: we resolve to **linear gain** (not dB) before reaching
`NormalizationConfig`, because peak clamping happens in linear space
and we want the rodio chain to receive the final scalar. This
diverges from the earlier draft of the plan which used
`amplify_decibel`; the cross-validated math (Feishin
`web-player.tsx:361-380`, Navidrome `calculateReplayGain.js:1-9`) is
linear-domain, and `Source::amplify(f32)` is the cleaner fit.

### `RodioOutput::create_stream` — `data/src/audio/rodio_output.rs:126-185`

Change signature from:
```rust
pub fn create_stream(
    &self,
    sample_rate: u32,
    channels: u16,
    initial_volume: f32,
    volume_normalization: bool,
    normalization_target_level: f32,
    eq_state: Option<super::eq::EqState>,
) -> ActiveStream
```
to:
```rust
pub fn create_stream(
    &self,
    sample_rate: u32,
    channels: u16,
    initial_volume: f32,
    norm: NormalizationConfig,
    eq_state: Option<super::eq::EqState>,
) -> ActiveStream
```

Chain dispatch (three sibling branches, no `Box<dyn>`):
```rust
use rodio::source::{AutomaticGainControlSettings, LimitSettings, Source};
match norm {
    NormalizationConfig::Off => {
        self.mixer.add(source.limit(LimitSettings::dynamic_content()));
    }
    NormalizationConfig::Agc { target_level } => {
        let agc_settings = AutomaticGainControlSettings {
            target_level,
            ..AutomaticGainControlSettings::default()
        };
        self.mixer.add(
            source
                .automatic_gain_control(agc_settings)
                .limit(LimitSettings::dynamic_content()),
        );
    }
    NormalizationConfig::Static(factor) => {
        self.mixer.add(
            source
                .amplify(factor)
                .limit(LimitSettings::dynamic_content()),
        );
    }
}
```

`amplify` placement before `limit` is correct — the limiter sees the
post-gain signal and catches overshoot. AGC and the RG static factor
are mutually exclusive by design (RG is static, AGC is dynamic;
stacking them defeats the purpose). Settings UI enforces this; the
audio code just respects the resolved config.

Reference: `reference-rodio/src/source/amplify.rs:1-103` (simple `*=
factor` per sample, sample-rate / channel passthrough).

### `AudioRenderer` — `data/src/audio/renderer.rs`

Replace fields (`renderer.rs:83-85`):
```rust
volume_normalization_mode: VolumeNormalizationMode,
normalization_target_level: f32,                    // AGC-only, ignored otherwise
replay_gain_preamp_db: f32,
replay_gain_fallback_db: f32,
replay_gain_fallback_to_agc: bool,
replay_gain_prevent_clipping: bool,

// Per-track RG tags for the *current* primary stream — set by the
// engine before init()/seek(); used to resolve the gain factor in
// each subsequent create_stream() call.
pending_replay_gain: Option<ReplayGain>,
// Per-track RG tags for the *incoming* crossfade stream.
pending_crossfade_replay_gain: Option<ReplayGain>,
```

Replace `set_volume_normalization(enabled: bool, target_level: f32)`
(`renderer.rs:97`) with:
```rust
pub fn set_volume_normalization(
    &mut self,
    mode: VolumeNormalizationMode,
    target_level: f32,
    preamp_db: f32,
    fallback_db: f32,
    fallback_to_agc: bool,
    prevent_clipping: bool,
)
```

Add setters:
```rust
pub fn set_pending_replay_gain(&mut self, rg: Option<ReplayGain>) { ... }
pub fn set_pending_crossfade_replay_gain(&mut self, rg: Option<ReplayGain>) { ... }
```

### Resolver

```rust
fn resolve_norm_for_track(&self, rg: Option<&ReplayGain>) -> NormalizationConfig {
    use VolumeNormalizationMode::*;
    let prefer_album = self.volume_normalization_mode.prefers_album();

    if !self.volume_normalization_mode.is_replay_gain() {
        return match self.volume_normalization_mode {
            Off => NormalizationConfig::off(),
            Agc => NormalizationConfig::agc(self.normalization_target_level),
            _ => unreachable!(),
        };
    }

    // Smart cross-fallback (always on): track↔album silently
    let (gain_db, peak) = match rg {
        Some(r) if !prefer_album => {
            (r.track_gain.or(r.album_gain), r.track_peak.or(r.album_peak))
        }
        Some(r) /* prefer_album */ => {
            (r.album_gain.or(r.track_gain), r.album_peak.or(r.track_peak))
        }
        None => (None, None),
    };

    let resolved_db = match gain_db {
        Some(db) => db as f32,
        None if self.replay_gain_fallback_to_agc => {
            return NormalizationConfig::agc(self.normalization_target_level);
        }
        None => self.replay_gain_fallback_db,    // typically 0.0 = unity
    };

    let total_db = resolved_db + self.replay_gain_preamp_db;
    let linear = 10f32.powf(total_db / 20.0);

    let effective = match (self.replay_gain_prevent_clipping, peak) {
        (true, Some(p)) if p > 0.0 => linear.min(1.0 / p as f32),
        _ => linear,
    };

    NormalizationConfig::r#static(effective)
}
```

Update the three `output.create_stream(...)` call sites:

- **`renderer.rs:210` (`init`)**: pass
  `self.resolve_norm_for_track(self.pending_replay_gain.as_ref())`.
- **`renderer.rs:381` (`seek`)**: same — seek recreates the stream
  for the *current* track, so reuse `pending_replay_gain`.
- **`renderer.rs:578` (`start_crossfade`)**: pass
  `self.resolve_norm_for_track(self.pending_crossfade_replay_gain.as_ref())`.

Crossfade interaction — `tick_crossfade` (`renderer.rs:684-689`) and
`finalize_crossfade` (`:717-718`) call `stream.set_volume(...)` for
the user volume / fade coefficient. That sits **after** the
`amplify` stage in the rodio chain (StreamingSource's `set_volume`
is at the source's head, `amplify` wraps it, limiter wraps that).
The two multiply cleanly:
`final = decoded * rg_linear * fade * user_vol`. No additional
crossfade-side work needed.

### `CustomAudioEngine::set_volume_normalization` — `data/src/audio/engine.rs:1207-1210`

Update signature in lockstep:
```rust
pub fn set_volume_normalization(
    &mut self,
    mode: VolumeNormalizationMode,
    target_level: f32,
    preamp_db: f32,
    fallback_db: f32,
    fallback_to_agc: bool,
    prevent_clipping: bool,
)
```
Delegate to renderer.

Add per-track plumbing:
```rust
pub fn set_pending_replay_gain(&mut self, rg: Option<ReplayGain>) { ... }
pub fn set_pending_crossfade_replay_gain(&mut self, rg: Option<ReplayGain>) { ... }
```

## 3. Plumbing — how per-song gain reaches the audio layer

**Decision: pass `Option<ReplayGain>`, not pre-resolved dB.** Reasons:
1. Mode + fallback policy + pre-amp lives in the renderer (already
   sees settings). Resolving upstream would duplicate state.
2. `ReplayGain` is small and `Clone`.
3. Symmetric call sites: every place that knows "we're about to
   play song X" calls `engine.set_pending_replay_gain(song.replay_gain.clone())`
   immediately before `engine.load_track`/`engine.set_source`.

### Call sites

- **`data/src/services/playback.rs:367-368` (`play_song_direct`)**:
  has `&Song`. Add `engine.set_pending_replay_gain(...)` before
  `engine.load_track`.
- **`data/src/backend/playback_controller.rs:223-224` and `:279-280`
  (resume / cold start)**: same pattern.
- **`data/src/backend/playback_controller.rs:617-618, 669-670`
  (`play_songs_from_index`)**: same pattern.
- **`data/src/backend/playback_controller.rs:558-573` (gapless prep,
  `store_prepared_decoder`)**: capture
  `next_result.song.replay_gain.clone()` alongside the URL (lines
  511-518); pass into the spawn closure; call
  `engine.set_pending_crossfade_replay_gain(rg)` before
  `engine.store_prepared_decoder(...)`. Cleaner: extend
  `store_prepared_decoder` and `prepare_next_track` (`engine.rs:1073`,
  `:1115`) signatures to take `replay_gain: Option<ReplayGain>` and
  let the engine stash it.
- **`data/src/services/api/playlists.rs:281-288`**: already populates
  `replay_gain`. No change.
- **`data/src/services/api/songs.rs:210-215`**: serde-derived. No
  change.

### Settings push from UI to engine

`src/update/playback.rs:1015-1036` and
`src/update/settings.rs:739-778` both call
`engine.set_volume_normalization(enabled, target_level)`. Update to
the new six-arg form.

`src/state.rs:286-288` (`EngineState`): replace
`volume_normalization: bool` with the new typed field; add the four
new fields.

`data/src/backend/settings.rs:513-524`: add persistence shims for
the four new fields; update `set_volume_normalization` parameter
type.

`data/src/services/settings.rs:312-320`: same parameter-type change.

### Gapless reuse vs. per-track gain

The gapless path (`renderer.rs:177-186`) reuses the primary stream
when the next track has the same format. With `ReplayGainTrack`
mode, the static `amplify` factor cannot change mid-stream — so the
second track plays through with the first track's gain.

**Fix:** in `renderer.rs:177`, gate gapless reuse on
`self.volume_normalization_mode != ReplayGainTrack || self.pending_replay_gain.track_gain == new_track_rg.track_gain`.

This requires the engine to set `pending_replay_gain` for the *next*
track before calling `renderer.init()` in the gapless transition
branch (`engine.rs:1153-1172`, `consume_gapless_transition`).
Tractable since `prepare_next_track` already sees the song.

Album mode is unaffected — same album means same album_gain by
definition. AGC and Off modes are unaffected.

## 4. UI — `src/views/settings/items_playback.rs`

Current shape (line 62 area):
```rust
SettingItem::bool_val("general.volume_normalization", "Volume Normalization", ...),
SettingItem::enum_val("general.normalization_level", "Normalization Level", ...),
```

New shape:
```rust
SettingItem::enum_val(
    meta!("general.volume_normalization", "Volume Normalization",
          "Off · ReplayGain (track) · ReplayGain (album) · AGC (real-time)"),
    data.volume_normalization_label, "Off",
    vec!["Off", "ReplayGain (Track)", "ReplayGain (Album)", "AGC"],
),

// Visible only when mode == Agc:
if data.volume_normalization_mode.is_agc() {
    SettingItem::enum_val(
        meta!("general.normalization_level", "AGC Target Level",
              "Quiet (headroom) · Normal · Loud (boost)"),
        data.normalization_level, "Normal",
        vec!["Quiet", "Normal", "Loud"],
    )
}

// Visible when mode is RG-track or RG-album:
if data.volume_normalization_mode.is_replay_gain() {
    SettingItem::int(
        meta!("general.replay_gain_preamp_db", "ReplayGain Pre-amp",
              "Boost on top of ReplayGain (typical: 0 to +6 dB)"),
        data.replay_gain_preamp_db, 0, -15, 15, 1, "dB",
    );
    SettingItem::int(
        meta!("general.replay_gain_fallback_db", "Untagged Track Fallback",
              "dB applied to tracks with no ReplayGain tags"),
        data.replay_gain_fallback_db, 0, -15, 15, 1, "dB",
    );
    SettingItem::bool_val(
        meta!("general.replay_gain_fallback_to_agc", "Use AGC for Untagged Tracks",
              "Falls through to real-time AGC instead of the fixed dB above"),
        data.replay_gain_fallback_to_agc,
    );
    SettingItem::bool_val(
        meta!("general.replay_gain_prevent_clipping", "Prevent Clipping",
              "Clamp gain so track_peak × gain ≤ 1.0"),
        data.replay_gain_prevent_clipping,
    );
}
```

`PlaybackSettingsData<'a>` grows to mirror the new fields. Setting-change
dispatch in `src/update/settings.rs:739-778` adds five new arms (one for
each new key plus the type change on `volume_normalization`).

Test fixtures `src/views/settings/items.rs:805-806`, `:961-962` and
`entries.rs:105-106` need their types updated.

### Hotkey

No existing normalization hotkey. Defer.

If desired in a follow-up, mirror `ToggleCrossfade`'s pattern
(`HotkeyAction::CycleVolumeNormalizationMode` cycling 4-way Off →
RG-track → RG-album → AGC → Off). One universal cycle is cleaner
than separate AGC and RG hotkeys.

## 5. Documentation changes

### `nokkvi-docs/src/content/docs/reference/config.mdx`

Playback-settings table (lines 121-122 area):

- `volume_normalization`: type changes from boolean to enum; values
  `off | replay_gain_track | replay_gain_album | agc`; default
  `"off"`.
- `normalization_level`: keep, note "applies only when
  `volume_normalization = "agc"`".
- New `replay_gain_preamp_db`: `0`, dB, range -15 to +15.
- New `replay_gain_fallback_db`: `0`, dB, range -15 to +15.
- New `replay_gain_fallback_to_agc`: `false`.
- New `replay_gain_prevent_clipping`: `true`.

Update the "Crossfade and normalization" Aside (line 127): the
artifact applies only in AGC mode; either ReplayGain mode pre-levels
both streams.

### `nokkvi-docs/src/content/docs/guides/audio.mdx`

Rewrite the Volume Normalization section (lines 26-42):

- Lead with the four-mode picker.
- Explain ReplayGain (per-track and per-album), reading tags from the
  Subsonic API, common tag tools (rsgain, loudgain, foobar2000,
  mp3gain). Note Navidrome's R128 → ReplayGain conversion happens
  server-side; the client just sees dB.
- Pre-amp explanation: ReplayGain reference level is conservative
  (~-18 LUFS for track gain); +6 dB is typical for "modern loudness"
  listeners.
- Fallback: configurable dB plus optional AGC fallthrough.
- AGC stays as the alternative for un-tagged libraries.
- Update the "Crossfade and normalization" Aside: the artifact only
  occurs in AGC mode; ReplayGain modes don't have it.

## 6. Testing plan

### Bootstrap a tagged test corpus

The user has no naturally-tagged content. Synthesize:

```bash
mkdir -p /tmp/rg-test && cp /path/to/music/*.{mp3,flac,ogg} /tmp/rg-test/
rsgain easy /tmp/rg-test    # EBU R128, true peak + loudness range
ffprobe -hide_banner -show_format /tmp/rg-test/track1.flac | grep -i replaygain
```

Add `/tmp/rg-test` to Navidrome's library config (or symlink into the
existing music root); trigger a rescan. API smoke test: open the
song info modal in nokkvi — does it show
`replayGain: track=..., album=...`? That path is already wired
(`data/src/types/info_modal.rs:691-692`).

### A/B listening tests

Per mode, with a track that has both album_gain and track_gain:
1. `Off`, pre-amp 0 — baseline level.
2. `ReplayGainTrack`, pre-amp 0 — should attenuate by `track_gain` dB
   (typically -6 to -12 dB for modern masters).
3. Same, pre-amp +6 — should compensate halfway.
4. `ReplayGainAlbum` — for a low-DR album track, similar to track
   mode; for a high-DR album, preserves within-album loudness contrast.
5. `Agc` — should sound like today.

Verify the dB delta perceptually matches the tag value.

### Edge cases

- Track with only `album_gain` populated, mode `ReplayGainTrack` —
  silent cross-fallback should engage.
- Track with no tags at all — `replay_gain_fallback_db` applies
  (default 0 = unity).
- Track with no tags, `replay_gain_fallback_to_agc = true` — AGC
  should engage for that track only.
- Crossfade between two RG-tagged tracks of differing loudness — both
  sides at target throughout the fade; no level pump.
- Gapless transition between two `ReplayGainTrack` tracks of the same
  format with different `track_gain` — confirm gapless-reuse guard
  forces a fresh stream. Verify with debug logs.
- Seek mid-track in RG mode — recreated stream gets the same gain.
- Pre-amp +15 on track tagged `track_gain = +3` — limiter catches
  overshoot audibly. With `prevent_clipping = true`, clamping
  kicks in first.

### Mock-injection alternative

In `data/src/services/api/songs.rs:203-220` (`parse_song_response`),
debug-only override for fast iteration:
```rust
#[cfg(debug_assertions)]
if std::env::var("NOKKVI_FAKE_RG").is_ok() {
    for s in &mut songs {
        s.replay_gain = Some(ReplayGain {
            album_gain: Some(-9.0),
            track_gain: Some(-6.0),
            album_peak: Some(0.95),
            track_peak: Some(0.97),
        });
    }
}
```

### Unit tests

- `resolve_norm_for_track` truth table — every (mode, has_track_gain,
  has_album_gain, has_peak, fallback_to_agc) combination → expected
  `NormalizationConfig`. Lives in `data/src/audio/renderer.rs` test
  module.
- `VolumeNormalizationMode` serde round-trip (mirrors
  `NormalizationLevel` test at `player_settings.rs:660+`).
- `UserSettings` migration test: deserialize legacy
  `{"volume_normalization": true, "normalization_level": "normal", ...}`
  JSON; assert mode == `Agc`.

## 7. Open questions / known limitations

1. **OpenSubsonic `BaseGain`/`FallbackGain` extensions.** Not parsed
   today (`data/src/types/song.rs:9-18` only has the four standard
   fields). v2 enhancement.
2. **Hotkey** for cycling modes. Defer; one-cycle pattern in §4 if
   added later.
3. **Gapless guard** is opt-out by definition. Power users who care
   about gapless and use track mode get a fresh stream at every
   transition — slight cost, correct behaviour. Worth documenting.

## 8. Suggested commit / PR breakdown

Three natural seams:

### PR 1 — schema + persistence (no behavior change)

- New enum types in `player_settings.rs`.
- New fields on `PlayerSettings`, `UserSettings`, `TomlSettings` with
  serde defaults.
- Legacy-bool migration in `SettingsManager::new` and TOML loader.
- New persistence shims in `services/settings.rs` and
  `backend/settings.rs`; update `set_volume_normalization` signature
  through the persistence layer only.
- Settings round-trip + migration unit tests.
- The audio engine still ignores the new fields. Mode `Off` and `Agc`
  behave identically to today's `bool false` / `bool true`.

### PR 2 — audio chain + plumbing

- `NormalizationConfig` struct (`data/src/audio/normalization.rs`).
- `RodioOutput::create_stream` signature change.
- `AudioRenderer` field changes, `set_volume_normalization` signature
  change, `resolve_norm_for_track` helper, `pending_replay_gain`
  setters.
- `CustomAudioEngine::set_volume_normalization` signature change;
  `set_pending_replay_gain` and crossfade variant.
- All call sites in `services/playback.rs`,
  `backend/playback_controller.rs`, `update/playback.rs`,
  `update/settings.rs` wire `Song::replay_gain` through.
- Gapless-reuse guard for `ReplayGainTrack` mode.
- Renderer unit tests for `resolve_norm_for_track`.

### PR 3 — UI + docs

- `items_playback.rs` rework: enum picker, conditional widgets.
- `PlaybackSettingsData` field additions.
- Test fixtures in `items.rs`, `entries.rs`.
- Setting-change dispatch in `update/settings.rs` for new keys.
- nokkvi-docs updates in `audio.mdx` and `config.mdx`.

PR 1 is small and low-risk. PR 2 is the meat. PR 3 is mostly
mechanical.

## Critical files

- `/home/foogs/nokkvi/data/src/audio/rodio_output.rs`
- `/home/foogs/nokkvi/data/src/audio/renderer.rs`
- `/home/foogs/nokkvi/data/src/audio/engine.rs`
- `/home/foogs/nokkvi/data/src/types/player_settings.rs`
- `/home/foogs/nokkvi/data/src/types/settings.rs`
- `/home/foogs/nokkvi/data/src/types/toml_settings.rs`
- `/home/foogs/nokkvi/data/src/services/settings.rs`
- `/home/foogs/nokkvi/data/src/backend/settings.rs`
- `/home/foogs/nokkvi/data/src/backend/playback_controller.rs`
- `/home/foogs/nokkvi/data/src/services/playback.rs`
- `/home/foogs/nokkvi/src/state.rs`
- `/home/foogs/nokkvi/src/update/playback.rs`
- `/home/foogs/nokkvi/src/update/settings.rs`
- `/home/foogs/nokkvi/src/views/settings/items_playback.rs`
- `/home/foogs/nokkvi-docs/src/content/docs/reference/config.mdx`
- `/home/foogs/nokkvi-docs/src/content/docs/guides/audio.mdx`
