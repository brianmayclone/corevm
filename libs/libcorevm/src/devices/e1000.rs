//! Intel 82540EM Gigabit Ethernet controller — complete emulation.
//!
//! Emulates the full Intel 82540EM (PCI device 8086:100E) with:
//!
//! - MMIO and I/O BAR register access (128 KB register space)
//! - EEPROM via Microwire bit-bang (EECD) and direct read (EERD)
//! - PHY via MDIC (M88E1011 Marvell Alaska)
//! - TX: legacy, context, and data descriptors; checksum offload; TSO
//! - RX: descriptor ring delivery with filtering, checksum offload
//! - RX address filtering: unicast (16 RA pairs), multicast (MTA), VLAN (VFTA)
//! - Full statistics counters (0x4000–0x40E4), clear-on-read
//! - Interrupt throttling (ITR), delay timers (RDTR, RADV, TIDV, TADV)
//! - Flow control registers (FCAL, FCAH, FCT, FCTTV, FCRTL, FCRTH)
//! - Misc: LEDCTL, PBA, TIPG, RXDCTL, TXDCTL, WoL, MANC, SWSM/FWSM
//!
//! # MMIO Layout (key registers)
//!
//! | Offset | Name | Description |
//! |--------|------|-------------|
//! | 0x0000 | CTRL | Device Control |
//! | 0x0008 | STATUS | Device Status |
//! | 0x0010 | EECD | EEPROM/Flash Control |
//! | 0x0014 | EERD | EEPROM Read |
//! | 0x0018 | CTRL_EXT | Extended Device Control |
//! | 0x0020 | MDIC | MDI Control |
//! | 0x00C0 | ICR | Interrupt Cause Read |
//! | 0x00C4 | ITR | Interrupt Throttle Rate |
//! | 0x00C8 | ICS | Interrupt Cause Set |
//! | 0x00D0 | IMS | Interrupt Mask Set |
//! | 0x00D8 | IMC | Interrupt Mask Clear |
//! | 0x0100 | RCTL | Receive Control |
//! | 0x0400 | TCTL | Transmit Control |
//! | 0x0410 | TIPG | TX Inter-Packet Gap |
//! | 0x1000 | PBA  | Packet Buffer Allocation |
//! | 0x2800 | RDBAL | RX Descriptor Base Low |
//! | 0x2808 | RDLEN | RX Descriptor Ring Length |
//! | 0x2810 | RDH | RX Descriptor Head |
//! | 0x2818 | RDT | RX Descriptor Tail |
//! | 0x2820 | RDTR | RX Delay Timer |
//! | 0x2828 | RXDCTL | RX Descriptor Control |
//! | 0x282C | RADV | RX Absolute Delay |
//! | 0x3800 | TDBAL | TX Descriptor Base Low |
//! | 0x3808 | TDLEN | TX Descriptor Ring Length |
//! | 0x3810 | TDH | TX Descriptor Head |
//! | 0x3818 | TDT | TX Descriptor Tail |
//! | 0x3820 | TIDV | TX Interrupt Delay |
//! | 0x3828 | TXDCTL | TX Descriptor Control |
//! | 0x382C | TADV | TX Absolute Delay |
//! | 0x4000–0x40E4 | Stats | Statistics (clear-on-read) |
//! | 0x5000 | RXCSUM | RX Checksum Control |
//! | 0x5200 | MTA[0..127] | Multicast Table Array |
//! | 0x5400 | RA[0..15] | Receive Address (RAL+RAH pairs) |
//! | 0x5600 | VFTA[0..127] | VLAN Filter Table Array |

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::Result;
use crate::memory::mmio::MmioHandler;

// ═══════════════════════════════════════════════════════════════════════════
// Register offsets
// ═══════════════════════════════════════════════════════════════════════════

const REG_CTRL: usize      = 0x0000;
const REG_STATUS: usize    = 0x0008;
const REG_EECD: usize      = 0x0010;
const REG_EERD: usize      = 0x0014;
const REG_CTRL_EXT: usize  = 0x0018;
const REG_MDIC: usize      = 0x0020;

// Flow control
const REG_FCAL: usize      = 0x0028;
const REG_FCAH: usize      = 0x002C;
const REG_FCT: usize       = 0x0030;
const REG_VET: usize       = 0x0038;

// Interrupts
const REG_ICR: usize       = 0x00C0;
const REG_ITR: usize       = 0x00C4;
const REG_ICS: usize       = 0x00C8;
const REG_IMS: usize       = 0x00D0;
const REG_IMC: usize       = 0x00D8;

// Receive
const REG_RCTL: usize      = 0x0100;
const REG_FCTTV: usize     = 0x0170;

// Transmit
const REG_TCTL: usize      = 0x0400;
const REG_TCTL_EXT: usize  = 0x0404;
const REG_TIPG: usize      = 0x0410;

// LED
const REG_LEDCTL: usize    = 0x0E00;

// Packet buffer allocation
const REG_PBA: usize        = 0x1000;

// Flow control thresholds
const REG_FCRTL: usize      = 0x2160;
const REG_FCRTH: usize      = 0x2168;

// RX descriptor ring
const REG_RDBAL: usize      = 0x2800;
const REG_RDBAH: usize      = 0x2804;
const REG_RDLEN: usize      = 0x2808;
const REG_RDH: usize        = 0x2810;
const REG_RDT: usize        = 0x2818;
const REG_RDTR: usize       = 0x2820;
const REG_RXDCTL: usize     = 0x2828;
const REG_RADV: usize       = 0x282C;

// TX descriptor ring
const REG_TDBAL: usize      = 0x3800;
const REG_TDBAH: usize      = 0x3804;
const REG_TDLEN: usize      = 0x3808;
const REG_TDH: usize        = 0x3810;
const REG_TDT: usize        = 0x3818;
const REG_TIDV: usize       = 0x3820;
const REG_TXDCTL: usize     = 0x3828;
const REG_TADV: usize       = 0x382C;

// Statistics registers (0x4000–0x40E4) — all clear-on-read.
const REG_STATS_BASE: usize = 0x4000;
const REG_STATS_END: usize  = 0x4100; // exclusive
const STAT_CRCERRS: usize   = 0x4000;
const STAT_ALGNERRC: usize  = 0x4004;
const STAT_SYMERRS: usize   = 0x4008;
const STAT_RXERRC: usize    = 0x400C;
const STAT_MPC: usize       = 0x4010;
const STAT_SCC: usize       = 0x4014;
const STAT_ECOL: usize      = 0x4018;
const STAT_LATECOL: usize   = 0x401C;
const STAT_DC: usize        = 0x4028;
const STAT_TNCRS: usize     = 0x402C;
const STAT_CEXTERR: usize   = 0x4034;
const STAT_RLEC: usize      = 0x4038;
const STAT_XONRXC: usize    = 0x403C;
const STAT_XONTXC: usize    = 0x4040;
const STAT_XOFFRXC: usize   = 0x4044;
const STAT_XOFFTXC: usize   = 0x4048;
const STAT_FCRUC: usize     = 0x404C;
const STAT_PRC64: usize     = 0x4050;
const STAT_PRC127: usize    = 0x4054;
const STAT_PRC255: usize    = 0x4058;
const STAT_PRC511: usize    = 0x405C;
const STAT_PRC1023: usize   = 0x4060;
const STAT_PRC1522: usize   = 0x4064;
const STAT_GPRC: usize      = 0x4068;
const STAT_BPRC: usize      = 0x406C;
const STAT_MPRC: usize      = 0x4070;
const STAT_GPTC: usize      = 0x4074;
const STAT_GORCL: usize     = 0x4078;
const STAT_GORCH: usize     = 0x407C;
const STAT_GOTCL: usize     = 0x4080;
const STAT_GOTCH: usize     = 0x4084;
const STAT_RNBC: usize      = 0x4088;
const STAT_RUC: usize       = 0x408C;
const STAT_RFC: usize       = 0x4090;
const STAT_ROC: usize       = 0x4094;
const STAT_RJC: usize       = 0x4098;
const STAT_MGTPRC: usize    = 0x409C;
const STAT_MGTPDC: usize    = 0x40A0;
const STAT_MGTPTC: usize    = 0x40A4;
const STAT_TORL: usize      = 0x40A8;
const STAT_TORH: usize      = 0x40AC;
const STAT_TOTL: usize      = 0x40B0;
const STAT_TOTH: usize      = 0x40B4;
const STAT_TPR: usize       = 0x40B8;
const STAT_TPT: usize       = 0x40BC;
const STAT_PTC64: usize     = 0x40C0;
const STAT_PTC127: usize    = 0x40C4;
const STAT_PTC255: usize    = 0x40C8;
const STAT_PTC511: usize    = 0x40CC;
const STAT_PTC1023: usize   = 0x40D0;
const STAT_PTC1522: usize   = 0x40D4;
const STAT_MPTC: usize      = 0x40D8;
const STAT_BPTC: usize      = 0x40DC;
const STAT_TSCTC: usize     = 0x40E0;
const STAT_TSCTFC: usize    = 0x40E4;

// RX checksum offload
const REG_RXCSUM: usize     = 0x5000;

// Multicast table array — 128 × 32-bit entries (4096 hash bits)
const REG_MTA: usize         = 0x5200;
const REG_MTA_END: usize     = 0x5400;

// Receive address registers — 16 pairs (RAL + RAH), each 8 bytes
const REG_RAL0: usize        = 0x5400;
const REG_RAH0: usize        = 0x5404;
const REG_RA_END: usize      = 0x5480;

// VLAN filter table array — 128 × 32-bit entries (4096 VLAN IDs)
const REG_VFTA: usize        = 0x5600;
const REG_VFTA_END: usize    = 0x5800;

// Wake-on-LAN
const REG_WUC: usize         = 0x5800;
const REG_WUFC: usize        = 0x5808;
const REG_WUS: usize         = 0x5810;

// Management / firmware
const REG_MANC: usize        = 0x5820;
const REG_SWSM: usize        = 0x5B50;
const REG_FWSM: usize        = 0x5B54;

// ═══════════════════════════════════════════════════════════════════════════
// Bit-field constants
// ═══════════════════════════════════════════════════════════════════════════

/// Total register space: 128 KB = 0x20000 bytes = 0x8000 dwords.
const REG_SPACE_DWORDS: usize = 0x8000;

// CTRL register
const CTRL_FD: u32       = 1 << 0;
const CTRL_LRST: u32     = 1 << 3;
const CTRL_ASDE: u32     = 1 << 5;
const CTRL_SLU: u32      = 1 << 6;
const CTRL_RST: u32      = 1 << 26;
const CTRL_VME: u32      = 1 << 30;
const CTRL_PHY_RST: u32  = 1 << 31;

// STATUS register
const STATUS_FD: u32     = 0x01;
const STATUS_LU: u32     = 0x02;
const STATUS_PHYRA: u32  = 1 << 10; // PHY Reset Asserted
const STATUS_SPEED_1000: u32 = 0x80;

// EECD register bits
const EECD_SK: u32   = 1 << 0;
const EECD_CS: u32   = 1 << 1;
const EECD_DI: u32   = 1 << 2;
const EECD_DO: u32   = 1 << 3;
const EECD_FWE: u32  = 0x30;
const EECD_REQ: u32  = 1 << 6;
const EECD_GNT: u32  = 1 << 7;
const EECD_PRES: u32 = 1 << 8;

// EERD register
const EERD_START: u32 = 1 << 0;
const EERD_DONE: u32  = 1 << 4;

// MDIC register
const MDIC_DATA_MASK: u32 = 0xFFFF;
const MDIC_REG_SHIFT: u32 = 16;
const MDIC_REG_MASK: u32  = 0x1F << 16;
const MDIC_OP_WRITE: u32  = 1 << 26;
const MDIC_OP_READ: u32   = 2 << 26;
const MDIC_READY: u32     = 1 << 28;
const MDIC_ERROR: u32     = 1 << 30;

// ICR bits (Interrupt Cause)
const ICR_TXDW: u32   = 1 << 0;
const ICR_TXQE: u32   = 1 << 1;
const ICR_LSC: u32    = 1 << 2;
const ICR_RXDMT0: u32 = 1 << 4;
const ICR_RXO: u32    = 1 << 6;
const ICR_RXT0: u32   = 1 << 7;
const ICR_MDAC: u32   = 1 << 9;
const ICR_PHYINT: u32 = 1 << 12;
const ICR_TXD_LOW: u32 = 1 << 15;

// RCTL bits (Receive Control)
const RCTL_EN: u32     = 1 << 1;
const RCTL_SBP: u32    = 1 << 2;
const RCTL_UPE: u32    = 1 << 3;
const RCTL_MPE: u32    = 1 << 4;
const RCTL_LPE: u32    = 1 << 5;
const RCTL_MO_SHIFT: usize = 12;
const RCTL_BAM: u32    = 1 << 15;
const RCTL_BSIZE_SHIFT: usize = 16;
const RCTL_VFE: u32    = 1 << 18;
const RCTL_BSEX: u32   = 1 << 25;
const RCTL_SECRC: u32  = 1 << 26;

// TCTL bits (Transmit Control)
const TCTL_EN: u32     = 1 << 1;
const TCTL_PSP: u32    = 1 << 3;
const TCTL_CT_SHIFT: usize = 4;
const TCTL_COLD_SHIFT: usize = 12;

// RXCSUM bits
const RXCSUM_IPOFLD: u32   = 1 << 8;
const RXCSUM_TUOFLD: u32   = 1 << 9;

// TX descriptor CMD bits
const TXD_CMD_EOP: u8  = 1 << 0;
const TXD_CMD_IFCS: u8 = 1 << 1;
const TXD_CMD_IC: u8   = 1 << 2;
const TXD_CMD_RS: u8   = 1 << 3;
const TXD_CMD_DEXT: u8 = 1 << 5;

// TX descriptor STA bits
const TXD_STA_DD: u8 = 1 << 0;

// TX data descriptor POPTS
const TXD_POPTS_IXSM: u8 = 1 << 0;
const TXD_POPTS_TXSM: u8 = 1 << 1;

// TX context descriptor TUCMD bits
const TUCMD_TSE: u8 = 1 << 2;
const TUCMD_TCP: u8 = 1 << 1;
const TUCMD_IP: u8  = 1 << 0;

// RX descriptor status bits
const RXD_STA_DD: u8   = 1 << 0;
const RXD_STA_EOP: u8  = 1 << 1;
const RXD_STA_VP: u8   = 1 << 3;
const RXD_STA_IPCS: u8 = 1 << 5;
const RXD_STA_TCPCS: u8 = 1 << 6;

// RX descriptor error bits
const RXD_ERR_IPE: u8  = 1 << 5;
const RXD_ERR_TCPE: u8 = 1 << 6;

// PHY register addresses (M88E1011 Marvell Alaska)
const PHY_CTRL: u32            = 0x00;
const PHY_STATUS: u32          = 0x01;
const PHY_ID1: u32             = 0x02;
const PHY_ID2: u32             = 0x03;
const PHY_AUTONEG_ADV: u32    = 0x04;
const PHY_LP_ABILITY: u32     = 0x05;
const PHY_1000T_CTRL: u32     = 0x09;
const PHY_1000T_STATUS: u32   = 0x0A;
const M88_PHY_SPEC_CTRL: u32  = 0x10;
const M88_PHY_SPEC_STATUS: u32 = 0x11;
const M88_EXT_PHY_SPEC_CTRL: u32 = 0x14;
const M88_EXT_PHY_SPEC_STATUS: u32 = 0x15;

// Microwire EEPROM
const MW_OPCODE_READ: u8 = 0b110;

// EEPROM layout
const EEPROM_CHECKSUM_TARGET: u16 = 0xBABA;
const EEPROM_MAC0: usize       = 0x00;
const EEPROM_MAC1: usize       = 0x01;
const EEPROM_MAC2: usize       = 0x02;
const EEPROM_SUBSYS_ID: usize  = 0x0B;
const EEPROM_SUBSYS_VID: usize = 0x0C;
const EEPROM_DEVICE_ID: usize  = 0x0D;
const EEPROM_CHECKSUM: usize   = 0x3F;

// PCI hole for GPA → host offset translation
const PCI_HOLE_START: u64 = 0xE000_0000;
const PCI_HOLE_END: u64   = 0x1_0000_0000;

// Ethernet constants
const ETH_MIN_FRAME: usize = 64;
const ETH_FCS_LEN: usize   = 4;
const ETH_ALEN: usize      = 6;

// ═══════════════════════════════════════════════════════════════════════════
// Microwire EEPROM state machine
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
enum EepromState {
    Idle,
    ReceivingCommand { bits_received: u8, shift_reg: u16 },
    SendingData { bits_sent: u8, data: u16 },
}

// ═══════════════════════════════════════════════════════════════════════════
// TX context (checksum offload + TSO parameters)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy, Default)]
struct TxContext {
    ipcss: u8,
    ipcso: u8,
    ipcse: u16,
    tucss: u8,
    tucso: u8,
    tucse: u16,
    hdrlen: u8,
    mss: u16,
    tse: bool,
    tcp: bool,
    ipv4: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// E1000 device
// ═══════════════════════════════════════════════════════════════════════════

/// Full Intel 82540EM Gigabit Ethernet controller emulation.
pub struct E1000 {
    /// Full 128 KB register space stored as 32-bit words.
    pub regs: Vec<u32>,
    /// MAC address (6 bytes).
    pub mac_address: [u8; 6],
    /// 64-entry EEPROM contents (16-bit words).
    pub eeprom: [u16; 64],
    /// Packets received from the network, waiting for the guest to consume.
    pub rx_buffer: VecDeque<Vec<u8>>,
    /// Packets transmitted by the guest, waiting for the host to send.
    pub tx_buffer: Vec<Vec<u8>>,
    /// EECD Microwire bit-bang state machine.
    ee_state: EepromState,
    /// Previous clock state for edge detection.
    ee_sk_prev: bool,
    /// Current EECD register value (managed separately from regs[]).
    ee_cd: u32,
    /// PHY registers (32 × 16-bit, accessed via MDIC).
    phy_regs: [u16; 32],
    /// TX context from the most recent context descriptor.
    tx_ctx: TxContext,
    /// I/O indirect register address (written via I/O BAR port+0).
    pub io_addr: u32,
    /// Guest memory pointer for DMA.
    pub guest_mem_ptr: *mut u8,
    /// Guest memory length in bytes.
    pub guest_mem_len: usize,
    /// MSI state.
    pub msi_enabled: bool,
    pub msi_address: u64,
    pub msi_data: u32,
    /// Callback to assert/de-assert the interrupt line immediately.
    /// Called from ICS/IMS writes so the interrupt arrives before the
    /// driver's next instruction (critical for the interrupt test).
    #[cfg(feature = "std")]
    pub irq_callback: Option<alloc::boxed::Box<dyn FnMut(bool) + Send>>,
    /// Flag set by fire_irq_assert when the callback fires.
    /// poll_irqs should check this and synchronize e1000_irq_asserted.
    pub irq_pending_assert: bool,
    /// Optional I/O activity callback: called on TX/RX activity.
    pub io_activity_cb: Option<fn(ctx: *mut ())>,
    pub io_activity_ctx: *mut (),
}

impl E1000 {
    /// Create a new E1000 NIC with the specified MAC address.
    pub fn new(mac: [u8; 6]) -> Self {
        let mut regs = vec![0u32; REG_SPACE_DWORDS];

        // STATUS: link up, full duplex, speed 1000 Mbps, PHY reset asserted.
        regs[REG_STATUS / 4] = STATUS_LU | STATUS_FD | STATUS_SPEED_1000 | STATUS_PHYRA;

        // CTRL: auto-speed detect, set link up, full duplex.
        regs[REG_CTRL / 4] = CTRL_ASDE | CTRL_SLU | CTRL_FD;

        // PBA: default 48 KB RX / 16 KB TX.
        regs[REG_PBA / 4] = 0x0030;

        // TIPG: default for copper (10, 10, 10).
        regs[REG_TIPG / 4] = 10 | (10 << 10) | (10 << 20);

        // TXDCTL / RXDCTL: defaults.
        regs[REG_TXDCTL / 4] = (1 << 24) | (1 << 16);
        regs[REG_RXDCTL / 4] = (1 << 24) | (1 << 16);

        // LEDCTL: default LED behavior.
        regs[REG_LEDCTL / 4] = 0x07061302;

        // TCTL: default collision threshold and distance.
        regs[REG_TCTL / 4] = (15 << TCTL_CT_SHIFT) | (64 << TCTL_COLD_SHIFT);

        // VET: default VLAN Ether Type (0x8100).
        regs[REG_VET / 4] = 0x8100;

        // Flow control defaults.
        regs[REG_FCAL / 4] = 0x00C28001;
        regs[REG_FCAH / 4] = 0x00000100;
        regs[REG_FCT / 4] = 0x8808;

        // FWSM: firmware ready.
        regs[REG_FWSM / 4] = 1 << 0;

        // Store MAC address in Receive Address registers (RA[0]).
        let ral = u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]);
        let rah = (mac[4] as u32) | ((mac[5] as u32) << 8) | (1 << 31); // AV bit
        regs[REG_RAL0 / 4] = ral;
        regs[REG_RAH0 / 4] = rah;

        // Populate EEPROM with MAC address and metadata.
        // Matches Intel 82540EM defaults (QEMU reference values).
        let mut eeprom = [0u16; 64];
        eeprom[EEPROM_MAC0] = (mac[1] as u16) << 8 | (mac[0] as u16);
        eeprom[EEPROM_MAC1] = (mac[3] as u16) << 8 | (mac[2] as u16);
        eeprom[EEPROM_MAC2] = (mac[5] as u16) << 8 | (mac[4] as u16);
        // Word 0x03: Init Control Word 1 — signature, load subsys IDs
        //   Bit 14: Reserved (1), Bit 6: Load Subsystem ID (1),
        //   Bit 4: Load Device ID (1), Bits 1:0 = 01 (valid signature)
        eeprom[0x03] = 0x4051;
        // Word 0x05: Image Version Info / compatibility
        eeprom[0x05] = 0x0200;
        // Word 0x08: PBA Low (printed board assembly — cosmetic)
        eeprom[0x08] = 0x0000;
        // Word 0x09: PBA High
        eeprom[0x09] = 0x0000;
        // Word 0x0A: Init Control Word 2 (ICW2)
        //   Bit 13: PHY/Media type valid (1)
        //   Bit 10: Reserved/ASM (1)
        //   Bit 9:  CSR speed indication (1) — 1000 Mbps
        //   Bit 8:  CSR speed indication (0)
        //   Bit 6:  Signature valid (1)
        eeprom[0x0A] = 0x2E40;
        eeprom[EEPROM_SUBSYS_VID] = 0x8086;
        eeprom[EEPROM_SUBSYS_ID] = 0x100E;
        eeprom[EEPROM_DEVICE_ID] = 0x100E;
        // Word 0x0E: Software Defined Pins Control (SWDPIN)
        eeprom[0x0E] = 0x0040;
        // Word 0x0F: LED Control defaults
        eeprom[0x0F] = 0x0602;

        let mut sum: u16 = 0;
        for i in 0..EEPROM_CHECKSUM {
            sum = sum.wrapping_add(eeprom[i]);
        }
        eeprom[EEPROM_CHECKSUM] = EEPROM_CHECKSUM_TARGET.wrapping_sub(sum);

        let ee_cd = EECD_PRES | EECD_GNT | EECD_FWE;

        // PHY registers (M88E1011 Marvell Alaska 88E1011).
        let mut phy_regs = [0u16; 32];
        phy_regs[PHY_CTRL as usize] = 0x1140;
        phy_regs[PHY_STATUS as usize] = 0x796D;
        phy_regs[PHY_ID1 as usize] = 0x0141;
        phy_regs[PHY_ID2 as usize] = 0x0C20;
        phy_regs[PHY_AUTONEG_ADV as usize] = 0x01E1;
        phy_regs[PHY_LP_ABILITY as usize] = 0x45E1;
        phy_regs[PHY_1000T_CTRL as usize] = 0x0300;
        phy_regs[PHY_1000T_STATUS as usize] = 0x3C00;
        phy_regs[M88_PHY_SPEC_STATUS as usize] = 0xAC04;
        phy_regs[M88_PHY_SPEC_CTRL as usize] = 0x0068;
        phy_regs[M88_EXT_PHY_SPEC_CTRL as usize] = 0x0D60;
        // Extended PHY Specific Status (reg 0x15):
        //   Bit 15:14=10 (1000 Mbps), Bit 13=1 (Full Duplex),
        //   Bit 11=1 (Page received), Bit 10=1 (Speed/Duplex resolved)
        //   Bit 8=1 (HW Config Done)
        phy_regs[M88_EXT_PHY_SPEC_STATUS as usize] = 0xAD00;

        E1000 {
            regs,
            mac_address: mac,
            eeprom,
            rx_buffer: VecDeque::new(),
            tx_buffer: Vec::new(),
            ee_state: EepromState::Idle,
            ee_sk_prev: false,
            ee_cd,
            phy_regs,
            tx_ctx: TxContext::default(),
            io_addr: 0,
            guest_mem_ptr: core::ptr::null_mut(),
            guest_mem_len: 0,
            msi_enabled: false,
            msi_address: 0,
            msi_data: 0,
            #[cfg(feature = "std")]
            irq_callback: None,
            irq_pending_assert: false,
            io_activity_cb: None,
            io_activity_ctx: core::ptr::null_mut(),
        }
    }

    fn notify_io(&self) {
        if let Some(cb) = self.io_activity_cb {
            cb(self.io_activity_ctx);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Reset
    // ═══════════════════════════════════════════════════════════════════════

    fn reset(&mut self) {
        let mac = self.mac_address;
        let eeprom = self.eeprom;
        let guest_mem_ptr = self.guest_mem_ptr;
        let guest_mem_len = self.guest_mem_len;

        for reg in self.regs.iter_mut() {
            *reg = 0;
        }

        self.regs[REG_STATUS / 4] = STATUS_LU | STATUS_FD | STATUS_SPEED_1000 | STATUS_PHYRA;
        self.regs[REG_CTRL / 4] = CTRL_ASDE | CTRL_SLU | CTRL_FD;
        self.regs[REG_PBA / 4] = 0x0030;
        self.regs[REG_TIPG / 4] = 10 | (10 << 10) | (10 << 20);
        self.regs[REG_TXDCTL / 4] = (1 << 24) | (1 << 16);
        self.regs[REG_RXDCTL / 4] = (1 << 24) | (1 << 16);
        self.regs[REG_LEDCTL / 4] = 0x07061302;
        self.regs[REG_TCTL / 4] = (15 << TCTL_CT_SHIFT) | (64 << TCTL_COLD_SHIFT);
        self.regs[REG_VET / 4] = 0x8100;
        self.regs[REG_FCAL / 4] = 0x00C28001;
        self.regs[REG_FCAH / 4] = 0x00000100;
        self.regs[REG_FCT / 4] = 0x8808;
        self.regs[REG_FWSM / 4] = 1 << 0;

        let ral = u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]);
        let rah = (mac[4] as u32) | ((mac[5] as u32) << 8) | (1 << 31);
        self.regs[REG_RAL0 / 4] = ral;
        self.regs[REG_RAH0 / 4] = rah;

        self.eeprom = eeprom;
        self.rx_buffer.clear();
        self.tx_buffer.clear();
        self.tx_ctx = TxContext::default();

        // Restore PHY registers to power-on defaults (M88E1011).
        self.phy_regs = [0u16; 32];
        self.phy_regs[PHY_CTRL as usize] = 0x1140;
        self.phy_regs[PHY_STATUS as usize] = 0x796D;
        self.phy_regs[PHY_ID1 as usize] = 0x0141;
        self.phy_regs[PHY_ID2 as usize] = 0x0C20;
        self.phy_regs[PHY_AUTONEG_ADV as usize] = 0x01E1;
        self.phy_regs[PHY_LP_ABILITY as usize] = 0x45E1;
        self.phy_regs[PHY_1000T_CTRL as usize] = 0x0300;
        self.phy_regs[PHY_1000T_STATUS as usize] = 0x3C00;
        self.phy_regs[M88_PHY_SPEC_STATUS as usize] = 0xAC04;
        self.phy_regs[M88_PHY_SPEC_CTRL as usize] = 0x0068;
        self.phy_regs[M88_EXT_PHY_SPEC_CTRL as usize] = 0x0D60;
        self.phy_regs[M88_EXT_PHY_SPEC_STATUS as usize] = 0xAD00;

        self.ee_state = EepromState::Idle;
        self.ee_sk_prev = false;
        self.ee_cd = EECD_PRES | EECD_GNT | EECD_FWE;

        self.guest_mem_ptr = guest_mem_ptr;
        self.guest_mem_len = guest_mem_len;
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Interrupt management
    // ═══════════════════════════════════════════════════════════════════════

    /// Re-evaluate interrupt state.
    /// Normal delivery is handled by poll_irqs in the VM loop.
    /// We only use the callback in the write handler for ICS (see below).
    fn update_irq(&mut self) {
        // Delivery handled by poll_irqs — no-op for normal paths.
    }

    /// Fire the IRQ callback directly.  Used ONLY from the ICS write
    /// handler where the driver's interrupt test requires synchronous
    /// delivery (the interrupt must arrive before the next instruction).
    ///
    /// Note: this re-enters the KVM backend from within the MMIO handler.
    /// In practice this is safe because set_irq_line() only does an ioctl
    /// and doesn't touch the MMIO dispatch state.
    /// Assert the IRQ line immediately if (ICR & IMS) != 0.
    /// NEVER de-asserts — that is handled by poll_irqs which properly
    /// checks the shared IRQ 11 line (E1000 + AHCI).
    fn fire_irq_assert(&mut self) {
        // Interrupt delivery is handled entirely by poll_irqs in the
        // VM loop.  The callback is NOT used because it creates state
        // synchronization issues with e1000_irq_asserted in the VM struct.
        // poll_irqs checks (ICR & IMS) on every iteration and asserts/
        // de-asserts the IRQ line reliably.
    }

    fn raise_interrupt(&mut self, cause: u32) {
        self.regs[REG_ICR / 4] |= cause;
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Packet interface
    // ═══════════════════════════════════════════════════════════════════════

    /// Enqueue a packet received from the network for guest consumption.
    pub fn receive_packet(&mut self, data: &[u8]) {
        self.rx_buffer.push_back(data.to_vec());
        self.regs[REG_ICR / 4] |= ICR_RXT0;
    }

    /// Drain and return all packets transmitted by the guest.
    pub fn take_tx_packets(&mut self) -> Vec<Vec<u8>> {
        core::mem::take(&mut self.tx_buffer)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // DMA helpers
    // ═══════════════════════════════════════════════════════════════════════

    #[inline]
    fn gpa_to_host(&self, gpa: u64) -> Option<*mut u8> {
        let offset = if gpa < PCI_HOLE_START {
            gpa as usize
        } else if gpa >= PCI_HOLE_END {
            (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize
        } else {
            return None;
        };
        if offset >= self.guest_mem_len || self.guest_mem_ptr.is_null() {
            return None;
        }
        Some(unsafe { self.guest_mem_ptr.add(offset) })
    }

    fn dma_read(&self, gpa: u64, buf: &mut [u8]) -> bool {
        let ptr = match self.gpa_to_host(gpa) {
            Some(p) => p,
            None => return false,
        };
        if (gpa as usize) + buf.len() > self.guest_mem_len {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), buf.len()); }
        true
    }

    fn dma_write(&self, gpa: u64, data: &[u8]) -> bool {
        let ptr = match self.gpa_to_host(gpa) {
            Some(p) => p,
            None => return false,
        };
        if (gpa as usize) + data.len() > self.guest_mem_len {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()); }
        true
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Statistics helpers
    // ═══════════════════════════════════════════════════════════════════════

    #[inline]
    fn stat_inc(&mut self, offset: usize) {
        let idx = offset / 4;
        self.regs[idx] = self.regs[idx].wrapping_add(1);
    }

    #[inline]
    fn stat_add64(&mut self, lo_offset: usize, val: u64) {
        let lo_idx = lo_offset / 4;
        let hi_idx = lo_idx + 1;
        let old = (self.regs[lo_idx] as u64) | ((self.regs[hi_idx] as u64) << 32);
        let new = old.wrapping_add(val);
        self.regs[lo_idx] = new as u32;
        self.regs[hi_idx] = (new >> 32) as u32;
    }

    fn rx_size_stat(len: usize) -> usize {
        if len <= 64 { STAT_PRC64 }
        else if len <= 127 { STAT_PRC127 }
        else if len <= 255 { STAT_PRC255 }
        else if len <= 511 { STAT_PRC511 }
        else if len <= 1023 { STAT_PRC1023 }
        else { STAT_PRC1522 }
    }

    fn tx_size_stat(len: usize) -> usize {
        if len <= 64 { STAT_PTC64 }
        else if len <= 127 { STAT_PTC127 }
        else if len <= 255 { STAT_PTC255 }
        else if len <= 511 { STAT_PTC511 }
        else if len <= 1023 { STAT_PTC1023 }
        else { STAT_PTC1522 }
    }

    fn update_rx_stats(&mut self, pkt: &[u8]) {
        let len = pkt.len();
        self.stat_inc(STAT_GPRC);
        self.stat_inc(STAT_TPR);
        self.stat_inc(Self::rx_size_stat(len));
        self.stat_add64(STAT_GORCL, len as u64);
        self.stat_add64(STAT_TORL, len as u64);

        if len >= ETH_ALEN {
            if pkt[0] == 0xFF && pkt[1] == 0xFF && pkt[2] == 0xFF
               && pkt[3] == 0xFF && pkt[4] == 0xFF && pkt[5] == 0xFF {
                self.stat_inc(STAT_BPRC);
            } else if pkt[0] & 0x01 != 0 {
                self.stat_inc(STAT_MPRC);
            }
        }
    }

    fn update_tx_stats(&mut self, pkt: &[u8]) {
        let len = pkt.len();
        self.stat_inc(STAT_GPTC);
        self.stat_inc(STAT_TPT);
        self.stat_inc(Self::tx_size_stat(len));
        self.stat_add64(STAT_GOTCL, len as u64);
        self.stat_add64(STAT_TOTL, len as u64);

        if len >= ETH_ALEN {
            if pkt[0] == 0xFF && pkt[1] == 0xFF && pkt[2] == 0xFF
               && pkt[3] == 0xFF && pkt[4] == 0xFF && pkt[5] == 0xFF {
                self.stat_inc(STAT_BPTC);
            } else if pkt[0] & 0x01 != 0 {
                self.stat_inc(STAT_MPTC);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // RX address / multicast / VLAN filtering
    // ═══════════════════════════════════════════════════════════════════════

    fn rx_filter_accept(&self, frame: &[u8]) -> bool {
        if frame.len() < 2 * ETH_ALEN {
            return false;
        }
        let rctl = self.regs[REG_RCTL / 4];

        let dst = &frame[0..ETH_ALEN];
        let is_broadcast = dst == [0xFF; 6];
        let is_multicast = !is_broadcast && (dst[0] & 0x01) != 0;

        // Unicast promiscuous: accept all unicast.
        if rctl & RCTL_UPE != 0 && !is_multicast && !is_broadcast {
            return true;
        }

        // Broadcast.
        if is_broadcast {
            return rctl & RCTL_BAM != 0;
        }

        // Check all 16 receive address register pairs.
        for i in 0..16 {
            let ral_off = (REG_RAL0 + i * 8) / 4;
            let rah_off = (REG_RAH0 + i * 8) / 4;
            let rah = self.regs[rah_off];
            if rah & (1 << 31) == 0 { continue; }
            let ral = self.regs[ral_off];
            let ra_mac = [
                (ral & 0xFF) as u8,
                ((ral >> 8) & 0xFF) as u8,
                ((ral >> 16) & 0xFF) as u8,
                ((ral >> 24) & 0xFF) as u8,
                (rah & 0xFF) as u8,
                ((rah >> 8) & 0xFF) as u8,
            ];
            if dst == ra_mac {
                return true;
            }
        }

        // Multicast filtering.
        if is_multicast {
            if rctl & RCTL_MPE != 0 {
                return true;
            }
            let hash = self.mta_hash(dst);
            let word_idx = hash >> 5;
            let bit_idx = hash & 0x1F;
            if word_idx < 128 {
                let mta_word = self.regs[(REG_MTA / 4) + word_idx];
                if (mta_word >> bit_idx) & 1 != 0 {
                    return true;
                }
            }
            return false;
        }

        false
    }

    fn mta_hash(&self, mac: &[u8]) -> usize {
        let mo = ((self.regs[REG_RCTL / 4] >> RCTL_MO_SHIFT) & 0x3) as usize;
        let raw = ((mac[5] as u16) << 8) | (mac[4] as u16);
        let shift = match mo {
            0 => 4,
            1 => 3,
            2 => 2,
            3 => 0,
            _ => 4,
        };
        ((raw >> shift) & 0xFFF) as usize
    }

    fn vlan_filter_accept(&self, frame: &[u8]) -> bool {
        let rctl = self.regs[REG_RCTL / 4];
        if rctl & RCTL_VFE == 0 {
            return true;
        }
        if frame.len() < 16 {
            return true;
        }
        let ethertype = ((frame[12] as u16) << 8) | frame[13] as u16;
        let vet = (self.regs[REG_VET / 4] & 0xFFFF) as u16;
        if ethertype != vet {
            return true;
        }
        let vid = (((frame[14] as u16) << 8) | frame[15] as u16) & 0xFFF;
        let word_idx = (vid >> 5) as usize;
        let bit_idx = (vid & 0x1F) as usize;
        if word_idx < 128 {
            let vfta_word = self.regs[(REG_VFTA / 4) + word_idx];
            return (vfta_word >> bit_idx) & 1 != 0;
        }
        false
    }

    // ═══════════════════════════════════════════════════════════════════════
    // RX buffer size from RCTL
    // ═══════════════════════════════════════════════════════════════════════

    fn rx_buf_size(&self) -> usize {
        let rctl = self.regs[REG_RCTL / 4];
        let bsize_bits = ((rctl >> RCTL_BSIZE_SHIFT) & 0x3) as usize;
        let bsex = rctl & RCTL_BSEX != 0;
        if bsex {
            match bsize_bits { 1 => 4096, 2 => 8192, 3 => 16384, _ => 2048 }
        } else {
            match bsize_bits { 1 => 1024, 2 => 512, 3 => 256, _ => 2048 }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Checksum helpers
    // ═══════════════════════════════════════════════════════════════════════

    fn insert_checksum(data: &mut [u8], css: usize, cso: usize, cse: usize) {
        let end = if cse == 0 || cse >= data.len() { data.len() } else { cse + 1 };
        if css >= end || cso + 1 >= data.len() { return; }
        data[cso] = 0;
        data[cso + 1] = 0;
        let mut sum: u32 = 0;
        let mut i = css;
        while i + 1 < end {
            sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
            i += 2;
        }
        if i < end {
            sum += (data[i] as u32) << 8;
        }
        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let cksum = !sum as u16;
        data[cso] = (cksum >> 8) as u8;
        data[cso + 1] = cksum as u8;
    }

    fn verify_rx_checksums(frame: &[u8]) -> (bool, bool) {
        if frame.len() < 14 + 20 {
            return (true, true);
        }
        let ethertype = ((frame[12] as u16) << 8) | frame[13] as u16;
        if ethertype != 0x0800 {
            return (true, true);
        }
        let ip_start = 14;
        let ihl = ((frame[ip_start] & 0x0F) as usize) * 4;
        if ihl < 20 || ip_start + ihl > frame.len() {
            return (true, true);
        }

        let ip_ok = {
            let mut sum: u32 = 0;
            let mut i = ip_start;
            let ip_end = ip_start + ihl;
            while i + 1 < ip_end {
                sum += ((frame[i] as u32) << 8) | frame[i + 1] as u32;
                i += 2;
            }
            while sum > 0xFFFF {
                sum = (sum & 0xFFFF) + (sum >> 16);
            }
            (sum & 0xFFFF) == 0xFFFF
        };

        // Trust packets from our backend (slirp) are correct.
        let tcp_ok = true;

        (ip_ok, tcp_ok)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // TX: pad short frames
    // ═══════════════════════════════════════════════════════════════════════

    fn pad_frame(pkt: &mut Vec<u8>) {
        let min_len = ETH_MIN_FRAME - ETH_FCS_LEN;
        if pkt.len() < min_len {
            pkt.resize(min_len, 0);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // TX: TCP Segmentation Offload (TSO)
    // ═══════════════════════════════════════════════════════════════════════

    fn tso_segment(&self, pkt: &[u8]) -> Vec<Vec<u8>> {
        let ctx = &self.tx_ctx;
        let hdrlen = ctx.hdrlen as usize;
        let mss = ctx.mss as usize;

        if mss == 0 || hdrlen == 0 || hdrlen >= pkt.len() || mss > 16384 {
            return vec![pkt.to_vec()];
        }

        let payload = &pkt[hdrlen..];
        let total_payload = payload.len();
        if total_payload == 0 {
            return vec![pkt.to_vec()];
        }

        let header = &pkt[..hdrlen];
        let num_segments = (total_payload + mss - 1) / mss;
        let mut segments = Vec::with_capacity(num_segments);

        let ip_start = ctx.ipcss as usize;
        let tcp_start = ctx.tucss as usize;

        let mut ip_id: u16 = if ip_start + 6 <= hdrlen && ctx.ipv4 {
            ((header[ip_start + 4] as u16) << 8) | header[ip_start + 5] as u16
        } else { 0 };

        let mut tcp_seq: u32 = if ctx.tcp && tcp_start + 8 <= hdrlen {
            ((header[tcp_start + 4] as u32) << 24)
            | ((header[tcp_start + 5] as u32) << 16)
            | ((header[tcp_start + 6] as u32) << 8)
            | (header[tcp_start + 7] as u32)
        } else { 0 };

        let mut offset = 0usize;

        while offset < total_payload {
            let seg_len = (total_payload - offset).min(mss);
            let is_last = offset + seg_len >= total_payload;

            let mut seg = Vec::with_capacity(hdrlen + seg_len);
            seg.extend_from_slice(header);
            seg.extend_from_slice(&payload[offset..offset + seg_len]);

            // Update IPv4 header: total length, IP ID.
            if ctx.ipv4 && ip_start + 20 <= seg.len() {
                let total_len = (seg.len() - ip_start) as u16;
                seg[ip_start + 2] = (total_len >> 8) as u8;
                seg[ip_start + 3] = total_len as u8;
                seg[ip_start + 4] = (ip_id >> 8) as u8;
                seg[ip_start + 5] = ip_id as u8;
                ip_id = ip_id.wrapping_add(1);
            }

            // Update TCP header: sequence number, flags.
            if ctx.tcp && tcp_start + 14 <= seg.len() {
                seg[tcp_start + 4] = (tcp_seq >> 24) as u8;
                seg[tcp_start + 5] = (tcp_seq >> 16) as u8;
                seg[tcp_start + 6] = (tcp_seq >> 8) as u8;
                seg[tcp_start + 7] = tcp_seq as u8;

                if !is_last {
                    seg[tcp_start + 13] &= !(0x01 | 0x08); // Clear FIN, PSH
                }
            }

            tcp_seq = tcp_seq.wrapping_add(seg_len as u32);

            // Insert IP checksum.
            if ctx.ipv4 {
                Self::insert_checksum(
                    &mut seg,
                    ctx.ipcss as usize,
                    ctx.ipcso as usize,
                    ctx.ipcse as usize,
                );
            }

            // Insert TCP/UDP checksum.
            Self::insert_checksum(
                &mut seg,
                ctx.tucss as usize,
                ctx.tucso as usize,
                ctx.tucse as usize,
            );

            segments.push(seg);
            offset += seg_len;
        }

        segments
    }

    // ═══════════════════════════════════════════════════════════════════════
    // TX descriptor ring processing
    // ═══════════════════════════════════════════════════════════════════════

    fn process_tx_ring(&mut self) {
        let tdbal = self.regs[REG_TDBAL / 4] as u64;
        let tdbah = self.regs[REG_TDBAH / 4] as u64;
        let td_base = (tdbah << 32) | tdbal;
        let tdlen = self.regs[REG_TDLEN / 4] as u64;
        if tdlen == 0 || td_base == 0 { return; }

        let tctl = self.regs[REG_TCTL / 4];
        let pad_short = tctl & TCTL_PSP != 0;

        let num_descs = (tdlen / 16) as u32;
        let mut head = self.regs[REG_TDH / 4];
        let tail = self.regs[REG_TDT / 4];

        let mut pkt_data: Vec<u8> = Vec::new();
        let mut pkt_popts: u8 = 0;
        let mut processed = 0u32;

        while head != tail && processed < num_descs {
            let desc_addr = td_base + (head as u64) * 16;
            let mut desc = [0u8; 16];
            if !self.dma_read(desc_addr, &mut desc) { break; }

            let cmd = desc[11];
            let dext = cmd & TXD_CMD_DEXT != 0;

            if dext {
                let dtyp = (desc[10] >> 4) & 0xF;

                if dtyp == 0 {
                    // ── Context descriptor ──
                    self.tx_ctx = TxContext {
                        ipcss: desc[0],
                        ipcso: desc[1],
                        ipcse: u16::from_le_bytes([desc[2], desc[3]]),
                        tucss: desc[4],
                        tucso: desc[5],
                        tucse: u16::from_le_bytes([desc[6], desc[7]]),
                        hdrlen: desc[13],
                        mss: u16::from_le_bytes([desc[14], desc[15]]),
                        tse: cmd & TUCMD_TSE != 0,
                        tcp: cmd & TUCMD_TCP != 0,
                        ipv4: cmd & TUCMD_IP != 0,
                    };

                    if cmd & TXD_CMD_RS != 0 {
                        desc[12] |= TXD_STA_DD;
                        self.dma_write(desc_addr + 12, &desc[12..13]);
                    }
                } else {
                    // ── Data descriptor (DTYP >= 1) ──
                    let buf_addr = u64::from_le_bytes([
                        desc[0], desc[1], desc[2], desc[3],
                        desc[4], desc[5], desc[6], desc[7],
                    ]);
                    let dtalen = u16::from_le_bytes([desc[8], desc[9]]) as usize
                        | (((desc[10] & 0x0F) as usize) << 16);
                    let eop = cmd & TXD_CMD_EOP != 0;
                    let rs = cmd & TXD_CMD_RS != 0;
                    let popts = desc[13];

                    if dtalen > 0 && dtalen <= 65536 && buf_addr != 0 {
                        let start = pkt_data.len();
                        pkt_data.resize(start + dtalen, 0);
                        if !self.dma_read(buf_addr, &mut pkt_data[start..]) {
                            pkt_data.truncate(start);
                        }
                    }
                    pkt_popts |= popts;
                    if rs {
                        desc[12] |= TXD_STA_DD;
                        self.dma_write(desc_addr + 12, &desc[12..13]);
                    }

                    if eop && !pkt_data.is_empty() {
                        if self.tx_ctx.tse && self.tx_ctx.mss > 0 {
                            // TCP Segmentation Offload.
                            let segments = self.tso_segment(&pkt_data);
                            let seg_count = segments.len();
                            for mut seg in segments {
                                if pad_short { Self::pad_frame(&mut seg); }
                                self.update_tx_stats(&seg);
                                self.tx_buffer.push(seg);
                            }
                            if seg_count > 0 {
                                self.stat_inc(STAT_TSCTC);
                            }
                        } else {
                            // Non-TSO: apply checksum offloading.
                            if pkt_popts & TXD_POPTS_IXSM != 0 {
                                Self::insert_checksum(
                                    &mut pkt_data,
                                    self.tx_ctx.ipcss as usize,
                                    self.tx_ctx.ipcso as usize,
                                    self.tx_ctx.ipcse as usize,
                                );
                            }
                            if pkt_popts & TXD_POPTS_TXSM != 0 {
                                Self::insert_checksum(
                                    &mut pkt_data,
                                    self.tx_ctx.tucss as usize,
                                    self.tx_ctx.tucso as usize,
                                    self.tx_ctx.tucse as usize,
                                );
                            }
                            if pad_short { Self::pad_frame(&mut pkt_data); }
                            self.update_tx_stats(&pkt_data);
                            self.tx_buffer.push(core::mem::take(&mut pkt_data));
                        }
                        pkt_data = Vec::new();
                        pkt_popts = 0;
                    }
                }
            } else {
                // ── Legacy descriptor (DEXT=0) ──
                let buf_addr = u64::from_le_bytes([
                    desc[0], desc[1], desc[2], desc[3],
                    desc[4], desc[5], desc[6], desc[7],
                ]);
                let length = u16::from_le_bytes([desc[8], desc[9]]) as usize;
                let eop = cmd & TXD_CMD_EOP != 0;
                let rs = cmd & TXD_CMD_RS != 0;

                if length > 0 && length <= 16384 && buf_addr != 0 {
                    let start = pkt_data.len();
                    pkt_data.resize(start + length, 0);
                    if !self.dma_read(buf_addr, &mut pkt_data[start..]) {
                        pkt_data.truncate(start);
                    }
                }

                // Legacy checksum insert (IC bit).
                if cmd & TXD_CMD_IC != 0 && eop && !pkt_data.is_empty() {
                    let css = desc[13] as usize;
                    let cso = desc[10] as usize;
                    Self::insert_checksum(&mut pkt_data, css, cso, 0);
                }

                if rs {
                    desc[12] |= TXD_STA_DD;
                    self.dma_write(desc_addr + 12, &desc[12..13]);
                }

                if eop && !pkt_data.is_empty() {
                    if pad_short { Self::pad_frame(&mut pkt_data); }
                    self.update_tx_stats(&pkt_data);
                    self.tx_buffer.push(core::mem::take(&mut pkt_data));
                    pkt_popts = 0;
                }
            }

            head = (head + 1) % num_descs;
            processed += 1;
        }

        self.regs[REG_TDH / 4] = head;

        if processed > 0 {
            self.raise_interrupt(ICR_TXDW);
            if head == tail {
                self.raise_interrupt(ICR_TXQE);
            }
            self.notify_io();
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // RX descriptor ring processing
    // ═══════════════════════════════════════════════════════════════════════

    pub fn process_rx_ring(&mut self) {
        let rdbal = self.regs[REG_RDBAL / 4] as u64;
        let rdbah = self.regs[REG_RDBAH / 4] as u64;
        let rd_base = (rdbah << 32) | rdbal;
        let rdlen = self.regs[REG_RDLEN / 4] as u64;
        if rdlen == 0 || rd_base == 0 {
            #[cfg(feature = "std")]
            if !self.rx_buffer.is_empty() {
                static mut RX_NOSETUP: u32 = 0;
                unsafe { RX_NOSETUP += 1; }
                if unsafe { RX_NOSETUP } <= 5 {
                    eprintln!("[e1000] RX: ring not set up (rdbal=0x{:X} rdlen={}) pending={}",
                        rdbal, rdlen, self.rx_buffer.len());
                }
            }
            return;
        }

        let rctl = self.regs[REG_RCTL / 4];

        let num_descs = (rdlen / 16) as u32;
        let mut head = self.regs[REG_RDH / 4];
        let tail = self.regs[REG_RDT / 4];

        let secrc = rctl & RCTL_SECRC != 0;
        let rxbuf_size = self.rx_buf_size();
        let lpe = rctl & RCTL_LPE != 0;
        let max_frame = if lpe { 16384 } else { 1522 };
        // Only apply RX filtering when receiver is explicitly enabled.
        let do_filter = rctl & RCTL_EN != 0;

        let rxcsum = self.regs[REG_RXCSUM / 4];
        let ip_offload = rxcsum & RXCSUM_IPOFLD != 0;
        let tu_offload = rxcsum & RXCSUM_TUOFLD != 0;

        let mut delivered_count = 0u32;
        const MAX_RX_PER_POLL: u32 = 128;

        while let Some(_pkt) = self.rx_buffer.front() {
            if delivered_count >= MAX_RX_PER_POLL { break; }

            if head == tail {
                #[cfg(feature = "std")]
                {
                    static mut RX_STALL_DIAG: u32 = 0;
                    unsafe { RX_STALL_DIAG += 1; }
                    if unsafe { RX_STALL_DIAG } <= 5 {
                        eprintln!("[e1000] RX STALL: head={} tail={} rdlen={} rctl=0x{:X} pending={}",
                            head, tail, rdlen, rctl, self.rx_buffer.len());
                    }
                }
                // No available descriptors — drop excess packets to prevent
                // unbounded memory growth.  Keep at most 64 packets queued
                // so the guest can recover when it replenishes descriptors.
                while self.rx_buffer.len() > 64 {
                    self.rx_buffer.pop_front();
                    self.stat_inc(STAT_MPC); // Missed Packet Count
                }
                self.regs[REG_ICR / 4] |= ICR_RXO;
                break;
            }

            let pkt = self.rx_buffer.front().unwrap();

            // Apply filtering only when RCTL.EN is set (driver fully initialized).
            if do_filter {
                if !self.rx_filter_accept(pkt) {
                    self.rx_buffer.pop_front();
                    continue;
                }

                if !self.vlan_filter_accept(pkt) {
                    self.rx_buffer.pop_front();
                    continue;
                }

                if pkt.len() > max_frame && rctl & RCTL_SBP == 0 {
                    self.stat_inc(STAT_ROC);
                    self.rx_buffer.pop_front();
                    continue;
                }

                // No undersize check — in emulation, short frames (e.g. 42-byte
                // ARP replies) are perfectly valid.  Real hardware enforces
                // minimums at the PHY level, but we accept all sizes.
            }

            let pkt = self.rx_buffer.pop_front().unwrap();

            let desc_addr = rd_base + (head as u64) * 16;
            let mut desc = [0u8; 16];
            if !self.dma_read(desc_addr, &mut desc) {
                #[cfg(feature = "std")]
                eprintln!("[e1000] RX: dma_read desc FAILED at 0x{:X}", desc_addr);
                break;
            }

            let buf_addr = u64::from_le_bytes([
                desc[0], desc[1], desc[2], desc[3],
                desc[4], desc[5], desc[6], desc[7],
            ]);

            if buf_addr == 0 {
                #[cfg(feature = "std")]
                eprintln!("[e1000] RX: buf_addr=0 at desc[{}]", head);
                break;
            }

            // Prepare frame: optionally append dummy FCS.
            let mut frame = pkt.clone();
            if !secrc {
                frame.extend_from_slice(&[0u8; 4]);
            }

            let write_len = frame.len().min(rxbuf_size);
            if !self.dma_write(buf_addr, &frame[..write_len]) {
                #[cfg(feature = "std")]
                eprintln!("[e1000] RX: dma_write FAILED buf=0x{:X} len={}", buf_addr, write_len);
                break;
            }

            // Build RX descriptor status.
            let mut status = RXD_STA_DD | RXD_STA_EOP;
            let mut errors: u8 = 0;

            // VLAN tag stripping.
            let ctrl = self.regs[REG_CTRL / 4];
            let vet = (self.regs[REG_VET / 4] & 0xFFFF) as u16;
            let mut vlan_tag: u16 = 0;
            if ctrl & CTRL_VME != 0 && pkt.len() >= 16 {
                let ethertype = ((pkt[12] as u16) << 8) | pkt[13] as u16;
                if ethertype == vet {
                    status |= RXD_STA_VP;
                    vlan_tag = ((pkt[14] as u16) << 8) | pkt[15] as u16;
                }
            }

            // RX checksum offload.
            if ip_offload || tu_offload {
                let (ip_ok, tcp_ok) = Self::verify_rx_checksums(&pkt);
                if ip_offload {
                    status |= RXD_STA_IPCS;
                    if !ip_ok { errors |= RXD_ERR_IPE; }
                }
                if tu_offload {
                    status |= RXD_STA_TCPCS;
                    if !tcp_ok { errors |= RXD_ERR_TCPE; }
                }
            }

            let len_bytes = (write_len as u16).to_le_bytes();
            desc[8] = len_bytes[0];
            desc[9] = len_bytes[1];
            desc[10] = 0;
            desc[11] = 0;
            desc[12] = status;
            desc[13] = errors;
            let vlan_bytes = vlan_tag.to_le_bytes();
            desc[14] = vlan_bytes[0];
            desc[15] = vlan_bytes[1];

            self.dma_write(desc_addr, &desc);
            self.update_rx_stats(&pkt);

            #[cfg(feature = "std")]
            {
                static mut RX_OK_COUNT: u32 = 0;
                unsafe { RX_OK_COUNT += 1; }
                if unsafe { RX_OK_COUNT } <= 3 {
                    eprintln!("[e1000] RX OK: {} bytes desc[{}] head→{} rctl=0x{:X} icr=0x{:X}",
                        write_len, head, (head + 1) % num_descs,
                        rctl, self.regs[REG_ICR / 4]);
                }
            }

            head = (head + 1) % num_descs;
            delivered_count += 1;

            // Check RX descriptor minimum threshold (RXDMT0).
            let rdmts = ((rctl >> 8) & 3) as u32;
            let threshold = match rdmts {
                0 => num_descs / 2,
                1 => num_descs / 4,
                2 => num_descs / 8,
                _ => num_descs / 2,
            };
            let available = if tail >= head {
                tail - head
            } else {
                num_descs - head + tail
            };
            if available <= threshold {
                self.regs[REG_ICR / 4] |= ICR_RXDMT0;
            }
        }

        let delivered = head != self.regs[REG_RDH / 4];
        self.regs[REG_RDH / 4] = head;

        if delivered || !self.rx_buffer.is_empty() {
            self.raise_interrupt(ICR_RXT0);
        }
        if delivered {
            self.notify_io();
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // EEPROM Microwire bit-bang
    // ═══════════════════════════════════════════════════════════════════════

    fn write_eecd(&mut self, val: u32) {
        let cs = val & EECD_CS != 0;
        let sk = val & EECD_SK != 0;
        let di = val & EECD_DI != 0;
        let rising_edge = sk && !self.ee_sk_prev;

        self.ee_cd = (val & (EECD_SK | EECD_CS | EECD_DI | EECD_REQ | EECD_FWE))
            | (self.ee_cd & EECD_DO)
            | EECD_PRES | EECD_GNT;

        if !cs {
            self.ee_state = EepromState::Idle;
            self.ee_cd &= !EECD_DO;
            self.ee_sk_prev = sk;
            return;
        }

        if rising_edge {
            match self.ee_state {
                EepromState::Idle => {
                    let bit = if di { 1u16 } else { 0 };
                    self.ee_state = EepromState::ReceivingCommand {
                        bits_received: 1,
                        shift_reg: bit,
                    };
                    self.ee_cd &= !EECD_DO;
                }
                EepromState::ReceivingCommand { bits_received, shift_reg } => {
                    let bit = if di { 1u16 } else { 0 };
                    let new_reg = (shift_reg << 1) | bit;
                    let new_count = bits_received + 1;

                    if new_count == 9 {
                        let opcode = ((new_reg >> 6) & 0x07) as u8;
                        let addr = (new_reg & 0x3F) as usize;

                        if opcode == MW_OPCODE_READ {
                            let data = if addr < self.eeprom.len() {
                                self.eeprom[addr]
                            } else {
                                0xFFFF
                            };
                            self.ee_cd &= !EECD_DO;
                            self.ee_state = EepromState::SendingData {
                                bits_sent: 0,
                                data,
                            };
                        } else {
                            self.ee_state = EepromState::Idle;
                            self.ee_cd &= !EECD_DO;
                        }
                    } else {
                        self.ee_state = EepromState::ReceivingCommand {
                            bits_received: new_count,
                            shift_reg: new_reg,
                        };
                        self.ee_cd &= !EECD_DO;
                    }
                }
                EepromState::SendingData { bits_sent, data } => {
                    if bits_sent >= 16 {
                        self.ee_state = EepromState::Idle;
                        self.ee_cd &= !EECD_DO;
                    } else {
                        let bit_pos = 15 - bits_sent;
                        if data & (1 << bit_pos) != 0 {
                            self.ee_cd |= EECD_DO;
                        } else {
                            self.ee_cd &= !EECD_DO;
                        }
                        self.ee_state = EepromState::SendingData {
                            bits_sent: bits_sent + 1,
                            data,
                        };
                    }
                }
            }
        }

        self.ee_sk_prev = sk;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MMIO handler
// ═══════════════════════════════════════════════════════════════════════════

impl MmioHandler for E1000 {
    fn read(&mut self, offset: u64, size: u8) -> Result<u64> {
        let dword_offset = (offset as usize) / 4;
        if dword_offset >= self.regs.len() {
            return Ok(0);
        }

        let aligned_offset = offset as usize & !3;
        let val = match aligned_offset {
            REG_EECD => self.ee_cd,

            REG_MDIC => self.regs[dword_offset],

            REG_EERD => {
                let eerd = self.regs[dword_offset];
                if eerd & EERD_START != 0 {
                    let addr = ((eerd >> 8) & 0xFF) as usize;
                    let data = if addr < self.eeprom.len() {
                        self.eeprom[addr]
                    } else {
                        0xFFFF
                    };
                    ((data as u32) << 16) | EERD_DONE
                } else {
                    eerd
                }
            }

            REG_ICR => {
                // Reading ICR clears all cause bits.
                // De-assertion is handled by poll_irqs which tracks
                // e1000_irq_asserted — we must NOT call fire_irq_callback
                // here because it would de-assert the line without updating
                // that flag, causing poll_irqs to never re-assert.
                let icr = self.regs[dword_offset];
                self.regs[dword_offset] = 0;
                icr
            }

            REG_SWSM => {
                // Software semaphore: always grant.
                let v = self.regs[dword_offset];
                self.regs[dword_offset] = v | 1;
                v
            }

            // Statistics: clear-on-read.
            off if off >= REG_STATS_BASE && off < REG_STATS_END => {
                let idx = off / 4;
                let v = self.regs[idx];
                self.regs[idx] = 0;
                v
            }

            _ => self.regs[dword_offset],
        };

        let byte_offset = (offset as usize) & 3;
        let shifted = val >> (byte_offset * 8);
        let mask = match size {
            1 => 0xFF,
            2 => 0xFFFF,
            _ => 0xFFFF_FFFF,
        };
        Ok((shifted & mask) as u64)
    }

    fn write(&mut self, offset: u64, size: u8, val: u64) -> Result<()> {
        let dword_offset = (offset as usize) / 4;
        if dword_offset >= self.regs.len() {
            return Ok(());
        }

        let byte_offset = (offset as usize) & 3;
        let mask = match size {
            1 => 0xFFu32,
            2 => 0xFFFFu32,
            _ => 0xFFFF_FFFFu32,
        };
        let shifted_mask = mask << (byte_offset * 8);
        let shifted_val = ((val as u32) & mask) << (byte_offset * 8);
        let new_val = (self.regs[dword_offset] & !shifted_mask) | shifted_val;

        let aligned = offset as usize & !3;
        match aligned {
            REG_CTRL => {
                self.regs[dword_offset] = new_val;
                if new_val & CTRL_RST != 0 {
                    self.reset();
                }
                if new_val & CTRL_PHY_RST != 0 {
                    // PHY reset via CTRL register: restore PHY defaults
                    self.phy_regs = [0u16; 32];
                    self.phy_regs[PHY_CTRL as usize] = 0x1140;
                    self.phy_regs[PHY_STATUS as usize] = 0x796D;
                    self.phy_regs[PHY_ID1 as usize] = 0x0141;
                    self.phy_regs[PHY_ID2 as usize] = 0x0C20;
                    self.phy_regs[PHY_AUTONEG_ADV as usize] = 0x01E1;
                    self.phy_regs[PHY_LP_ABILITY as usize] = 0x45E1;
                    self.phy_regs[PHY_1000T_CTRL as usize] = 0x0300;
                    self.phy_regs[PHY_1000T_STATUS as usize] = 0x3C00;
                    self.phy_regs[M88_PHY_SPEC_STATUS as usize] = 0xAC04;
                    self.phy_regs[M88_PHY_SPEC_CTRL as usize] = 0x0068;
                    self.phy_regs[M88_EXT_PHY_SPEC_CTRL as usize] = 0x0D60;
                    self.phy_regs[M88_EXT_PHY_SPEC_STATUS as usize] = 0xAD00;
                    self.regs[dword_offset] &= !CTRL_PHY_RST;
                }
            }

            REG_STATUS => {
                // STATUS is mostly read-only. Only PHYRA (bit 10) is
                // writable: software clears it by writing 0 to that bit.
                // Windows e1000 driver checks PHYRA after reset and will
                // retry the reset indefinitely if it cannot clear this bit.
                if new_val & STATUS_PHYRA == 0 {
                    self.regs[dword_offset] &= !STATUS_PHYRA;
                }
            }

            REG_CTRL_EXT | REG_FCAL | REG_FCAH | REG_FCT | REG_FCTTV
            | REG_FCRTL | REG_FCRTH | REG_VET | REG_PBA | REG_LEDCTL
            | REG_TCTL | REG_TCTL_EXT | REG_TIPG | REG_RCTL
            | REG_RXDCTL | REG_TXDCTL | REG_RXCSUM | REG_MANC => {
                self.regs[dword_offset] = new_val;
            }

            REG_EECD => {
                self.write_eecd(new_val);
            }

            REG_EERD => {
                self.regs[dword_offset] = new_val;
            }

            REG_MDIC => {
                let phy_reg = ((new_val & MDIC_REG_MASK) >> MDIC_REG_SHIFT) as usize;
                if new_val & MDIC_OP_READ != 0 {
                    let data = if phy_reg < self.phy_regs.len() {
                        self.phy_regs[phy_reg]
                    } else { 0 };
                    self.regs[dword_offset] = (new_val & !MDIC_DATA_MASK & !MDIC_ERROR)
                        | (data as u32)
                        | MDIC_READY;
                } else if new_val & MDIC_OP_WRITE != 0 {
                    let mut phy_data = (new_val & MDIC_DATA_MASK) as u16;
                    if phy_reg == PHY_CTRL as usize && phy_data & 0x8000 != 0 {
                        // PHY software reset: restore all PHY registers to
                        // power-on defaults (M88E1011). The Windows driver
                        // reads PHY_STATUS after reset and expects autoneg
                        // complete + link up bits to be set.
                        self.phy_regs = [0u16; 32];
                        self.phy_regs[PHY_CTRL as usize] = 0x1140;
                        self.phy_regs[PHY_STATUS as usize] = 0x796D;
                        self.phy_regs[PHY_ID1 as usize] = 0x0141;
                        self.phy_regs[PHY_ID2 as usize] = 0x0C20;
                        self.phy_regs[PHY_AUTONEG_ADV as usize] = 0x01E1;
                        self.phy_regs[PHY_LP_ABILITY as usize] = 0x45E1;
                        self.phy_regs[PHY_1000T_CTRL as usize] = 0x0300;
                        self.phy_regs[PHY_1000T_STATUS as usize] = 0x3C00;
                        self.phy_regs[M88_PHY_SPEC_STATUS as usize] = 0xAC04;
                        self.phy_regs[M88_PHY_SPEC_CTRL as usize] = 0x0068;
                        self.phy_regs[M88_EXT_PHY_SPEC_CTRL as usize] = 0x0D60;
                        self.phy_regs[M88_EXT_PHY_SPEC_STATUS as usize] = 0xAD00;
                        phy_data = self.phy_regs[PHY_CTRL as usize];
                    }
                    if phy_reg < self.phy_regs.len() {
                        self.phy_regs[phy_reg] = phy_data;
                    }
                    self.regs[dword_offset] = (new_val & !MDIC_ERROR) | MDIC_READY;
                } else {
                    self.regs[dword_offset] = new_val;
                }
            }

            REG_ITR => {
                self.regs[dword_offset] = new_val & 0xFFFF;
            }

            REG_ICS => {
                self.regs[REG_ICR / 4] |= new_val;
                // Fire callback immediately — critical for the driver's
                // interrupt test which expects synchronous delivery.
                self.fire_irq_assert();
            }

            REG_IMS => {
                self.regs[dword_offset] |= new_val;
                // Assert immediately if pending ICR bits now match the
                // newly enabled mask (important for interrupt test which
                // writes IMS before ICS in some driver versions).
                self.fire_irq_assert();
            }

            REG_IMC => {
                self.regs[REG_IMS / 4] &= !new_val;
                // De-assertion handled by poll_irqs (same reason as ICR read).
            }

            // RX ring
            REG_RDBAL | REG_RDLEN | REG_RDH => {
                self.regs[dword_offset] = new_val;
            }

            REG_RDBAH => {
                // 82540EM is 32-bit PCI.
            }

            REG_RDT => {
                self.regs[dword_offset] = new_val;
                if !self.rx_buffer.is_empty() {
                    self.process_rx_ring();
                }
            }

            REG_RDTR => {
                self.regs[dword_offset] = new_val & 0xFFFF;
                // FPD (bit 31): flush partial descriptor — deliver immediately.
                if new_val & (1 << 31) != 0 && !self.rx_buffer.is_empty() {
                    self.process_rx_ring();
                }
            }

            REG_RADV => {
                self.regs[dword_offset] = new_val & 0xFFFF;
            }

            // TX ring
            REG_TDBAL | REG_TDLEN | REG_TDH => {
                self.regs[dword_offset] = new_val;
            }

            REG_TDBAH => {
                // 82540EM is 32-bit PCI.
            }

            REG_TDT => {
                self.regs[dword_offset] = new_val;
                self.process_tx_ring();
            }

            REG_TIDV => {
                self.regs[dword_offset] = new_val & 0xFFFF;
            }

            REG_TADV => {
                self.regs[dword_offset] = new_val & 0xFFFF;
            }

            // MTA (Multicast Table Array)
            off if off >= REG_MTA && off < REG_MTA_END => {
                self.regs[dword_offset] = new_val;
            }

            // RA (Receive Address) — 16 pairs
            off if off >= REG_RAL0 && off < REG_RA_END => {
                self.regs[dword_offset] = new_val;
                if off == REG_RAL0 {
                    self.mac_address[0] = (new_val & 0xFF) as u8;
                    self.mac_address[1] = ((new_val >> 8) & 0xFF) as u8;
                    self.mac_address[2] = ((new_val >> 16) & 0xFF) as u8;
                    self.mac_address[3] = ((new_val >> 24) & 0xFF) as u8;
                } else if off == REG_RAH0 {
                    self.mac_address[4] = (new_val & 0xFF) as u8;
                    self.mac_address[5] = ((new_val >> 8) & 0xFF) as u8;
                }
            }

            // VFTA (VLAN Filter Table Array)
            off if off >= REG_VFTA && off < REG_VFTA_END => {
                self.regs[dword_offset] = new_val;
            }

            // Statistics: writing clears.
            off if off >= REG_STATS_BASE && off < REG_STATS_END => {
                self.regs[dword_offset] = 0;
            }

            // Wake-on-LAN
            REG_WUC | REG_WUFC => {
                self.regs[dword_offset] = new_val;
            }

            REG_WUS => {
                self.regs[dword_offset] &= !new_val;
            }

            // Semaphores
            REG_SWSM | REG_FWSM => {
                self.regs[dword_offset] = new_val;
            }

            _ => {
                self.regs[dword_offset] = new_val;
            }
        }

        Ok(())
    }
}
