//! EDID block generator for the virtual Intel HD Graphics monitor.
//!
//! Generates a 128-byte EDID block describing a "CoreVM HD" monitor
//! with 1920×1080 @ 60Hz as the preferred timing.

/// Build a 128-byte EDID block for a 1920×1080 @ 60Hz monitor.
pub const fn build_edid_1080p() -> [u8; 128] {
    let mut e = [0u8; 128];

    // ── Header ──
    e[0] = 0x00; e[1] = 0xFF; e[2] = 0xFF; e[3] = 0xFF;
    e[4] = 0xFF; e[5] = 0xFF; e[6] = 0xFF; e[7] = 0x00;

    // Manufacturer ID: "CVM" (CoreVM)
    e[8] = 0x0A; e[9] = 0xD6;
    // Product code
    e[10] = 0x01; e[11] = 0x00;
    // Serial number
    e[12] = 0x01; e[13] = 0x00; e[14] = 0x00; e[15] = 0x00;
    // Week 1, Year 2024 (2024 − 1990 = 34)
    e[16] = 1; e[17] = 34;
    // EDID version 1.4
    e[18] = 1; e[19] = 4;

    // ── Basic display parameters ──
    // Digital input, 8 bpc, DFP 1.x compatible
    e[20] = 0xA5;
    // Max image size: 53 cm × 30 cm (≈ 24″)
    e[21] = 53; e[22] = 30;
    // Gamma 2.2 (encoded: 2.2 × 100 − 100 = 120)
    e[23] = 120;
    // Features: RGB, preferred timing in DTD 1
    e[24] = 0x06;

    // ── Chromaticity (sRGB) ──
    e[25] = 0xEE; e[26] = 0x91; e[27] = 0xA3; e[28] = 0x54;
    e[29] = 0x4C; e[30] = 0x99; e[31] = 0x26; e[32] = 0x0F;
    e[33] = 0x50; e[34] = 0x54;

    // ── Established timings ──
    e[35] = 0x21; // 640×480@60, 800×600@60
    e[36] = 0x08; // 1024×768@60
    e[37] = 0x00;

    // ── Standard timings (unused — DTD covers 1080p) ──
    let mut i = 38;
    while i < 54 {
        e[i] = 0x01; e[i + 1] = 0x01;
        i += 2;
    }

    // ── DTD 1: 1920×1080 @ 60 Hz ──
    // Pixel clock: 148.50 MHz = 14850 (in 10 kHz units) = 0x3A02 LE
    e[54] = 0x02; e[55] = 0x3A;
    // H active low 8: 1920 & 0xFF = 0x80
    e[56] = 0x80;
    // H blanking low 8: 280 & 0xFF = 0x18
    e[57] = 0x18;
    // H active high 4 | H blanking high 4: (1920>>8=7)<<4 | (280>>8=1) = 0x71
    e[58] = 0x71;
    // V active low 8: 1080 & 0xFF = 0x38
    e[59] = 0x38;
    // V blanking low 8: 45
    e[60] = 0x2D;
    // V active high 4 | V blanking high 4: (1080>>8=4)<<4 | 0 = 0x40
    e[61] = 0x40;
    // H front porch: 88, H sync width: 44
    e[62] = 88;
    e[63] = 44;
    // V front porch 4 | V sync 5 → (4<<4)|5 = 0x45
    e[64] = 0x45;
    // High bits: all zero
    e[65] = 0x00;
    // Image size: 530 mm × 300 mm
    e[66] = 0x12; // 530 & 0xFF
    e[67] = 0x2C; // 300 & 0xFF
    e[68] = 0x21; // (530>>8)<<4 | (300>>8)
    // No border
    e[69] = 0; e[70] = 0;
    // Non-interlaced, digital separate sync, H+/V+
    e[71] = 0x1E;

    // ── Descriptor 2: Monitor name "CoreVM HD" ──
    e[72] = 0; e[73] = 0; e[74] = 0;
    e[75] = 0xFC; // Monitor name tag
    e[76] = 0;
    e[77] = b'C'; e[78] = b'o'; e[79] = b'r'; e[80] = b'e';
    e[81] = b'V'; e[82] = b'M'; e[83] = b' '; e[84] = b'H';
    e[85] = b'D'; e[86] = 0x0A;
    e[87] = 0x20; e[88] = 0x20; e[89] = 0x20;

    // ── Descriptor 3: Monitor range limits ──
    e[90] = 0; e[91] = 0; e[92] = 0;
    e[93] = 0xFD; // Range limits tag
    e[94] = 0;
    e[95] = 50;  // Min V: 50 Hz
    e[96] = 75;  // Max V: 75 Hz
    e[97] = 30;  // Min H: 30 kHz
    e[98] = 80;  // Max H: 80 kHz
    e[99] = 16;  // Max pixel clock: 160 MHz
    e[100] = 0;
    e[101] = 0x0A;
    e[102] = 0x20; e[103] = 0x20; e[104] = 0x20;
    e[105] = 0x20; e[106] = 0x20; e[107] = 0x20;

    // Descriptor 4: unused (zero-filled)
    // e[108..126] already 0

    // Extension count
    e[126] = 0;

    // ── Checksum ──
    let mut sum: u32 = 0;
    let mut j = 0;
    while j < 127 {
        sum += e[j] as u32;
        j += 1;
    }
    e[127] = (256 - (sum & 0xFF)) as u8;

    e
}

/// Default EDID for the virtual monitor.
pub const DEFAULT_EDID: [u8; 128] = build_edid_1080p();
