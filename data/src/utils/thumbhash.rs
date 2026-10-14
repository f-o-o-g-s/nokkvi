//! ThumbHash placeholders: the ~25-byte picture summary Navidrome 0.64+ sends
//! as `thumbHash` (base64) on albums, artists and playlists, decoded into a
//! small blurred picture to show while the real cover loads.
//!
//! The decoder is a port of the reference implementation,
//! <https://github.com/evanw/thumbhash> (MIT License, Copyright (c) 2023 Evan
//! Wallace). It keeps the reference's `f32` arithmetic in the reference's
//! order, so it draws the reference's pictures (`decodes_like_the_reference`).
//! Two changes: the cosine terms, which depend on a pixel's column or row
//! alone, are worked out once per column and row instead of once per pixel,
//! and a hash whose picture would be zero pixels wide or tall is `None`.

use std::f32::consts::PI;

/// A decoded placeholder: `width` x `height` RGBA pixels, row by row, with
/// straight (not premultiplied) alpha. 32 px on its long side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbHashImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decode Navidrome's `thumbHash` (base64 of the ThumbHash bytes) into its
/// placeholder picture. `None` when it isn't base64 or is too short.
pub fn decode_thumb_hash(base64: &str) -> Option<ThumbHashImage> {
    thumb_hash_to_rgba(&crate::utils::base64::decode(base64)?)
}

/// The hash's coefficients, read four bits at a time after the header.
struct Nibbles<'a> {
    bytes: std::slice::Iter<'a, u8>,
    high: Option<u8>,
}

impl Nibbles<'_> {
    fn next(&mut self) -> Option<u8> {
        if let Some(high) = self.high.take() {
            return Some(high);
        }
        let byte = *self.bytes.next()?;
        self.high = Some(byte >> 4);
        Some(byte & 15)
    }

    /// One channel's AC terms: the DCT coefficients of an `nx` x `ny`
    /// triangle, the DC term left out, scaled back from 4 bits.
    fn channel(&mut self, nx: usize, ny: usize, scale: f32) -> Option<Vec<f32>> {
        let mut ac = Vec::with_capacity(nx * ny);
        for cy in 0..ny {
            let mut cx = usize::from(cy == 0);
            while cx * ny < nx * (ny - cy) {
                ac.push((f32::from(self.next()?) / 7.5 - 1.0) * scale);
                cx += 1;
            }
        }
        Some(ac)
    }
}

/// `cos(PI / n * (i + 0.5) * c)` for every column (or row) `i` of an `n`
/// pixel side and every frequency `c` below `count`, as the reference
/// computes it per pixel.
fn cosines(n: usize, count: usize) -> Vec<[f32; 7]> {
    (0..n)
        .map(|i| {
            let mut row = [0.0; 7];
            for (c, slot) in row.iter_mut().enumerate().take(count) {
                *slot = (PI / n as f32 * (i as f32 + 0.5) * c as f32).cos();
            }
            row
        })
        .collect()
}

fn thumb_hash_to_rgba(hash: &[u8]) -> Option<ThumbHashImage> {
    let header = hash.get(..5)?;
    let header24 =
        u32::from(header[0]) | (u32::from(header[1]) << 8) | (u32::from(header[2]) << 16);
    let header16 = u16::from(header[3]) | (u16::from(header[4]) << 8);
    let l_dc = (header24 & 63) as f32 / 63.0;
    let p_dc = ((header24 >> 6) & 63) as f32 / 31.5 - 1.0;
    let q_dc = ((header24 >> 12) & 63) as f32 / 31.5 - 1.0;
    let l_scale = ((header24 >> 18) & 31) as f32 / 31.0;
    let has_alpha = (header24 >> 23) != 0;
    let p_scale = ((header16 >> 3) & 63) as f32 / 63.0;
    let q_scale = ((header16 >> 9) & 63) as f32 / 63.0;
    let is_landscape = (header16 >> 15) != 0;
    let l_max: u16 = if has_alpha { 5 } else { 7 };
    let l_min = header16 & 7;
    // The reference's approximate aspect ratio: the coefficient counts along
    // each side, before they are raised to at least 3.
    let ratio = if is_landscape {
        f32::from(l_max) / f32::from(l_min)
    } else {
        f32::from(l_min) / f32::from(l_max)
    };
    let lx = usize::from(3.max(if is_landscape { l_max } else { l_min }));
    let ly = usize::from(3.max(if is_landscape { l_min } else { l_max }));

    let mut rest = hash.get(5..)?.iter();
    let (a_dc, a_scale) = if has_alpha {
        let header8 = *rest.next()?;
        (
            f32::from(header8 & 15) / 15.0,
            f32::from(header8 >> 4) / 15.0,
        )
    } else {
        (1.0, 1.0)
    };

    // Saturation is boosted 1.25x to make up for the quantization.
    let mut nibbles = Nibbles {
        bytes: rest,
        high: None,
    };
    let l_ac = nibbles.channel(lx, ly, l_scale)?;
    let p_ac = nibbles.channel(3, 3, p_scale * 1.25)?;
    let q_ac = nibbles.channel(3, 3, q_scale * 1.25)?;
    let a_ac = if has_alpha {
        nibbles.channel(5, 5, a_scale)?
    } else {
        Vec::new()
    };

    let (w, h) = if ratio > 1.0 {
        (32, (32.0 / ratio).round() as usize)
    } else {
        ((32.0 * ratio).round() as usize, 32)
    };
    if w == 0 || h == 0 {
        return None;
    }
    let min_terms = if has_alpha { 5 } else { 3 };
    let column_cosines = cosines(w, lx.max(min_terms));
    let row_cosines = cosines(h, ly.max(min_terms));

    let mut rgba = Vec::with_capacity(w * h * 4);
    for fy in &row_cosines {
        for fx in &column_cosines {
            let mut l = l_dc;
            let mut p = p_dc;
            let mut q = q_dc;
            let mut a = a_dc;

            let mut j = 0;
            for (cy, fy_c) in fy.iter().enumerate().take(ly) {
                let mut cx = usize::from(cy == 0);
                let fy2 = fy_c * 2.0;
                while cx * ly < lx * (ly - cy) {
                    l += l_ac[j] * fx[cx] * fy2;
                    j += 1;
                    cx += 1;
                }
            }

            let mut j = 0;
            for (cy, fy_c) in fy.iter().enumerate().take(3) {
                let mut cx = usize::from(cy == 0);
                let fy2 = fy_c * 2.0;
                while cx < 3 - cy {
                    let f = fx[cx] * fy2;
                    p += p_ac[j] * f;
                    q += q_ac[j] * f;
                    j += 1;
                    cx += 1;
                }
            }

            if has_alpha {
                let mut j = 0;
                for (cy, fy_c) in fy.iter().enumerate().take(5) {
                    let mut cx = usize::from(cy == 0);
                    let fy2 = fy_c * 2.0;
                    while cx < 5 - cy {
                        a += a_ac[j] * fx[cx] * fy2;
                        j += 1;
                        cx += 1;
                    }
                }
            }

            let b = l - 2.0 / 3.0 * p;
            let r = (3.0 * l - b + q) / 2.0;
            let g = r - q;
            rgba.extend_from_slice(&[
                (r.clamp(0.0, 1.0) * 255.0) as u8,
                (g.clamp(0.0, 1.0) * 255.0) as u8,
                (b.clamp(0.0, 1.0) * 255.0) as u8,
                (a.clamp(0.0, 1.0) * 255.0) as u8,
            ]);
        }
    }
    Some(ThumbHashImage {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture the reference decoder (evanw/thumbhash's Rust) drew, as its
    /// size and the rounded average colour of each cell of a 4 x 4 grid.
    struct Vector {
        name: &'static str,
        b64: &'static str,
        width: u32,
        height: u32,
        grid: [[u8; 4]; 16],
    }

    // Hashes from the reference encoder (synthetic pictures, and two real
    // covers from the owner's library) plus image_info.rs's fixture.
    // Regenerate with the reference crate, never from this decoder.
    const VECTORS: &[Vector] = &[
        Vector {
            name: "square_gradient",
            b64: "3wcKNxqAh3dwiIiIiHiIh4eHBwiIgI8H",
            width: 32,
            height: 32,
            grid: [
                [42, 31, 142, 255],
                [86, 20, 133, 255],
                [172, 15, 130, 255],
                [244, 14, 133, 255],
                [27, 79, 133, 255],
                [82, 77, 134, 255],
                [171, 70, 131, 255],
                [242, 63, 127, 255],
                [16, 171, 130, 255],
                [73, 169, 131, 255],
                [157, 152, 118, 255],
                [235, 148, 117, 255],
                [12, 247, 133, 255],
                [64, 242, 127, 255],
                [155, 229, 117, 255],
                [228, 217, 109, 255],
            ],
        },
        Vector {
            name: "landscape_halves",
            b64: "14YClJ6wl6+JeIeJeId/iPh3hw==",
            width: 32,
            height: 18,
            grid: [
                [237, 48, 2, 255],
                [177, 51, 50, 255],
                [76, 56, 147, 255],
                [4, 61, 225, 255],
                [226, 41, 12, 255],
                [166, 45, 66, 255],
                [64, 54, 160, 255],
                [1, 61, 236, 255],
                [226, 28, 24, 255],
                [166, 36, 76, 255],
                [63, 50, 165, 255],
                [1, 61, 237, 255],
                [239, 17, 22, 255],
                [178, 28, 72, 255],
                [75, 45, 157, 255],
                [3, 59, 228, 255],
            ],
        },
        Vector {
            name: "portrait_disc",
            b64: "0PgRTARHeIgHqHi3d3hHh3B0CA==",
            width: 18,
            height: 32,
            grid: [
                [1, 4, 56, 255],
                [21, 32, 37, 255],
                [25, 36, 35, 255],
                [2, 7, 52, 255],
                [88, 87, 37, 255],
                [160, 148, 48, 255],
                [173, 160, 52, 255],
                [99, 95, 34, 255],
                [84, 83, 33, 255],
                [172, 159, 57, 255],
                [200, 188, 75, 255],
                [122, 116, 47, 255],
                [0, 3, 55, 255],
                [23, 33, 32, 255],
                [45, 53, 40, 255],
                [5, 14, 41, 255],
            ],
        },
        Vector {
            name: "alpha_vignette",
            b64: "J3WBBQAnBniCiIF5d/mImY+oB3iHeICJdw==",
            width: 32,
            height: 32,
            grid: [
                [92, 161, 219, 39],
                [92, 161, 219, 108],
                [92, 161, 219, 108],
                [92, 161, 219, 37],
                [92, 161, 219, 108],
                [92, 161, 219, 202],
                [92, 161, 219, 208],
                [92, 161, 219, 110],
                [92, 161, 219, 108],
                [92, 161, 219, 208],
                [92, 161, 219, 225],
                [92, 161, 219, 126],
                [92, 161, 219, 37],
                [92, 161, 219, 110],
                [92, 161, 219, 126],
                [92, 161, 219, 41],
            ],
        },
        Vector {
            name: "cover_division_bell",
            b64: "3hcKHwRmJ6d+eYmAh7h4iIiIhQJ4SYAK",
            width: 32,
            height: 32,
            grid: [
                [105, 108, 150, 255],
                [133, 126, 147, 255],
                [132, 130, 151, 255],
                [112, 124, 168, 255],
                [117, 119, 144, 255],
                [148, 140, 143, 255],
                [145, 140, 143, 255],
                [132, 142, 168, 255],
                [130, 117, 128, 255],
                [164, 141, 129, 255],
                [168, 148, 135, 255],
                [137, 131, 141, 255],
                [78, 48, 54, 255],
                [113, 71, 57, 255],
                [119, 79, 64, 255],
                [79, 53, 59, 255],
            ],
        },
        Vector {
            name: "cover_animals_panel",
            b64: "VfgNHYaIiXeGdodweEeZe6iAlgqI",
            width: 32,
            height: 23,
            grid: [
                [31, 48, 50, 255],
                [9, 40, 48, 255],
                [8, 43, 51, 255],
                [6, 36, 38, 255],
                [116, 122, 108, 255],
                [104, 126, 118, 255],
                [117, 145, 137, 255],
                [102, 121, 106, 255],
                [151, 135, 109, 255],
                [101, 101, 81, 255],
                [114, 118, 97, 255],
                [116, 110, 82, 255],
                [108, 74, 49, 255],
                [67, 48, 29, 255],
                [90, 74, 53, 255],
                [103, 76, 48, 255],
            ],
        },
        Vector {
            name: "fixture_image_info",
            b64: "1QcSHQRnh493V4dIh4eXh1h4kJUI",
            width: 23,
            height: 32,
            grid: [
                [90, 95, 121, 255],
                [121, 121, 137, 255],
                [118, 120, 134, 255],
                [98, 112, 133, 255],
                [147, 129, 128, 255],
                [189, 165, 155, 255],
                [179, 157, 146, 255],
                [148, 138, 134, 255],
                [43, 29, 27, 255],
                [71, 50, 40, 255],
                [69, 51, 39, 255],
                [51, 43, 41, 255],
                [1, 7, 28, 255],
                [11, 15, 30, 255],
                [23, 31, 46, 255],
                [12, 24, 49, 255],
            ],
        },
    ];

    /// The 4 x 4 grid of rounded average colours, cut the way the vectors were.
    fn grid(image: &ThumbHashImage) -> [[u8; 4]; 16] {
        let (w, h) = (image.width as usize, image.height as usize);
        let mut out = [[0; 4]; 16];
        for by in 0..4 {
            for bx in 0..4 {
                let mut sum = [0u64; 4];
                let mut n = 0u64;
                for y in by * h / 4..(by + 1) * h / 4 {
                    for x in bx * w / 4..(bx + 1) * w / 4 {
                        let i = (y * w + x) * 4;
                        for (c, s) in sum.iter_mut().enumerate() {
                            *s += u64::from(image.rgba[i + c]);
                        }
                        n += 1;
                    }
                }
                for c in 0..4 {
                    out[by * 4 + bx][c] = ((sum[c] + n / 2) / n) as u8;
                }
            }
        }
        out
    }

    /// The vectors pin a cell to within 1: the cosines come from the system
    /// libm, which may round a last bit differently from the machine the
    /// vectors were made on. A porting slip moves cells by tens.
    #[test]
    fn decodes_like_the_reference() {
        for v in VECTORS {
            let image =
                decode_thumb_hash(v.b64).unwrap_or_else(|| panic!("{}: no picture", v.name));
            assert_eq!(
                (image.width, image.height),
                (v.width, v.height),
                "{}",
                v.name
            );
            assert_eq!(
                image.rgba.len(),
                (v.width * v.height * 4) as usize,
                "{}",
                v.name
            );
            for (cell, (got, want)) in grid(&image).iter().zip(v.grid.iter()).enumerate() {
                for c in 0..4 {
                    assert!(
                        got[c].abs_diff(want[c]) <= 1,
                        "{} cell {cell} channel {c}: {} vs reference {}",
                        v.name,
                        got[c],
                        want[c],
                    );
                }
            }
        }
    }

    #[test]
    fn rejects_what_is_not_a_thumb_hash() {
        // Not base64.
        assert_eq!(decode_thumb_hash("not a hash!"), None);
        // Four bytes: shorter than the five-byte header.
        assert_eq!(decode_thumb_hash("AAAAAA=="), None);
        // A header promising more coefficients than the bytes hold.
        assert_eq!(decode_thumb_hash("3wcKNxqAh3dw"), None);
    }

    #[test]
    fn a_hash_with_no_width_draws_nothing() {
        // Header byte 3 carries the short side's coefficient count in its
        // low three bits; zero makes a portrait picture zero pixels wide.
        assert_eq!(decode_thumb_hash("3wcKAACIiIiIiIiIiIiIiIiIiIiI"), None);
    }
}
