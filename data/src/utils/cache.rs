use std::{fs, path::PathBuf};

use md5;

/// Detect image format from magic bytes and return the correct file extension.
///
/// JPEG starts with `FF D8 FF`, PNG starts with the 8-byte signature
/// `89 50 4E 47 0D 0A 1A 0A`, WebP starts with `RIFF....WEBP`.
/// Falls back to `.bin` for unknown formats.
fn detect_image_extension(data: &[u8]) -> &'static str {
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        ".jpg"
    } else if data.len() >= 8 && data[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        ".png"
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        ".webp"
    } else {
        ".bin"
    }
}

/// Utility for persistent disk caching
#[derive(Clone)]
pub struct DiskCache {
    base_dir: PathBuf,
}

impl DiskCache {
    pub fn new(name: &str) -> Option<Self> {
        let cache_dir = crate::utils::paths::get_cache_subdir(name).ok()?;
        Some(Self {
            base_dir: cache_dir,
        })
    }

    /// Hash the key to produce the base filename (no extension).
    fn hash_key(key: &str) -> String {
        format!("{:x}", md5::compute(key))
    }

    /// Return the on-disk path for a key, probing for whichever extension
    /// actually exists (`.jpg`, `.png`, `.webp`, then legacy `.bin`).
    ///
    /// If no file exists yet, returns the `.jpg` default (most Navidrome
    /// artwork is JPEG). The correct extension is determined at `insert()`
    /// time by sniffing the image magic bytes.
    pub fn get_path(&self, key: &str) -> PathBuf {
        let hash = Self::hash_key(key);
        for ext in [".jpg", ".png", ".webp", ".bin"] {
            let candidate = self.base_dir.join(format!("{hash}{ext}"));
            if candidate.exists() {
                return candidate;
            }
        }
        // Not cached yet — return .jpg as presumptive default
        self.base_dir.join(format!("{hash}.jpg"))
    }

    /// Check whether a key exists in the cache without reading the file.
    ///
    /// Uses `path.exists()` which is a single `stat()` syscall, avoiding the
    /// full file read that `get()` performs.
    pub fn contains(&self, key: &str) -> bool {
        let hash = Self::hash_key(key);
        [".jpg", ".png", ".webp", ".bin"]
            .iter()
            .any(|ext| self.base_dir.join(format!("{hash}{ext}")).exists())
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.get_path(key);
        fs::read(path).ok()
    }

    /// Insert image data into the cache, using the correct file extension
    /// based on image magic bytes (JPEG → `.jpg`, PNG → `.png`).
    pub fn insert(&self, key: &str, data: &[u8]) -> Option<()> {
        let hash = Self::hash_key(key);
        let ext = detect_image_extension(data);
        let path = self.base_dir.join(format!("{hash}{ext}"));

        // Remove stale files with wrong extensions before writing
        for old_ext in [".jpg", ".png", ".webp", ".bin"] {
            if old_ext != ext {
                let old_path = self.base_dir.join(format!("{hash}{old_ext}"));
                let _ = fs::remove_file(old_path);
            }
        }

        fs::write(path, data).ok()
    }

    /// Remove all cached files from this cache directory.
    /// The directory itself is preserved.
    pub fn clear(&self) -> usize {
        let mut removed = 0;
        if let Ok(entries) = fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            }
        }
        removed
    }
}
