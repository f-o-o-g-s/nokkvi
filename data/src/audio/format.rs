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
