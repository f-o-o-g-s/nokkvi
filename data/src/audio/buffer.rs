use crate::audio::format::AudioFormat;

/// Decoded PCM data with its format.
#[derive(Clone)]
pub struct AudioBuffer {
    format: AudioFormat,
    data: Vec<u8>,
}

impl AudioBuffer {
    /// Create a new buffer from data
    pub fn from_data(data: Vec<u8>, format: AudioFormat) -> Self {
        Self { format, data }
    }

    /// Create an invalid buffer
    pub fn invalid() -> Self {
        Self {
            format: AudioFormat::invalid(),
            data: Vec::new(),
        }
    }

    /// Check if buffer is valid
    pub fn is_valid(&self) -> bool {
        self.format.is_valid() && !self.data.is_empty()
    }

    /// Get byte count
    pub fn byte_count(&self) -> usize {
        self.data.len()
    }

    /// Get raw data slice
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

impl Default for AudioBuffer {
    fn default() -> Self {
        Self::invalid()
    }
}
