//! # P2P NAT Traversal, STUN Binding & ICE Wire Protocol (`axiom-net::nat`)
//!
//! 0-Float, 64-Byte L1 cache aligned P2P direct hole-punching and STUN protocol implementation.
//! Provides RFC 5389 compliant STUN packet structures and a 16-peer full-mesh
//! NAT Traversal state machine for direct peer-to-peer UDP connectivity.

use core::mem::{align_of, size_of};

/// Standard RFC 5389 STUN Magic Cookie (0x2112A442 in network byte order).
pub const STUN_MAGIC_COOKIE: u32 = 0x2112_A442;

/// Supported candidate types under Interactive Connectivity Establishment (ICE RFC 8445).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NatCandidateKind {
    #[default]
    Host = 0,             // Direct local interface IP
    ServerReflexive = 1,  // Public IP discovered via STUN
    PeerReflexive = 2,    // Public IP discovered during direct peer probing
    Relayed = 3,          // Relayed fallback via TURN
}

/// NAT candidate connection status.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NatConnectionState {
    #[default]
    Idle = 0,
    Probing = 1,
    Connected = 2,
    Failed = 3,
}

/// Exact 64-Byte L1 cache aligned ICE Candidate Descriptor.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NatCandidate64 {
    pub kind: NatCandidateKind,       // 1B: Candidate kind
    pub state: NatConnectionState,   // 1B: Connection state
    pub port: u16,                    // 2B: UDP Port
    pub ip_v4: [u8; 4],               // 4B: IPv4 Address
    pub ip_v6: [u8; 16],              // 16B: IPv6 Address
    pub priority: u32,                // 4B: ICE Candidate Priority
    pub rtt_micros: u32,              // 4B: Round-Trip Time latency
    pub is_v6: u8,                    // 1B: IPv6 flag
    pub component_id: u8,             // 1B: Media component (1 = Rollback Wire)
    pub foundation: u16,              // 2B: Candidate foundation ID
    pub _padding: [u8; 28],           // 28B: Exact padding to 64 bytes
}

const _: () = assert!(size_of::<NatCandidate64>() == 64);
const _: () = assert!(align_of::<NatCandidate64>() == 64);

impl Default for NatCandidate64 {
    fn default() -> Self {
        Self {
            kind: NatCandidateKind::Host,
            state: NatConnectionState::Idle,
            port: 0,
            ip_v4: [0; 4],
            ip_v6: [0; 16],
            priority: 0,
            rtt_micros: 0,
            is_v6: 0,
            component_id: 1,
            foundation: 0,
            _padding: [0u8; 28],
        }
    }
}

impl NatCandidate64 {
    /// Creates a new IPv4 Host Candidate.
    pub fn new_ipv4_host(ip: [u8; 4], port: u16, priority: u32) -> Self {
        Self {
            kind: NatCandidateKind::Host,
            state: NatConnectionState::Idle,
            port,
            ip_v4: ip,
            ip_v6: [0; 16],
            priority,
            rtt_micros: 0,
            is_v6: 0,
            component_id: 1,
            foundation: 100,
            _padding: [0u8; 28],
        }
    }

    /// Creates a new IPv4 Server-Reflexive (STUN discovered) Candidate.
    pub fn new_ipv4_srflx(ip: [u8; 4], port: u16, priority: u32) -> Self {
        Self {
            kind: NatCandidateKind::ServerReflexive,
            state: NatConnectionState::Idle,
            port,
            ip_v4: ip,
            ip_v6: [0; 16],
            priority,
            rtt_micros: 0,
            is_v6: 0,
            component_id: 1,
            foundation: 200,
            _padding: [0u8; 28],
        }
    }
}

/// Standard STUN Message Types (RFC 5389).
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StunMessageType {
    BindingRequest = 0x0001,
    BindingResponse = 0x0101,
    BindingError = 0x0111,
    NatPunchPing = 0x0002,
    NatPunchPong = 0x0102,
}

/// Exact 64-Byte L1 cache aligned STUN Binding Wire Packet.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StunPacket64 {
    pub magic_cookie: u32,        // 4B: STUN_MAGIC_COOKIE (0x2112A442)
    pub msg_type: u16,            // 2B: StunMessageType
    pub message_length: u16,      // 2B: Payload length
    pub transaction_id: [u8; 12], // 12B: 96-bit cryptographic nonce
    pub mapped_port: u16,         // 2B: XOR-mapped port
    pub mapped_ipv4: [u8; 4],     // 4B: XOR-mapped IPv4
    pub sender_peer_id: u8,       // 1B: Sending peer index (0..15)
    pub receiver_peer_id: u8,     // 1B: Target peer index (0..15)
    pub sequence: u16,            // 2B: Packet sequence counter
    pub rtt_echo_micros: u32,     // 4B: Sender microsecond timestamp
    pub _padding: [u8; 28],       // 28B: Exact padding to 64 bytes
}

const _: () = assert!(size_of::<StunPacket64>() == 64);
const _: () = assert!(align_of::<StunPacket64>() == 64);

impl Default for StunPacket64 {
    fn default() -> Self {
        Self {
            magic_cookie: STUN_MAGIC_COOKIE,
            msg_type: StunMessageType::BindingRequest as u16,
            message_length: 0,
            transaction_id: [0u8; 12],
            mapped_port: 0,
            mapped_ipv4: [0; 4],
            sender_peer_id: 0,
            receiver_peer_id: 0,
            sequence: 0,
            rtt_echo_micros: 0,
            _padding: [0u8; 28],
        }
    }
}

impl StunPacket64 {
    /// Encodes XOR-MAPPED-ADDRESS attributes according to RFC 5389.
    pub fn encode_xor_mapped(&mut self, port: u16, ip: [u8; 4]) {
        let cookie_hi = (STUN_MAGIC_COOKIE >> 16) as u16;
        self.mapped_port = port ^ cookie_hi;

        let ip_u32 = u32::from_be_bytes(ip);
        let xored_ip = (ip_u32 ^ STUN_MAGIC_COOKIE).to_be_bytes();
        self.mapped_ipv4 = xored_ip;
    }

    /// Decodes XOR-MAPPED-ADDRESS attributes according to RFC 5389.
    pub fn decode_xor_mapped(&self) -> (u16, [u8; 4]) {
        let cookie_hi = (STUN_MAGIC_COOKIE >> 16) as u16;
        let real_port = self.mapped_port ^ cookie_hi;

        let xored_u32 = u32::from_be_bytes(self.mapped_ipv4);
        let real_ip = (xored_u32 ^ STUN_MAGIC_COOKIE).to_be_bytes();
        (real_port, real_ip)
    }

    /// Serializes to an exact 64-byte slice.
    pub fn to_bytes(&self) -> [u8; 64] {
        unsafe { core::mem::transmute_copy(self) }
    }

    /// Deserializes from a 64-byte slice. Returns None if magic cookie is invalid.
    pub fn from_bytes(bytes: &[u8; 64]) -> Option<Self> {
        let pkt: Self = unsafe { core::mem::transmute_copy(bytes) };
        if pkt.magic_cookie == STUN_MAGIC_COOKIE {
            Some(pkt)
        } else {
            None
        }
    }
}

/// Full-Mesh NAT Traversal Session Coordinator for up to 16 peers.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct NatTraversalSession<const MAX_PEERS: usize = 16> {
    pub local_peer_id: u8,
    pub active_peer_mask: u16,
    pub connected_mask: u16,
    pub total_probes_sent: u32,
    pub total_probes_received: u32,
    pub local_candidate: NatCandidate64,
    pub peer_candidates: [NatCandidate64; MAX_PEERS],
}

const _: () = assert!(size_of::<NatTraversalSession<16>>() == 64 + 64 * 16 + 64);
const _: () = assert!(align_of::<NatTraversalSession<16>>() == 64);

impl<const MAX_PEERS: usize> NatTraversalSession<MAX_PEERS> {
    pub fn new(local_peer_id: u8, local_candidate: NatCandidate64) -> Self {
        Self {
            local_peer_id,
            active_peer_mask: 0,
            connected_mask: 0,
            total_probes_sent: 0,
            total_probes_received: 0,
            local_candidate,
            peer_candidates: [NatCandidate64::default(); MAX_PEERS],
        }
    }

    /// Registers a peer's candidate information and marks them for connectivity checks.
    pub fn add_peer_candidate(&mut self, peer_id: usize, candidate: NatCandidate64) {
        if peer_id < MAX_PEERS && peer_id != self.local_peer_id as usize {
            self.peer_candidates[peer_id] = candidate;
            self.peer_candidates[peer_id].state = NatConnectionState::Probing;
            self.active_peer_mask |= 1 << peer_id;
        }
    }

    /// Computes candidate pair priority under RFC 8445: (2^32 * min(G, D)) + (2 * max(G, D)) + (G > D ? 1 : 0)
    pub fn compute_pair_priority(local_prio: u32, remote_prio: u32) -> u64 {
        let g = local_prio as u64;
        let d = remote_prio as u64;
        let min_p = g.min(d);
        let max_p = g.max(d);
        (min_p << 32) + (max_p << 1) + if g > d { 1 } else { 0 }
    }

    /// Generates a STUN binding request packet targeted at a specific peer.
    pub fn create_probe_ping(&mut self, peer_id: u8, seq: u16, timestamp_micros: u32) -> StunPacket64 {
        self.total_probes_sent += 1;
        let mut pkt = StunPacket64::default();
        pkt.msg_type = StunMessageType::NatPunchPing as u16;
        pkt.sender_peer_id = self.local_peer_id;
        pkt.receiver_peer_id = peer_id;
        pkt.sequence = seq;
        pkt.rtt_echo_micros = timestamp_micros;
        pkt.encode_xor_mapped(self.local_candidate.port, self.local_candidate.ip_v4);
        pkt
    }

    /// Processes an incoming STUN probe packet. If ping, returns pong response.
    pub fn process_stun_packet(
        &mut self,
        pkt: &StunPacket64,
        current_time_micros: u32,
    ) -> Option<StunPacket64> {
        self.total_probes_received += 1;
        let sender = pkt.sender_peer_id as usize;
        if sender >= MAX_PEERS {
            return None;
        }

        match pkt.msg_type {
            t if t == StunMessageType::NatPunchPing as u16 => {
                // Incoming Ping: record connectivity and construct Pong
                let (public_port, public_ip) = pkt.decode_xor_mapped();
                self.peer_candidates[sender].port = public_port;
                self.peer_candidates[sender].ip_v4 = public_ip;
                self.peer_candidates[sender].state = NatConnectionState::Connected;
                self.connected_mask |= 1 << sender;

                let mut pong = StunPacket64::default();
                pong.msg_type = StunMessageType::NatPunchPong as u16;
                pong.sender_peer_id = self.local_peer_id;
                pong.receiver_peer_id = pkt.sender_peer_id;
                pong.sequence = pkt.sequence;
                pong.rtt_echo_micros = pkt.rtt_echo_micros; // Echo back for RTT calculation
                pong.encode_xor_mapped(self.local_candidate.port, self.local_candidate.ip_v4);
                Some(pong)
            }
            t if t == StunMessageType::NatPunchPong as u16 => {
                // Incoming Pong: calculate round-trip latency
                if current_time_micros >= pkt.rtt_echo_micros {
                    self.peer_candidates[sender].rtt_micros = current_time_micros - pkt.rtt_echo_micros;
                }
                self.peer_candidates[sender].state = NatConnectionState::Connected;
                self.connected_mask |= 1 << sender;
                None
            }
            _ => None,
        }
    }

    /// Returns true if a specific peer has successfully established a direct hole-punched P2P connection.
    pub fn is_peer_connected(&self, peer_id: usize) -> bool {
        if peer_id < MAX_PEERS {
            (self.connected_mask & (1 << peer_id)) != 0
        } else {
            false
        }
    }

    /// Returns true if all active registered peers have established direct hole-punched P2P connectivity.
    pub fn all_peers_connected(&self) -> bool {
        self.active_peer_mask != 0 && (self.connected_mask & self.active_peer_mask) == self.active_peer_mask
    }
}
