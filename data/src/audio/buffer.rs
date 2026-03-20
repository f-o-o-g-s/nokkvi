use crate::audio::format::AudioFormat;

/// Audio buffer containing PCM data with format and timing information
#[derive(Clone)]
pub struct AudioBuffer {
    format: AudioFormat,
    data: Vec<u8>,
    start_time: u64, // milliseconds
}

impl AudioBuffer {
    /// Create a new empty buffer with format and start time
    pub fn new(format: AudioFormat, start_time: u64) -> Self {
        Self {
            format,
            data: Vec::new(),
            start_time,
        }
    }

    /// Create a new buffer from data
    pub fn from_data(data: Vec<u8>, format: AudioFormat, start_time: u64) -> Self {
        Self {
            format,
            data,
            start_time,
        }
    }

    /// Create an invalid buffer
    pub fn invalid() -> Self {
        Self {
            format: AudioFormat::invalid(),
            data: Vec::new(),
            start_time: 0,
        }
    }

    /// Check if buffer is valid
    pub fn is_valid(&self) -> bool {
        self.format.is_valid() && !self.data.is_empty()
    }

    /// Get the format
    pub fn format(&self) -> &AudioFormat {
        &self.format
    }

    /// Get frame count
    pub fn frame_count(&self) -> usize {
        if !self.is_valid() {
            return 0;
        }
        self.format.frames_for_bytes(self.data.len())
    }

    /// Get sample count (frames * channels)
    pub fn sample_count(&self) -> usize {
        self.frame_count() * self.format.channel_count() as usize
    }

    /// Get byte count
    pub fn byte_count(&self) -> usize {
        self.data.len()
    }

    /// Get start time in milliseconds
    pub fn start_time(&self) -> u64 {
        self.start_time
    }

    /// Get duration in milliseconds
    pub fn duration(&self) -> u64 {
        if !self.is_valid() {
            return 0;
        }
        self.format.duration_for_bytes(self.data.len())
    }

    /// Get raw data slice
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get data as Vec<u8>
    pub fn to_vec(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// Reserve capacity
    pub fn reserve(&mut self, size: usize) {
        self.data.reserve(size);
    }

    /// Resize buffer
    pub fn resize(&mut self, size: usize) {
        self.data.resize(size, 0);
    }

    /// Append data
    pub fn append(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// Clear buffer
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Reset buffer (clear and set to invalid format)
    pub fn reset(&mut self) {
        self.data.clear();
        self.format = AudioFormat::invalid();
        self.start_time = 0;
    }

    /// Scale buffer by volume (0.0-1.0)
    pub fn scale(&mut self, volume: f64) {
        if !self.is_valid() || !(0.0..=1.0).contains(&volume) {
            return;
        }

        match self.format.sample_format() {
            crate::audio::format::SampleFormat::S16 => {
                let samples: &mut [i16] = bytemuck::cast_slice_mut(&mut self.data);
                for sample in samples {
                    *sample = (*sample as f64 * volume) as i16;
                }
            }
            crate::audio::format::SampleFormat::S32 => {
                let samples: &mut [i32] = bytemuck::cast_slice_mut(&mut self.data);
                for sample in samples {
                    *sample = (*sample as f64 * volume) as i32;
                }
            }
            crate::audio::format::SampleFormat::F32 => {
                let samples: &mut [f32] = bytemuck::cast_slice_mut(&mut self.data);
                for sample in samples {
                    *sample *= volume as f32;
                }
            }
            crate::audio::format::SampleFormat::F64 => {
                let samples: &mut [f64] = bytemuck::cast_slice_mut(&mut self.data);
                for sample in samples {
                    *sample *= volume;
                }
            }
            _ => {
                // U8, S24, Unknown - not implemented for scaling
            }
        }
    }
}

impl Default for AudioBuffer {
    fn default() -> Self {
        Self::invalid()
    }
}
