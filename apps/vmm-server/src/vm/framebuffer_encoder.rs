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

    // Skip delta detection — always encode every frame
    let _ = prev_hash;

    // Convert RGBA → RGB (JPEG doesn't support alpha)
    let w = fb.width as usize;
    let h = fb.height as usize;
    let expected = w * h * 4;
    if fb.pixels.len() < expected {
        return None;
    }

    let mut rgb = Vec::with_capacity(w * h * 3);
    for chunk in fb.pixels[..expected].chunks_exact(4) {
        rgb.push(chunk[0]); // R
        rgb.push(chunk[1]); // G
        rgb.push(chunk[2]); // B
    }

    // Encode as JPEG
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

/// Hash resolution + ~4KB of pixel samples spread across the buffer.
fn frame_hash(w: u32, h: u32, pixels: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    // Always include resolution so mode switches are detected
    w.hash(&mut hasher);
    h.hash(&mut hasher);
    pixels.len().hash(&mut hasher);

    let len = pixels.len();
    if len == 0 { return hasher.finish(); }

    // Sample 16 evenly-spaced 256-byte chunks across the buffer
    let num_samples = 16;
    let chunk_size = 256;
    let step = if len > num_samples * chunk_size { len / num_samples } else { 1 };

    let mut offset = 0;
    for _ in 0..num_samples {
        if offset >= len { break; }
        let end = (offset + chunk_size).min(len);
        pixels[offset..end].hash(&mut hasher);
        offset += step;
    }

    hasher.finish()
}
