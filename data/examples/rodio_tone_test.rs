// Standalone test: can rodio/cpal produce audible output on this system?
// Run: cargo run --example rodio_tone_test -p navidrome-data
#![allow(clippy::print_stdout, clippy::unwrap_used)]

use std::{num::NonZero, time::Duration};

use rodio::{DeviceSinkBuilder, buffer::SamplesBuffer};

fn main() {
    println!("Opening default audio device...");
    let mut sink = DeviceSinkBuilder::open_default_sink().expect("Failed to open audio output");
    sink.log_on_drop(false);

    let config = sink.config();
    println!(
        "Device: {}ch, {}Hz, format={:?}, buf={:?}",
        config.channel_count(),
        config.sample_rate(),
        config.sample_format(),
        config.buffer_size(),
    );

    // Generate a 440Hz sine wave at the sink's sample rate
    let sample_rate = config.sample_rate().get();
    let channels = config.channel_count().get() as u32;
    let duration_secs = 2.0;
    let num_samples = (sample_rate as f64 * duration_secs) as usize;

    let mut samples = Vec::with_capacity(num_samples * channels as usize);
    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;
        let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.3;
        for _ in 0..channels {
            samples.push(sample);
        }
    }

    println!(
        "Playing 440Hz tone for {}s ({} samples)...",
        duration_secs,
        samples.len()
    );
    let buffer = SamplesBuffer::new(
        config.channel_count(),
        NonZero::new(sample_rate).unwrap(),
        samples,
    );
    sink.mixer().add(buffer);

    // Wait for playback
    std::thread::sleep(Duration::from_secs_f64(duration_secs + 0.5));
    println!("Done.");
}
