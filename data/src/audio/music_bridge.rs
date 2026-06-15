//! Lock-free bridge between the renderer-owned music sink and the parts of the
//! app that must reach it without taking the async engine lock.
//!
//! The music output sink (its rodio mixer + PipeWire node) is owned by the
//! [`AudioRenderer`](super::renderer::AudioRenderer), which rebuilds it at each
//! track's native rate in bit-perfect mode. Two consumers still need access:
//!
//! - the **SFX engine**, which adds UI-sound voices to the *current* music mixer
//!   (per the design decision to route SFX through the one music stream so the
//!   device can switch rate), and
//! - the **volume-drag / now-playing UI path**, which mirrors title + volume to
//!   the PipeWire node on a hot path that must stay lock-free.
//!
//! The renderer `publish`es the current mixer + an IPC forwarder whenever it
//! (re)builds the sink; consumers read through the bridge. A `parking_lot`
//! mutex (not `arc-swap`, which isn't a data-crate dependency) guards the two
//! swappable slots — both are touched rarely (publish on rebuild; SFX/volume on
//! user action), so the lock is uncontended.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use parking_lot::Mutex;
use rodio::mixer::Mixer;

/// A command forwarded to the music sink's PipeWire node.
pub enum MusicCommand {
    /// Update the node's `MEDIA_NAME` (the now-playing title).
    SetTitle(String),
    /// Set the node's channel volume (already curved to linear, 0.0–1.0).
    SetVolume(f32),
}

/// Lock-free forwarder that mirrors a [`MusicCommand`] to the live sink's
/// PipeWire node. Built by the sink (capturing its IPC senders) and published
/// into the bridge on each (re)build.
pub type MusicCommandFn = Box<dyn Fn(MusicCommand) + Send>;

/// Shared handle the renderer publishes into and SFX / the volume UI read from.
pub struct MusicOutputBridge {
    /// The current music mixer — SFX voices are added here so they share the
    /// one music stream. `None` until the renderer first builds the sink.
    mixer: Mutex<Option<Mixer>>,
    /// Forwards title/volume to the live sink's PipeWire node. `None` on the
    /// cpal fallback (no node volume) or before the first sink is built.
    command: Mutex<Option<MusicCommandFn>>,
    /// Whether the live music sink supports native PipeWire volume control.
    native_volume: AtomicBool,
    /// Last title pushed by the UI. Re-applied to the node on every `publish`
    /// so a per-track sink rebuild keeps the now-playing name. `None` until set.
    last_title: Mutex<Option<String>>,
    /// Last (already curved) volume pushed by the UI, as f32 bits. Re-applied on
    /// every `publish` so a freshly built node (which starts at full volume)
    /// doesn't reset the user's level. Defaults to 1.0 (the node default), so a
    /// publish before any volume is set is a no-op.
    last_volume: AtomicU32,
}

impl Default for MusicOutputBridge {
    fn default() -> Self {
        Self {
            mixer: Mutex::new(None),
            command: Mutex::new(None),
            native_volume: AtomicBool::new(false),
            last_title: Mutex::new(None),
            last_volume: AtomicU32::new(1.0_f32.to_bits()),
        }
    }
}

impl MusicOutputBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish a freshly (re)built music sink: its mixer, an IPC forwarder
    /// (`None` for cpal), and whether it has native PipeWire volume. Called by
    /// the renderer on every sink build / per-rate rebuild.
    ///
    /// A freshly built PipeWire node starts at its defaults (full volume, blank
    /// title), so after swapping in the new forwarder we immediately re-apply
    /// the last known title + volume. This keeps a per-track native-rate rebuild
    /// — and the deferred login publish — from resetting the node to 100% /
    /// blank.
    ///
    /// The forwarder swap AND the stored-value reapply happen UNDER A SINGLE
    /// `command` lock. `set_title`/`set_volume` take the same lock before they
    /// mutate their slot + forward, so a concurrent UI volume/title change can
    /// no longer interleave between "read the stored value" and "swap the
    /// forwarder" and leave the fresh node at the stale value (a lost update on
    /// a per-track sink rebuild). `last_title` is locked only while `command` is
    /// held, in that order — `set_title` uses the same order, so there is no
    /// lock-cycle. `mixer` is published before `command`; nothing takes `command`
    /// before `mixer`.
    pub fn publish(&self, mixer: Mixer, command: Option<MusicCommandFn>, native_volume: bool) {
        *self.mixer.lock() = Some(mixer);
        self.native_volume.store(native_volume, Ordering::Release);
        let mut command_slot = self.command.lock();
        *command_slot = command;
        if let Some(cmd) = command_slot.as_ref() {
            if let Some(title) = self.last_title.lock().clone() {
                cmd(MusicCommand::SetTitle(title));
            }
            cmd(MusicCommand::SetVolume(f32::from_bits(
                self.last_volume.load(Ordering::Acquire),
            )));
        }
    }

    /// Drop the live sink references (mixer + IPC forwarder) after the sink is
    /// torn down without a successful replacement — e.g. a failed native-rate
    /// rebuild. Consumers then see `mixer() == None` (SFX silently no-ops
    /// instead of feeding a dead mixer) and `set_title`/`set_volume` become
    /// no-ops, until the next successful `publish`. The last title/volume are
    /// kept so that publish re-applies them to the rebuilt node.
    pub fn clear(&self) {
        *self.mixer.lock() = None;
        self.native_volume.store(false, Ordering::Release);
        *self.command.lock() = None;
    }

    /// The current music mixer (a cheap clone of the rodio handle), or `None`
    /// before the first sink is built.
    pub fn mixer(&self) -> Option<Mixer> {
        self.mixer.lock().clone()
    }

    /// Whether the live music sink supports native PipeWire volume control.
    pub fn has_native_volume(&self) -> bool {
        self.native_volume.load(Ordering::Acquire)
    }

    /// Mirror the now-playing title to the music node (no-op on cpal). Stored so
    /// the next `publish` (a sink rebuild) re-applies it to the fresh node.
    ///
    /// The `command` lock is taken BEFORE the slot is written + forwarded, so a
    /// concurrent `publish` (sink rebuild) sees either the old or the new title
    /// atomically and never re-applies a stale one. Lock order is `command` then
    /// `last_title`, matching `publish`.
    pub fn set_title(&self, title: String) {
        let command_slot = self.command.lock();
        *self.last_title.lock() = Some(title.clone());
        if let Some(cmd) = command_slot.as_ref() {
            cmd(MusicCommand::SetTitle(title));
        }
    }

    /// Mirror the user's (already curved) volume to the music node (no-op on
    /// cpal). Stored so the next `publish` (a sink rebuild, or the deferred
    /// login publish) re-applies it instead of leaving the fresh node at 100%.
    ///
    /// Holds the `command` lock across the store + forward so a concurrent
    /// `publish` cannot read the pre-store volume and then re-apply it to the
    /// freshly built node (the lost-update race a per-track rebuild exposed).
    pub fn set_volume(&self, linear: f32) {
        let command_slot = self.command.lock();
        self.last_volume.store(linear.to_bits(), Ordering::Release);
        if let Some(cmd) = command_slot.as_ref() {
            cmd(MusicCommand::SetVolume(linear));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn dummy_mixer() -> Mixer {
        let (mixer, _source) = rodio::mixer::mixer(
            std::num::NonZeroU16::new(2).expect("2 is nonzero"),
            std::num::NonZeroU32::new(48_000).expect("48000 is nonzero"),
        );
        mixer
    }

    /// A freshly published sink (a per-track native-rate rebuild, or the
    /// deferred login publish) must receive the CURRENT title + volume, so the
    /// node isn't left at its 100% / blank defaults. Values set before any sink
    /// exists are stored and applied on the first publish.
    #[test]
    fn publish_reapplies_last_title_and_volume_to_the_new_node() {
        let bridge = MusicOutputBridge::new();

        // No sink yet — these are stored, not forwarded (command slot is None).
        bridge.set_title("Song A".to_owned());
        bridge.set_volume(0.3);

        // Publish a sink whose forwarder records every command it receives.
        let log: Arc<Mutex<Vec<MusicCommand>>> = Arc::new(Mutex::new(Vec::new()));
        let log_for_cmd = Arc::clone(&log);
        let cmd: MusicCommandFn = Box::new(move |c| log_for_cmd.lock().push(c));
        bridge.publish(dummy_mixer(), Some(cmd), true);

        let recorded = log.lock();
        assert!(
            recorded
                .iter()
                .any(|c| matches!(c, MusicCommand::SetTitle(t) if t == "Song A")),
            "publish must re-send the current title to the fresh node"
        );
        assert!(
            recorded
                .iter()
                .any(|c| matches!(c, MusicCommand::SetVolume(v) if (*v - 0.3).abs() < 1e-6)),
            "publish must re-send the current volume to the fresh node (not the node's 100% default)"
        );
    }

    /// Before any volume is set, the stored default is 1.0 (the node default),
    /// so a publish does not silence a fresh node.
    #[test]
    fn publish_before_any_volume_set_reapplies_unity_not_zero() {
        let bridge = MusicOutputBridge::new();
        let log: Arc<Mutex<Vec<MusicCommand>>> = Arc::new(Mutex::new(Vec::new()));
        let log_for_cmd = Arc::clone(&log);
        let cmd: MusicCommandFn = Box::new(move |c| log_for_cmd.lock().push(c));
        bridge.publish(dummy_mixer(), Some(cmd), true);

        let recorded = log.lock();
        assert!(
            recorded
                .iter()
                .any(|c| matches!(c, MusicCommand::SetVolume(v) if (*v - 1.0).abs() < 1e-6)),
            "default re-applied volume must be unity, never 0.0"
        );
    }

    /// A publish must re-apply the LATEST stored volume, not an earlier one. The
    /// store + forward and the publish reapply share the `command` lock, so the
    /// last value set always wins — the property the single-lock rewrite pins
    /// (closing the per-track-rebuild lost-update race).
    #[test]
    fn publish_reapplies_the_latest_volume_not_a_stale_one() {
        let bridge = MusicOutputBridge::new();
        bridge.set_volume(0.2);
        bridge.set_volume(0.7); // latest wins

        let log: Arc<Mutex<Vec<MusicCommand>>> = Arc::new(Mutex::new(Vec::new()));
        let log_for_cmd = Arc::clone(&log);
        let cmd: MusicCommandFn = Box::new(move |c| log_for_cmd.lock().push(c));
        bridge.publish(dummy_mixer(), Some(cmd), true);

        let recorded = log.lock();
        let last_volume = recorded
            .iter()
            .filter_map(|c| match c {
                MusicCommand::SetVolume(v) => Some(*v),
                MusicCommand::SetTitle(_) => None,
            })
            .next_back()
            .expect("a volume command");
        assert!(
            (last_volume - 0.7).abs() < 1e-6,
            "publish must re-apply the latest volume (0.7), got {last_volume}"
        );
    }

    /// `clear()` drops the live sink references so consumers see no mixer / no
    /// node volume (graceful degradation after a failed rebuild), but keeps the
    /// stored title + volume so the NEXT successful publish re-applies them.
    #[test]
    fn clear_drops_live_refs_but_keeps_stored_title_and_volume() {
        let bridge = MusicOutputBridge::new();
        bridge.set_title("Song A".to_owned());
        bridge.set_volume(0.3);
        bridge.publish(dummy_mixer(), None, true);
        assert!(bridge.mixer().is_some());
        assert!(bridge.has_native_volume());

        bridge.clear();
        assert!(bridge.mixer().is_none(), "clear drops the mixer");
        assert!(
            !bridge.has_native_volume(),
            "clear reports no native volume (no live sink)"
        );

        // The stored title/volume survive: the next publish re-applies them.
        let log: Arc<Mutex<Vec<MusicCommand>>> = Arc::new(Mutex::new(Vec::new()));
        let log_for_cmd = Arc::clone(&log);
        let cmd: MusicCommandFn = Box::new(move |c| log_for_cmd.lock().push(c));
        bridge.publish(dummy_mixer(), Some(cmd), true);
        let recorded = log.lock();
        assert!(
            recorded
                .iter()
                .any(|c| matches!(c, MusicCommand::SetTitle(t) if t == "Song A")),
            "publish after clear must re-apply the kept title"
        );
        assert!(
            recorded
                .iter()
                .any(|c| matches!(c, MusicCommand::SetVolume(v) if (*v - 0.3).abs() < 1e-6)),
            "publish after clear must re-apply the kept volume"
        );
    }
}
