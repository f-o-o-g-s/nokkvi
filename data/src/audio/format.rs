/// Audio sample format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    Unknown = 0,
    S16,
    U8,
    S24,
    S32,
    F32,
    F64,
}

/// Audio format metadata (sample format, rate, channels)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFormat {
    sample_format: SampleFormat,
    sample_format_planar: bool,
    channel_count: u32,
    sample_rate: u32,
}

impl AudioFormat {
    /// Create a new AudioFormat
    pub fn new(sample_format: SampleFormat, sample_rate: u32, channel_count: u32) -> Self {
        Self {
            sample_format,
            sample_format_planar: false,
            channel_count,
            sample_rate,
        }
    }

    /// Create an invalid/empty format
    pub fn invalid() -> Self {
        Self {
            sample_format: SampleFormat::Unknown,
            sample_format_planar: false,
            channel_count: 0,
            sample_rate: 0,
        }
    }

    /// Check if format is valid
    pub fn is_valid(&self) -> bool {
        self.sample_format != SampleFormat::Unknown
            && self.sample_rate > 0
            && self.channel_count > 0
    }

    /// Get sample format
    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    /// Get channel count
    pub fn channel_count(&self) -> u32 {
        self.channel_count
    }

    /// Get sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Interleaved samples per second (`sample_rate * channel_count`) — the
    /// rate at which decoded f32 samples flow through the ring buffers. An
    /// invalid format yields `0` (matching the open-coded products this
    /// replaces), which the decode loops and rebuffer logic already treat as
    /// "rate not yet known: never backpressure / never rebuffer".
    pub fn frame_rate(&self) -> u32 {
        self.sample_rate * self.channel_count
    }

    /// Calculate bytes needed for a given duration in milliseconds
    pub fn bytes_for_duration(&self, ms: u64) -> usize {
        if !self.is_valid() {
            return 0;
        }
        let frames = self.frames_for_duration(ms);
        self.bytes_for_frames(frames)
    }

    /// Calculate duration in milliseconds for a given byte count
    pub fn duration_for_bytes(&self, byte_count: usize) -> u64 {
        if !self.is_valid() {
            return 0;
        }
        let frames = self.frames_for_bytes(byte_count);
        self.duration_for_frames(frames)
    }

    /// Calculate bytes needed for a given frame count
    pub fn bytes_for_frames(&self, frame_count: usize) -> usize {
        frame_count * self.bytes_per_frame()
    }

    /// Calculate frame count for a given byte count
    pub fn frames_for_bytes(&self, byte_count: usize) -> usize {
        let bytes_per_frame = self.bytes_per_frame();
        if bytes_per_frame == 0 {
            return 0;
        }
        byte_count / bytes_per_frame
    }

    /// Calculate frame count for a given duration in milliseconds
    pub fn frames_for_duration(&self, ms: u64) -> usize {
        if !self.is_valid() {
            return 0;
        }
        ((ms as f64 / 1000.0) * self.sample_rate as f64) as usize
    }

    /// Calculate duration in milliseconds for a given frame count
    pub fn duration_for_frames(&self, frame_count: usize) -> u64 {
        if !self.is_valid() {
            return 0;
        }
        ((frame_count as f64 / self.sample_rate as f64) * 1000.0) as u64
    }

    /// Get bytes per frame
    pub fn bytes_per_frame(&self) -> usize {
        self.bytes_per_sample() * self.channel_count as usize
    }

    /// Get bytes per sample
    pub fn bytes_per_sample(&self) -> usize {
        match self.sample_format {
            SampleFormat::U8 => 1,
            SampleFormat::S16 => 2,
            SampleFormat::S24 => 3,
            SampleFormat::S32 => 4,
            SampleFormat::F32 => 4,
            SampleFormat::F64 => 8,
            SampleFormat::Unknown => 0,
        }
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::invalid()
    }
}

/// Convert a duration (ms) to an interleaved-sample count at the given
/// `frame_rate` (see [`AudioFormat::frame_rate`]). Single source of truth for
/// the engine's backpressure watermarks, the renderer's rebuffer thresholds,
/// and the radio jitter gate — all duration-based so they hold a constant TIME
/// budget at any sample rate.
///
/// Deliberately integer math (`(frame_rate * ms) / 1000`), NOT the f64 path of
/// [`AudioFormat::frames_for_duration`]: the watermark consumers were tuned
/// against this exact rounding and must stay bit-for-bit identical. `const fn`
/// so tests and derived consts can evaluate it at compile time.
pub(crate) const fn samples_for_duration(frame_rate: u32, ms: u64) -> usize {
    ((frame_rate as u64 * ms) / 1000) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rate_is_sample_rate_times_channels() {
        let fmt = AudioFormat::new(SampleFormat::S16, 44_100, 2);
        assert_eq!(fmt.frame_rate(), 88_200);
    }

    #[test]
    fn frame_rate_of_invalid_format_is_zero() {
        assert_eq!(AudioFormat::invalid().frame_rate(), 0);
    }

    #[test]
    fn samples_for_duration_one_second_is_frame_rate() {
        assert_eq!(samples_for_duration(88_200, 1000), 88_200);
    }

    #[test]
    fn samples_for_duration_pins_historical_radio_jitter_gate() {
        // 5s at 44.1k stereo == the historical hardcoded 441_000-sample radio
        // jitter gate in the engine decode loop — pins that the derived gate
        // is behavior-identical at the rate the literal was tuned for.
        assert_eq!(samples_for_duration(88_200, 5000), 441_000);
    }

    #[test]
    fn samples_for_duration_zero_frame_rate_is_zero() {
        assert_eq!(samples_for_duration(0, 5000), 0);
    }
}
