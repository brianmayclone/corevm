//! Framebuffer → JPEG encoding with delta detection.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use crate::vm::event_handler::FrameBufferData;

/// Encode the framebuffer as JPEG if it changed since the last frame.
/// Returns None if unchanged (skip sending to client).
pub fn encode_frame(
    fb: &FrameBufferData,
    prev_hash: &mut u64,
    quality: u8,
) -> Option<Vec<u8>> {
    if fb.width == 0 || fb.height == 0 || fb.pixels.is_empty() {
        return None;
    }
    if !fb.dirty {
        return None;
    }

    // Fast hash for delta detection — hash a sample of pixels, not all.
    let hash = fast_hash(&fb.pixels);
    if hash == *prev_hash {
        return None;
    }
    *prev_hash = hash;

    // Convert RGBA → RGB (JPEG doesn't support alpha)
    let w = fb.width as usize;
    let h = fb.height as usize;
    let mut rgb = Vec::with_capacity(w * h * 3);
    for chunk in fb.pixels.chunks_exact(4) {
        rgb.push(chunk[0]); // R
        rgb.push(chunk[1]); // G
        rgb.push(chunk[2]); // B
    }

    // Encode as JPEG using the `image` crate
    let mut buf: Vec<u8> = Vec::with_capacity(w * h / 4);
    {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
        if encoder.encode(&rgb, fb.width, fb.height, image::ExtendedColorType::Rgb8).is_err() {
            return None;
        }
    }

    Some(buf)
}

/// Fast hash: sample evenly spaced chunks across the pixel buffer.
fn fast_hash(pixels: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    let len = pixels.len();
    if len == 0 { return 0; }

    // Hash first 256 bytes, middle 256 bytes, last 256 bytes
    let sample_size = 256.min(len);
    pixels[..sample_size].hash(&mut hasher);
    if len > sample_size * 2 {
        let mid = len / 2;
        let mid_start = mid.saturating_sub(sample_size / 2);
        pixels[mid_start..mid_start + sample_size.min(len - mid_start)].hash(&mut hasher);
    }
    if len > sample_size {
        pixels[len - sample_size..].hash(&mut hasher);
    }
    hasher.finish()
}
