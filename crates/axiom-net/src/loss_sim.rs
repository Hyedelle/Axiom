//! Simulated Network Pipeline with Deterministic Packet Loss and Latency (T-161).

use crate::packet::NetPacket64;
use axiom_core::Rng;

/// Network simulation channel for verifying loss-recovery and rollback resilience.
#[derive(Debug, Clone)]
pub struct NetworkLossSimulator {
    pub loss_permille: u32,
    pub latency_frames: u32,
    pub rng: Rng,
    pub in_flight: Vec<(usize, u32, NetPacket64)>,
    pub dropped_packets: u32,
    pub delivered_packets: u32,
}

impl NetworkLossSimulator {
    pub fn new(loss_permille: u32, latency_frames: u32, seed: u64) -> Self {
        Self {
            loss_permille,
            latency_frames,
            rng: Rng::with_seed(seed),
            in_flight: Vec::new(),
            dropped_packets: 0,
            delivered_packets: 0,
        }
    }

    /// Submits a packet to a specific destination peer on the simulated link.
    pub fn send_to(&mut self, dst: usize, pkt: NetPacket64, current_frame: u32) {
        let roll = self.rng.range_u32(0, 1000);
        if roll < self.loss_permille {
            self.dropped_packets += 1;
            return; // Dropped by network loss
        }

        let deliver_at = current_frame + self.latency_frames;
        self.in_flight.push((dst, deliver_at, pkt));
    }

    /// Backwards compatible send (dst = 0).
    pub fn send(&mut self, pkt: NetPacket64, current_frame: u32) {
        self.send_to(0, pkt, current_frame);
    }

    /// Polls packets scheduled for delivery up to `current_frame`.
    pub fn poll_deliveries(&mut self, current_frame: u32) -> Vec<(usize, NetPacket64)> {
        let mut delivered = Vec::new();
        let mut i = 0;
        while i < self.in_flight.len() {
            if self.in_flight[i].1 <= current_frame {
                let (dst, _, pkt) = self.in_flight.swap_remove(i);
                delivered.push((dst, pkt));
                self.delivered_packets += 1;
            } else {
                i += 1;
            }
        }
        delivered
    }
}
