//! Unit tests for `axiom-core::plasma` Magnetohydrodynamics (MHD) & Electromagnetic Induction Engine.

use axiom_core::plasma::*;

#[test]
fn test_plasma_slab_alignments_and_sizes() {
    assert_eq!(core::mem::size_of::<PlasmaCellSlab64>(), 64, "PlasmaCellSlab64 must be exactly 64 bytes");
    assert_eq!(core::mem::align_of::<PlasmaCellSlab64>(), 64, "PlasmaCellSlab64 must be 64-byte aligned");

    assert_eq!(core::mem::size_of::<MagneticFieldSlab64>(), 64, "MagneticFieldSlab64 must be exactly 64 bytes");
    assert_eq!(core::mem::align_of::<MagneticFieldSlab64>(), 64, "MagneticFieldSlab64 must be 64-byte aligned");

    assert_eq!(core::mem::size_of::<PlasmaEventReport>(), 40, "PlasmaEventReport must be exactly 40 bytes POD");
}

#[test]
fn test_magnetic_pressure_calculation() {
    // 1 Tesla = 1000 mT
    let b_1t = [1000, 0, 0];
    let p_1t = compute_magnetic_pressure_kpa(b_1t);
    // Theoretical: 1000^2 / 2513 = 397.93 -> 397 kPa (or 398 kPa)
    assert!(p_1t >= 397 && p_1t <= 398, "1T magnetic pressure must be ~398 kPa, got {}", p_1t);

    // 0 field -> 0 pressure
    assert_eq!(compute_magnetic_pressure_kpa([0, 0, 0]), 0);

    // Multi-axis 3D field: [600, 800, 0] mT -> |B|^2 = 360000 + 640000 = 1000000 -> 1T equivalent
    let b_diag = [600, 800, 0];
    assert_eq!(compute_magnetic_pressure_kpa(b_diag), p_1t);
}

#[test]
fn test_lorentz_force_cross_product() {
    let mut cell = PlasmaCellSlab64::new(1, [0, 0, 0], [0, 1000, 0], 5000); // J = +Y (1000 mA/mm2)
    let mut field = MagneticFieldSlab64::new(10, [0, 0, 0], [0, 0, 500], 1000); // B = +Z (500 mT)

    // F_L = J x B -> (+Y) x (+Z) = +X
    let force = compute_mhd_forces(&cell, &field);
    assert!(force[0] > 0, "Force X must be positive (+X)");
    assert_eq!(force[1], 0, "Force Y must be 0");
    assert_eq!(force[2], 0, "Force Z must be 0");
    assert_eq!(force[0], 500_000, "1000 * 500 = 500,000 mN");

    // Inactive flags -> 0 force
    cell.flags = 0;
    assert_eq!(compute_mhd_forces(&cell, &field), [0, 0, 0]);

    cell.flags = PLASMA_FLAG_ACTIVE;
    field.flags = 0;
    assert_eq!(compute_mhd_forces(&cell, &field), [0, 0, 0]);
}

#[test]
fn test_faraday_induced_emf_and_emp() {
    // 10,000 mm2 loop (100mm x 100mm)
    let area = 10_000u32;
    // Rapid field collapse of 50 mT in 1 tick (delta B = -50 mT)
    let delta_b = -50i32;

    let emf = compute_induced_emf_mv(area, delta_b);
    // E = - (3 * 10000 * -50) / 50 = +30,000 mV = +30 V
    assert_eq!(emf, 30_000, "Induced EMF must equal +30,000 mV (+30V)");

    // Positive field ramp (delta B = +100 mT) -> negative opposing EMF
    let emf_oppose = compute_induced_emf_mv(area, 100);
    assert_eq!(emf_oppose, -60_000, "Induced EMF must oppose flux increase");
}

#[test]
fn test_plasma_event_report_lossless_roundtrip() {
    let original = PlasmaEventReport {
        pos_xyz: [-1250, 4800, 310],
        force_or_b_xyz: [15000, -8500, 2200],
        pressure_or_voltage: 450,
        temp_or_alfven: 125000,
        current_or_flux: 3500,
        beta_or_cell_id: 8,
        emitter_id: 3,
        target_id: 7,
        flags: PLASMA_FLAG_ACTIVE | PLASMA_FLAG_PINCHED,
        event_kind: PLASMA_EVENT_KIND_DISCHARGE,
    };

    let encoded = original.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = PlasmaEventReport::decode(encoded);
    assert_eq!(original, decoded, "Decoded report must exactly match original POD");
}

#[test]
fn test_mecha_plasma_engine_simulation_and_consensus_hash() {
    let mut engine = MechaPlasmaEngine::<8, 4>::new();
    assert_eq!(engine.active_cells_count, 0);
    assert_eq!(engine.active_fields_count, 0);

    // Register active plasma cell with high Z-current for Z-pinch
    let cell = PlasmaCellSlab64::new(1, [100, 200, 50], [100, 200, 800], 8000);
    let cell_idx = engine.register_cell(cell).expect("Must register cell");
    assert_eq!(cell_idx, 0);

    // Register active magnetic field with high flux
    let field = MagneticFieldSlab64::new(10, [100, 200, 50], [500, 500, 500], 1200);
    let field_idx = engine.register_field(field).expect("Must register field");
    assert_eq!(field_idx, 0);

    // Step simulation
    let (active_cells, active_fields) = engine.step_simulation(101, DEFAULT_PLASMA_DENSITY_G_M3);
    assert_eq!(active_cells, 1);
    assert_eq!(active_fields, 1);
    assert_eq!(engine.tick, 1);

    // Verify cell acquired magnetic pressure and Z-pinch flag
    let updated_cell = &engine.cells[0];
    assert!(updated_cell.magnetic_pressure_kpa > 0, "Cell must record magnetic pressure");
    assert!((updated_cell.flags & PLASMA_FLAG_PINCHED) != 0, "High Jz + high P_B must trigger PINCHED flag");

    // Verify field computed Alfvén speed and induced EMF
    let updated_field = &engine.fields[0];
    assert!(updated_field.alfven_velocity_mms > 0, "Alfvén speed must be non-zero");

    // Consensus hash must be non-zero and strictly deterministic
    let hash1 = engine.compute_consensus_hash();
    assert_ne!(hash1, 0);
    assert_eq!(hash1, engine.compute_consensus_hash(), "Hash must be idempotent on identical state");
}
