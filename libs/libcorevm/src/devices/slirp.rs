//! User-mode networking (SLIRP-style) backend.
//!
//! Provides NAT + DHCP + DNS for guest VMs without requiring root or TAP
//! devices.  The guest sees a virtual 10.0.2.0/24 network:
//!
//! - Gateway / NAT router:  10.0.2.2
//! - DHCP server:           10.0.2.2
//! - DNS relay:             10.0.2.3
//! - Guest (DHCP-assigned): 10.0.2.15
//!
//! Architecture:
//! - ARP: answered locally (no real ARP needed)
//! - DHCP: minimal DHCP server (DISCOVER→OFFER, REQUEST→ACK)
//! - DNS: UDP relay to host resolver
//! - TCP: per-connection host socket, non-blocking
//! - UDP: per-flow host socket, non-blocking
//! - ICMP: silently dropped (would need raw sockets / root)

use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use alloc::collections::BTreeMap;
use alloc::string::String;
use std::net::{TcpStream, UdpSocket, SocketAddr, Ipv4Addr, TcpListener};
use std::io::{Read, Write, ErrorKind};
use std::time::Instant;
use super::net::NetBackend;

// ── Network configuration ────────────────────────────────────────────────────

const NET_PREFIX: [u8; 3] = [10, 0, 2];
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
const DNS_IP: [u8; 4]     = [10, 0, 2, 3];
const GUEST_IP: [u8; 4]   = [10, 0, 2, 15];
const NETMASK: [u8; 4]    = [255, 255, 255, 0];
const BROADCAST: [u8; 4]  = [10, 0, 2, 255];

/// MAC address of the virtual gateway.
const GW_MAC: [u8; 6] = [0x52, 0x55, 0x0A, 0x00, 0x02, 0x02];

/// Maximum Ethernet frame we handle.
const MAX_FRAME: usize = 1514;

// ── Ethernet / IP / TCP / UDP helpers ────────────────────────────────────────

const ETH_HDR: usize = 14;
const IP_HDR_MIN: usize = 20;

const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;

const IP_PROTO_ICMP: u8 = 1;
const IP_PROTO_TCP: u8 = 6;
const IP_PROTO_UDP: u8 = 17;

fn u16be(b: &[u8], off: usize) -> u16 {
    ((b[off] as u16) << 8) | b[off + 1] as u16
}

fn u32be(b: &[u8], off: usize) -> u32 {
    ((b[off] as u32) << 24) | ((b[off+1] as u32) << 16) |
    ((b[off+2] as u32) << 8) | b[off+3] as u32
}

fn put_u16be(b: &mut [u8], off: usize, v: u16) {
    b[off] = (v >> 8) as u8;
    b[off + 1] = v as u8;
}

fn put_u32be(b: &mut [u8], off: usize, v: u32) {
    b[off]   = (v >> 24) as u8;
    b[off+1] = (v >> 16) as u8;
    b[off+2] = (v >> 8) as u8;
    b[off+3] = v as u8;
}

fn ip_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum as u16
}

/// Build an Ethernet header.
fn eth_header(dst: &[u8; 6], src: &[u8; 6], ethertype: u16) -> [u8; 14] {
    let mut h = [0u8; 14];
    h[0..6].copy_from_slice(dst);
    h[6..12].copy_from_slice(src);
    put_u16be(&mut h, 12, ethertype);
    h
}

/// Build a minimal IPv4 header (no options).
fn ip_header(proto: u8, src: [u8; 4], dst: [u8; 4], payload_len: u16, id: u16) -> [u8; 20] {
    let total_len = 20 + payload_len;
    let mut h = [0u8; 20];
    h[0] = 0x45; // version=4, ihl=5
    put_u16be(&mut h, 2, total_len);
    put_u16be(&mut h, 4, id);
    h[6] = 0x40; // DF flag
    h[8] = 64;   // TTL
    h[9] = proto;
    h[12..16].copy_from_slice(&src);
    h[16..20].copy_from_slice(&dst);
    let cksum = ip_checksum(&h);
    put_u16be(&mut h, 10, cksum);
    h
}

/// Build a UDP header (without checksum — optional for IPv4).
fn udp_header(src_port: u16, dst_port: u16, payload_len: u16) -> [u8; 8] {
    let total = 8 + payload_len;
    let mut h = [0u8; 8];
    put_u16be(&mut h, 0, src_port);
    put_u16be(&mut h, 2, dst_port);
    put_u16be(&mut h, 4, total);
    // checksum 0 = not computed (valid for IPv4 UDP)
    h
}

// ── TCP connection tracking ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TcpFlowKey {
    guest_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum TcpState {
    SynSent,     // We initiated a connection to the guest (FTP data)
    SynReceived,
    Established,
    FinWait,
    Closed,
}

struct TcpConnection {
    stream: TcpStream,
    state: TcpState,
    /// Our (gateway) sequence number.
    our_seq: u32,
    /// Bytes sent but not yet ACKed by guest.
    unacked: u32,
    /// Next expected guest sequence number (bytes we've received up to).
    guest_seq: u32,
    /// Initial guest sequence from SYN.
    guest_isn: u32,
    /// Read buffer for data from host socket.
    read_buf: [u8; 16384],
    /// Last advertised window from the guest (for flow control), already scaled.
    guest_window: u32,
    /// TCP Window Scale shift count from the guest's SYN (RFC 1323).
    guest_wscale: u8,
    /// Retransmit queue: (seq, data) for segments not yet ACKed by guest.
    retransmit_queue: Vec<(u32, Vec<u8>)>,
    /// Timestamp of last retransmit check.
    last_retransmit: Option<Instant>,
}

// ── UDP flow tracking ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct UdpFlowKey {
    guest_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,
}

struct UdpFlow {
    socket: UdpSocket,
    last_active: Instant,
}

// ── FTP ALG (Application Layer Gateway) ──────────────────────────────────────

/// State for an FTP control connection (port 21).
struct FtpControlState {
    /// Line reassembly buffer for guest→server data (PORT commands).
    guest_line_buf: Vec<u8>,
    /// The host-side local address of the control connection (for PORT rewriting).
    local_addr: Option<SocketAddr>,
}

/// A host-side listener waiting for an Active FTP data connection from the server.
struct FtpDataListener {
    listener: TcpListener,
    /// Guest's data port (from the original PORT command).
    guest_data_port: u16,
    /// Remote FTP server IP.
    server_ip: [u8; 4],
    /// Creation timestamp for timeout.
    created: Instant,
}

// ── SLIRP backend ────────────────────────────────────────────────────────────

pub struct SlirpNet {
    /// Guest MAC address (learned from first frame or DHCP).
    guest_mac: [u8; 6],
    /// Frames ready to be delivered to the guest.
    rx_queue: VecDeque<Vec<u8>>,
    /// Active TCP connections.
    tcp_conns: BTreeMap<TcpFlowKey, TcpConnection>,
    /// Active UDP flows.
    udp_flows: BTreeMap<UdpFlowKey, UdpFlow>,
    /// DNS relay socket (shared for all DNS queries).
    dns_socket: Option<UdpSocket>,
    /// Pending DNS replies keyed by (guest_src_port, dns_txid).
    dns_pending: BTreeMap<u16, u16>, // txid → guest_src_port
    /// IP identification counter.
    ip_id: u16,
    /// Host DNS server address.
    host_dns: SocketAddr,
    /// DHCP state — true once guest has been offered an address.
    dhcp_offered: bool,
    /// Pending TCP connects (non-blocking, completed in poll_tcp).
    /// Tuple: (key, receiver, guest_seq, our_seq, guest_wscale)
    pending_connects: Vec<(TcpFlowKey, std::sync::mpsc::Receiver<std::io::Result<TcpStream>>, u32, u32, u8)>,
    /// FTP ALG: tracks FTP control connections for PORT command rewriting.
    ftp_control: BTreeMap<TcpFlowKey, FtpControlState>,
    /// FTP ALG: active-mode data connection listeners.
    ftp_data_listeners: Vec<FtpDataListener>,
}

impl SlirpNet {
    pub fn new() -> Self {
        // Detect host DNS resolver
        let host_dns = detect_host_dns();
        eprintln!("[slirp] DNS relay → {}", host_dns);

        SlirpNet {
            guest_mac: [0; 6],
            rx_queue: VecDeque::new(),
            tcp_conns: BTreeMap::new(),
            udp_flows: BTreeMap::new(),
            dns_socket: None,
            dns_pending: BTreeMap::new(),
            ip_id: 1,
            host_dns,
            dhcp_offered: false,
            pending_connects: Vec::new(),
            ftp_control: BTreeMap::new(),
            ftp_data_listeners: Vec::new(),
        }
    }

    fn next_ip_id(&mut self) -> u16 {
        let id = self.ip_id;
        self.ip_id = self.ip_id.wrapping_add(1);
        id
    }

    /// Process an incoming Ethernet frame from the guest.
    fn process_frame(&mut self, frame: &[u8]) {
        if frame.len() < ETH_HDR { return; }

        // Learn guest MAC from source
        self.guest_mac.copy_from_slice(&frame[6..12]);

        let ethertype = u16be(frame, 12);
        match ethertype {
            ETHERTYPE_ARP => self.handle_arp(frame),
            ETHERTYPE_IPV4 => self.handle_ipv4(frame),
            _ => {} // drop unknown
        }
    }

    // ── ARP ──────────────────────────────────────────────────────────────

    fn handle_arp(&mut self, frame: &[u8]) {
        if frame.len() < ETH_HDR + 28 { return; }
        let arp = &frame[ETH_HDR..];
        let op = u16be(arp, 6);

        let target_ip = &arp[24..28];

        if op != 1 { return; } // only handle ARP Request
        // Reply for any IP in our subnet (gateway, DNS) but NOT the guest's own IP.
        // The guest performs Duplicate Address Detection (DAD) by ARP-probing its
        // assigned IP. If we reply, the guest thinks the address is already taken
        // and rejects the DHCP lease.
        if target_ip[0] != NET_PREFIX[0] || target_ip[1] != NET_PREFIX[1] || target_ip[2] != NET_PREFIX[2] {
            return;
        }
        if target_ip == &GUEST_IP {
            return;
        }

        let mut reply = vec![0u8; ETH_HDR + 28];
        // Ethernet header
        reply[0..6].copy_from_slice(&self.guest_mac);
        reply[6..12].copy_from_slice(&GW_MAC);
        put_u16be(&mut reply, 12, ETHERTYPE_ARP);
        // ARP reply
        let r = &mut reply[ETH_HDR..];
        put_u16be(r, 0, 1);    // HTYPE = Ethernet
        put_u16be(r, 2, 0x0800); // PTYPE = IPv4
        r[4] = 6; // HLEN
        r[5] = 4; // PLEN
        put_u16be(r, 6, 2);    // OPER = Reply
        r[8..14].copy_from_slice(&GW_MAC); // sender MAC
        r[14..18].copy_from_slice(target_ip); // sender IP = requested IP
        r[18..24].copy_from_slice(&self.guest_mac); // target MAC
        r[24..28].copy_from_slice(&arp[14..18]); // target IP = requester's IP

        self.rx_queue.push_back(reply);
    }

    // ── IPv4 ─────────────────────────────────────────────────────────────

    fn handle_ipv4(&mut self, frame: &[u8]) {
        if frame.len() < ETH_HDR + IP_HDR_MIN { return; }
        let ip = &frame[ETH_HDR..];
        let ihl = ((ip[0] & 0x0F) as usize) * 4;
        if ip.len() < ihl { return; }
        let total_len = u16be(ip, 2) as usize;
        if ip.len() < total_len { return; }

        let proto = ip[9];
        let src_ip: [u8; 4] = [ip[12], ip[13], ip[14], ip[15]];
        let dst_ip: [u8; 4] = [ip[16], ip[17], ip[18], ip[19]];
        let payload = &ip[ihl..total_len];

        match proto {
            IP_PROTO_UDP => self.handle_udp(src_ip, dst_ip, payload),
            IP_PROTO_TCP => self.handle_tcp(src_ip, dst_ip, payload),
            IP_PROTO_ICMP => self.handle_icmp(src_ip, dst_ip, payload),
            _ => {}
        }
    }

    // ── ICMP ─────────────────────────────────────────────────────────────

    fn handle_icmp(&mut self, src_ip: [u8; 4], dst_ip: [u8; 4], payload: &[u8]) {
        if payload.len() < 8 { return; }
        let icmp_type = payload[0];
        if icmp_type != 8 { return; } // only Echo Request

        // Build Echo Reply
        let mut icmp_reply = payload.to_vec();
        icmp_reply[0] = 0; // Echo Reply
        icmp_reply[2] = 0; icmp_reply[3] = 0; // clear checksum
        let cksum = ip_checksum(&icmp_reply);
        put_u16be(&mut icmp_reply, 2, cksum);

        self.send_ip_packet(IP_PROTO_ICMP, dst_ip, src_ip, &icmp_reply);
    }

    // ── UDP ──────────────────────────────────────────────────────────────

    fn handle_udp(&mut self, src_ip: [u8; 4], dst_ip: [u8; 4], payload: &[u8]) {
        if payload.len() < 8 { return; }
        let src_port = u16be(payload, 0);
        let dst_port = u16be(payload, 2);
        let udp_data = &payload[8..];

        // DHCP (guest → broadcast or gateway, port 67)
        if dst_port == 67 {
            self.handle_dhcp(src_ip, udp_data, src_port);
            return;
        }

        // DNS (destination = DNS_IP:53 or gateway:53)
        if dst_port == 53 && (dst_ip == DNS_IP || dst_ip == GATEWAY_IP) {
            self.handle_dns(src_port, udp_data);
            return;
        }

        // General UDP — NAT to host
        self.handle_udp_nat(src_ip, src_port, dst_ip, dst_port, udp_data);
    }

    fn handle_udp_nat(&mut self, _src_ip: [u8; 4], src_port: u16, dst_ip: [u8; 4], dst_port: u16, data: &[u8]) {
        let key = UdpFlowKey { guest_port: src_port, remote_ip: dst_ip, remote_port: dst_port };

        // Create flow if new
        if !self.udp_flows.contains_key(&key) {
            let sock = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(_) => return,
            };
            let _ = sock.set_nonblocking(true);
            self.udp_flows.insert(key, UdpFlow {
                socket: sock,
                last_active: Instant::now(),
            });
        }

        if let Some(flow) = self.udp_flows.get_mut(&key) {
            let dst = SocketAddr::new(
                Ipv4Addr::new(dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3]).into(),
                dst_port,
            );
            let _ = flow.socket.send_to(data, dst);
            flow.last_active = Instant::now();
        }
    }

    fn poll_udp(&mut self) {
        let mut responses: Vec<(UdpFlowKey, Vec<u8>)> = Vec::new();

        for (key, flow) in &mut self.udp_flows {
            let mut buf = [0u8; 2048];
            match flow.socket.recv_from(&mut buf) {
                Ok((n, _addr)) => {
                    flow.last_active = Instant::now();
                    responses.push((*key, buf[..n].to_vec()));
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
        }

        for (key, data) in responses {
            self.send_udp_packet(key.remote_ip, GUEST_IP, key.remote_port, key.guest_port, &data);
        }

        // Expire old flows (>60s idle)
        let now = Instant::now();
        self.udp_flows.retain(|_, flow| now.duration_since(flow.last_active).as_secs() < 60);
    }

    // ── DNS relay ────────────────────────────────────────────────────────

    fn handle_dns(&mut self, guest_src_port: u16, data: &[u8]) {
        if data.len() < 12 { return; }
        let txid = u16be(data, 0);

        // Lazy-init shared DNS socket
        if self.dns_socket.is_none() {
            if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
                let _ = s.set_nonblocking(true);
                self.dns_socket = Some(s);
            }
        }

        if let Some(ref sock) = self.dns_socket {
            let _ = sock.send_to(data, self.host_dns);
            self.dns_pending.insert(txid, guest_src_port);
        }
    }

    fn poll_dns(&mut self) {
        // Collect replies first, then send (avoids borrow conflict on self)
        let mut replies: Vec<(u16, Vec<u8>)> = Vec::new(); // (guest_port, dns_data)
        if let Some(ref sock) = self.dns_socket {
            let mut buf = [0u8; 2048];
            loop {
                match sock.recv_from(&mut buf) {
                    Ok((n, _)) => {
                        if n < 12 { continue; }
                        let txid = u16be(&buf, 0);
                        if let Some(guest_port) = self.dns_pending.remove(&txid) {
                            replies.push((guest_port, buf[..n].to_vec()));
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
        for (guest_port, data) in replies {
            self.send_udp_packet(DNS_IP, GUEST_IP, 53, guest_port, &data);
        }
    }

    // ── DHCP server ──────────────────────────────────────────────────────

    fn handle_dhcp(&mut self, _src_ip: [u8; 4], data: &[u8], _src_port: u16) {
        // Minimal DHCP: parse enough to distinguish DISCOVER vs REQUEST
        if data.len() < 240 { return; }
        let msg_type = data[0]; // op: 1=BOOTREQUEST
        if msg_type != 1 { return; }

        let xid = &data[4..8];
        let flags = [data[10], data[11]]; // flags (broadcast bit etc.)
        let chaddr = &data[28..34]; // client hardware address (first 6 bytes)

        // Find DHCP message type in options (after magic cookie at offset 236)
        let magic = &data[236..240];
        eprintln!("[slirp] DHCP: magic={:?}", magic);
        if magic != [99, 130, 83, 99] { eprintln!("[slirp] DHCP: bad magic"); return; }

        let options = &data[240..];
        let dhcp_msg_type = find_dhcp_option(options, 53);
        let dhcp_msg_type = match dhcp_msg_type {
            Some(t) if !t.is_empty() => { eprintln!("[slirp] DHCP: type={}", t[0]); t[0] },
            _ => { eprintln!("[slirp] DHCP: no type option"); return; },
        };

        match dhcp_msg_type {
            1 => self.send_dhcp_offer(xid, chaddr, flags),
            3 => self.send_dhcp_ack(xid, chaddr, flags),
            8 => {
                // DHCPINFORM: client has an IP, wants config (DNS, gateway).
                // RFC 2131: MUST NOT set yiaddr in response to INFORM.
                eprintln!("[slirp] DHCP INFORM → sending ACK with DNS config");
                self.build_dhcp_inform_ack(xid, chaddr, flags);
            }
            _ => { eprintln!("[slirp] DHCP: unhandled type={}", dhcp_msg_type); }
        }
    }

    fn send_dhcp_offer(&mut self, xid: &[u8], chaddr: &[u8], flags: [u8; 2]) {
        self.dhcp_offered = true;
        self.build_dhcp_reply(xid, chaddr, flags, 2); // DHCPOFFER
    }

    fn send_dhcp_ack(&mut self, xid: &[u8], chaddr: &[u8], flags: [u8; 2]) {
        self.build_dhcp_reply(xid, chaddr, flags, 5); // DHCPACK
    }

    /// Build DHCPACK for DHCPINFORM — yiaddr MUST be 0 (RFC 2131 §4.3.5).
    fn build_dhcp_inform_ack(&mut self, xid: &[u8], chaddr: &[u8], flags: [u8; 2]) {
        let mut reply = vec![0u8; 576];
        reply[0] = 2; // op = BOOTREPLY
        reply[1] = 1; // htype = Ethernet
        reply[2] = 6; // hlen
        reply[4..8].copy_from_slice(xid);
        reply[10] = flags[0];
        reply[11] = flags[1];
        // yiaddr = 0 (bytes 16..20 already zero) — MUST NOT set for INFORM
        reply[20..24].copy_from_slice(&GATEWAY_IP); // siaddr
        let mac_len = chaddr.len().min(16);
        reply[28..28 + mac_len].copy_from_slice(&chaddr[..mac_len]);

        // DHCP magic cookie
        reply[236..240].copy_from_slice(&[99, 130, 83, 99]);

        let mut off = 240;
        // 53: DHCP Message Type = ACK
        reply[off] = 53; reply[off+1] = 1; reply[off+2] = 5; off += 3;
        // 54: Server Identifier
        reply[off] = 54; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&GATEWAY_IP); off += 6;
        // 1: Subnet Mask
        reply[off] = 1; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&NETMASK); off += 6;
        // 3: Router
        reply[off] = 3; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&GATEWAY_IP); off += 6;
        // 6: DNS Server
        reply[off] = 6; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&DNS_IP); off += 6;
        // 28: Broadcast Address
        reply[off] = 28; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&BROADCAST); off += 6;
        // 255: End
        reply[off] = 255;
        off += 1;

        let actual_len = off.max(300);
        reply.truncate(actual_len);

        eprintln!("[slirp] DHCP INFORM ACK: yiaddr=0 DNS={}.{}.{}.{} len={}",
            DNS_IP[0], DNS_IP[1], DNS_IP[2], DNS_IP[3], reply.len());

        // Send to guest's IP (unicast), not broadcast.
        let src_ip = GATEWAY_IP;
        let dst_ip = GUEST_IP;
        let udp_len = (8 + reply.len()) as u16;
        let mut udp_payload = Vec::with_capacity(8 + reply.len());
        udp_payload.extend_from_slice(&udp_header(67, 68, reply.len() as u16));
        udp_payload.extend_from_slice(&reply);

        let mut pseudo = Vec::with_capacity(12 + udp_payload.len());
        pseudo.extend_from_slice(&src_ip);
        pseudo.extend_from_slice(&dst_ip);
        pseudo.push(0);
        pseudo.push(IP_PROTO_UDP);
        pseudo.extend_from_slice(&udp_len.to_be_bytes());
        pseudo.extend_from_slice(&udp_payload);
        let cksum = ip_checksum(&pseudo);
        let cksum = if cksum == 0 { 0xFFFF } else { cksum };
        put_u16be(&mut udp_payload, 6, cksum);

        self.send_ip_packet(IP_PROTO_UDP, src_ip, dst_ip, &udp_payload);
    }

    fn build_dhcp_reply(&mut self, xid: &[u8], chaddr: &[u8], flags: [u8; 2], msg_type: u8) {
        let mut reply = vec![0u8; 576]; // minimum DHCP packet
        reply[0] = 2; // op = BOOTREPLY
        reply[1] = 1; // htype = Ethernet
        reply[2] = 6; // hlen
        reply[4..8].copy_from_slice(xid);
        reply[10] = flags[0]; // flags (broadcast bit etc.)
        reply[11] = flags[1];
        reply[16..20].copy_from_slice(&GUEST_IP); // yiaddr
        reply[20..24].copy_from_slice(&GATEWAY_IP); // siaddr (next server)
        // chaddr
        let mac_len = chaddr.len().min(16);
        reply[28..28 + mac_len].copy_from_slice(&chaddr[..mac_len]);

        // DHCP magic cookie
        reply[236..240].copy_from_slice(&[99, 130, 83, 99]);

        // Options
        let mut off = 240;
        // 53: DHCP Message Type
        reply[off] = 53; reply[off+1] = 1; reply[off+2] = msg_type; off += 3;
        // 54: Server Identifier
        reply[off] = 54; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&GATEWAY_IP); off += 6;
        // 51: Lease Time (86400 = 24h)
        reply[off] = 51; reply[off+1] = 4; put_u32be(&mut reply, off+2, 86400); off += 6;
        // 1: Subnet Mask
        reply[off] = 1; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&NETMASK); off += 6;
        // 3: Router
        reply[off] = 3; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&GATEWAY_IP); off += 6;
        // 6: DNS Server
        reply[off] = 6; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&DNS_IP); off += 6;
        // 28: Broadcast Address
        reply[off] = 28; reply[off+1] = 4; reply[off+2..off+6].copy_from_slice(&BROADCAST); off += 6;
        // 255: End
        reply[off] = 255;
        off += 1;

        // Truncate reply to actual size (options end here, rest is unused padding).
        // DHCP minimum is 300 bytes (RFC 2131), pad to that if shorter.
        let actual_len = off.max(300);
        reply.truncate(actual_len);

        eprintln!("[slirp] DHCP reply: op={} xid={:02X}{:02X}{:02X}{:02X} yiaddr={}.{}.{}.{} chaddr={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X} type={} len={}",
            reply[0], reply[4], reply[5], reply[6], reply[7],
            reply[16], reply[17], reply[18], reply[19],
            reply[28], reply[29], reply[30], reply[31], reply[32], reply[33],
            msg_type, reply.len());

        // Build UDP with checksum
        let src_ip = GATEWAY_IP;
        let dst_ip: [u8; 4] = [255, 255, 255, 255];
        let udp_len = (8 + reply.len()) as u16;
        let mut udp_payload = Vec::with_capacity(8 + reply.len());
        udp_payload.extend_from_slice(&udp_header(67, 68, reply.len() as u16));
        udp_payload.extend_from_slice(&reply);

        // Compute UDP checksum over pseudo-header + UDP header + data
        let mut pseudo = Vec::with_capacity(12 + udp_payload.len());
        pseudo.extend_from_slice(&src_ip);
        pseudo.extend_from_slice(&dst_ip);
        pseudo.push(0); // zero
        pseudo.push(IP_PROTO_UDP);
        let len_be = udp_len.to_be_bytes();
        pseudo.extend_from_slice(&len_be);
        pseudo.extend_from_slice(&udp_payload);
        let cksum = ip_checksum(&pseudo);
        // UDP checksum of 0x0000 is transmitted as 0xFFFF
        let cksum = if cksum == 0 { 0xFFFF } else { cksum };
        put_u16be(&mut udp_payload, 6, cksum);

        // Build IP + Ethernet
        let id = self.next_ip_id();
        let ip_hdr = ip_header(IP_PROTO_UDP, src_ip, dst_ip, udp_payload.len() as u16, id);
        let eth = eth_header(&[0xFF; 6], &GW_MAC, ETHERTYPE_IPV4);

        let mut frame = Vec::with_capacity(ETH_HDR + 20 + udp_payload.len());
        frame.extend_from_slice(&eth);
        frame.extend_from_slice(&ip_hdr);
        frame.extend_from_slice(&udp_payload);

        self.rx_queue.push_back(frame);
    }

    // ── TCP NAT ──────────────────────────────────────────────────────────

    /// Parse TCP options from a SYN segment and extract the Window Scale shift count.
    fn parse_wscale(tcp_payload: &[u8]) -> u8 {
        let data_offset = ((tcp_payload[12] >> 4) as usize) * 4;
        if data_offset <= 20 || data_offset > tcp_payload.len() {
            return 0; // no options
        }
        let options = &tcp_payload[20..data_offset];
        let mut i = 0;
        while i < options.len() {
            match options[i] {
                0 => break,             // End of Option List
                1 => { i += 1; }        // NOP
                3 => {                   // Window Scale (kind=3, len=3, shift_count)
                    if i + 2 < options.len() && options[i + 1] == 3 {
                        return options[i + 2].min(14); // RFC 1323: max shift is 14
                    }
                    break;
                }
                _ => {                   // Skip unknown option
                    if i + 1 >= options.len() { break; }
                    let len = options[i + 1] as usize;
                    if len < 2 { break; }
                    i += len;
                }
            }
        }
        0
    }

    fn handle_tcp(&mut self, src_ip: [u8; 4], dst_ip: [u8; 4], payload: &[u8]) {
        if payload.len() < 20 { return; }
        let src_port = u16be(payload, 0);
        let dst_port = u16be(payload, 2);
        let seq = u32be(payload, 4);
        let ack = u32be(payload, 8);
        let data_offset = ((payload[12] >> 4) as usize) * 4;
        let flags = payload[13];
        let tcp_data = if data_offset < payload.len() { &payload[data_offset..] } else { &[] };

        let syn = flags & 0x02 != 0;
        let ack_flag = flags & 0x10 != 0;
        let fin = flags & 0x01 != 0;
        let rst = flags & 0x04 != 0;

        let key = TcpFlowKey { guest_port: src_port, remote_ip: dst_ip, remote_port: dst_port };

        if rst {
            // Guest sent RST — close connection
            self.tcp_conns.remove(&key);
            self.ftp_control.remove(&key);
            return;
        }

        // Handle SYN-ACK from guest (response to our SYN for FTP data connection).
        if syn && ack_flag {
            if let Some(conn) = self.tcp_conns.get_mut(&key) {
                if conn.state == TcpState::SynSent {
                    conn.guest_isn = seq;
                    conn.guest_seq = seq.wrapping_add(1);
                    conn.our_seq = conn.our_seq.wrapping_add(1); // SYN consumed
                    conn.guest_wscale = Self::parse_wscale(payload);
                    conn.state = TcpState::Established;
                    let our_seq = conn.our_seq;
                    let guest_seq = conn.guest_seq;
                    // Send ACK to complete the handshake.
                    self.send_tcp_flags(key, 0x10, our_seq, guest_seq, &[]);
                    return;
                }
            }
        }

        if syn && !ack_flag {
            // New connection: SYN
            let addr = SocketAddr::new(
                Ipv4Addr::new(dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3]).into(),
                dst_port,
            );
            // Connect in a background thread to avoid blocking the VM loop.
            // Store the receiver so we can check for completion in poll_tcp.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5));
                let _ = tx.send(result);
            });

            let our_seq: u32 = 0x1000_0000;
            let wscale = Self::parse_wscale(payload);
            self.pending_connects.push((key, rx, seq, our_seq, wscale));
            // FTP ALG: track control connections (port 21).
            if dst_port == 21 {
                eprintln!("[slirp-ftp] FTP control connection detected: :{} → {}:21", src_port,
                    Ipv4Addr::new(dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3]));
                self.ftp_control.insert(key, FtpControlState {
                    guest_line_buf: Vec::new(),
                    local_addr: None,
                });
            }
            return;
        }

        // Existing connection — extract values to avoid borrow conflicts
        {
            let conn = match self.tcp_conns.get_mut(&key) {
                Some(c) => c,
                None => return,
            };

            // Track guest window size for flow control (apply Window Scale from SYN)
            let raw_window = u16be(payload, 14) as u32;
            conn.guest_window = raw_window << conn.guest_wscale;

            if ack_flag && conn.state == TcpState::SynReceived {
                conn.state = TcpState::Established;
                conn.our_seq = conn.our_seq.wrapping_add(1); // SYN consumed
                // Clear retransmit queue — SYN-ACK was acknowledged
                conn.retransmit_queue.clear();
            }

            // Process ACK: reduce unacked byte count and purge retransmit queue
            if ack_flag && (conn.state == TcpState::Established || conn.state == TcpState::FinWait) {
                let ack_num = u32be(payload, 8);
                let oldest_unacked = conn.our_seq.wrapping_sub(conn.unacked);
                let acked = ack_num.wrapping_sub(oldest_unacked);
                if acked > 0 && acked <= conn.unacked {
                    conn.unacked -= acked;
                    // Remove ACKed segments from retransmit queue
                    conn.retransmit_queue.retain(|(seg_seq, seg_data)| {
                        let seg_end = seg_seq.wrapping_add(seg_data.len() as u32);
                        seg_end.wrapping_sub(ack_num) < 0x8000_0000 // seg_end > ack_num (wrapping)
                    });
                }
            }

            // Data from guest → write to host socket (non-blocking)
            // Guard against retransmits: only forward data we haven't seen yet.
            if !tcp_data.is_empty() && conn.state == TcpState::Established {
                let expected = conn.guest_seq;
                let seg_end = seq.wrapping_add(tcp_data.len() as u32);

                // Determine the new data bytes to forward.
                let new_data: &[u8] = if seq == expected {
                    tcp_data
                } else if seq.wrapping_sub(expected) < 0x8000_0000 {
                    tcp_data
                } else {
                    let overlap = expected.wrapping_sub(seq) as usize;
                    if overlap < tcp_data.len() { &tcp_data[overlap..] } else { &[] }
                };

                if !new_data.is_empty() {
                    // FTP ALG: intercept PORT commands on FTP control connections.
                    let is_ftp = self.ftp_control.contains_key(&key);
                    let write_data;
                    if is_ftp {
                        // Learn local address on first data write.
                        if self.ftp_control.get(&key).and_then(|s| s.local_addr).is_none() {
                            if let Ok(addr) = conn.stream.local_addr() {
                                if let Some(state) = self.ftp_control.get_mut(&key) {
                                    state.local_addr = Some(addr);
                                }
                            }
                        }
                        // Can't call &mut self method while conn is borrowed;
                        // collect what we need and process after dropping conn.
                        write_data = None; // handled below
                    } else {
                        write_data = Some(new_data.to_vec());
                    }

                    if let Some(data) = write_data {
                        let _ = conn.stream.set_nonblocking(true);
                        let _ = conn.stream.write_all(&data);
                        let _ = conn.stream.set_nonblocking(false);
                    }
                    conn.guest_seq = seg_end;
                }
            }

            if fin {
                conn.guest_seq = conn.guest_seq.wrapping_add(1); // FIN counts as 1
            }
        }

        // FTP ALG: process PORT commands outside the conn borrow.
        if self.ftp_control.contains_key(&key) && !tcp_data.is_empty() {
            let rewritten = self.ftp_alg_rewrite_guest_data(&key, tcp_data);
            if let Some(conn) = self.tcp_conns.get_mut(&key) {
                let _ = conn.stream.set_nonblocking(true);
                let _ = conn.stream.write_all(&rewritten);
                let _ = conn.stream.set_nonblocking(false);
            }
        }

        // Now send TCP responses outside the mutable borrow of tcp_conns.
        let (our_seq, guest_seq, conn_state) = match self.tcp_conns.get(&key) {
            Some(conn) => (conn.our_seq, conn.guest_seq, Some(conn.state == TcpState::FinWait)),
            None => return,
        };

        if !tcp_data.is_empty() {
            self.send_tcp_flags(key, 0x10, our_seq, guest_seq, &[]); // ACK
        }

        if fin {
            self.send_tcp_flags(key, 0x10, our_seq, guest_seq, &[]); // ACK FIN
            if conn_state == Some(true) {
                // We already sent our FIN; guest is now closing too → fully closed.
                if let Some(c) = self.tcp_conns.get_mut(&key) {
                    c.state = TcpState::Closed;
                }
            } else {
                // Guest initiated close — send our FIN+ACK
                self.send_tcp_flags(key, 0x11, our_seq, guest_seq, &[]); // FIN+ACK
                if let Some(c) = self.tcp_conns.get_mut(&key) {
                    c.state = TcpState::Closed;
                }
            }
        }
    }

    // ── FTP ALG ──────────────────────────────────────────────────────────

    /// Process guest→server FTP data, rewriting PORT commands.
    /// Returns the data to forward to the host socket.
    fn ftp_alg_rewrite_guest_data(&mut self, key: &TcpFlowKey, data: &[u8]) -> Vec<u8> {
        let local_addr = match self.ftp_control.get(key) {
            Some(s) => s.local_addr,
            None => return data.to_vec(),
        };

        // Append to line buffer for reassembly.
        if let Some(state) = self.ftp_control.get_mut(key) {
            state.guest_line_buf.extend_from_slice(data);
        }

        let mut output = Vec::new();
        loop {
            let line_end = {
                let state = match self.ftp_control.get(key) {
                    Some(s) => s,
                    None => break,
                };
                state.guest_line_buf.windows(2).position(|w| w == b"\r\n")
                    .map(|p| p + 2)
            };

            match line_end {
                Some(end) => {
                    let line: Vec<u8> = {
                        let state = self.ftp_control.get_mut(key).unwrap();
                        state.guest_line_buf.drain(..end).collect()
                    };

                    // Check for PORT command.
                    if let Some(rewritten) = self.ftp_rewrite_port(&line, key, local_addr) {
                        output.extend_from_slice(&rewritten);
                    } else {
                        output.extend_from_slice(&line);
                    }
                }
                None => break, // No complete line yet — keep buffered
            }
        }

        output
    }

    /// Parse and rewrite a PORT command, opening a host-side listener.
    fn ftp_rewrite_port(&mut self, line: &[u8], key: &TcpFlowKey, local_addr: Option<SocketAddr>) -> Option<Vec<u8>> {
        let text = core::str::from_utf8(line).ok()?;
        let upper = text.to_uppercase();

        if !upper.starts_with("PORT ") {
            return None;
        }

        // Parse PORT h1,h2,h3,h4,p1,p2\r\n
        let args = text[5..].trim();
        let parts: Vec<u16> = args.split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if parts.len() != 6 { return None; }

        let guest_data_port = (parts[4] << 8) | parts[5];

        // Create host-side listener for the FTP server to connect to.
        let listener = TcpListener::bind("0.0.0.0:0").ok()?;
        let _ = listener.set_nonblocking(true);
        let host_port = listener.local_addr().ok()?.port();

        // Get host IP from the control connection's local address.
        let host_ip = match local_addr {
            Some(SocketAddr::V4(addr)) => addr.ip().octets(),
            _ => [127, 0, 0, 1], // fallback
        };

        self.ftp_data_listeners.push(FtpDataListener {
            listener,
            guest_data_port,
            server_ip: key.remote_ip,
            created: Instant::now(),
        });

        let rewritten = alloc::format!("PORT {},{},{},{},{},{}\r\n",
            host_ip[0], host_ip[1], host_ip[2], host_ip[3],
            host_port >> 8, host_port & 0xFF);

        eprintln!("[slirp-ftp] PORT rewrite: guest :{} → host {}:{}", guest_data_port,
            Ipv4Addr::new(host_ip[0], host_ip[1], host_ip[2], host_ip[3]), host_port);

        Some(rewritten.into_bytes())
    }

    /// Initiate a TCP connection from gateway to guest (for FTP active data).
    fn ftp_initiate_data_conn(&mut self, server_ip: [u8; 4], server_port: u16,
                               guest_port: u16, host_stream: TcpStream) {
        let our_seq: u32 = 0x2000_0000;
        let key = TcpFlowKey {
            guest_port,
            remote_ip: server_ip,
            remote_port: server_port,
        };

        let conn = TcpConnection {
            stream: host_stream,
            state: TcpState::SynSent,
            our_seq,
            unacked: 0,
            guest_seq: 0,
            guest_isn: 0,
            read_buf: [0u8; 16384],
            guest_window: 0,
            guest_wscale: 0, // will be updated from SYN-ACK
            retransmit_queue: Vec::new(),
            last_retransmit: None,
        };

        self.tcp_conns.insert(key, conn);
        // Send SYN to guest — the guest's FTP client is listening on this port.
        self.send_tcp_flags(key, 0x02, our_seq, 0, &[]); // SYN
        eprintln!("[slirp-ftp] Initiated data connection: {}:{} → guest:{}",
            Ipv4Addr::new(server_ip[0], server_ip[1], server_ip[2], server_ip[3]),
            server_port, guest_port);
    }

    /// Poll FTP data listeners for incoming connections from FTP servers.
    fn poll_ftp(&mut self) {
        let now = Instant::now();
        let mut accepted: Vec<(u16, [u8; 4], TcpStream, u16)> = Vec::new(); // (guest_port, server_ip, stream, server_port)
        let mut to_remove: Vec<usize> = Vec::new();

        for (i, listener) in self.ftp_data_listeners.iter().enumerate() {
            // Timeout after 30 seconds.
            if now.duration_since(listener.created).as_secs() > 30 {
                to_remove.push(i);
                continue;
            }

            match listener.listener.accept() {
                Ok((stream, addr)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_nodelay(true);
                    accepted.push((
                        listener.guest_data_port,
                        listener.server_ip,
                        stream,
                        addr.port(),
                    ));
                    to_remove.push(i);
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => { to_remove.push(i); }
            }
        }

        // Remove processed/expired listeners (reverse order).
        to_remove.sort_unstable();
        for i in to_remove.into_iter().rev() {
            self.ftp_data_listeners.swap_remove(i);
        }

        // Initiate data connections to the guest.
        for (guest_port, server_ip, stream, server_port) in accepted {
            self.ftp_initiate_data_conn(server_ip, server_port, guest_port, stream);
        }
    }

    fn poll_tcp(&mut self) {
        // Complete pending connects (non-blocking check)
        let mut completed = Vec::new();
        let mut i = 0;
        while i < self.pending_connects.len() {
            match self.pending_connects[i].1.try_recv() {
                Ok(Ok(stream)) => {
                    let (key, _, guest_seq_raw, our_seq, wscale) = self.pending_connects.remove(i);
                    // Keep socket blocking for writes (write_all must not lose data).
                    // Reads use set_nonblocking temporarily in poll_tcp.
                    let _ = stream.set_nodelay(true);
                    let conn = TcpConnection {
                        stream,
                        state: TcpState::SynReceived,
                        our_seq,
                        unacked: 0,
                        guest_seq: guest_seq_raw.wrapping_add(1),
                        guest_isn: guest_seq_raw,
                        read_buf: [0u8; 16384],
                        guest_window: 0,
                        guest_wscale: wscale,
                        retransmit_queue: Vec::new(),
                        last_retransmit: None,
                    };
                    completed.push((key, conn, our_seq, guest_seq_raw));
                }
                Ok(Err(_)) => {
                    let (key, _, guest_seq_raw, _, _) = self.pending_connects.remove(i);
                    self.send_tcp_flags(key, 0x14, 0, guest_seq_raw.wrapping_add(1), &[]);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => { i += 1; } // still connecting
                Err(_) => { self.pending_connects.remove(i); } // thread died
            }
        }
        for (key, conn, our_seq, guest_seq_raw) in completed {
            self.tcp_conns.insert(key, conn);
            self.send_tcp_flags(key, 0x12, our_seq, guest_seq_raw.wrapping_add(1), &[]);
        }

        const TCP_MSS: usize = 1460;
        const MAX_UNACKED_CAP: u32 = 512 * 1024; // 512 KB — generous for scaled windows
        const RETRANSMIT_TIMEOUT_MS: u128 = 200;
        const MAX_SEGMENTS_PER_POLL: usize = 128;

        let mut data_to_send: Vec<(TcpFlowKey, u32, Vec<u8>)> = Vec::new(); // (key, seq, data)
        let mut closed: Vec<TcpFlowKey> = Vec::new();
        let mut retransmits: Vec<(TcpFlowKey, u32, Vec<u8>)> = Vec::new();

        let now = Instant::now();

        for (key, conn) in &mut self.tcp_conns {
            if conn.state != TcpState::Established { continue; }

            // Respect rx_queue backpressure — don't flood the E1000 ring
            if self.rx_queue.len() > 1024 { break; }

            // Non-blocking read: temporarily switch to non-blocking mode
            let _ = conn.stream.set_nonblocking(true);

            // Read multiple segments per poll to improve throughput.
            let mut segments_this_conn = 0;
            loop {
                if segments_this_conn >= MAX_SEGMENTS_PER_POLL { break; }
                if self.rx_queue.len() + data_to_send.len() > 1024 { break; }

                // Flow control: respect guest's advertised window (already scaled), with fallback
                let guest_win = if conn.guest_window > 0 { conn.guest_window } else { 65535 };
                let max_unacked = guest_win.min(MAX_UNACKED_CAP);
                if conn.unacked >= max_unacked { break; }

                let remaining_window = max_unacked.saturating_sub(conn.unacked) as usize;
                let max_read = TCP_MSS.min(conn.read_buf.len()).min(remaining_window.max(TCP_MSS));
                match conn.stream.read(&mut conn.read_buf[..max_read]) {
                    Ok(0) => { closed.push(*key); break; }
                    Ok(n) => {
                        let seg_data = conn.read_buf[..n].to_vec();
                        let seg_seq = conn.our_seq;
                        conn.retransmit_queue.push((seg_seq, seg_data.clone()));
                        conn.our_seq = conn.our_seq.wrapping_add(n as u32);
                        conn.unacked = conn.unacked.wrapping_add(n as u32);
                        data_to_send.push((*key, seg_seq, seg_data));
                        segments_this_conn += 1;
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => { closed.push(*key); break; }
                }
            }

            let _ = conn.stream.set_nonblocking(false);
            if segments_this_conn > 0 {
                conn.last_retransmit = Some(now);
            }

            // Retransmit check: if we have unacked segments and haven't
            // heard back in a while, retransmit the oldest segment.
            if conn.unacked > 0 && !conn.retransmit_queue.is_empty() {
                let should_retransmit = match conn.last_retransmit {
                    Some(last) => now.duration_since(last).as_millis() >= RETRANSMIT_TIMEOUT_MS,
                    None => true,
                };
                if should_retransmit {
                    // Retransmit the first (oldest) unacked segment
                    let (seg_seq, seg_data) = conn.retransmit_queue[0].clone();
                    retransmits.push((*key, seg_seq, seg_data));
                    conn.last_retransmit = Some(now);
                }
            }
        }

        // Send each chunk as a single TCP segment.
        // Sequence numbers and unacked counters were already updated in the
        // read loop above — here we just build and queue the actual packets.
        for (key, seg_seq, data) in &data_to_send {
            if let Some(conn) = self.tcp_conns.get(key) {
                let ack = conn.guest_seq;
                self.send_tcp_flags(*key, 0x18, *seg_seq, ack, data); // PSH+ACK
            }
        }

        // Send retransmits
        for (key, seg_seq, seg_data) in retransmits {
            if let Some(conn) = self.tcp_conns.get(&key) {
                let ack = conn.guest_seq;
                self.send_tcp_flags(key, 0x18, seg_seq, ack, &seg_data); // PSH+ACK retransmit
            }
        }

        for key in closed {
            if let Some(conn) = self.tcp_conns.get(&key) {
                let seq = conn.our_seq;
                let ack = conn.guest_seq;
                self.send_tcp_flags(key, 0x11, seq, ack, &[]); // FIN+ACK
            }
            // Move to FinWait instead of removing — wait for guest's FIN/ACK.
            if let Some(conn) = self.tcp_conns.get_mut(&key) {
                conn.state = TcpState::FinWait;
                conn.our_seq = conn.our_seq.wrapping_add(1); // FIN consumes one seq
            }
        }

        // Only remove connections that have been fully closed (both sides FIN'd)
        // or that have been in FinWait for too long (>10s timeout).
        self.tcp_conns.retain(|key, conn| {
            let remove = if conn.state == TcpState::Closed { true }
                else if conn.state == TcpState::FinWait {
                    conn.last_retransmit.map_or(false, |last| now.duration_since(last).as_secs() > 10)
                } else { false };
            if remove {
                // Clean up FTP ALG state for closed connections.
                self.ftp_control.remove(key);
            }
            !remove
        });

    }

    fn send_tcp_flags(&mut self, key: TcpFlowKey, flags: u8, seq: u32, ack: u32, data: &[u8]) {
        let syn = flags & 0x02 != 0;
        // SYN-ACK needs MSS (4) + NOP (1) + Window Scale (3) = 8 bytes options
        // → 28-byte TCP header (must be 4-byte aligned: 20 + 8 = 28).
        let hdr_len: u8 = if syn { 28 } else { 20 };
        let mut tcp = vec![0u8; hdr_len as usize + data.len()];
        put_u16be(&mut tcp, 0, key.remote_port); // src port (from gateway perspective)
        put_u16be(&mut tcp, 2, key.guest_port);  // dst port
        put_u32be(&mut tcp, 4, seq);
        put_u32be(&mut tcp, 8, ack);
        tcp[12] = (hdr_len / 4) << 4; // data offset
        tcp[13] = flags;
        // Advertise a 64KB raw window. With our wscale=8, the guest sees 64KB << 8 = 16 MB.
        put_u16be(&mut tcp, 14, 65535);
        if syn {
            // TCP Option: MSS = 1460 (kind=2, len=4, value=0x05B4)
            tcp[20] = 2;    // Kind: Maximum Segment Size
            tcp[21] = 4;    // Length: 4 bytes
            tcp[22] = 0x05; // MSS high byte (1460 = 0x05B4)
            tcp[23] = 0xB4; // MSS low byte
            // TCP Option: NOP (padding for alignment)
            tcp[24] = 1;    // Kind: NOP
            // TCP Option: Window Scale (kind=3, len=3, shift_count=8)
            tcp[25] = 3;    // Kind: Window Scale
            tcp[26] = 3;    // Length: 3 bytes
            tcp[27] = 8;    // Shift count: 8 (our window = raw_window << 8)
        }
        if !data.is_empty() {
            tcp[hdr_len as usize..].copy_from_slice(data);
        }

        // TCP checksum (pseudo-header + TCP header + data)
        let src = key.remote_ip;
        let dst = GUEST_IP;
        let tcp_len = tcp.len() as u16;
        let mut pseudo = vec![0u8; 12 + tcp.len()];
        pseudo[0..4].copy_from_slice(&src);
        pseudo[4..8].copy_from_slice(&dst);
        pseudo[9] = IP_PROTO_TCP;
        put_u16be(&mut pseudo, 10, tcp_len);
        pseudo[12..].copy_from_slice(&tcp);
        let cksum = ip_checksum(&pseudo);
        put_u16be(&mut tcp, 16, cksum);

        self.send_ip_packet(IP_PROTO_TCP, key.remote_ip, GUEST_IP, &tcp);
    }

    // ── Common packet builder ────────────────────────────────────────────

    fn send_ip_packet(&mut self, proto: u8, src: [u8; 4], dst: [u8; 4], payload: &[u8]) {
        let id = self.next_ip_id();
        let ip_hdr = ip_header(proto, src, dst, payload.len() as u16, id);
        let eth = eth_header(&self.guest_mac, &GW_MAC, ETHERTYPE_IPV4);

        let mut frame = Vec::with_capacity(ETH_HDR + 20 + payload.len());
        frame.extend_from_slice(&eth);
        frame.extend_from_slice(&ip_hdr);
        frame.extend_from_slice(payload);

        self.rx_queue.push_back(frame);
    }

    /// Send a UDP packet with correct checksum.
    fn send_udp_packet(&mut self, src_ip: [u8; 4], dst_ip: [u8; 4], src_port: u16, dst_port: u16, data: &[u8]) {
        let udp_len = (8 + data.len()) as u16;
        let mut udp = Vec::with_capacity(8 + data.len());
        udp.extend_from_slice(&udp_header(src_port, dst_port, data.len() as u16));
        udp.extend_from_slice(data);

        // UDP checksum over pseudo-header
        let mut pseudo = Vec::with_capacity(12 + udp.len());
        pseudo.extend_from_slice(&src_ip);
        pseudo.extend_from_slice(&dst_ip);
        pseudo.push(0);
        pseudo.push(IP_PROTO_UDP);
        pseudo.extend_from_slice(&udp_len.to_be_bytes());
        pseudo.extend_from_slice(&udp);
        let cksum = ip_checksum(&pseudo);
        let cksum = if cksum == 0 { 0xFFFF } else { cksum };
        put_u16be(&mut udp, 6, cksum);

        self.send_ip_packet(IP_PROTO_UDP, src_ip, dst_ip, &udp);
    }
}

impl NetBackend for SlirpNet {
    fn send(&mut self, frame: &[u8]) {
        self.process_frame(frame);
    }

    fn recv(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::with_capacity(self.rx_queue.len());
        while let Some(f) = self.rx_queue.pop_front() {
            out.push(f);
        }
        out
    }

    fn poll(&mut self) {
        self.poll_dns();
        self.poll_udp();
        self.poll_ftp();
        self.poll_tcp();
    }

    fn description(&self) -> &str {
        "user-mode NAT (10.0.2.0/24)"
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn find_dhcp_option<'a>(options: &'a [u8], code: u8) -> Option<&'a [u8]> {
    let mut i = 0;
    while i < options.len() {
        let opt = options[i];
        if opt == 255 { break; } // end
        if opt == 0 { i += 1; continue; } // pad
        if i + 1 >= options.len() { break; }
        let len = options[i + 1] as usize;
        if i + 2 + len > options.len() { break; }
        if opt == code {
            return Some(&options[i + 2..i + 2 + len]);
        }
        i += 2 + len;
    }
    None
}

fn detect_host_dns() -> SocketAddr {
    // Try systemd-resolve to get the real upstream DNS server
    if let Ok(output) = std::process::Command::new("resolvectl").arg("status").output() {
        if let Ok(text) = std::str::from_utf8(&output.stdout) {
            for line in text.lines() {
                if line.contains("DNS Server") {
                    for part in line.split_whitespace() {
                        if let Ok(ip) = part.parse::<Ipv4Addr>() {
                            if !ip.is_loopback() && ip.octets()[0] != 127 {
                                return SocketAddr::new(ip.into(), 53);
                            }
                        }
                    }
                }
            }
        }
    }

    // Try /etc/resolv.conf, skip loopback/stub addresses
    if let Ok(contents) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in contents.lines() {
            let line = line.trim();
            if line.starts_with("nameserver") {
                if let Some(addr) = line.split_whitespace().nth(1) {
                    if let Ok(ip) = addr.parse::<Ipv4Addr>() {
                        if ip.octets()[0] != 127 {
                            return SocketAddr::new(ip.into(), 53);
                        }
                    }
                }
            }
        }
    }

    // Fallback: Google DNS
    SocketAddr::new(Ipv4Addr::new(8, 8, 8, 8).into(), 53)
}
