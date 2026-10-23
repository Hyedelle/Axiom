//! 64-Byte L1 Cache Aligned Distributed Wire Packet Protocol (T-161).
//!
//! Provides zero-allocation, zero-copy wire serialization for 16-player
//! deterministic rollback synchronization across WebRTC, UDP, and raw sockets.

pub const NET_WIRE_MAGIC: u16 = 0x584E; // 'X' 'N' (Axiom Net)
pub const MAX_PEERS: usize = 16;
pub const INPUT_WINDOW_FRAMES: usize = 8;

/// Exact 64-byte L1 cache aligned distributed rollback network packet.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetPacket64 {
    pub magic: u16,                           // 2B: Protocol identifier (0x584E)
    pub sender_id: u8,                        // 1B: Peer index (0..15)
    pub packet_flags: u8,                     // 1B: 0x01=Keyframe, 0x02=DesyncAlert, 0x04=Ping, 0x08=Pong
    pub sequence: u32,                        // 4B: Monotonic packet counter
    pub current_frame: u32,                   // 4B: Local simulation frame
    pub ack_frame: u32,                       // 4B: Highest globally confirmed frame
    pub state_hash: u64,                      // 8B: 64-bit FNV-1a state consensus hash
    pub rtt_timestamp: u32,                   // 4B: Timestamp for RTT roundtrip calculation
    pub loss_permille: u16,                   // 2B: Local observed packet loss (0..1000)
    pub active_peers_mask: u16,               // 2B: 16-bit bitmask of online peers
    pub window_inputs: [u16; INPUT_WINDOW_FRAMES], // 16B: 8-frame sliding window of redundant inputs
    pub peer_acks: [u8; MAX_PEERS],           // 16B: Frame offsets acknowledged per peer
}

const _: () = assert!(core::mem::size_of::<NetPacket64>() == 64);
const _: () = assert!(core::mem::align_of::<NetPacket64>() == 64);

impl Default for NetPacket64 {
    fn default() -> Self {
        Self {
            magic: NET_WIRE_MAGIC,
            sender_id: 0,
            packet_flags: 0,
            sequence: 0,
            current_frame: 0,
            ack_frame: 0,
            state_hash: 0,
            rtt_timestamp: 0,
            loss_permille: 0,
            active_peers_mask: 1, // Peer 0 active by default
            window_inputs: [0u16; INPUT_WINDOW_FRAMES],
            peer_acks: [0u8; MAX_PEERS],
        }
    }
}

impl NetPacket64 {
    pub const SIZE: usize = 64;

    /// Serializes packet to exact 64-byte array with zero allocations.
    #[inline(always)]
    pub fn to_bytes(&self) -> [u8; 64] {
        unsafe { core::mem::transmute_copy(self) }
    }

    /// Deserializes packet from 64-byte slice with magic verification.
    #[inline(always)]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let pkt: Self = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) };
        if pkt.magic != NET_WIRE_MAGIC {
            return None;
        }
        Some(pkt)
    }

    /// Checks if a specific peer is marked active in this packet.
    #[inline(always)]
    pub fn is_peer_active(&self, peer_id: usize) -> bool {
        if peer_id >= MAX_PEERS {
            return false;
        }
        (self.active_peers_mask & (1 << peer_id)) != 0
    }
}
