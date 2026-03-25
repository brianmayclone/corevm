//! Generic network backend abstraction.
//!
//! A [`NetBackend`] moves Ethernet frames between the E1000 device model and
//! some host-side transport (TAP, SLIRP, vhost-net, …).  The trait is
//! intentionally minimal so new backends can be added with little effort.

#[cfg(feature = "std")]
extern crate alloc;
#[cfg(feature = "std")]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use alloc::collections::VecDeque;

/// Network backend mode — selectable from VMManager / config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetMode {
    /// No host networking — packets are silently dropped.
    None,
    /// User-mode networking (built-in NAT + DHCP, no root required).
    UserMode,
    /// TAP device bridged to a host interface (requires CAP_NET_ADMIN).
    Tap,
}

impl Default for NetMode {
    fn default() -> Self { NetMode::None }
}

/// Trait that every network backend must implement.
///
/// The VM loop calls [`send`] with Ethernet frames from the guest (TX path)
/// and [`recv`] to collect frames destined for the guest (RX path).
/// Both methods must be non-blocking.
#[cfg(feature = "std")]
pub trait NetBackend: Send {
    /// Send an Ethernet frame from guest to the network.
    /// Must not block. Drop silently if the backend cannot accept right now.
    fn send(&mut self, frame: &[u8]);

    /// Receive pending Ethernet frames destined for the guest.
    /// Returns frames collected since the last call.  Must not block.
    fn recv(&mut self) -> Vec<Vec<u8>>;

    /// Tick / poll — called periodically from the VM loop.
    /// Backends that need periodic work (e.g. SLIRP timer expiry) do it here.
    fn poll(&mut self) {}

    /// Human-readable description for diagnostics.
    fn description(&self) -> &str;
}

/// Null backend — silently drops all traffic.
#[cfg(feature = "std")]
pub struct NullNet;

#[cfg(feature = "std")]
impl NetBackend for NullNet {
    fn send(&mut self, _frame: &[u8]) {}
    fn recv(&mut self) -> Vec<Vec<u8>> { Vec::new() }
    fn description(&self) -> &str { "none (disconnected)" }
}

// ── TAP backend ──────────────────────────────────────────────────────────────

/// TAP network backend — bridges guest traffic to a Linux TAP device.
///
/// The TAP device is created on construction and optionally joined to an
/// existing Linux bridge (e.g. `br0`).  All Ethernet frames pass through
/// unmodified, giving the guest a real presence on the host network.
#[cfg(feature = "linux")]
pub struct TapNet {
    tap: crate::net::tap::TapDevice,
    rx_buf: [u8; 2048],
    description: alloc::string::String,
}

#[cfg(feature = "linux")]
impl TapNet {
    /// Create a new TAP backend.
    ///
    /// * `tap_name` — requested TAP device name (empty = kernel assigns one).
    /// * `bridge`   — if non-empty, join this Linux bridge after creation.
    ///
    /// Returns an error if the TAP device cannot be created (missing
    /// CAP_NET_ADMIN, /dev/net/tun unavailable, etc.).
    pub fn new(tap_name: &str, bridge: &str) -> Result<Self, alloc::string::String> {
        let tap = crate::net::tap::TapDevice::new(tap_name)
            .map_err(|e| alloc::format!("TAP create '{}': {}", tap_name, e))?;

        tap.bring_up()
            .map_err(|e| alloc::format!("TAP bring_up '{}': {}", tap.name(), e))?;

        if !bridge.is_empty() {
            tap.add_to_bridge(bridge)
                .map_err(|e| alloc::format!("TAP add '{}' to bridge '{}': {}", tap.name(), bridge, e))?;
        }

        let desc = alloc::format!("tap:{} bridge:{}", tap.name(),
            if bridge.is_empty() { "none" } else { bridge });

        Ok(Self {
            tap,
            rx_buf: [0u8; 2048],
            description: desc,
        })
    }
}

#[cfg(feature = "linux")]
impl NetBackend for TapNet {
    fn send(&mut self, frame: &[u8]) {
        let _ = self.tap.write_frame(frame);
    }

    fn recv(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        loop {
            match self.tap.read_frame(&mut self.rx_buf) {
                Ok(0) => break,
                Ok(n) => frames.push(self.rx_buf[..n].to_vec()),
                Err(_) => break,
            }
        }
        frames
    }

    fn description(&self) -> &str {
        &self.description
    }
}
