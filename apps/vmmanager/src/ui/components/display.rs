use eframe::egui;

use crate::app::FrameBufferData;

#[cfg(target_os = "linux")]
use crate::engine::evdev_input::EvdevInputReader;

// Standard VGA 16-color palette as RGBA
const VGA_PALETTE: [[u8; 4]; 16] = [
    [0, 0, 0, 255],       // 0: Black
    [0, 0, 170, 255],     // 1: Blue
    [0, 170, 0, 255],     // 2: Green
    [0, 170, 170, 255],   // 3: Cyan
    [170, 0, 0, 255],     // 4: Red
    [170, 0, 170, 255],   // 5: Magenta
    [170, 85, 0, 255],    // 6: Brown
    [170, 170, 170, 255], // 7: Light Gray
    [85, 85, 85, 255],    // 8: Dark Gray
    [85, 85, 255, 255],   // 9: Light Blue
    [85, 255, 85, 255],   // 10: Light Green
    [85, 255, 255, 255],  // 11: Light Cyan
    [255, 85, 85, 255],   // 12: Light Red
    [255, 85, 255, 255],  // 13: Light Magenta
    [255, 255, 85, 255],  // 14: Yellow
    [255, 255, 255, 255], // 15: White
];

// CP437 8x16 bitmap font. 256 chars * 16 bytes each = 4096 bytes.
// Each byte represents one row of 8 pixels (MSB = leftmost pixel).
// Non-printable characters (outside 0x20-0x7E) are blank.
const VGA_FONT_8X16: [u8; 4096] = {
    let mut font = [0u8; 4096];

    // We build the font at compile time using a const block.
    // Only ASCII 0x20..=0x7E are defined; rest stays zero (blank).

    macro_rules! glyph {
        ($ch:expr, $($row:expr),+ $(,)?) => {{
            let base = ($ch as usize) * 16;
            let rows: [u8; 16] = [$($row),+];
            let mut i = 0;
            while i < 16 {
                font[base + i] = rows[i];
                i += 1;
            }
        }};
    }

    // Space (0x20)
    glyph!(0x20, 0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00);
    // ! (0x21)
    glyph!(0x21, 0x00,0x00,0x18,0x3C,0x3C,0x3C,0x18,0x18,0x18,0x00,0x18,0x18,0x00,0x00,0x00,0x00);
    // " (0x22)
    glyph!(0x22, 0x00,0x66,0x66,0x66,0x24,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00);
    // # (0x23)
    glyph!(0x23, 0x00,0x00,0x00,0x6C,0x6C,0xFE,0x6C,0x6C,0x6C,0xFE,0x6C,0x6C,0x00,0x00,0x00,0x00);
    // $ (0x24)
    glyph!(0x24, 0x18,0x18,0x7C,0xC6,0xC2,0xC0,0x7C,0x06,0x06,0x86,0xC6,0x7C,0x18,0x18,0x00,0x00);
    // % (0x25)
    glyph!(0x25, 0x00,0x00,0x00,0x00,0xC2,0xC6,0x0C,0x18,0x30,0x60,0xC6,0x86,0x00,0x00,0x00,0x00);
    // & (0x26)
    glyph!(0x26, 0x00,0x00,0x38,0x6C,0x6C,0x38,0x76,0xDC,0xCC,0xCC,0xCC,0x76,0x00,0x00,0x00,0x00);
    // ' (0x27)
    glyph!(0x27, 0x00,0x30,0x30,0x30,0x60,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00);
    // ( (0x28)
    glyph!(0x28, 0x00,0x00,0x0C,0x18,0x30,0x30,0x30,0x30,0x30,0x30,0x18,0x0C,0x00,0x00,0x00,0x00);
    // ) (0x29)
    glyph!(0x29, 0x00,0x00,0x30,0x18,0x0C,0x0C,0x0C,0x0C,0x0C,0x0C,0x18,0x30,0x00,0x00,0x00,0x00);
    // * (0x2A)
    glyph!(0x2A, 0x00,0x00,0x00,0x00,0x00,0x66,0x3C,0xFF,0x3C,0x66,0x00,0x00,0x00,0x00,0x00,0x00);
    // + (0x2B)
    glyph!(0x2B, 0x00,0x00,0x00,0x00,0x00,0x18,0x18,0x7E,0x18,0x18,0x00,0x00,0x00,0x00,0x00,0x00);
    // , (0x2C)
    glyph!(0x2C, 0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x18,0x18,0x18,0x30,0x00,0x00,0x00);
    // - (0x2D)
    glyph!(0x2D, 0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xFE,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00);
    // . (0x2E)
    glyph!(0x2E, 0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x18,0x18,0x00,0x00,0x00,0x00);
    // / (0x2F)
    glyph!(0x2F, 0x00,0x00,0x00,0x00,0x02,0x06,0x0C,0x18,0x30,0x60,0xC0,0x80,0x00,0x00,0x00,0x00);
    // 0 (0x30)
    glyph!(0x30, 0x00,0x00,0x7C,0xC6,0xC6,0xCE,0xDE,0xF6,0xE6,0xC6,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // 1 (0x31)
    glyph!(0x31, 0x00,0x00,0x18,0x38,0x78,0x18,0x18,0x18,0x18,0x18,0x18,0x7E,0x00,0x00,0x00,0x00);
    // 2 (0x32)
    glyph!(0x32, 0x00,0x00,0x7C,0xC6,0x06,0x0C,0x18,0x30,0x60,0xC0,0xC6,0xFE,0x00,0x00,0x00,0x00);
    // 3 (0x33)
    glyph!(0x33, 0x00,0x00,0x7C,0xC6,0x06,0x06,0x3C,0x06,0x06,0x06,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // 4 (0x34)
    glyph!(0x34, 0x00,0x00,0x0C,0x1C,0x3C,0x6C,0xCC,0xFE,0x0C,0x0C,0x0C,0x1E,0x00,0x00,0x00,0x00);
    // 5 (0x35)
    glyph!(0x35, 0x00,0x00,0xFE,0xC0,0xC0,0xC0,0xFC,0x06,0x06,0x06,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // 6 (0x36)
    glyph!(0x36, 0x00,0x00,0x38,0x60,0xC0,0xC0,0xFC,0xC6,0xC6,0xC6,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // 7 (0x37)
    glyph!(0x37, 0x00,0x00,0xFE,0xC6,0x06,0x06,0x0C,0x18,0x30,0x30,0x30,0x30,0x00,0x00,0x00,0x00);
    // 8 (0x38)
    glyph!(0x38, 0x00,0x00,0x7C,0xC6,0xC6,0xC6,0x7C,0xC6,0xC6,0xC6,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // 9 (0x39)
    glyph!(0x39, 0x00,0x00,0x7C,0xC6,0xC6,0xC6,0x7E,0x06,0x06,0x06,0x0C,0x78,0x00,0x00,0x00,0x00);
    // : (0x3A)
    glyph!(0x3A, 0x00,0x00,0x00,0x00,0x18,0x18,0x00,0x00,0x00,0x18,0x18,0x00,0x00,0x00,0x00,0x00);
    // ; (0x3B)
    glyph!(0x3B, 0x00,0x00,0x00,0x00,0x18,0x18,0x00,0x00,0x00,0x18,0x18,0x30,0x00,0x00,0x00,0x00);
    // < (0x3C)
    glyph!(0x3C, 0x00,0x00,0x00,0x06,0x0C,0x18,0x30,0x60,0x30,0x18,0x0C,0x06,0x00,0x00,0x00,0x00);
    // = (0x3D)
    glyph!(0x3D, 0x00,0x00,0x00,0x00,0x00,0x7E,0x00,0x00,0x7E,0x00,0x00,0x00,0x00,0x00,0x00,0x00);
    // > (0x3E)
    glyph!(0x3E, 0x00,0x00,0x00,0x60,0x30,0x18,0x0C,0x06,0x0C,0x18,0x30,0x60,0x00,0x00,0x00,0x00);
    // ? (0x3F)
    glyph!(0x3F, 0x00,0x00,0x7C,0xC6,0xC6,0x0C,0x18,0x18,0x18,0x00,0x18,0x18,0x00,0x00,0x00,0x00);
    // @ (0x40)
    glyph!(0x40, 0x00,0x00,0x7C,0xC6,0xC6,0xC6,0xDE,0xDE,0xDE,0xDC,0xC0,0x7C,0x00,0x00,0x00,0x00);
    // A (0x41)
    glyph!(0x41, 0x00,0x00,0x10,0x38,0x6C,0xC6,0xC6,0xFE,0xC6,0xC6,0xC6,0xC6,0x00,0x00,0x00,0x00);
    // B (0x42)
    glyph!(0x42, 0x00,0x00,0xFC,0x66,0x66,0x66,0x7C,0x66,0x66,0x66,0x66,0xFC,0x00,0x00,0x00,0x00);
    // C (0x43)
    glyph!(0x43, 0x00,0x00,0x3C,0x66,0xC2,0xC0,0xC0,0xC0,0xC0,0xC2,0x66,0x3C,0x00,0x00,0x00,0x00);
    // D (0x44)
    glyph!(0x44, 0x00,0x00,0xF8,0x6C,0x66,0x66,0x66,0x66,0x66,0x66,0x6C,0xF8,0x00,0x00,0x00,0x00);
    // E (0x45)
    glyph!(0x45, 0x00,0x00,0xFE,0x66,0x62,0x68,0x78,0x68,0x60,0x62,0x66,0xFE,0x00,0x00,0x00,0x00);
    // F (0x46)
    glyph!(0x46, 0x00,0x00,0xFE,0x66,0x62,0x68,0x78,0x68,0x60,0x60,0x60,0xF0,0x00,0x00,0x00,0x00);
    // G (0x47)
    glyph!(0x47, 0x00,0x00,0x3C,0x66,0xC2,0xC0,0xC0,0xDE,0xC6,0xC6,0x66,0x3A,0x00,0x00,0x00,0x00);
    // H (0x48)
    glyph!(0x48, 0x00,0x00,0xC6,0xC6,0xC6,0xC6,0xFE,0xC6,0xC6,0xC6,0xC6,0xC6,0x00,0x00,0x00,0x00);
    // I (0x49)
    glyph!(0x49, 0x00,0x00,0x3C,0x18,0x18,0x18,0x18,0x18,0x18,0x18,0x18,0x3C,0x00,0x00,0x00,0x00);
    // J (0x4A)
    glyph!(0x4A, 0x00,0x00,0x1E,0x0C,0x0C,0x0C,0x0C,0x0C,0xCC,0xCC,0xCC,0x78,0x00,0x00,0x00,0x00);
    // K (0x4B)
    glyph!(0x4B, 0x00,0x00,0xE6,0x66,0x66,0x6C,0x78,0x78,0x6C,0x66,0x66,0xE6,0x00,0x00,0x00,0x00);
    // L (0x4C)
    glyph!(0x4C, 0x00,0x00,0xF0,0x60,0x60,0x60,0x60,0x60,0x60,0x62,0x66,0xFE,0x00,0x00,0x00,0x00);
    // M (0x4D)
    glyph!(0x4D, 0x00,0x00,0xC6,0xEE,0xFE,0xFE,0xD6,0xC6,0xC6,0xC6,0xC6,0xC6,0x00,0x00,0x00,0x00);
    // N (0x4E)
    glyph!(0x4E, 0x00,0x00,0xC6,0xE6,0xF6,0xFE,0xDE,0xCE,0xC6,0xC6,0xC6,0xC6,0x00,0x00,0x00,0x00);
    // O (0x4F)
    glyph!(0x4F, 0x00,0x00,0x7C,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // P (0x50)
    glyph!(0x50, 0x00,0x00,0xFC,0x66,0x66,0x66,0x7C,0x60,0x60,0x60,0x60,0xF0,0x00,0x00,0x00,0x00);
    // Q (0x51)
    glyph!(0x51, 0x00,0x00,0x7C,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0xD6,0xDE,0x7C,0x0C,0x0E,0x00,0x00);
    // R (0x52)
    glyph!(0x52, 0x00,0x00,0xFC,0x66,0x66,0x66,0x7C,0x6C,0x66,0x66,0x66,0xE6,0x00,0x00,0x00,0x00);
    // S (0x53)
    glyph!(0x53, 0x00,0x00,0x7C,0xC6,0xC6,0x60,0x38,0x0C,0x06,0xC6,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // T (0x54)
    glyph!(0x54, 0x00,0x00,0xFF,0xDB,0x99,0x18,0x18,0x18,0x18,0x18,0x18,0x3C,0x00,0x00,0x00,0x00);
    // U (0x55)
    glyph!(0x55, 0x00,0x00,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // V (0x56)
    glyph!(0x56, 0x00,0x00,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0x6C,0x38,0x10,0x00,0x00,0x00,0x00);
    // W (0x57)
    glyph!(0x57, 0x00,0x00,0xC6,0xC6,0xC6,0xC6,0xD6,0xD6,0xD6,0xFE,0xEE,0x6C,0x00,0x00,0x00,0x00);
    // X (0x58)
    glyph!(0x58, 0x00,0x00,0xC6,0xC6,0x6C,0x7C,0x38,0x38,0x7C,0x6C,0xC6,0xC6,0x00,0x00,0x00,0x00);
    // Y (0x59)
    glyph!(0x59, 0x00,0x00,0xC6,0xC6,0xC6,0x6C,0x38,0x18,0x18,0x18,0x18,0x3C,0x00,0x00,0x00,0x00);
    // Z (0x5A)
    glyph!(0x5A, 0x00,0x00,0xFE,0xC6,0x86,0x0C,0x18,0x30,0x60,0xC2,0xC6,0xFE,0x00,0x00,0x00,0x00);
    // [ (0x5B)
    glyph!(0x5B, 0x00,0x00,0x3C,0x30,0x30,0x30,0x30,0x30,0x30,0x30,0x30,0x3C,0x00,0x00,0x00,0x00);
    // \ (0x5C)
    glyph!(0x5C, 0x00,0x00,0x00,0x80,0xC0,0x60,0x30,0x18,0x0C,0x06,0x02,0x00,0x00,0x00,0x00,0x00);
    // ] (0x5D)
    glyph!(0x5D, 0x00,0x00,0x3C,0x0C,0x0C,0x0C,0x0C,0x0C,0x0C,0x0C,0x0C,0x3C,0x00,0x00,0x00,0x00);
    // ^ (0x5E)
    glyph!(0x5E, 0x10,0x38,0x6C,0xC6,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00);
    // _ (0x5F)
    glyph!(0x5F, 0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xFF,0x00,0x00,0x00);
    // ` (0x60)
    glyph!(0x60, 0x00,0x30,0x18,0x0C,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00);
    // a (0x61)
    glyph!(0x61, 0x00,0x00,0x00,0x00,0x00,0x78,0x0C,0x7C,0xCC,0xCC,0xCC,0x76,0x00,0x00,0x00,0x00);
    // b (0x62)
    glyph!(0x62, 0x00,0x00,0xE0,0x60,0x60,0x78,0x6C,0x66,0x66,0x66,0x66,0x7C,0x00,0x00,0x00,0x00);
    // c (0x63)
    glyph!(0x63, 0x00,0x00,0x00,0x00,0x00,0x7C,0xC6,0xC0,0xC0,0xC0,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // d (0x64)
    glyph!(0x64, 0x00,0x00,0x1C,0x0C,0x0C,0x3C,0x6C,0xCC,0xCC,0xCC,0xCC,0x76,0x00,0x00,0x00,0x00);
    // e (0x65)
    glyph!(0x65, 0x00,0x00,0x00,0x00,0x00,0x7C,0xC6,0xFE,0xC0,0xC0,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // f (0x66)
    glyph!(0x66, 0x00,0x00,0x38,0x6C,0x64,0x60,0xF0,0x60,0x60,0x60,0x60,0xF0,0x00,0x00,0x00,0x00);
    // g (0x67)
    glyph!(0x67, 0x00,0x00,0x00,0x00,0x00,0x76,0xCC,0xCC,0xCC,0xCC,0xCC,0x7C,0x0C,0xCC,0x78,0x00);
    // h (0x68)
    glyph!(0x68, 0x00,0x00,0xE0,0x60,0x60,0x6C,0x76,0x66,0x66,0x66,0x66,0xE6,0x00,0x00,0x00,0x00);
    // i (0x69)
    glyph!(0x69, 0x00,0x00,0x18,0x18,0x00,0x38,0x18,0x18,0x18,0x18,0x18,0x3C,0x00,0x00,0x00,0x00);
    // j (0x6A)
    glyph!(0x6A, 0x00,0x00,0x06,0x06,0x00,0x0E,0x06,0x06,0x06,0x06,0x06,0x06,0x66,0x66,0x3C,0x00);
    // k (0x6B)
    glyph!(0x6B, 0x00,0x00,0xE0,0x60,0x60,0x66,0x6C,0x78,0x78,0x6C,0x66,0xE6,0x00,0x00,0x00,0x00);
    // l (0x6C)
    glyph!(0x6C, 0x00,0x00,0x38,0x18,0x18,0x18,0x18,0x18,0x18,0x18,0x18,0x3C,0x00,0x00,0x00,0x00);
    // m (0x6D)
    glyph!(0x6D, 0x00,0x00,0x00,0x00,0x00,0xEC,0xFE,0xD6,0xD6,0xD6,0xD6,0xC6,0x00,0x00,0x00,0x00);
    // n (0x6E)
    glyph!(0x6E, 0x00,0x00,0x00,0x00,0x00,0xDC,0x66,0x66,0x66,0x66,0x66,0x66,0x00,0x00,0x00,0x00);
    // o (0x6F)
    glyph!(0x6F, 0x00,0x00,0x00,0x00,0x00,0x7C,0xC6,0xC6,0xC6,0xC6,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // p (0x70)
    glyph!(0x70, 0x00,0x00,0x00,0x00,0x00,0xDC,0x66,0x66,0x66,0x66,0x66,0x7C,0x60,0x60,0xF0,0x00);
    // q (0x71)
    glyph!(0x71, 0x00,0x00,0x00,0x00,0x00,0x76,0xCC,0xCC,0xCC,0xCC,0xCC,0x7C,0x0C,0x0C,0x1E,0x00);
    // r (0x72)
    glyph!(0x72, 0x00,0x00,0x00,0x00,0x00,0xDC,0x76,0x66,0x60,0x60,0x60,0xF0,0x00,0x00,0x00,0x00);
    // s (0x73)
    glyph!(0x73, 0x00,0x00,0x00,0x00,0x00,0x7C,0xC6,0x60,0x38,0x0C,0xC6,0x7C,0x00,0x00,0x00,0x00);
    // t (0x74)
    glyph!(0x74, 0x00,0x00,0x10,0x30,0x30,0xFC,0x30,0x30,0x30,0x30,0x36,0x1C,0x00,0x00,0x00,0x00);
    // u (0x75)
    glyph!(0x75, 0x00,0x00,0x00,0x00,0x00,0xCC,0xCC,0xCC,0xCC,0xCC,0xCC,0x76,0x00,0x00,0x00,0x00);
    // v (0x76)
    glyph!(0x76, 0x00,0x00,0x00,0x00,0x00,0xC6,0xC6,0xC6,0xC6,0x6C,0x38,0x10,0x00,0x00,0x00,0x00);
    // w (0x77)
    glyph!(0x77, 0x00,0x00,0x00,0x00,0x00,0xC6,0xC6,0xD6,0xD6,0xD6,0xFE,0x6C,0x00,0x00,0x00,0x00);
    // x (0x78)
    glyph!(0x78, 0x00,0x00,0x00,0x00,0x00,0xC6,0x6C,0x38,0x38,0x38,0x6C,0xC6,0x00,0x00,0x00,0x00);
    // y (0x79)
    glyph!(0x79, 0x00,0x00,0x00,0x00,0x00,0xC6,0xC6,0xC6,0xC6,0xC6,0xC6,0x7E,0x06,0x0C,0xF8,0x00);
    // z (0x7A)
    glyph!(0x7A, 0x00,0x00,0x00,0x00,0x00,0xFE,0xCC,0x18,0x30,0x60,0xC6,0xFE,0x00,0x00,0x00,0x00);
    // { (0x7B)
    glyph!(0x7B, 0x00,0x00,0x0E,0x18,0x18,0x18,0x70,0x18,0x18,0x18,0x18,0x0E,0x00,0x00,0x00,0x00);
    // | (0x7C)
    glyph!(0x7C, 0x00,0x00,0x18,0x18,0x18,0x18,0x00,0x18,0x18,0x18,0x18,0x18,0x00,0x00,0x00,0x00);
    // } (0x7D)
    glyph!(0x7D, 0x00,0x00,0x70,0x18,0x18,0x18,0x0E,0x18,0x18,0x18,0x18,0x70,0x00,0x00,0x00,0x00);
    // ~ (0x7E)
    glyph!(0x7E, 0x00,0x00,0x76,0xDC,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00);

    font
};

/// Render 80x25 text mode buffer to 640x400 RGBA pixels.
pub fn render_text_mode(text_buffer: &[u16], output: &mut Vec<u8>) -> (u32, u32) {
    const COLS: usize = 80;
    const ROWS: usize = 25;
    const CHAR_W: usize = 8;
    const CHAR_H: usize = 16;
    const WIDTH: usize = COLS * CHAR_W;  // 640
    const HEIGHT: usize = ROWS * CHAR_H; // 400

    output.resize(WIDTH * HEIGHT * 4, 0);

    for row in 0..ROWS {
        for col in 0..COLS {
            let idx = row * COLS + col;
            let cell = if idx < text_buffer.len() {
                text_buffer[idx]
            } else {
                0x0720 // light gray on black space
            };

            let ch = (cell & 0xFF) as usize;
            let attr = ((cell >> 8) & 0xFF) as usize;
            let fg = &VGA_PALETTE[attr & 0x0F];
            let bg = &VGA_PALETTE[(attr >> 4) & 0x0F];

            let glyph_base = ch * 16;

            for glyph_row in 0..CHAR_H {
                let bits = VGA_FONT_8X16[glyph_base + glyph_row];
                let py = row * CHAR_H + glyph_row;

                for bit in 0..CHAR_W {
                    let px = col * CHAR_W + bit;
                    let pixel_offset = (py * WIDTH + px) * 4;
                    let color = if bits & (0x80 >> bit) != 0 { fg } else { bg };
                    output[pixel_offset] = color[0];
                    output[pixel_offset + 1] = color[1];
                    output[pixel_offset + 2] = color[2];
                    output[pixel_offset + 3] = color[3];
                }
            }
        }
    }

    (WIDTH as u32, HEIGHT as u32)
}

/// Convert raw framebuffer bytes to RGBA32.
pub fn render_graphics_mode(fb: &[u8], width: u32, height: u32, bpp: u8, output: &mut Vec<u8>) {
    let npixels = (width as usize) * (height as usize);
    output.resize(npixels * 4, 255);

    match bpp {
        4 => {
            // 4bpp: each byte = 2 pixels, index into 16-color palette
            for i in 0..npixels {
                let byte_idx = i / 2;
                let nibble = if byte_idx < fb.len() {
                    if i % 2 == 0 {
                        (fb[byte_idx] >> 4) & 0x0F
                    } else {
                        fb[byte_idx] & 0x0F
                    }
                } else {
                    0
                };
                let c = &VGA_PALETTE[nibble as usize];
                let o = i * 4;
                output[o] = c[0];
                output[o + 1] = c[1];
                output[o + 2] = c[2];
                output[o + 3] = c[3];
            }
        }
        8 => {
            // 8bpp: grayscale
            for i in 0..npixels {
                let val = if i < fb.len() { fb[i] } else { 0 };
                let o = i * 4;
                output[o] = val;
                output[o + 1] = val;
                output[o + 2] = val;
                output[o + 3] = 255;
            }
        }
        16 => {
            // 16bpp: RGB565
            for i in 0..npixels {
                let src = i * 2;
                let (lo, hi) = if src + 1 < fb.len() {
                    (fb[src], fb[src + 1])
                } else {
                    (0, 0)
                };
                let pixel = (hi as u16) << 8 | lo as u16;
                let r = ((pixel >> 11) & 0x1F) as u8;
                let g = ((pixel >> 5) & 0x3F) as u8;
                let b = (pixel & 0x1F) as u8;
                let o = i * 4;
                output[o] = (r << 3) | (r >> 2);
                output[o + 1] = (g << 2) | (g >> 4);
                output[o + 2] = (b << 3) | (b >> 2);
                output[o + 3] = 255;
            }
        }
        24 => {
            // 24bpp: BGR -> RGBA
            for i in 0..npixels {
                let src = i * 3;
                let o = i * 4;
                if src + 2 < fb.len() {
                    output[o] = fb[src + 2];     // R
                    output[o + 1] = fb[src + 1]; // G
                    output[o + 2] = fb[src];     // B
                    output[o + 3] = 255;
                }
            }
        }
        32 => {
            // 32bpp: BGRX -> RGBA (VBE framebuffers use X=unused, not alpha)
            for i in 0..npixels {
                let src = i * 4;
                let o = i * 4;
                if src + 3 < fb.len() {
                    output[o] = fb[src + 2];     // R
                    output[o + 1] = fb[src + 1]; // G
                    output[o + 2] = fb[src];     // B
                    output[o + 3] = 255;         // Always opaque
                }
            }
        }
        _ => {
            // Unknown BPP: fill black
            for i in 0..npixels {
                let o = i * 4;
                output[o] = 0;
                output[o + 1] = 0;
                output[o + 2] = 0;
                output[o + 3] = 255;
            }
        }
    }
}

/// Widget that renders a VM framebuffer as an egui texture.
pub struct DisplayWidget {
    texture: Option<egui::TextureHandle>,
    last_width: u32,
    last_height: u32,
    last_seq: u64,
    last_mouse_pos: Option<egui::Pos2>,
    last_mouse_buttons: u8,
    mouse_debug_counter: u64,
    /// Accumulated fractional mouse deltas (sub-pixel precision).
    mouse_accum_x: f32,
    mouse_accum_y: f32,
    /// Whether the mouse is currently captured (grabbed) by the VM display.
    /// Click on display to capture, Ctrl+Alt+G to release.
    pub mouse_captured: bool,
    /// Frame counter for periodic cursor re-center (prevents edge-sticking on X11).
    warp_counter: u32,
    /// Set when mouse_captured is cleared without ctx (e.g., toolbar stop).
    /// update() checks this to restore cursor state.
    pub needs_cursor_restore: bool,
    /// Frames to skip after a cursor warp (warp generates a phantom delta).
    warp_skip_frames: u8,
    /// Timestamp when right mouse button was first pressed (for hold-to-release).
    right_click_start: Option<std::time::Instant>,
    /// Consecutive Escape press times for triple-press detection.
    escape_presses: Vec<std::time::Instant>,
    /// USB tablet mode: send absolute coordinates via USB tablet device.
    pub usb_tablet_mode: bool,
    /// Virtual absolute cursor position (0..32767) for USB tablet/VirtIO in capture mode.
    abs_cursor_x: u16,
    abs_cursor_y: u16,
    /// Low-level evdev input reader (Linux only). Provides reliable raw mouse
    /// deltas and keyboard events independent of the windowing system.
    #[cfg(target_os = "linux")]
    pub evdev_input: Option<EvdevInputReader>,
    /// Timestamp when mouse was captured — used for grace period to ignore
    /// spurious focus-loss events right after grabbing.
    pub capture_time: Option<std::time::Instant>,
}

impl DisplayWidget {
    pub fn new() -> Self {
        Self {
            texture: None,
            last_width: 0,
            last_height: 0,
            last_seq: 0,
            last_mouse_pos: None,
            last_mouse_buttons: 0,
            mouse_debug_counter: 0,
            mouse_accum_x: 0.0,
            mouse_accum_y: 0.0,
            mouse_captured: false,
            warp_counter: 0,
            needs_cursor_restore: false,
            warp_skip_frames: 0,
            right_click_start: None,
            escape_presses: Vec::new(),
            usb_tablet_mode: false,
            abs_cursor_x: 16383, // center
            abs_cursor_y: 16383, // center
            #[cfg(target_os = "linux")]
            evdev_input: {
                // Skip evdev under WSL2 — no /dev/input devices available
                if std::fs::read_to_string("/proc/version")
                    .map(|v| { let l = v.to_lowercase(); l.contains("microsoft") || l.contains("wsl") })
                    .unwrap_or(false)
                {
                    None
                } else {
                    EvdevInputReader::open()
                }
            },
            capture_time: None,
        }
    }

    /// Update the texture from framebuffer data. Returns true if updated.
    pub fn update_texture(&mut self, ctx: &egui::Context, fb: &FrameBufferData) -> bool {
        if fb.seq == self.last_seq || fb.width == 0 || fb.height == 0 || fb.pixels.is_empty() {
            return false;
        }

        let expected_size = (fb.width as usize) * (fb.height as usize) * 4;
        if fb.pixels.len() < expected_size {
            return false;
        }

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [fb.width as usize, fb.height as usize],
            &fb.pixels[..expected_size],
        );

        let size_changed = fb.width != self.last_width || fb.height != self.last_height;

        if size_changed || self.texture.is_none() {
            self.texture = Some(ctx.load_texture(
                "vm_display",
                image,
                egui::TextureOptions::NEAREST,
            ));
        } else if let Some(ref mut tex) = self.texture {
            tex.set(image, egui::TextureOptions::NEAREST);
        }

        self.last_width = fb.width;
        self.last_height = fb.height;
        self.last_seq = fb.seq;

        true
    }

    /// Render the display, filling available space while maintaining aspect ratio.
    /// Returns (focused, Option<display_rect>) for keyboard/mouse capture.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, fb: &FrameBufferData, vm_handle: Option<u64>, use_evdev: bool) -> (bool, Option<egui::Rect>) {
        self.update_texture(ctx, fb);

        let display_id = egui::Id::new("vm_display_area");

        // Multiple redundant release mechanisms — at least one must work
        // even when CursorGrab::Locked interferes with event delivery.
        if self.mouse_captured {
            let release = self.check_mouse_release(ui, ctx);
            if release {
                self.release_mouse_with_handle(ctx, vm_handle);
            }
        }

        if let Some(ref texture) = self.texture {
            let available = ui.available_size();
            let tex_w = self.last_width as f32;
            let tex_h = self.last_height as f32;

            if tex_w > 0.0 && tex_h > 0.0 {
                let aspect = tex_w / tex_h;
                let (disp_w, disp_h) = if available.x / available.y > aspect {
                    (available.y * aspect, available.y)
                } else {
                    (available.x, available.x / aspect)
                };

                let size = egui::vec2(disp_w, disp_h);

                // Center the display in the available area
                let centered_rect = egui::Rect::from_center_size(
                    ui.max_rect().center(),
                    size,
                );
                let response = ui.allocate_rect(centered_rect, egui::Sense::click_and_drag());
                let rect = centered_rect;

                // Hide host cursor only when captured (locked mode hides it,
                // but set_cursor_icon as extra safety). Show normal cursor otherwise.
                if self.mouse_captured {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                }

                // Draw the texture
                if ui.is_rect_visible(rect) {
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );

                    // Show capture hint overlay when not captured and hovering
                    if !self.mouse_captured && response.hovered() {
                        let hint = "Click to capture input";
                        let text_pos = egui::pos2(rect.min.x + 8.0, rect.max.y - 24.0);
                        ui.painter().text(
                            text_pos,
                            egui::Align2::LEFT_BOTTOM,
                            hint,
                            egui::FontId::proportional(13.0),
                            egui::Color32::from_rgba_premultiplied(200, 200, 200, 180),
                        );
                    }
                }

                // Click on display → capture mouse and keyboard input
                if response.clicked() || response.drag_started() {
                    ui.memory_mut(|m| m.request_focus(display_id));
                    if use_evdev && !self.mouse_captured {
                        self.capture_mouse(ctx, use_evdev);
                    }
                }

                // Auto-focus when VM is running
                if !ui.memory(|m| m.has_focus(display_id)) {
                    ui.memory_mut(|m| m.request_focus(display_id));
                }

                // Mouse handling: send mouse events to VM
                if let Some(handle) = vm_handle {
                    self.handle_mouse_input(ui, ctx, rect, handle, &response);
                }

                let focused = ui.memory(|m| m.has_focus(display_id));
                return (focused, Some(rect));
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("No display");
            });
        }
        (false, None)
    }

    /// Capture the mouse: lock cursor and start evdev reader for raw deltas.
    /// `use_evdev`: whether to use evdev raw capture (from preferences).
    fn capture_mouse(&mut self, ctx: &egui::Context, use_evdev: bool) {
        self.mouse_captured = true;
        self.capture_time = Some(std::time::Instant::now());
        self.last_mouse_pos = None;
        self.mouse_accum_x = 0.0;
        self.mouse_accum_y = 0.0;
        self.abs_cursor_x = 16383; // center
        self.abs_cursor_y = 16383; // center
        self.warp_skip_frames = 0;
        self.warp_counter = 0;

        if use_evdev {
            // Start evdev raw input reader (Linux) for reliable mouse deltas + keyboard
            #[cfg(target_os = "linux")]
            if let Some(ref mut evdev) = self.evdev_input {
                evdev.start();
                // Exclusively grab the mouse device so the host desktop stops
                // receiving events — this freezes the host cursor in place.
                let grabbed = evdev.grab_mouse();
                eprintln!("[evdev] input reader started (mouse={}, kbd={}, grabbed={})",
                    evdev.has_mouse(), evdev.has_keyboard(), grabbed);
            }

            // Lock cursor — prevents it from leaving the window.
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::Locked));
        }
        // When evdev is disabled, no cursor lock — mouse stays free (USB tablet mode).
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
        eprintln!("[mouse] Captured — Ctrl+Alt+G/F to release");
    }

    /// Check all release mechanisms. Returns true if the mouse should be released.
    fn check_mouse_release(&mut self, ui: &egui::Ui, ctx: &egui::Context) -> bool {
        // 1. Check evdev modifier state for Ctrl+Alt+G/F/Escape (Linux)
        //    The evdev reader tracks modifier state atomically — no need to consume events.
        #[cfg(target_os = "linux")]
        {
            if let Some(ref evdev) = self.evdev_input {
                if evdev.is_running() && evdev.has_keyboard() {
                    if evdev.check_release_combo() {
                        eprintln!("[mouse] Release via Ctrl+Alt+G/F/Esc (evdev)");
                        return true;
                    }
                }
            }
        }

        // 2. Ctrl+Alt+G/F/Escape (via egui events — fallback when evdev not active)
        let key_release = ui.input(|i| {
            i.events.iter().any(|e| match e {
                egui::Event::Key { key: egui::Key::G, pressed: true, modifiers, .. }
                    if modifiers.ctrl && modifiers.alt => true,
                egui::Event::Key { key: egui::Key::F, pressed: true, modifiers, .. }
                    if modifiers.ctrl && modifiers.alt => true,
                egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. }
                    if modifiers.ctrl && modifiers.alt => true,
                _ => false,
            })
        });
        if key_release {
            eprintln!("[mouse] Release via Ctrl+Alt+G/F/Esc (event)");
            return true;
        }

        // 3. Ctrl+Alt held + G/F pressed (via modifier state — works even if events are broken)
        let mod_release = ctx.input(|i| {
            i.modifiers.ctrl && i.modifiers.alt
                && (i.key_pressed(egui::Key::G) || i.key_pressed(egui::Key::F) || i.key_pressed(egui::Key::Escape))
        });
        if mod_release {
            eprintln!("[mouse] Release via Ctrl+Alt+G/F/Esc (modifier state)");
            return true;
        }

        // // 3. Right mouse button held for 1 second — emergency release
        // let right_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Secondary));
        // if right_down {
        //     if self.right_click_start.is_none() {
        //         self.right_click_start = Some(std::time::Instant::now());
        //     } else if self.right_click_start.unwrap().elapsed() >= std::time::Duration::from_secs(1) {
        //         eprintln!("[mouse] Release via right-click hold (1s)");
        //         self.right_click_start = None;
        //         return true;
        //     }
        // } else {
        //     self.right_click_start = None;
        // }

        // // 4. Middle mouse button click — instant release
        // let middle_clicked = ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Middle));
        // if middle_clicked {
        //     eprintln!("[mouse] Release via middle click");
        //     return true;
        // }

        // 5. Window focus lost — auto release.
        //    On Linux, CursorGrab::Locked is unreliable across WMs and may cause
        //    false focus-loss reports.  Only release on focus loss when evdev is NOT
        //    available (non-Linux or no evdev reader).
        #[cfg(target_os = "linux")]
        let has_evdev = self.evdev_input.as_ref()
            .map_or(false, |e| e.is_running());
        #[cfg(not(target_os = "linux"))]
        let has_evdev = false;

        if !has_evdev {
            let has_focus = ctx.input(|i| i.focused);
            if !has_focus {
                eprintln!("[mouse] Release via focus loss");
                return true;
            }
        }

        false
    }

    /// Release the mouse: show cursor, ungrab, and stop evdev reader.
    pub fn release_mouse(&mut self, ctx: &egui::Context) {
        self.release_mouse_with_handle(ctx, None);
    }

    pub fn release_mouse_with_handle(&mut self, ctx: &egui::Context, vm_handle: Option<u64>) {
        self.mouse_captured = false;
        self.capture_time = None;
        self.last_mouse_pos = None;
        self.right_click_start = None;
        self.escape_presses.clear();

        // Release exclusive mouse grab and stop evdev raw input reader
        #[cfg(target_os = "linux")]
        if let Some(ref mut evdev) = self.evdev_input {
            evdev.ungrab_mouse();
            evdev.stop();
            eprintln!("[evdev] input reader stopped");
        }

        // Release all modifier keys in the guest. When the user presses
        // Ctrl+Alt+G to release the mouse, the evdev reader thread is stopped
        // before the key-up events arrive — so the guest still thinks Ctrl and
        // Alt are held down. Send explicit release scancodes for all modifiers.
        if let Some(h) = vm_handle {
            // PS/2 Set 1 break codes (scancode | 0x80)
            libcorevm::ffi::corevm_ps2_key_release(h, 0x1D); // Left Ctrl
            libcorevm::ffi::corevm_ps2_key_release(h, 0x38); // Left Alt
            libcorevm::ffi::corevm_ps2_key_release(h, 0x2A); // Left Shift
            // Extended keys need E0 prefix
            libcorevm::ffi::corevm_ps2_key_press(h, 0xE0);
            libcorevm::ffi::corevm_ps2_key_release(h, 0x1D); // Right Ctrl
            libcorevm::ffi::corevm_ps2_key_press(h, 0xE0);
            libcorevm::ffi::corevm_ps2_key_release(h, 0x38); // Right Alt
            libcorevm::ffi::corevm_ps2_key_release(h, 0x36); // Right Shift
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        eprintln!("[mouse] Released");
    }

    /// Handle mouse input over the display area and inject PS/2 events into the VM.
    fn handle_mouse_input(&mut self, ui: &egui::Ui, ctx: &egui::Context, display_rect: egui::Rect, vm_handle: u64, _response: &egui::Response) {
        // Read button state: prefer evdev when mouse is exclusively grabbed
        // (EVIOCGRAB prevents egui from seeing button events).
        let buttons = {
            let mut from_evdev = false;
            #[cfg(target_os = "linux")]
            {
                if let Some(ref evdev) = self.evdev_input {
                    if evdev.is_mouse_grabbed() {
                        from_evdev = true;
                    }
                }
            }
            if from_evdev {
                #[cfg(target_os = "linux")]
                {
                    self.evdev_input.as_ref().map(|e| e.get_buttons()).unwrap_or(0)
                }
                #[cfg(not(target_os = "linux"))]
                { 0u8 }
            } else {
                ui.input(|i| {
                    let mut b = 0u8;
                    if i.pointer.button_down(egui::PointerButton::Primary) { b |= 1; }
                    if i.pointer.button_down(egui::PointerButton::Secondary) { b |= 2; }
                    if i.pointer.button_down(egui::PointerButton::Middle) { b |= 4; }
                    b
                })
            }
        };

        self.mouse_debug_counter += 1;

        let has_virtio_input = libcorevm::ffi::corevm_has_virtio_input(vm_handle) != 0;

        // Capture scroll wheel delta
        let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
        let wheel: i8 = if scroll_y > 1.0 { 1 } else if scroll_y < -1.0 { -1 } else { 0 };

        // When not captured: send absolute mouse position via USB tablet (if enabled),
        // but skip PS/2 relative mouse and evdev.
        if !self.mouse_captured {
            if self.usb_tablet_mode {
                if let Some(hover_pos) = _response.hover_pos() {
                    let rel_x = ((hover_pos.x - display_rect.min.x) / display_rect.width()).clamp(0.0, 1.0);
                    let rel_y = ((hover_pos.y - display_rect.min.y) / display_rect.height()).clamp(0.0, 1.0);
                    let abs_x = (rel_x * 32767.0) as u16;
                    let abs_y = (rel_y * 32767.0) as u16;
                    if abs_x != self.abs_cursor_x || abs_y != self.abs_cursor_y || buttons != self.last_mouse_buttons || wheel != 0 {
                        self.abs_cursor_x = abs_x;
                        self.abs_cursor_y = abs_y;
                        libcorevm::ffi::corevm_usb_tablet_move_wheel(
                            vm_handle, abs_x, abs_y, buttons, wheel,
                        );
                        if has_virtio_input {
                            libcorevm::ffi::corevm_virtio_tablet_move(
                                vm_handle, abs_x as u32, abs_y as u32, buttons,
                            );
                        }
                    }
                }
            }
            self.last_mouse_buttons = buttons;
            return;
        }

        // Skip frames right after a cursor warp to discard phantom deltas
        // caused by the warp itself.
        if self.warp_skip_frames > 0 {
            self.warp_skip_frames -= 1;
            self.mouse_accum_x = 0.0;
            self.mouse_accum_y = 0.0;
            // Still handle button changes during skip
            if buttons != self.last_mouse_buttons {
                libcorevm::ffi::corevm_ps2_mouse_move(vm_handle, 0, 0, buttons);
                if self.usb_tablet_mode {
                    libcorevm::ffi::corevm_usb_tablet_move_wheel(
                        vm_handle, self.abs_cursor_x, self.abs_cursor_y, buttons, 0,
                    );
                }
                self.last_mouse_buttons = buttons;
            }
            ctx.request_repaint();
            return;
        }

        // Get raw mouse deltas: try evdev first (Linux), fall back to egui pointer.
        let (evdev_dx, evdev_dy, evdev_wheel): (i32, i32, i32);

        // Try evdev deltas (Linux only)
        let mut got_evdev = false;
        #[cfg(target_os = "linux")]
        {
            if let Some(ref evdev) = self.evdev_input {
                if evdev.is_running() && evdev.has_mouse() {
                    let (dx, dy, w) = evdev.take_mouse_deltas();
                    if dx != 0 || dy != 0 || w != 0 {
                        evdev_dx = dx;
                        evdev_dy = dy;
                        evdev_wheel = w;
                        got_evdev = true;
                    } else {
                        evdev_dx = 0;
                        evdev_dy = 0;
                        evdev_wheel = 0;
                    }
                } else {
                    evdev_dx = 0;
                    evdev_dy = 0;
                    evdev_wheel = 0;
                }
            } else {
                evdev_dx = 0;
                evdev_dy = 0;
                evdev_wheel = 0;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            evdev_dx = 0;
            evdev_dy = 0;
            evdev_wheel = 0;
        }

        // Fallback: egui pointer.delta() when evdev provided nothing.
        // Works on some WMs with CursorGrab::Locked.
        let (final_dx, final_dy) = if got_evdev {
            (evdev_dx, evdev_dy)
        } else {
            let d = ui.input(|i| i.pointer.delta());
            (d.x as i32, d.y as i32)
        };

        // Deltas: +X = right, +Y = down (screen coordinates)
        let ps2_dx = final_dx as i16;
        let ps2_dy = -(final_dy as i16); // PS/2: positive Y = UP

        // Use evdev wheel if available, otherwise egui scroll
        let final_wheel: i8 = if evdev_wheel != 0 {
            evdev_wheel.clamp(-1, 1) as i8
        } else {
            wheel
        };

        // Send PS/2 relative mouse events (with scroll wheel)
        if ps2_dx != 0 || ps2_dy != 0 || final_wheel != 0 {
            if self.mouse_debug_counter % 60 == 0 {
                eprintln!("[mouse] dx={} dy={} wheel={} abs=({},{}) src={}",
                    ps2_dx, ps2_dy, final_wheel, self.abs_cursor_x, self.abs_cursor_y,
                    if got_evdev { "evdev" } else { "egui" });
            }
            libcorevm::ffi::corevm_ps2_mouse_move_wheel(vm_handle, ps2_dx, ps2_dy, buttons, final_wheel);
        } else if buttons != self.last_mouse_buttons {
            libcorevm::ffi::corevm_ps2_mouse_move(vm_handle, 0, 0, buttons);
        }

        // For USB tablet / VirtIO: also send absolute coordinates.
        // Track a virtual cursor position from accumulated deltas.
        if final_dx != 0 || final_dy != 0 || buttons != self.last_mouse_buttons || final_wheel != 0 {
            let scale_x = 32767.0 / display_rect.width();
            let scale_y = 32767.0 / display_rect.height();
            let new_x = (self.abs_cursor_x as f32 + final_dx as f32 * scale_x)
                .clamp(0.0, 32767.0) as u16;
            let new_y = (self.abs_cursor_y as f32 + final_dy as f32 * scale_y)
                .clamp(0.0, 32767.0) as u16;
            self.abs_cursor_x = new_x;
            self.abs_cursor_y = new_y;

            if has_virtio_input {
                libcorevm::ffi::corevm_virtio_tablet_move(
                    vm_handle, new_x as u32, new_y as u32, buttons,
                );
            }
            if self.usb_tablet_mode {
                libcorevm::ffi::corevm_usb_tablet_move_wheel(
                    vm_handle, new_x, new_y, buttons, final_wheel,
                );
            }
        }

        self.last_mouse_buttons = buttons;
        ctx.request_repaint();
    }

    /// Process keyboard events from evdev and send to VM.
    /// Returns a label for the last key pressed (for status bar), if any.
    /// Only active when mouse is captured (= input locked to VM).
    #[cfg(target_os = "linux")]
    pub fn handle_evdev_keyboard(&mut self, vm_handle: u64) -> Option<String> {
        if !self.mouse_captured {
            return None;
        }

        let evdev = match self.evdev_input {
            Some(ref evdev) if evdev.is_running() && evdev.has_keyboard() => evdev,
            _ => return None,
        };

        let key_events = evdev.take_key_events();
        if key_events.is_empty() {
            return None;
        }

        let has_virtio_input = libcorevm::ffi::corevm_has_virtio_input(vm_handle) != 0;
        let mut last_label = None;

        for ev in &key_events {
            if ev.pressed {
                if ev.extended {
                    libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0xE0);
                }
                libcorevm::ffi::corevm_ps2_key_press(vm_handle, ev.scancode);
                if has_virtio_input {
                    if ev.extended {
                        libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, 0xE0);
                    }
                    libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, ev.scancode);
                }
                last_label = Some(format!("0x{:02X}{}", ev.scancode,
                    if ev.extended { " (E0)" } else { "" }));
            } else {
                if ev.extended {
                    libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0xE0);
                }
                libcorevm::ffi::corevm_ps2_key_release(vm_handle, ev.scancode);
                if has_virtio_input {
                    if ev.extended {
                        libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, 0xE0);
                    }
                    libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, ev.scancode | 0x80);
                }
            }
        }

        last_label
    }
}
