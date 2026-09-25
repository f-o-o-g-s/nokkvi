//! MilkDrop mode: plays MilkDrop presets through the `particle-milkdrop` engine.
//!
//! The FFT worker feeds a `particle_audio::Analyzer` from the same visualizer tap
//! the other modes use (`VisualizerState::tick`) and publishes its latest
//! `Features` into [`MilkdropShared`], which the render side reads.

pub(crate) mod shared;

pub(crate) use shared::MilkdropShared;
