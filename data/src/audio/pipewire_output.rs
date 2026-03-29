use std::thread::{self, JoinHandle};

use anyhow::Result;
use pipewire as pw;
use pipewire::spa::pod::Pod;
use pw::properties::properties;
use rodio::Source;

pub struct NativePipeWireSink {
    mixer: rodio::mixer::Mixer,
    handle: Option<JoinHandle<()>>,
    quit_tx: std::sync::mpsc::Sender<()>,
    title_tx: pw::channel::Sender<String>,
}

pub struct UserData {
    mixer_source: Box<dyn Source<Item = f32> + Send>,
}

impl NativePipeWireSink {
    pub fn new(
        mixer: rodio::mixer::Mixer,
        mixer_source: Box<dyn Source<Item = f32> + Send>,
    ) -> Result<Self> {
        let (quit_tx, quit_rx) = std::sync::mpsc::channel();
        let (title_tx, title_rx) = pw::channel::channel::<String>();
        let mixer_clone = mixer.clone();

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

                let data = UserData {
                    mixer_source,
                };

                let stream = match pw::stream::StreamRc::new(
                    core,
                    "Nokkvi-Playback",
                    properties! {
                        *pw::keys::MEDIA_TYPE => "Audio",
                        *pw::keys::MEDIA_CATEGORY => "Playback",
                        *pw::keys::MEDIA_ROLE => "Music",
                        *pw::keys::MEDIA_NAME => "Nokkvi",
                    },
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to create stream: {}", e);
                        return;
                    }
                };

                let loop_clone = mainloop.clone();
                let stream_clone = stream.clone();

                let _title_receiver = title_rx.attach(mainloop.loop_(), move |title: String| {
                    tracing::info!("🔊 PipeWire IPC: Updating graph MEDIA_NAME to {:?}", title);
                    let props = properties! {
                        *pw::keys::MEDIA_NAME => title.as_str()
                    };
                    unsafe {
                        pw::sys::pw_stream_update_properties(
                            stream_clone.as_raw_ptr(),
                            props.dict().as_raw_ptr(),
                        );
                    }
                });

                let listener = stream
                    .add_local_listener_with_user_data(data)
                    .state_changed(|_, _, old, new| {
                        tracing::trace!("PipeWire Stream State: {:?} -> {:?}", old, new);
                    })
                    .process(move |stream, user_data| {
                        if quit_rx.try_recv().is_ok() {
                            loop_clone.quit();
                            return;
                        }

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
                    .register()
                    .expect("Failed to register stream listener");

                let mut audio_info = pw::spa::param::audio::AudioInfoRaw::new();
                audio_info.set_format(pw::spa::param::audio::AudioFormat::F32LE);
                audio_info.set_rate(48000);
                audio_info.set_channels(2);

                let obj = pw::spa::pod::Object {
                    type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                    id: pw::spa::param::ParamType::EnumFormat.as_raw(),
                    properties: audio_info.into(),
                };

                let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
                    std::io::Cursor::new(Vec::new()),
                    &pw::spa::pod::Value::Object(obj),
                )
                .unwrap()
                .0
                .into_inner();

                let mut params = [Pod::from_bytes(&values).unwrap()];

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

                // Keep things alive
                drop(_title_receiver);
                drop(listener);
                drop(stream);
            })
            .expect("Failed to spawn native pipewire loop");

        Ok(Self {
            mixer: mixer_clone,
            handle: Some(handle),
            quit_tx,
            title_tx,
        })
    }

    pub fn set_title(&self, title: String) {
        tracing::info!("🔊 SfxEngine routing title update downward: {:?}", title);
        let _ = self.title_tx.send(title);
    }

    pub fn mixer(&self) -> rodio::mixer::Mixer {
        self.mixer.clone()
    }
}

impl Drop for NativePipeWireSink {
    fn drop(&mut self) {
        let _ = self.quit_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
