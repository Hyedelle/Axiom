//! Unit tests for `axiom-core::superconductor` Type I/II Quantum Levitation & Meissner Engine.

use axiom_core::superconductor::*;

#[test]
fn test_superconductor_slab_alignment_and_sizes() {
    assert_eq!(
        core::mem::size_of::<SuperconductorSlab64>(),
        64,
        "SuperconductorSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        core::mem::align_of::<SuperconductorSlab64>(),
        64,
        "SuperconductorSlab64 must be 64-byte aligned"
    );

    assert_eq!(
        core::mem::size_of::<FluxPinningSlab64>(),
        64,
        "FluxPinningSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        core::mem::align_of::<FluxPinningSlab64>(),
        64,
        "FluxPinningSlab64 must be 64-byte aligned"
    );

    assert_eq!(
        core::mem::size_of::<SuperconductorEventReport>(),
        40,
        "SuperconductorEventReport must be exactly 40 bytes POD"
    );
}

#[test]
fn test_meissner_repulsion_calculation() {
    // 100x100mm disc = 10,000 mm2
    let area_mm2 = 10_000u32;

    // 1000 mT (1 Tesla) magnetic field:
    // F_rep = (1000^2 * 10000) / 2_513_000 = 10,000,000,000 / 2,513_000 = 3,979 N
    let rep_1t = compute_meissner_repulsion_n(1000, area_mm2);
    assert!(
        rep_1t >= 3975 && rep_1t <= 3985,
        "1T Meissner repulsion on 100cm2 disc should be ~3979 N, got {}",
        rep_1t
    );

    // 0 mT field -> 0 repulsion
    assert_eq!(compute_meissner_repulsion_n(0, area_mm2), 0);

    // 500 mT (0.5 T) -> 1/4th of 1T force: ~994 N
    let rep_half = compute_meissner_repulsion_n(500, area_mm2);
    assert!(
        rep_half >= 990 && rep_half <= 1000,
        "0.5T Meissner repulsion should be ~994 N, got {}",
        rep_half
    );
}

#[test]
fn test_flux_pinning_restoring_forces() {
    let mut cell = SuperconductorSlab64::new(
        1,
        [100, 200, 300], // pos in mm
        77,              // liquid N2 temp (77 K)
        93,              // YBCO Tc (93 K)
        30_000,          // Bc2 = 30 T
        SC_TYPE_II,      // Type II
    );
    cell.vel_xyz = [10, -20, 0]; // 10 mm/s X, -20 mm/s Y

    let pin = FluxPinningSlab64::new(
        2,
        1,
        [120, 200, 300], // anchor pos: dx = 100 - 120 = -20 mm
        100,             // 100 mWb
        65536 * 100,     // stiffness k = 100 mN/mm (in Q16.16)
        65536 * 10,      // damping c = 10 mN/(mm/s) (in Q16.16)
        10_000,          // max force 10 kN
        100,             // trapped height
    );

    let forces_mn = compute_superconductor_forces(&cell, &pin);

    // X axis: dx = -20 mm, vx = 10 mm/s
    // spring = - (100 * -20) = +2000 mN
    // damping = - (10 * 10) = -100 mN
    // net X = 2000 - 100 = +1900 mN
    assert_eq!(forces_mn[0], 1900, "X restoring force should be +1900 mN");

    // Y axis: dy = 0 mm, vy = -20 mm/s
    // spring = 0
    // damping = - (10 * -20) = +200 mN
    assert_eq!(forces_mn[1], 200, "Y damping force should be +200 mN");

    // Z axis: dz = 0, vz = 0 -> 0
    assert_eq!(forces_mn[2], 0, "Z force should be 0");
}

#[test]
fn test_superconductor_quench_dynamics() {
    let mut engine = MechaSuperconductorEngine::<4, 2>::new();

    // Register a Type-II superconductor cell (Tc = 93 K, Bc2 = 30,000 mT)
    let cell = SuperconductorSlab64::new(1, [0, 0, 50], 77, 93, 30_000, SC_TYPE_II);
    let pin = FluxPinningSlab64::new(
        2,
        1,
        [0, 0, 100], // anchor at z=100
        200,
        65536 * 50,
        65536 * 5,
        5000,
        50,
    );

    engine.register_cell(cell);
    engine.register_pin(pin);

    // Normal operation at 77 K in 1,000 mT field -> Should levitate and pin
    engine.step_simulation(1000);
    assert_eq!(engine.cells[0].flags & SC_FLAG_QUENCHED, 0);
    assert_ne!(engine.cells[0].flags & SC_FLAG_FLUX_PINNED, 0);
    assert!(engine.cells[0].pinning_force_n > 0);

    // Quench condition 1: Temperature spike above Tc (100 K >= 93 K)
    engine.cells[0].temp_k = 100;
    engine.step_simulation(1000);
    assert_ne!(
        engine.cells[0].flags & SC_FLAG_QUENCHED,
        0,
        "Cell must quench when T >= Tc"
    );
    assert_eq!(
        engine.cells[0].pinning_force_n, 0,
        "Pinning force must zero out on quench"
    );
    assert_eq!(engine.quench_count_accum, 1);

    // Cool down back to 77 K -> Restores superconductivity
    engine.cells[0].temp_k = 77;
    engine.step_simulation(1000);
    assert_eq!(
        engine.cells[0].flags & SC_FLAG_QUENCHED,
        0,
        "Cell must recover when cooled"
    );

    // Quench condition 2: Massive magnetic pulse exceeding Bc2 (35,000 mT >= 30,000 mT)
    engine.step_simulation(35_000);
    assert_ne!(
        engine.cells[0].flags & SC_FLAG_QUENCHED,
        0,
        "Cell must quench when B >= Bc2"
    );
    assert_eq!(engine.quench_count_accum, 2);
}

#[test]
fn test_superconductor_event_report_roundtrip() {
    let report = SuperconductorEventReport {
        pos_xyz: [12345, -67890, 54321],
        force_xyz: [1000, -2500, 8888],
        flux_or_voltage: 45000,
        temp_k: 77,
        pin_displacement_mm: 52,
        crit_ratio_q8: 212,
        emitter_id: 7,
        target_id: 14,
        flags: SC_FLAG_ACTIVE | SC_FLAG_FLUX_PINNED,
        event_kind: SC_EVENT_KIND_PINNING,
    };

    let encoded = report.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = SuperconductorEventReport::decode(encoded);
    assert_eq!(decoded, report, "Decoded event report must exactly match original");
}

#[test]
fn test_superconductor_consensus_hash() {
    let mut engine1 = MechaSuperconductorEngine::<4, 2>::new();
    let mut engine2 = MechaSuperconductorEngine::<4, 2>::new();

    let cell = SuperconductorSlab64::new(1, [100, 200, 50], 77, 93, 30_000, SC_TYPE_II);
    let pin = FluxPinningSlab64::new(2, 1, [100, 200, 100], 150, 65536 * 80, 65536 * 8, 20_000, 50);

    engine1.register_cell(cell);
    engine1.register_pin(pin);

    engine2.register_cell(cell);
    engine2.register_pin(pin);

    // Step both engines identically
    for _ in 0..10 {
        engine1.step_simulation(1500);
        engine2.step_simulation(1500);
    }

    let hash1 = engine1.compute_consensus_hash();
    let hash2 = engine2.compute_consensus_hash();
    assert_eq!(hash1, hash2, "Identically stepped superconductor engines must produce bit-exact consensus hashes");
    assert_ne!(hash1, 0, "Consensus hash must not be trivial zero");
}
