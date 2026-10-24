//! Unit tests for `axiom-core::metamaterial` Acousto-Optics & Metamaterial Cloaking Engine.

use axiom_core::metamaterial::*;

#[test]
fn test_metamaterial_slab_alignment_and_sizes() {
    assert_eq!(
        core::mem::size_of::<AcoustoOpticCellSlab64>(),
        64,
        "AcoustoOpticCellSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        core::mem::align_of::<AcoustoOpticCellSlab64>(),
        64,
        "AcoustoOpticCellSlab64 must be 64-byte aligned"
    );

    assert_eq!(
        core::mem::size_of::<CloakingFieldSlab64>(),
        64,
        "CloakingFieldSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        core::mem::align_of::<CloakingFieldSlab64>(),
        64,
        "CloakingFieldSlab64 must be 64-byte aligned"
    );

    assert_eq!(
        core::mem::size_of::<MetamaterialEventReport>(),
        40,
        "MetamaterialEventReport must be exactly 40 bytes POD"
    );
}

#[test]
fn test_metamaterial_event_report_roundtrip() {
    let report = MetamaterialEventReport {
        pos_xyz: [1234, -5678, 9012],
        deflection_dir_xyz: [30000, 2000, -1000],
        bragg_angle_mrad: 25,
        diffraction_eff_or_grad_q16: 52428,
        cloak_efficiency_pct: 98,
        laser_wavelength_nm: 1064,
        emitter_id: 7,
        target_id: 13,
        flags: AO_FLAG_ACTIVE | AO_FLAG_DEFLECTING,
        event_kind: META_EVENT_KIND_SCATTER,
    };

    let encoded = report.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = MetamaterialEventReport::decode(encoded);
    assert_eq!(report, decoded, "MetamaterialEventReport must roundtrip losslessly");
}

#[test]
fn test_acousto_optic_deflection_and_bragg_angle() {
    let mut cell = AcoustoOpticCellSlab64::new(
        1,
        [0, 0, 0],
        4_200_000, // sound speed 4,200 m/s = 4,200,000 mm/s
        100_000,   // frequency 100 MHz = 100,000 kHz
        1064,      // wavelength 1064 nm
    );
    cell.strain_q16 = 6553; // ~0.1 strain

    let input_ray = [32768, 0, 0]; // Q1.15 forward X
    let out_ray = compute_acousto_optic_deflection(&mut cell, input_ray);

    // theta_B (mrad) = (1064 * 100000) / (2 * 4200000) = 106,400,000 / 8,400,000 = 12 mrad
    assert_eq!(cell.bragg_angle_mrad, 12, "Bragg angle should be ~12 mrad");
    assert!(cell.delta_n_q16 > 0, "Refractive index perturbation must be non-zero");
    assert!(cell.diffraction_eff_q16 > 0, "Diffraction efficiency must be non-zero");

    // Ray deflection should deviate forward X slightly into Y
    assert!(out_ray[0] <= input_ray[0]);
    assert!(out_ray[1] > 0);
}

#[test]
fn test_transformation_optics_permittivity_and_ray_bend() {
    let mut field = CloakingFieldSlab64::new(
        1,
        2,
        [0, 0, 0],
        1000, // R1 = 1000 mm
        2000, // R2 = 2000 mm
    );

    // 1. Inside core (r = 500 mm <= R1)
    let (eps_r_core, eps_theta_core) =
        compute_transformation_optics_permittivity(&mut field, [500, 0, 0]);
    assert_eq!(eps_r_core, 0, "Inside core eps_r must be 0 (total shield)");
    assert!(eps_theta_core > 65536, "Inside core eps_theta must be asymptotic high");

    // 2. In transformation shell (r = 1500 mm, R1 < r < R2)
    let (eps_r_shell, eps_theta_shell) =
        compute_transformation_optics_permittivity(&mut field, [1500, 0, 0]);
    // eps_r = (1500 - 1000) / 1500 = 1/3 ~ 21845 Q16
    assert!(
        eps_r_shell >= 21000 && eps_r_shell <= 22500,
        "eps_r at r=1.5R1 should be ~1/3, got {}",
        eps_r_shell
    );
    // eps_theta = 1500 / 500 = 3.0 ~ 196608 Q16
    assert!(
        eps_theta_shell >= 190000 && eps_theta_shell <= 200000,
        "eps_theta at r=1.5R1 should be ~3.0, got {}",
        eps_theta_shell
    );

    // 3. Outside shell (r = 3000 mm >= R2)
    let (eps_r_out, eps_theta_out) =
        compute_transformation_optics_permittivity(&mut field, [3000, 0, 0]);
    assert_eq!(eps_r_out, 65536, "Outside shell eps_r must match vacuum (1.0)");
    assert_eq!(eps_theta_out, 65536, "Outside shell eps_theta must match vacuum (1.0)");

    // 4. Ray bending in shell
    let ray_pos = [1500, 0, 0];
    let ray_dir = [32768, 0, 0];
    let (bent_dir, was_bent) = compute_cloaking_ray_bend(&field, ray_pos, ray_dir);
    assert!(was_bent, "Ray inside transformation shell must be bent");
    assert_ne!(bent_dir, ray_dir, "Bent ray direction must differ from input");
}

#[test]
fn test_metamaterial_engine_step_and_consensus_hash() {
    let mut engine = MechaMetamaterialEngine::<16, 8>::new();
    assert_eq!(engine.active_cells_count, 0);
    assert_eq!(engine.active_fields_count, 0);

    let cell = AcoustoOpticCellSlab64::new(
        1,
        [100, 200, 300],
        4_200_000,
        100_000,
        1064,
    );
    assert_eq!(engine.add_cell(cell), Some(0));

    let field = CloakingFieldSlab64::new(
        2,
        3,
        [0, 0, 0],
        1000,
        2000,
    );
    assert_eq!(engine.add_field(field), Some(0));

    let hash_before = engine.compute_consensus_hash();

    let (_deflected, cloaked) = engine.step_simulation(10);
    assert_eq!(cloaked, 1, "One active field should be stepped");
    let hash_after = engine.compute_consensus_hash();

    assert_ne!(hash_before, hash_after, "Simulation step must evolve consensus hash");
    assert!(engine.total_cloaked_volume_accum_mm3 > 0, "Cloaked volume must accumulate");
}

#[test]
fn test_inactive_and_boundary_conditions() {
    let mut cell = AcoustoOpticCellSlab64::default();
    cell.flags = 0; // inactive

    let input_ray = [32768, 0, 0];
    let out_ray = compute_acousto_optic_deflection(&mut cell, input_ray);
    assert_eq!(out_ray, input_ray, "Inactive cell must not deflect ray");

    let mut field = CloakingFieldSlab64::default();
    field.flags = 0; // inactive

    let (eps_r, eps_theta) = compute_transformation_optics_permittivity(&mut field, [1500, 0, 0]);
    assert_eq!(eps_r, 65536, "Inactive field returns vacuum eps_r");
    assert_eq!(eps_theta, 65536, "Inactive field returns vacuum eps_theta");

    let (bent_dir, was_bent) = compute_cloaking_ray_bend(&field, [1500, 0, 0], input_ray);
    assert!(!was_bent, "Inactive field must not bend ray");
    assert_eq!(bent_dir, input_ray);
}
