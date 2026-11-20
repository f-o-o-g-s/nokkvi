//! PipeWire native output sink.
//!
//! # Safety
//!
//! Every `unsafe { pw::sys::* }` block in this file is justified by the
//! following invariants, which are stable across the file:
//!
//! 1. The cloned `Rc<Stream>` (e.g. `stream_clone`, `stream_vol_clone`)
//!    outlives the closure it is captured into, because the closure is
//!    attached to the mainloop and the mainloop owns the stream.
//! 2. The mainloop is single-threaded — there is no concurrent mutation
//!    of the stream from another thread.
//! 3. The FFI calls (`pw_stream_set_control`, buffer reads, etc.) read
//!    their arguments synchronously and return before the caller's borrow
//!    ends; no `unsafe` pointer outlives its borrow.
//! 4. Audio buffer reads/writes stay within bounds derived from the
//!    negotiated `F32LE` format's `maxsize` and `stride` — see the process
//!    callback for the specific bound calculation.
//!
//! Per-block `SAFETY` comments below restate only the call-site-specific
//! constraints (which control / which buffer / which lifecycle) without
//! re-deriving the file-wide invariants above.

use std::{
    cell::Cell,
    rc::Rc,
    thread::{self, JoinHandle},
};

use anyhow::{Result, anyhow};
use pipewire as pw;
use pipewire::spa::pod::Pod;
use pw::properties::properties;
use rodio::Source;

const CHANNELS: u32 = 2;

/// PipeWire SPA property ID for per-channel volume array.
/// From `spa/param/props.h`: `SPA_PROP_channelVolumes = SPA_PROP_START_Audio + 8 = 0x10008`.
const SPA_PROP_CHANNEL_VOLUMES: u32 = 0x10008;

pub struct NativePipeWireSink {
    mixer: rodio::mixer::Mixer,
    rate: u32,
    handle: Option<JoinHandle<()>>,
    quit_tx: pw::channel::Sender<()>,
    title_tx: pw::channel::Sender<String>,
    volume_tx: pw::channel::Sender<f32>,
}

pub(crate) struct UserData {
    mixer_source: Box<dyn Source<Item = f32> + Send>,
}

/// Set the stream's per-channel volume (already curved to linear, 0.0–1.0).
fn set_channel_volumes(stream: &pw::stream::Stream, vol: f32) {
    tracing::trace!("🔊 PipeWire IPC: Setting channelVolumes to [{vol:.3}, {vol:.3}]");
    let volumes = [vol, vol];
    // SAFETY: see the module-level safety doc. Runs on the mainloop thread
    // (the volume receiver and the stream listener), and `pw_stream_set_control`
    // reads `volumes`, a stack array, synchronously.
    let result = unsafe {
        pw::sys::pw_stream_set_control(
            stream.as_raw_ptr(),
            SPA_PROP_CHANNEL_VOLUMES,
            volumes.len() as u32,
            volumes.as_ptr() as *mut f32,
        )
    };
    if result < 0 {
        tracing::warn!(
            "🔊 PipeWire: set_control(channelVolumes) failed: {}",
            result
        );
    }
}

/// WirePlumber stream property that opts a node out of its stream-restore
/// (`state-stream.lua`): no restoring saved volumes onto it, no saving them.
const STATE_RESTORE_PROPS: &str = "state.restore-props";

/// Node properties for the output stream.
///
/// `state.restore-props = false` keeps WirePlumber's hands off the node's
/// volume. Nokkvi owns it: the music bridge re-sends the user's level to every
/// freshly built node (`MusicOutputBridge::publish`). WirePlumber restores
/// asynchronously AFTER the node appears, so it lands on top of that re-send.
/// Its key is this node's `MEDIA_NAME` (the per-track title; the app name sits
/// on the context, not the node), so a key it has never seen restored the
/// 100% default: a bit-perfect sample-rate rebuild played the next track at
/// full volume under a slider showing the user's level, until the slider
/// moved. The saving side also left one state entry per track title.
fn stream_properties(
    node_name: &str,
    rate: u32,
    request_native_rate: bool,
) -> pw::properties::PropertiesBox {
    let mut stream_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::MEDIA_NAME => node_name,
        STATE_RESTORE_PROPS => "false",
    };
    if request_native_rate {
        // Ask PipeWire to run the graph clock at the source rate.
        // It switches the device when the device is idle AND the rate
        // is in `default.clock.allowed-rates`; otherwise it resamples.
        // A polite request (NODE_RATE), not a forced graph rate.
        let node_rate = format!("1/{rate}");
        stream_props.insert(*pw::keys::NODE_RATE, node_rate);
    }
    stream_props
}

impl NativePipeWireSink {
    /// Create a native PipeWire output stream.
    ///
    /// `node_name` is the graph `MEDIA_NAME` (what pavucontrol / the desktop
    /// mixer shows for this stream). The music sink passes "Nokkvi" (later
    /// overridden per-track via [`set_title`]); the SFX sink passes a distinct
    /// name so the two nodes are individually identifiable.
    pub fn new(
        mixer: rodio::mixer::Mixer,
        mixer_source: Box<dyn Source<Item = f32> + Send>,
        node_name: String,
        rate: u32,
        request_native_rate: bool,
    ) -> Result<Self> {
        let (quit_tx, quit_rx) = pw::channel::channel::<()>();
        let (title_tx, title_rx) = pw::channel::channel::<String>();
        let (volume_tx, volume_rx) = pw::channel::channel::<f32>();

        // Spawn the dedicated pw_out thread
        let handle = thread::Builder::new()
            .name("pw_nokkvi_out".to_owned())
            .spawn(move || {
                pw::init();

                let mainloop = match pw::main_loop::MainLoopRc::new(None) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!("Failed to initialize pipewire main_loop: {}", e);
                        return;
                    }
                };

                let context = match pw::context::ContextRc::new(
                    &mainloop,
                    Some(properties! {
                        *pw::keys::APP_NAME => "Nokkvi",
                        *pw::keys::APP_ICON_NAME => "nokkvi",
                    }),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Failed to initialize pipewire context: {}", e);
                        return;
                    }
                };

                let core = match context.connect_rc(None) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Failed to connect pipewire core: {}", e);
                        return;
                    }
                };

                let data = UserData { mixer_source };

                let stream_props = stream_properties(&node_name, rate, request_native_rate);
                let stream = match pw::stream::StreamRc::new(core, "Nokkvi-Playback", stream_props)
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to create stream: {}", e);
                        return;
                    }
                };

                let loop_clone = mainloop.clone();
                let stream_clone = stream.clone();

                let _title_receiver = title_rx.attach(mainloop.loop_(), move |title: String| {
                    tracing::debug!("🔊 PipeWire IPC: Updating graph MEDIA_NAME to {:?}", title);
                    let props = properties! {
                        *pw::keys::MEDIA_NAME => title.as_str()
                    };
                    // SAFETY: title_receiver path — see module-level safety doc.
                    // Updates `MEDIA_NAME` via `props.dict()`, a stack-local
                    // `SpaPod` that outlives the FFI call.
                    unsafe {
                        pw::sys::pw_stream_update_properties(
                            stream_clone.as_raw_ptr(),
                            props.dict().as_raw_ptr(),
                        );
                    }
                });

                // The last volume asked for. PipeWire drops a `set_control`
                // sent before the stream has negotiated its format (it does not
                // know the control yet, and still returns success), and the
                // bridge re-sends the user's level the moment a sink is built,
                // which is before that. So the level is kept here and applied
                // again once the format lands and once the stream runs (the
                // listener below); without that, every rebuilt node played at
                // its 100% default under the user's slider.
                let wanted_volume: Rc<Cell<Option<f32>>> = Rc::new(Cell::new(None));

                let stream_vol_clone = stream.clone();
                let wanted_for_receiver = Rc::clone(&wanted_volume);
                let _volume_receiver =
                    volume_rx.attach(mainloop.loop_(), move |linear_vol: f32| {
                        let vol = linear_vol.clamp(0.0, 1.0);
                        wanted_for_receiver.set(Some(vol));
                        set_channel_volumes(&stream_vol_clone, vol);
                    });

                // Quit via a pw::channel (like title/volume) so it wakes the
                // loop directly. The old std::mpsc quit was polled only inside
                // the `process` callback, so the `Drop` join during a sink
                // teardown could hang if the graph stopped driving the node
                // (e.g. the device suspended) and the callback never fired again.
                let _quit_receiver = quit_rx.attach(mainloop.loop_(), move |()| {
                    tracing::debug!("🔊 PipeWire IPC: quit received, stopping main loop");
                    loop_clone.quit();
                });

                let wanted_on_state = Rc::clone(&wanted_volume);
                let listener = stream
                    .add_local_listener_with_user_data(data)
                    .state_changed(move |stream, _, old, new| {
                        tracing::trace!("PipeWire Stream State: {:?} -> {:?}", old, new);
                        if matches!(
                            new,
                            pw::stream::StreamState::Paused | pw::stream::StreamState::Streaming
                        ) && let Some(vol) = wanted_on_state.get()
                        {
                            set_channel_volumes(stream, vol);
                        }
                    })
                    .param_changed(move |stream, _, id, param| {
                        if id == pw::spa::param::ParamType::Format.as_raw()
                            && param.is_some()
                            && let Some(vol) = wanted_volume.get()
                        {
                            set_channel_volumes(stream, vol);
                        }
                    })
                    .process(move |stream, user_data| {
                        match stream.dequeue_buffer() {
                            None => {}
                            Some(mut buffer) => {
                                let requested = buffer.requested() as usize;
                                let datas = buffer.datas_mut();
                                if datas.is_empty() {
                                    return;
                                }

                                let data = &mut datas[0];
                                let n_channels = 2;
                                let sample_size = std::mem::size_of::<f32>();
                                let stride = n_channels * sample_size;

                                let frames = std::cmp::min(
                                    requested,
                                    data.as_raw().maxsize as usize / stride,
                                );
                                let n_samples = frames * n_channels;

                                let chunk = data.chunk_mut();
                                *chunk.offset_mut() = 0;
                                *chunk.stride_mut() = stride as i32;
                                *chunk.size_mut() = (frames * stride) as u32;

                                if let Some(out_slice) = data.data() {
                                    // SAFETY: process callback — see module-level
                                    // safety doc. Buffer is the negotiated F32LE
                                    // format; `n_samples` is bounded by `frames *
                                    // n_channels` derived from maxsize/stride above.
                                    let out = unsafe {
                                        std::slice::from_raw_parts_mut(
                                            out_slice.as_mut_ptr() as *mut f32,
                                            n_samples,
                                        )
                                    };

                                    for sample in out.iter_mut() {
                                        *sample = user_data.mixer_source.next().unwrap_or(0.0);
                                    }
                                }
                            }
                        }
                    })
                    .register();

                let listener = match listener {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::error!("Failed to register stream listener: {}", e);
                        return;
                    }
                };

                let mut audio_info = pw::spa::param::audio::AudioInfoRaw::new();
                audio_info.set_format(pw::spa::param::audio::AudioFormat::F32LE);
                audio_info.set_rate(rate);
                audio_info.set_channels(CHANNELS);

                let obj = pw::spa::pod::Object {
                    type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                    id: pw::spa::param::ParamType::EnumFormat.as_raw(),
                    properties: audio_info.into(),
                };

                let values: Vec<u8> = match pw::spa::pod::serialize::PodSerializer::serialize(
                    std::io::Cursor::new(Vec::new()),
                    &pw::spa::pod::Value::Object(obj),
                ) {
                    Ok((cursor, _)) => cursor.into_inner(),
                    Err(e) => {
                        tracing::error!("Failed to serialize audio format pod: {:?}", e);
                        return;
                    }
                };

                let Some(pod) = Pod::from_bytes(&values) else {
                    tracing::error!("Failed to parse serialized audio format pod");
                    return;
                };
                let mut params = [pod];

                if let Err(e) = stream.connect(
                    pw::spa::utils::Direction::Output,
                    None,
                    pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                    &mut params,
                ) {
                    tracing::error!("Failed to connect stream: {}", e);
                    return;
                }

                // Run the main audio loop
                tracing::info!("🔊 Native PipeWire sink activated successfully.");
                mainloop.run();

                // Drop order matters: the title/volume receiver closures capture clones
                // of `stream`, so they must be dropped before `stream` to avoid
                // dangling raw pointers in the FFI calls.
                drop(_title_receiver);
                drop(_volume_receiver);
                drop(_quit_receiver);
                drop(listener);
                drop(stream);
            })
            .map_err(|e| anyhow!("Failed to spawn native pipewire thread: {e}"))?;

        Ok(Self {
            mixer,
            rate,
            handle: Some(handle),
            quit_tx,
            title_tx,
            volume_tx,
        })
    }

    /// The sample rate this sink's PipeWire stream was connected at.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Clone the title + volume IPC senders so another owner (e.g. the music
    /// output bridge) can drive this sink's PipeWire node without holding the
    /// sink itself. The senders are valid for the sink's lifetime.
    pub fn controls(&self) -> (pw::channel::Sender<String>, pw::channel::Sender<f32>) {
        (self.title_tx.clone(), self.volume_tx.clone())
    }

    pub fn mixer(&self) -> rodio::mixer::Mixer {
        self.mixer.clone()
    }
}

impl Drop for NativePipeWireSink {
    fn drop(&mut self) {
        // A closed channel means the PipeWire thread already ended.
        if self.quit_tx.send(()).is_err() {
            tracing::debug!("🔊 PipeWire thread already gone at sink drop");
        }
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            tracing::warn!("🔊 PipeWire thread panicked before it stopped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WirePlumber must not restore (or save) the node's volume: its
    /// asynchronous restore overwrote the level nokkvi re-sends to each rebuilt
    /// node, so a rate-switching rebuild played at 100%.
    #[test]
    fn stream_opts_out_of_wireplumber_volume_restore() {
        for native in [false, true] {
            let props = stream_properties("Nokkvi", 96_000, native);
            assert_eq!(props.get(STATE_RESTORE_PROPS), Some("false"));
            assert_eq!(props.get(*pw::keys::MEDIA_NAME), Some("Nokkvi"));
        }
        let native = stream_properties("Nokkvi", 96_000, true);
        assert_eq!(native.get(*pw::keys::NODE_RATE), Some("1/96000"));
        let shared = stream_properties("Nokkvi", 96_000, false);
        assert_eq!(shared.get(*pw::keys::NODE_RATE), None);
    }
}

/// Live check against the running PipeWire daemon: `cargo test -p nokkvi-data
/// a_volume_sent_as_the_stream_connects -- --ignored`. Opens a silent stream.
#[cfg(test)]
mod live_pipewire {
    use super::*;

    /// The `channelVolumes` PipeWire reports for the node named `name`.
    fn node_channel_volumes(name: &str) -> Option<Vec<f64>> {
        let out = std::process::Command::new("pw-dump").output().ok()?;
        let dump: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        dump.as_array()?.iter().find_map(|o| {
            let info = o.get("info")?;
            if info.get("props")?.get("media.name")?.as_str()? != name {
                return None;
            }
            let props = info.get("params")?.get("Props")?.as_array()?;
            props.iter().find_map(|p| {
                let vols = p.get("channelVolumes")?.as_array()?;
                Some(vols.iter().filter_map(serde_json::Value::as_f64).collect())
            })
        })
    }

    /// A volume sent the moment the stream connects (what the bridge does to
    /// every rebuilt sink) reaches the node. PipeWire dropped it, the node
    /// played at 100%, until the level was applied again on format/state.
    #[test]
    #[ignore = "needs a live PipeWire session"]
    fn a_volume_sent_as_the_stream_connects_reaches_the_node() {
        let name = "nokkvi-volume-probe";
        let (mixer, source) = rodio::mixer::mixer(
            std::num::NonZeroU16::new(2).expect("2 is nonzero"),
            std::num::NonZeroU32::new(48_000).expect("48000 is nonzero"),
        );
        let sink = NativePipeWireSink::new(mixer, Box::new(source), name.to_owned(), 48_000, false)
            .expect("open a PipeWire stream");
        let (_title, volume) = sink.controls();
        volume.send(0.001).expect("send the volume");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut seen = None;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
            seen = node_channel_volumes(name);
            if seen.as_ref().is_some_and(|v| !v.is_empty()) {
                break;
            }
        }
        let vols = seen.expect("the probe node shows up in pw-dump");
        assert!(
            !vols.is_empty() && vols.iter().all(|v| (v - 0.001).abs() < 1e-6),
            "the node took the volume sent as it connected, got {vols:?}"
        );
    }
}
