//! Unit & Integration Tests for Deterministic Replay Recording & Ultra-Compact Serialization.

use axiom_core::replay::{
    DeterministicReplay, ReplayError, ReplayHeader, REPLAY_MAGIC, REPLAY_VERSION,
};

#[test]
fn test_replay_header_geometry_and_alignment() {
    assert_eq!(core::mem::size_of::<ReplayHeader>(), 64);
    assert_eq!(core::mem::align_of::<ReplayHeader>(), 64);

    let header = ReplayHeader::default();
    assert_eq!(header.magic, REPLAY_MAGIC);
    assert_eq!(header.version, REPLAY_VERSION);
}

#[test]
fn test_replay_recording_and_rle_compression() {
    let mut replay = DeterministicReplay::new(0x1234_5678_9ABC_DEF0, 0, 1);

    // Record 60 frames of holding Guard (p0=3, p1=0)
    for f in 0..60 {
        replay.record_frame(3, 0, 0x1000 + f as u64);
    }
    // Record 120 frames of trading Jabs (p0=4, p1=4)
    for f in 60..180 {
        replay.record_frame(4, 4, 0x2000 + f as u64);
    }
    // Record 30 frames of Idle (p0=0, p1=0)
    for f in 180..210 {
        replay.record_frame(0, 0, 0x3000 + f as u64);
    }

    replay.finalize(0, 100, 75);

    assert_eq!(replay.header.total_frames, 210);
    // 3 distinct action periods -> exactly 3 RLE input runs!
    assert_eq!(replay.input_runs.len(), 3);
    assert_eq!(replay.input_runs[0].repeat_count, 60);
    assert_eq!(replay.input_runs[1].repeat_count, 120);
    assert_eq!(replay.input_runs[2].repeat_count, 30);

    // Uncompressed was 210 * 2 = 420 bytes, compressed is 3 * 2 = 6 bytes!
    let savings = replay.compression_savings_percent();
    assert!(savings >= 90, "Savings was {}%", savings);

    // Verify keyframe hashes recorded at frames 0, 60, 120, 180
    assert_eq!(replay.keyframe_hashes.len(), 4);
    assert_eq!(replay.keyframe_hashes[0], 0x1000);
}

#[test]
fn test_replay_serialization_deserialization_roundtrip() {
    let mut original = DeterministicReplay::new(0xABCD_1234_5678_EEEE, 2, 4);

    // Record 600 frames of dynamic mecha combat (10 seconds)
    let mut mock_hash: u64 = 0xCAFE_BABE;
    for f in 0..600 {
        let p0_act = ((f / 30) % 7) as u8;
        let p1_act = ((f / 20) % 7) as u8;
        mock_hash = mock_hash.wrapping_mul(0x100000001b3) ^ (f as u64);
        original.record_frame(p0_act, p1_act, mock_hash);
    }

    original.finalize(2, 85, 0);

    // Serialize to byte buffer
    let bytes = original.serialize();
    assert!(bytes.len() < 500, "Serialized replay size was {} bytes", bytes.len());

    // Deserialize from byte buffer
    let restored = DeterministicReplay::deserialize(&bytes).expect("deserialization successful");

    assert_eq!(original.header, restored.header);
    assert_eq!(original.keyframe_hashes, restored.keyframe_hashes);
    assert_eq!(original.input_runs, restored.input_runs);

    // Verify decompressed inputs match original 1:1
    let decompressed = restored.decompress_inputs();
    assert_eq!(decompressed.len(), 600);

    for f in 0..600 {
        let expected_p0 = ((f / 30) % 7) as u8;
        let expected_p1 = ((f / 20) % 7) as u8;
        assert_eq!(decompressed[f], (expected_p0, expected_p1));
    }
}

#[test]
fn test_replay_keyframe_desync_detection() {
    let mut replay = DeterministicReplay::new(999, 0, 1);
    for f in 0..120 {
        replay.record_frame(1, 1, 0x5000 + f as u64);
    }
    replay.finalize(0, 50, 40);

    // Frame 60 should match expected hash
    let expected_hash_60 = 0x5000 + 60;
    assert!(replay.verify_keyframe(60, expected_hash_60).is_ok());

    // Simulated divergence / desync at frame 60
    let desync_result = replay.verify_keyframe(60, 0xDEAD_BEEF);
    assert!(matches!(
        desync_result,
        Err(ReplayError::StateHashMismatch { frame: 60, expected, actual: 0xDEAD_BEEF })
            if expected == expected_hash_60
    ));
}
