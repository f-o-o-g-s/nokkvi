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
    transition::{BlitPlan, SlotPair, fade_seed},
};

/// Width of the soft band where two presets mix, in pattern units (0..1).
const FADE_SOFTNESS: f32 = 0.2;

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
    /// The renderer on screen and, during a crossfade, the one it replaces.
    slots: SlotPair<Slot>,
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
        for slot in self.slots.release_below(below, |slot| slot.generation) {
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
        self.slots.take_all();
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
        let mut renderer = built.renderer;
        // Start from the picture on screen, as MilkDrop does: presets that grow
        // out of the previous image (self-sharpening feedback) have nothing to
        // grow from on black. Mid-fade that is the side that dominates the
        // picture. Declines on its own when the sizes differ.
        if let Some(old) = self.slots.seed_source() {
            let old = old.renderer.lock();
            match run_in_error_scopes(device, || renderer.seed_feedback_from(&old)) {
                Ok(seeded) => debug!(preset = %built.name, seeded, "milkdrop: picture handoff"),
                Err(e) => {
                    warn!(preset = %built.name, "milkdrop: GPU error on picture handoff: {e}");
                }
            }
        }
        let size = renderer.dimensions();
        let bind_group = make_bind_group(
            device,
            &self.layout,
            &self.sampler,
            &self.params,
            renderer.retained_comp_view(),
        );
        debug!(preset = %built.name, ?size, "milkdrop: renderer swapped in");
        let incoming = Slot {
            generation: built.generation,
            name: built.name,
            renderer: Mutex::new(renderer),
            bind_group,
            size,
            frames_rendered: 0,
            cover_version: built.cover_version,
            // Due at once: the first advance happens this frame.
            last_advance: Instant::now()
                .checked_sub(MILKDROP_FRAME_INTERVAL)
                .unwrap_or_else(Instant::now),
            rate_set: None,
            debouncer: MilkdropResizeDebouncer::default(),
        };
        let total = shared.crossfade_frames.load(Ordering::Relaxed);
        let seed = fade_seed(built.generation);
        let dropped = self
            .slots
            .arrive(incoming, total, seed, |slot| slot.frames_rendered > 0);
        for slot in dropped {
            debug!(preset = %slot.name, "milkdrop: renderer replaced");
        }
        if let Some(fade) = self.slots.fade() {
            debug!(
                frames = total,
                pattern = ?fade.pattern,
                seed,
                "milkdrop: crossfade started"
            );
        }
    }

    /// Write this frame's blit parameters from the blit plan (`draw` follows
    /// the same plan). `trim` has no queue, so the end state is written here.
    fn write_params(&self, queue: &wgpu::Queue, aspect: f32) {
        let (pattern, seed, progress) = match self.slots.blit_plan(|slot| slot.frames_rendered) {
            BlitPlan::Mix { progress, fade, .. } => (fade.pattern.shader_id(), fade.seed, progress),
            BlitPlan::Solo(_) | BlitPlan::Nothing => (0, 0, 1.0),
        };
        let words: [u32; 8] = [
            u32::from(self.format.is_srgb()),
            pattern,
            seed,
            0,
            progress.to_bits(),
            FADE_SOFTNESS.to_bits(),
            aspect.to_bits(),
            0,
        ];
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&words));
    }
}

impl Slot {
    /// A new playing cover: swap it into the live renderer in place. Returns
    /// false on a GPU error (the renderer is lost).
    fn sync_cover(&mut self, shared: &MilkdropShared, device: &wgpu::Device) -> bool {
        let cover_version = shared.cover_version();
        if cover_version == self.cover_version {
            return true;
        }
        self.cover_version = cover_version;
        let Some(cover) = shared.cover.lock().clone() else {
            return true;
        };
        let mut renderer = self.renderer.lock();
        if let Err(e) = run_in_error_scopes(device, || {
            renderer.set_named_texture(super::COVER_TEXTURE, &cover.rgba, cover.width, cover.height)
        }) {
            warn!(preset = %self.name, "milkdrop: GPU error updating the cover: {e}");
            return false;
        }
        true
    }

    /// Debounced resize toward `desired`, then one render (paused or not).
    /// Returns false on a GPU error (the renderer is lost).
    fn maybe_resize(
        &mut self,
        desired: (u32, u32),
        now: Instant,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        params: &wgpu::Buffer,
    ) -> bool {
        if desired != self.size {
            self.debouncer.request(desired.0, desired.1, now);
        } else {
            self.debouncer.clear();
        }
        let Some((w, h)) = self.debouncer.take_ready(now) else {
            return true;
        };
        let renderer = self.renderer.get_mut();
        // Render once right away: `try_resize` rebuilds the retained comp
        // texture black, and a paused (or not-yet-due) panel would blit that
        // until the next advance.
        let resized = run_in_error_scopes(device, || {
            renderer
                .try_resize(w, h)
                .map(|()| renderer.render_to_retained_comp())
        });
        match resized {
            Ok(Ok(())) => {
                self.size = renderer.dimensions();
                // The retained view is replaced on every resize.
                self.bind_group = make_bind_group(
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
                warn!(preset = %self.name, "milkdrop: GPU error on resize: {e}");
                return false;
            }
        }
        true
    }

    /// Feed the latest analysis frame and render one frame. A fresh renderer's
    /// first frame runs under error scopes. Returns false on a GPU error.
    fn advance(
        &mut self,
        shared: &MilkdropShared,
        device: &wgpu::Device,
        features: &particle_audio::Features,
    ) -> bool {
        let renderer = self.renderer.get_mut();
        if let Some(rate) = shared.analysis_rate()
            && self.rate_set != Some(rate)
        {
            renderer.set_enhanced_audio_sample_rate(rate);
            self.rate_set = Some(rate);
        }
        super::apply_features(
            renderer,
            features,
            shared.analysis_rate().unwrap_or(44_100.0),
        );
        if self.frames_rendered == 0 {
            // A validation error must never reach wgpu's panicking handler.
            if let Err(e) = run_in_error_scopes(device, || renderer.render_to_retained_comp()) {
                warn!(preset = %self.name, "milkdrop: GPU error on first frame: {e}");
                return false;
            }
        } else {
            renderer.render_to_retained_comp();
        }
        self.frames_rendered += 1;
        true
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
        let aspect = if bounds.height > 0.0 {
            bounds.width / bounds.height
        } else {
            1.0
        };
        let MilkdropPipeline {
            slots,
            layout,
            sampler,
            params,
            ..
        } = &mut *pipeline;

        // Cover and resize reach both sides of a fade. The outgoing is dropped
        // on a GPU error without blaming it (by now the name on screen is the
        // incoming's, and it already ran a whole interval).
        let (current, outgoing) = slots.both_mut();
        let mut lost = current.is_some_and(|slot| {
            !(slot.sync_cover(&shared, device)
                && slot.maybe_resize(desired, now, device, layout, sampler, params))
        });
        let outgoing_lost = outgoing.is_some_and(|slot| {
            !(slot.sync_cover(&shared, device)
                && slot.maybe_resize(desired, now, device, layout, sampler, params))
        });
        if outgoing_lost && let Some(slot) = slots.end_fade() {
            warn!(preset = %slot.name, "milkdrop: crossfade cut short, outgoing renderer lost");
        }

        if running
            && !lost
            && let (Some(slot), outgoing) = slots.both_mut()
            && now >= slot.last_advance + MILKDROP_FRAME_INTERVAL
        {
            // Catch-up schedule: 60 advances a second at 144 Hz (a plain
            // `last = now` gate gives ~48), re-based after a long stall. The
            // outgoing side of a fade advances in lockstep on this schedule.
            slot.last_advance += MILKDROP_FRAME_INTERVAL;
            if now.saturating_duration_since(slot.last_advance) > MILKDROP_FRAME_INTERVAL * 2 {
                slot.last_advance = now;
            }
            let features = *shared.features.lock();
            if slot.advance(&shared, device, &features) {
                if slot.frames_rendered == 1 {
                    shared.mark_shown(slot.generation);
                }
                // Steady-state advances are unscoped, so this never fails.
                if let Some(outgoing) = outgoing {
                    outgoing.advance(&shared, device, &features);
                }
                if let Some(done) = slots.advance_fade() {
                    debug!(preset = %done.name, "milkdrop: crossfade finished");
                }
            } else {
                lost = true;
            }
        }

        if lost {
            // The app's tick takes the generation and blames that load. Losing
            // the incoming ends any fade: the cover shows until the next load.
            let dropped = slots.take_all();
            if let Some(slot) = dropped.first() {
                shared.slot_lost.store(slot.generation, Ordering::Release);
            }
        }

        pipeline.write_params(queue, aspect);
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        // Nothing until the first frame: a fresh renderer is black, and the cover
        // underneath is the better placeholder.
        let (incoming, outgoing) = match pipeline.slots.blit_plan(|slot| slot.frames_rendered) {
            BlitPlan::Nothing => return true,
            // One group bound twice: progress 1 shows it alone.
            BlitPlan::Solo(slot) => (slot, slot),
            BlitPlan::Mix {
                incoming, outgoing, ..
            } => (incoming, outgoing),
        };
        render_pass.set_pipeline(&pipeline.blit);
        render_pass.set_bind_group(0, &incoming.bind_group, &[]);
        render_pass.set_bind_group(1, &outgoing.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
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
            // Group 0 the incoming preset, group 1 the outgoing: exactly iced's
            // `max_bind_groups: 2`.
            bind_group_layouts: &[Some(&layout), Some(&layout)],
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
        // Rewritten by `prepare` every frame (`write_params`).
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("milkdrop blit params"),
            contents: bytemuck::cast_slice(&[
                u32::from(format.is_srgb()),
                0,
                0,
                0,
                1.0f32.to_bits(),
                FADE_SOFTNESS.to_bits(),
                1.0f32.to_bits(),
                0,
            ]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            blit,
            layout,
            sampler,
            params,
            format,
            shared: None,
            slots: SlotPair::default(),
        }
    }

    fn trim(&mut self) {
        // Iced calls this every frame, mounted or not, so leaving the mode from
        // a view without the panel still frees the renderer's GPU memory.
        if let Some(shared) = self.shared.clone() {
            self.enforce_release(&shared);
            // Nobody is watching: end a fade at once (the jump is invisible)
            // and free the second renderer.
            if !shared.mounted_recently()
                && let Some(slot) = self.slots.end_fade()
            {
                debug!(preset = %slot.name, "milkdrop: crossfade ended off screen");
            }
        }
    }
}
