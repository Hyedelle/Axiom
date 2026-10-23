//! Deterministic Input Replay Recording & Ultra-Compact Byte-Stream Serialization.
//!
//! Zero-float, 64-byte L1 cache aligned deterministic replay container with
//! run-length input compression, periodic state hash validation, and sub-millisecond serialization.

extern crate alloc;

use alloc::vec::Vec;

pub const REPLAY_MAGIC: [u8; 4] = *b"AXRP";
pub const REPLAY_VERSION: u32 = 1;
pub const DEFAULT_KEYFRAME_INTERVAL: u16 = 60; // 1 keyframe per second at 60 FPS

/// Error types occurring during replay serialization or playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    InvalidMagic,
    UnsupportedVersion(u32),
    CorruptedHeader,
    UnexpectedEof,
    StateHashMismatch { frame: u32, expected: u64, actual: u64 },
}

/// 64-byte L1 cache aligned replay file header.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayHeader {
    pub match_seed: u64,
    pub final_state_hash: u64,
    pub magic: [u8; 4],
    pub version: u32,
    pub total_frames: u32,
    pub uncompressed_size: u32,
    pub compressed_size: u32,
    pub keyframe_interval: u16,
    pub player_id: u8,
    pub opponent_id: u8,
    pub winner_id: u8,
    pub final_score_p0: u8,
    pub final_score_p1: u8,
    pub _padding: [u8; 21],
}

impl Default for ReplayHeader {
    fn default() -> Self {
        Self {
            match_seed: 0,
            final_state_hash: 0,
            magic: REPLAY_MAGIC,
            version: REPLAY_VERSION,
            total_frames: 0,
            uncompressed_size: 0,
            compressed_size: 0,
            keyframe_interval: DEFAULT_KEYFRAME_INTERVAL,
            player_id: 0,
            opponent_id: 1,
            winner_id: 0xFF,
            final_score_p0: 0,
            final_score_p1: 0,
            _padding: [0; 21],
        }
    }
}

/// Run-length encoded input run: (packed_inputs: u8, repeat_count: u8).
/// Packed inputs: lower 4 bits = p0_action, upper 4 bits = p1_action.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RleInputRun {
    pub packed_inputs: u8,
    pub repeat_count: u8,
}

impl RleInputRun {
    #[inline(always)]
    pub fn new(p0_action: u8, p1_action: u8, count: u8) -> Self {
        let packed = (p0_action & 0x0F) | ((p1_action & 0x0F) << 4);
        Self {
            packed_inputs: packed,
            repeat_count: count,
        }
    }

    #[inline(always)]
    pub fn p0_action(&self) -> u8 {
        self.packed_inputs & 0x0F
    }

    #[inline(always)]
    pub fn p1_action(&self) -> u8 {
        (self.packed_inputs >> 4) & 0x0F
    }
}

/// Fully deterministic, ultra-compact mecha boxing replay recording container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicReplay {
    pub header: ReplayHeader,
    pub keyframe_hashes: Vec<u64>,
    pub input_runs: Vec<RleInputRun>,
}

impl Default for DeterministicReplay {
    fn default() -> Self {
        Self::new(0, 0, 1)
    }
}

impl DeterministicReplay {
    /// Creates a new replay recorder session.
    pub fn new(match_seed: u64, player_id: u8, opponent_id: u8) -> Self {
        let mut header = ReplayHeader::default();
        header.match_seed = match_seed;
        header.player_id = player_id;
        header.opponent_id = opponent_id;

        Self {
            header,
            keyframe_hashes: Vec::new(),
            input_runs: Vec::new(),
        }
    }

    /// Appends a single frame's inputs and checks for periodic keyframe hash recording.
    pub fn record_frame(&mut self, p0_action: u8, p1_action: u8, current_state_hash: u64) {
        let current_frame = self.header.total_frames;
        let interval = self.header.keyframe_interval as u32;

        // Record periodic keyframe state hash
        if interval > 0 && current_frame % interval == 0 {
            self.keyframe_hashes.push(current_state_hash);
        }

        let packed = (p0_action & 0x0F) | ((p1_action & 0x0F) << 4);

        // Run-length encoding: if same inputs as last run and count < 255, increment
        if let Some(last) = self.input_runs.last_mut() {
            if last.packed_inputs == packed && last.repeat_count < 255 {
                last.repeat_count += 1;
                self.header.total_frames += 1;
                self.header.final_state_hash = current_state_hash;
                return;
            }
        }

        // Otherwise push a new run
        self.input_runs.push(RleInputRun {
            packed_inputs: packed,
            repeat_count: 1,
        });

        self.header.total_frames += 1;
        self.header.final_state_hash = current_state_hash;
    }

    /// Finalizes the replay header with winner and final scores.
    pub fn finalize(&mut self, winner_id: u8, score_p0: u8, score_p1: u8) {
        self.header.winner_id = winner_id;
        self.header.final_score_p0 = score_p0;
        self.header.final_score_p1 = score_p1;
        self.header.uncompressed_size = self.header.total_frames * 2;
        self.header.compressed_size = (self.input_runs.len() * 2) as u32;
    }

    /// Serializes this replay container into an ultra-compact binary byte stream.
    pub fn serialize(&self) -> Vec<u8> {
        let header_size = core::mem::size_of::<ReplayHeader>();
        let num_keyframes = self.keyframe_hashes.len();
        let keyframes_size = num_keyframes * 8;
        let runs_size = self.input_runs.len() * 2;
        let total_size = header_size + 4 + keyframes_size + 4 + runs_size;

        let mut buf = Vec::with_capacity(total_size);

        // 1. 64-byte Header
        let header_ptr = &self.header as *const ReplayHeader as *const u8;
        let header_bytes = unsafe { core::slice::from_raw_parts(header_ptr, header_size) };
        buf.extend_from_slice(header_bytes);

        // 2. Keyframe hashes count & values
        buf.extend_from_slice(&(num_keyframes as u32).to_le_bytes());
        for &h in &self.keyframe_hashes {
            buf.extend_from_slice(&h.to_le_bytes());
        }

        // 3. RLE input runs count & byte pairs
        buf.extend_from_slice(&(self.input_runs.len() as u32).to_le_bytes());
        for run in &self.input_runs {
            buf.push(run.packed_inputs);
            buf.push(run.repeat_count);
        }

        buf
    }

    /// Deserializes a binary byte stream into a `DeterministicReplay` container.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, ReplayError> {
        let header_size = core::mem::size_of::<ReplayHeader>();
        if bytes.len() < header_size + 8 {
            return Err(ReplayError::UnexpectedEof);
        }

        // 1. Read and validate header
        let mut header = ReplayHeader::default();
        let header_dest = &mut header as *mut ReplayHeader as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), header_dest, header_size);
        }

        if header.magic != REPLAY_MAGIC {
            return Err(ReplayError::InvalidMagic);
        }
        if header.version != REPLAY_VERSION {
            return Err(ReplayError::UnsupportedVersion(header.version));
        }

        let mut offset = header_size;

        // 2. Read keyframes
        if offset + 4 > bytes.len() {
            return Err(ReplayError::UnexpectedEof);
        }
        let num_keyframes = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        if offset + num_keyframes * 8 > bytes.len() {
            return Err(ReplayError::UnexpectedEof);
        }
        let mut keyframe_hashes = Vec::with_capacity(num_keyframes);
        for _ in 0..num_keyframes {
            let h = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            keyframe_hashes.push(h);
            offset += 8;
        }

        // 3. Read RLE input runs
        if offset + 4 > bytes.len() {
            return Err(ReplayError::UnexpectedEof);
        }
        let num_runs = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        if offset + num_runs * 2 > bytes.len() {
            return Err(ReplayError::UnexpectedEof);
        }
        let mut input_runs = Vec::with_capacity(num_runs);
        for _ in 0..num_runs {
            let packed = bytes[offset];
            let repeat = bytes[offset + 1];
            input_runs.push(RleInputRun {
                packed_inputs: packed,
                repeat_count: repeat,
            });
            offset += 2;
        }

        Ok(Self {
            header,
            keyframe_hashes,
            input_runs,
        })
    }

    /// Decompresses all recorded inputs into a flat vector of `(p0_action, p1_action)` pairs.
    pub fn decompress_inputs(&self) -> Vec<(u8, u8)> {
        let mut result = Vec::with_capacity(self.header.total_frames as usize);
        for run in &self.input_runs {
            let p0 = run.p0_action();
            let p1 = run.p1_action();
            for _ in 0..run.repeat_count {
                result.push((p0, p1));
            }
        }
        result
    }

    /// Verifies a simulation frame against the recorded keyframe hash (if one exists for this frame).
    pub fn verify_keyframe(&self, frame: u32, actual_hash: u64) -> Result<(), ReplayError> {
        let interval = self.header.keyframe_interval as u32;
        if interval == 0 || frame % interval != 0 {
            return Ok(());
        }

        let idx = (frame / interval) as usize;
        if let Some(&expected) = self.keyframe_hashes.get(idx) {
            if expected != actual_hash {
                return Err(ReplayError::StateHashMismatch {
                    frame,
                    expected,
                    actual: actual_hash,
                });
            }
        }
        Ok(())
    }

    /// Compression ratio achieved (percentage of original size saved, 0..100).
    pub fn compression_savings_percent(&self) -> u32 {
        let raw = self.header.uncompressed_size;
        let comp = self.header.compressed_size;
        if raw == 0 || comp >= raw {
            0
        } else {
            (((raw - comp) as u64 * 100) / raw as u64) as u32
        }
    }
}
