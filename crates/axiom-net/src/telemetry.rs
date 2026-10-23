//! 64-Byte L1 Cache Aligned Network Telemetry & Diagnostics (T-161).

/// Exact 64-byte L1 cache aligned real-time network telemetry structure.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetTelemetry64 {
    pub total_packets_sent: u64,           // 8B: Cumulative packets sent
    pub total_packets_received: u64,       // 8B: Cumulative packets received
    pub total_rollbacks_triggered: u32,    // 4B: Count of speculative rollback rewinds
    pub total_resimulated_frames: u32,     // 4B: Cumulative frames stepped during rollback
    pub total_desyncs: u32,                // 4B: Count of detected consensus hash divergences
    pub avg_rtt_us: u32,                   // 4B: Exponential moving average RTT in microseconds
    pub max_rollback_depth: u16,           // 2B: Deepest frame rollback experienced
    pub observed_packet_loss_permille: u16,// 2B: Local observed packet loss (0..1000)
    pub active_peers: u16,                 // 2B: Count of active peer connections
    pub _pad: [u8; 26],                    // 26B: Exact padding to 64 bytes
}

const _: () = assert!(core::mem::size_of::<NetTelemetry64>() == 64);
const _: () = assert!(core::mem::align_of::<NetTelemetry64>() == 64);

impl Default for NetTelemetry64 {
    fn default() -> Self {
        Self {
            total_packets_sent: 0,
            total_packets_received: 0,
            total_rollbacks_triggered: 0,
            total_resimulated_frames: 0,
            total_desyncs: 0,
            avg_rtt_us: 0,
            max_rollback_depth: 0,
            observed_packet_loss_permille: 0,
            active_peers: 1,
            _pad: [0u8; 26],
        }
    }
}
