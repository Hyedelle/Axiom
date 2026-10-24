//! Unit tests for Axiom 320 Aerodynamics, Drag, and Vortex-Induced Vibration (VIV) Engine.

use axiom_core::aerodynamics::*;

#[test]
fn test_aero_slab_sizes_and_alignments() {
    assert_eq!(core::mem::size_of::<AeroBodySlab64>(), 64);
    assert_eq!(core::mem::align_of::<AeroBodySlab64>(), 64);

    assert_eq!(core::mem::size_of::<AirflowGridSlab64>(), 64);
    assert_eq!(core::mem::align_of::<AirflowGridSlab64>(), 64);

    assert_eq!(core::mem::size_of::<AeroEventReport>(), 40);
}

#[test]
fn test_aero_event_report_lossless_roundtrip() {
    let report = AeroEventReport {
        pos_xyz: [1500, -250, 4200],
        rel_vel_xyz: [25000, 0, -3200],
        drag_force_n: 1420,
        viv_force_n: -380,
        shedding_freq_q8: 10240, // 40.0 Hz
        entity_id: 3,
        body_id: 7,
        flags: AERO_FLAG_LOCK_IN | AERO_FLAG_HIGH_SPEED,
        body_type: AERO_BODY_CYLINDER,
        reserved: [0xAA, 0x55],
    };

    let encoded = report.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = AeroEventReport::decode(&encoded);
    assert_eq!(report, decoded);
}

#[test]
fn test_aero_drag_force_calculation() {
    let mut body = AeroBodySlab64::new(
        1,
        AERO_BODY_CYLINDER,
        50_000, // 50,000 mm^2 (0.05 m^2)
        50_000, // 50 kg
        100,    // 100 mm diameter
        52428,  // Cd ~ 0.8 in Q16.16
    );
    // Relative speed: 20,000 mm/s (20 m/s = 72 km/h)
    body.vel_xyz = [20_000, 0, 0];

    let wind = AirflowGridSlab64::new(0, [0, 0, 0]);

    let (drag_n, _viv_n) = compute_aerodynamic_forces(&mut body, &wind);
    assert!(drag_n > 0, "Drag force should be positive: {}", drag_n);
    assert_eq!(body.drag_force_n, drag_n);
}

#[test]
fn test_von_karman_shedding_and_phase() {
    let mut body = AeroBodySlab64::new(
        2,
        AERO_BODY_CYLINDER,
        40_000, // 0.04 m^2
        30_000,
        100, // 100 mm
        65535,
    );
    body.vel_xyz = [20_000, 0, 0]; // 20 m/s

    let wind = AirflowGridSlab64::new(0, [0, 0, 0]);

    let initial_phase = body.viv_phase;
    compute_aerodynamic_forces(&mut body, &wind);

    // At 20 m/s, D=100 mm, St~0.20:
    // f_s = (0.2 * 20000) / 100 = 40 Hz
    // In Q8.8: 40 * 256 = 10240
    let freq_hz = body.shedding_freq_q8 as f32 / 256.0;
    assert!((freq_hz - 40.0).abs() < 1.0, "Frequency should be ~40 Hz, got {}", freq_hz);
    assert_ne!(body.viv_phase, initial_phase, "Phase should advance");
}

#[test]
fn test_lock_in_resonance_amplification() {
    let mut body = AeroBodySlab64::new(
        3,
        AERO_BODY_CYLINDER,
        60_000,
        40_000,
        100, // 100 mm diameter
        52428,
    );
    // 20 m/s gives ~40 Hz shedding frequency (within 25~45 Hz lock-in band)
    body.vel_xyz = [20_000, 0, 0];
    let wind = AirflowGridSlab64::new(0, [0, 0, 0]);

    compute_aerodynamic_forces(&mut body, &wind);
    assert!(
        (body.flags & AERO_FLAG_LOCK_IN) != 0,
        "Body should trigger Lock-in resonance flag in 25~45 Hz band"
    );

    // Now test out of lock-in band (e.g. 5 m/s -> f_s = 10 Hz)
    body.vel_xyz = [5_000, 0, 0];
    compute_aerodynamic_forces(&mut body, &wind);
    assert_eq!(
        body.flags & AERO_FLAG_LOCK_IN,
        0,
        "Body should not be in Lock-in resonance at 10 Hz"
    );
}

#[test]
fn test_mecha_aerodynamics_engine_step_and_deceleration() {
    let mut engine = MechaAerodynamicsEngine::<16, 4>::new();

    let mut body = AeroBodySlab64::new(
        1,
        AERO_BODY_CYLINDER,
        50_000,
        20_000, // 20 kg
        120,
        52428,
    );
    body.vel_xyz = [35_000, 0, 0]; // 35 m/s high-speed punch
    body.pos_xyz = [0, 0, 1000];

    let cell = AirflowGridSlab64::new(0, [0, 0, 0]);
    engine.register_cell(cell);
    let idx = engine.register_body(body).expect("Should register body");

    let initial_vx = engine.bodies[idx].vel_xyz[0];
    let (eval_count, _lock_ins) = engine.step_simulation();

    assert_eq!(eval_count, 1);
    assert!(
        engine.bodies[idx].vel_xyz[0] < initial_vx,
        "Drag should decelerate forward velocity: before={}, after={}",
        initial_vx,
        engine.bodies[idx].vel_xyz[0]
    );
    assert!(engine.bodies[idx].pos_xyz[0] > 0, "Position should integrate forward");
    assert!(engine.total_drag_energy_j > 0, "Drag energy should accumulate");
}

#[test]
fn test_aerodynamics_consensus_hash() {
    let mut engine1 = MechaAerodynamicsEngine::<8, 2>::new();
    let mut engine2 = MechaAerodynamicsEngine::<8, 2>::new();

    let body = AeroBodySlab64::new(1, AERO_BODY_CYLINDER, 40_000, 25_000, 100, 52428);
    let cell = AirflowGridSlab64::new(0, [5000, 0, 0]); // 5 m/s headwind

    engine1.register_cell(cell);
    engine1.register_body(body);

    engine2.register_cell(cell);
    engine2.register_body(body);

    for _ in 0..10 {
        engine1.step_simulation();
        engine2.step_simulation();
    }

    let hash1 = engine1.compute_consensus_hash();
    let hash2 = engine2.compute_consensus_hash();

    assert_eq!(hash1, hash2, "Consensus hash must match exactly across identical runs");
}
