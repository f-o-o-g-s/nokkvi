//! The MilkDrop panel element: an iced `shader` program whose primitive drives
//! the engine offscreen in `prepare` and blits its retained composite in `draw`.
//!
//! The engine never sees iced's frame texture (`render(view)` clears whatever it
//! is given). It renders into its own retained texture, sized by the quality
//! cap, and a tiny blit scales that into the widget's viewport.

use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use iced::{
    Rectangle, mouse, wgpu,
    widget::shader::{self, Viewport},
};
use parking_lot::Mutex;
use particle_milkdrop::{MilkdropRenderer, MilkdropResizeDebouncer};
use tracing::{debug, warn};

use super::{
    MILKDROP_FRAME_INTERVAL, desired_render_size, run_in_error_scopes,
    shared::{GpuHandles, MilkdropShared},
};

/// The widget program. Event-transparent (it captures nothing), so the panel's
/// context menu and the theater hover still see every event.
#[derive(Clone)]
pub(crate) struct MilkdropProgram {
    pub shared: Arc<MilkdropShared>,
}

impl<Message> shader::Program<Message> for MilkdropProgram {
    type State = ();
    type Primitive = MilkdropPrimitive;

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        // Paused or stopped: no redraws, the last frame stays, the GPU idles.
        self.shared
            .running
            .load(Ordering::Relaxed)
            .then(shader::Action::request_redraw)
    }

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        MilkdropPrimitive {
            shared: self.shared.clone(),
        }
    }
}

pub(crate) struct MilkdropPrimitive {
    shared: Arc<MilkdropShared>,
}

impl std::fmt::Debug for MilkdropPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MilkdropPrimitive")
            .field("shared", &self.shared)
            .finish()
    }
}

/// The renderer on screen plus what the blit needs to show it.
struct Slot {
    generation: u64,
    name: String,
    /// `MilkdropRenderer` is `Send` but not `Sync` (a `RefCell` in its EEL
    /// program); iced's pipeline storage needs `Sync`. Only `prepare` and
    /// `trim` touch it, so the lock is uncontended.
    renderer: Mutex<MilkdropRenderer>,
    bind_group: wgpu::BindGroup,
    /// The renderer's current output size (`dimensions()`).
    size: (u32, u32),
    frames_rendered: u64,
    /// The cover version this renderer shows (0 = recheck on the next frame).
    cover_version: u64,
    last_advance: Instant,
    /// The analysis rate last handed to the engine.
    rate_set: Option<f32>,
    debouncer: MilkdropResizeDebouncer,
}

pub(crate) struct MilkdropPipeline {
    blit: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    format: wgpu::TextureFormat,
    /// Captured on the first `prepare` (`Pipeline::new` cannot receive it).
    shared: Option<Arc<MilkdropShared>>,
    slot: Option<Slot>,
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("milkdrop blit bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

impl MilkdropPipeline {
    /// Drop the renderer and any waiting one whose generation is below the
    /// app's release watermark.
    fn enforce_release(&mut self, shared: &MilkdropShared) {
        let below = shared.released_below.load(Ordering::Acquire);
        if self
            .slot
            .as_ref()
            .is_some_and(|slot| slot.generation < below)
            && let Some(slot) = self.slot.take()
        {
            debug!(preset = %slot.name, "milkdrop: renderer released");
        }
        let mut built = shared.built.lock();
        if built.as_ref().is_some_and(|b| b.generation < below) {
            built.take();
        }
    }

    /// Capture iced's device on first sight and again whenever it changes.
    /// Everything built on an old device is dropped.
    fn track_device(
        &mut self,
        shared: &MilkdropShared,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        let mut gpu = shared.gpu.lock();
        let same = gpu.as_ref().is_some_and(|g| *g.device == *device);
        if same {
            return;
        }
        let epoch = shared.epoch_counter.fetch_add(1, Ordering::AcqRel) + 1;
        debug!(epoch, format = ?self.format, "milkdrop: captured the GPU device");
        *gpu = Some(GpuHandles {
            device: Arc::new(device.clone()),
            queue: Arc::new(queue.clone()),
            format: self.format,
            epoch,
        });
        drop(gpu);
        self.slot = None;
        shared.built.lock().take();
    }

    /// Swap in a finished renderer if it is current for this device.
    fn swap_in_built(&mut self, shared: &MilkdropShared, device: &wgpu::Device) {
        let Some(built) = shared.built.lock().take() else {
            return;
        };
        let epoch = shared.gpu_epoch();
        let generation = shared.current_generation.load(Ordering::Acquire);
        if Some(built.epoch) != epoch || built.generation != generation {
            debug!(preset = %built.name, "milkdrop: dropped a stale renderer");
            return;
        }
        let size = built.renderer.dimensions();
        let bind_group = make_bind_group(
            device,
            &self.layout,
            &self.sampler,
            &self.params,
            built.renderer.retained_comp_view(),
        );
        debug!(preset = %built.name, ?size, "milkdrop: renderer swapped in");
        self.slot = Some(Slot {
            generation: built.generation,
            name: built.name,
            renderer: Mutex::new(built.renderer),
            bind_group,
            size,
            frames_rendered: 0,
            cover_version: 0,
            // Due at once: the first advance happens this frame.
            last_advance: Instant::now()
                .checked_sub(MILKDROP_FRAME_INTERVAL)
                .unwrap_or_else(Instant::now),
            rate_set: None,
            debouncer: MilkdropResizeDebouncer::default(),
        });
    }
}

impl shader::Primitive for MilkdropPrimitive {
    type Pipeline = MilkdropPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let shared = pipeline
            .shared
            .get_or_insert_with(|| self.shared.clone())
            .clone();
        shared.mark_mounted();
        pipeline.track_device(&shared, device, queue);
        pipeline.enforce_release(&shared);

        let running = shared.running.load(Ordering::Relaxed);
        if running {
            pipeline.swap_in_built(&shared, device);
        }

        // Desired size: the panel in physical px, the shorter side capped.
        let scale = viewport.scale_factor();
        let physical = (
            (bounds.width * scale).round().max(1.0) as u32,
            (bounds.height * scale).round().max(1.0) as u32,
        );
        let desired =
            desired_render_size(physical, shared.quality_short_side.load(Ordering::Relaxed));
        *shared.render_size.lock() = desired;

        let now = Instant::now();
        let MilkdropPipeline {
            slot: slot_opt,
            layout,
            sampler,
            params,
            ..
        } = pipeline;
        let Some(slot) = slot_opt.as_mut() else {
            return;
        };
        let mut lost = false;

        // A new playing cover: swap it into the live renderer in place.
        let cover_version = shared.cover_version();
        if cover_version != slot.cover_version {
            slot.cover_version = cover_version;
            if let Some(cover) = shared.cover.lock().clone() {
                let mut renderer = slot.renderer.lock();
                if let Err(e) = run_in_error_scopes(device, || {
                    renderer.set_named_texture(
                        super::COVER_TEXTURE,
                        &cover.rgba,
                        cover.width,
                        cover.height,
                    )
                }) {
                    warn!(preset = %slot.name, "milkdrop: GPU error updating the cover: {e}");
                    lost = true;
                }
            }
        }

        if desired != slot.size {
            slot.debouncer.request(desired.0, desired.1, now);
        } else {
            slot.debouncer.clear();
        }
        if let Some((w, h)) = slot.debouncer.take_ready(now) {
            let mut renderer = slot.renderer.lock();
            // Render once right away: `try_resize` rebuilds the retained comp
            // texture black, and a paused (or not-yet-due) panel would blit
            // that until the next advance.
            let resized = run_in_error_scopes(device, || {
                renderer
                    .try_resize(w, h)
                    .map(|()| renderer.render_to_retained_comp())
            });
            match resized {
                Ok(Ok(())) => {
                    slot.size = renderer.dimensions();
                    // The retained view is replaced on every resize.
                    slot.bind_group = make_bind_group(
                        device,
                        layout,
                        sampler,
                        params,
                        renderer.retained_comp_view(),
                    );
                }
                // Keeps the old size; the blit scales it.
                Ok(Err(e)) => warn!("milkdrop: resize to {w}x{h} refused: {e:?}"),
                Err(e) => {
                    warn!(preset = %slot.name, "milkdrop: GPU error on resize: {e}");
                    lost = true;
                }
            }
        }

        if running && !lost && now >= slot.last_advance + MILKDROP_FRAME_INTERVAL {
            // Catch-up schedule: 60 advances a second at 144 Hz (a plain
            // `last = now` gate gives ~48), re-based after a long stall.
            slot.last_advance += MILKDROP_FRAME_INTERVAL;
            if now.saturating_duration_since(slot.last_advance) > MILKDROP_FRAME_INTERVAL * 2 {
                slot.last_advance = now;
            }
            let features = *shared.features.lock();
            let mut renderer = slot.renderer.lock();
            if let Some(rate) = shared.analysis_rate()
                && slot.rate_set != Some(rate)
            {
                renderer.set_enhanced_audio_sample_rate(rate);
                slot.rate_set = Some(rate);
            }
            super::apply_features(&mut renderer, &features);
            if slot.frames_rendered == 0 {
                // A fresh renderer's first frame runs under error scopes: a
                // validation error must never reach wgpu's panicking handler.
                if let Err(e) = run_in_error_scopes(device, || renderer.render_to_retained_comp()) {
                    warn!(preset = %slot.name, "milkdrop: GPU error on first frame: {e}");
                    lost = true;
                }
            } else {
                renderer.render_to_retained_comp();
            }
            if !lost {
                if slot.frames_rendered == 0 {
                    shared.mark_shown(slot.generation);
                }
                slot.frames_rendered += 1;
            }
        }

        if lost {
            // The app's tick sees the flag and loads another preset.
            *slot_opt = None;
            shared.slot_lost.store(true, Ordering::Release);
        }
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        // Nothing until the first frame: a fresh renderer is black, and the cover
        // underneath is the better placeholder.
        if let Some(slot) = pipeline.slot.as_ref()
            && slot.frames_rendered > 0
        {
            render_pass.set_pipeline(&pipeline.blit);
            render_pass.set_bind_group(0, &slot.bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }
        true
    }
}

impl shader::Pipeline for MilkdropPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        use wgpu::util::DeviceExt;

        debug!(
            ?format,
            srgb = format.is_srgb(),
            "milkdrop: blit target format"
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("milkdrop blit shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(super::BLIT_WGSL)),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("milkdrop blit bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("milkdrop blit pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("milkdrop blit pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("milkdrop blit sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let decode_srgb = u32::from(format.is_srgb());
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("milkdrop blit params"),
            contents: bytemuck::cast_slice(&[decode_srgb, 0, 0, 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Self {
            blit,
            layout,
            sampler,
            params,
            format,
            shared: None,
            slot: None,
        }
    }

    fn trim(&mut self) {
        // Iced calls this every frame, mounted or not, so leaving the mode from
        // a view without the panel still frees the renderer's GPU memory.
        if let Some(shared) = self.shared.clone() {
            self.enforce_release(&shared);
        }
    }
}
