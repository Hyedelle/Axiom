//! Verification suite for Axiom 310-Series Mecha Pneumatic/Hydraulic Actuators & Artificial Muscle Dynamics.

use core::mem::{align_of, size_of};
use axiom_core::actuator::{
    compute_actuator_thrust, emit_actuator_to_event_bus, solve_actuator_step, step_chamber_pressure,
    ActuatorEventReport, ActuatorSlab64, HydraulicChamberSlab64, MechaActuatorEngine,
    ACTUATOR_FLAG_RELIEF, ACTUATOR_TYPE_HYDRAULIC, CHAMBER_FLAG_PUMPING, CHAMBER_FLAG_RELIEF,
    EVENT_ACTUATOR_PRESSURE,
};
use axiom_core::event_bus::DeterministicEventBus;

#[test]
fn test_actuator_slab_and_chamber_alignment_64() {
    assert_eq!(size_of::<ActuatorSlab64>(), 64, "ActuatorSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<ActuatorSlab64>(), 64, "ActuatorSlab64 must align to 64 bytes");

    assert_eq!(size_of::<HydraulicChamberSlab64>(), 64, "HydraulicChamberSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<HydraulicChamberSlab64>(), 64, "HydraulicChamberSlab64 must align to 64 bytes");

    assert_eq!(size_of::<ActuatorEventReport>(), 40, "ActuatorEventReport must be exactly 40 bytes");
}

#[test]
fn test_actuator_thrust_and_damping_calculation() {
    // 15,000 kPa (150 bar) * 2000 mm^2 = 30,000,000 mN = 30,000 N
    let mut act = ActuatorSlab64::new_hydraulic(
        0, 1,
        (0, 0, 0),
        (0, 300, 0),
        2000,
        200,
        50_000,
    );
    act.pressure_kpa = 15_000;

    let thrust_static = compute_actuator_thrust(&act, 0);
    assert_eq!(thrust_static, 30_000, "Static thrust must be 30,000 N");

    // Damping resistance when extending at 50 mm/tick
    // Damping: (16384 * 50 * 1000) / 65536 = 12,500 mN = 12.5 N
    let thrust_dynamic = compute_actuator_thrust(&act, 50);
    assert_eq!(thrust_dynamic, 29_987, "Dynamic thrust must account for fluid damping");
}

#[test]
fn test_hydraulic_chamber_bulk_modulus_and_relief() {
    let mut ch = HydraulicChamberSlab64::new(0, 1, 1_000_000, 15_000, 1_500_000);
    assert_eq!(ch.pressure_kpa, 15_000);

    // Inflow volume compression (+50,000 mm^3)
    step_chamber_pressure(&mut ch, 50_000);
    assert!(ch.pressure_kpa > 15_000, "Compression must increase chamber pressure");

    // Extreme compression pushing pressure past relief threshold (21,000 kPa) with pump stopped
    ch.flags &= !CHAMBER_FLAG_PUMPING;
    ch.pressure_kpa = 25_000;
    step_chamber_pressure(&mut ch, 0);
    assert!((ch.flags & CHAMBER_FLAG_RELIEF) != 0, "Overpressure must trigger safety relief valve");
    assert!(ch.pressure_kpa < 25_000, "Relief valve must bleed excess pressure");
}

#[test]
fn test_mckibben_pam_artificial_muscle_contraction() {
    let mut pam = ActuatorSlab64::new_mckibben_pam(
        0, 1,
        (0, 0, 0),
        (0, 400, 0),
        1000,
        1000,
    );
    pam.pressure_kpa = 600; // 6 bar
    pam.rest_len_mm = 400;

    // At rest length (L == L0): 0 tension (no initial strain)
    pam.current_len_mm = 400;
    let t0 = compute_actuator_thrust(&pam, 0);
    // At L0, stroke = 0 -> eff_factor = 65536 -> F = 600 * 1000 / 1000 = 600 N
    assert_eq!(t0, 600, "At L0 nominal pressure produces baseline isometric tension");

    // Contracted to 350 mm (stroke = 50 mm):
    pam.current_len_mm = 350;
    let t_contracted = compute_actuator_thrust(&pam, 0);
    assert!(t_contracted > 0 && t_contracted < 600, "Contracted PAM has reduced force output");

    // Stretched past L0 (L > L0): PAM cannot push
    pam.current_len_mm = 450;
    let t_stretched = compute_actuator_thrust(&pam, 0);
    assert_eq!(t_stretched, 0, "PAM cannot generate compressive pushing force");
}

#[test]
fn test_catastrophic_burst_rupture() {
    let mut act = ActuatorSlab64::new_hydraulic(
        0, 1,
        (0, 0, 0),
        (0, 300, 0),
        2000,
        200,
        40_000,
    );
    act.pressure_kpa = 42_000; // Exceeds 40,000 kPa burst limit

    let (thrust, _relief, burst) = solve_actuator_step(&mut act, None);
    assert!(burst, "Exceeding burst limit must trigger catastrophic pipe rupture");
    assert!(act.is_burst(), "Actuator must be flagged as burst");
    assert_eq!(act.pressure_kpa, 0, "Ruptured cylinder must immediately lose internal pressure");
    assert_eq!(thrust, 0, "Ruptured actuator produces 0 thrust");
}

#[test]
fn test_actuator_event_report_encode_decode_roundtrip() {
    let report = ActuatorEventReport {
        entity_id: 42,
        actuator_id: 3,
        chamber_id: 1,
        flags: ACTUATOR_FLAG_RELIEF,
        actuator_type: ACTUATOR_TYPE_HYDRAULIC,
        pressure_kpa: 18_500,
        thrust_n: 37_000,
        current_len_mm: 312,
        target_len_mm: 350,
        pos_x: 1200,
        pos_y: -450,
        pos_z: 80,
        impulse_m_ns: 75_000,
        _reserved: [0; 4],
    };

    let encoded = report.encode();
    assert_eq!(encoded.len(), 40);
    let decoded = ActuatorEventReport::decode(&encoded);
    assert_eq!(decoded, report, "40-byte report must roundtrip losslessly");

    // Event bus publish test
    let mut bus = DeterministicEventBus::<64>::new();
    let published = emit_actuator_to_event_bus(&report, 100, &mut bus);
    assert!(published, "Must publish to deterministic event bus");
    let ev = bus.peek().unwrap();
    assert_eq!(ev.event_type, EVENT_ACTUATOR_PRESSURE);
}

#[test]
fn test_engine_simulation_and_consensus_hash() {
    let mut engine = MechaActuatorEngine::<8, 2>::new();

    let ch = HydraulicChamberSlab64::new(0, 1, 1_000_000, 15_000, 1_500_000);
    let ch_idx = engine.add_chamber(ch).unwrap();

    let mut act0 = ActuatorSlab64::new_hydraulic(0, 1, (0, 0, 0), (0, 300, 0), 2000, 150, 45_000);
    act0.chamber_idx = ch_idx as u8;
    act0.target_len_mm = 350;
    engine.add_actuator(act0);

    let h0 = engine.compute_consensus_hash();
    assert_ne!(h0, 0);

    // Step simulation
    let (active, bursts) = engine.step_simulation();
    assert_eq!(active, 1);
    assert_eq!(bursts, 0);
    assert!(engine.peak_thrust_n > 0, "Simulation step must generate mechanical thrust");

    let h1 = engine.compute_consensus_hash();
    assert_ne!(h0, h1, "Advancing simulation must deterministically evolve consensus hash");

    // 100-step determinism run
    for _ in 0..100 {
        engine.step_simulation();
    }
    let h_end = engine.compute_consensus_hash();
    assert_ne!(h1, h_end);
}
