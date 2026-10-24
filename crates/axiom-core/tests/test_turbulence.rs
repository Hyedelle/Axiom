//! Unit tests for Axiom 330 Turbulence, Vorticity Confinement & Shock Boundary Layer Engine.

use axiom_core::turbulence::*;

#[test]
fn test_turbulence_slab_sizes_and_alignments() {
    assert_eq!(core::mem::size_of::<TurbulenceCellSlab64>(), 64);
    assert_eq!(core::mem::align_of::<TurbulenceCellSlab64>(), 64);

    assert_eq!(core::mem::size_of::<ShockBoundarySlab64>(), 64);
    assert_eq!(core::mem::align_of::<ShockBoundarySlab64>(), 64);

    assert_eq!(core::mem::size_of::<TurbulenceEventReport>(), 40);
}

#[test]
fn test_turbulence_event_report_lossless_roundtrip() {
    let report = TurbulenceEventReport {
        pos_xyz: [1200, -450, 3100],
        vector_xyz: [100, 2500, -800],
        impulse_or_force_n: 1450,
        tke_or_pressure_ratio_q16: 82000,
        mach_or_freq_q8: 384, // 1.5 Mach in Q8
        bl_thickness_or_cell_id: 15,
        emitter_id: 2,
        target_id: 1,
        flags: TURB_FLAG_ACTIVE | TURB_FLAG_VORTEX_CORE,
        event_kind: TURB_EVENT_KIND_VORTEX,
    };

    let encoded = report.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = TurbulenceEventReport::decode(&encoded);
    assert_eq!(report, decoded);
}

#[test]
fn test_vorticity_confinement_forces() {
    let mut cell = TurbulenceCellSlab64::new(
        1,
        [0, 1000, 0],
        [1500, 2000, 0], // Strong discrete vorticity in mrad/s
    );

    let f_vc = compute_turbulence_forces(&mut cell, 32768); // 0.5 confinement epsilon

    assert!(cell.flags & TURB_FLAG_VORTEX_CORE != 0, "High vorticity must trigger VORTEX_CORE flag");
    assert!(cell.flags & TURB_FLAG_CONFINED != 0, "Confinement must trigger CONFINED flag");
    assert!(f_vc[0] != 0 || f_vc[1] != 0 || f_vc[2] != 0, "Confinement force must be non-zero");
    assert!(cell.tke_q16 > 0, "Turbulence kinetic energy (TKE) must be generated");
}

#[test]
fn test_rankine_hugoniot_supersonic_jump() {
    let mut shock = ShockBoundarySlab64::new(
        1,
        [0, 0, 1000],
        [0, 32768, 0], // Unit normal pointing +Y
        12, // 12 mm boundary layer
        2500, // 2500 N peak impulse
    );

    // Speed: 510,000 mm/s = 510 m/s = 1.5 Mach
    let (p_ratio, _buffet) = compute_shock_boundary_interaction(&mut shock, 510_000);

    assert!(shock.flags & SHOCK_BL_SUPERSONIC != 0, "Must be tagged supersonic");
    assert!(p_ratio > 65536, "Rankine-Hugoniot pressure jump P2/P1 must be > 1.0 (got {})", p_ratio);
    assert!(shock.mach_q16 > 65536, "Mach number must be > 1.0");
}

#[test]
fn test_sbli_separation_and_buffeting() {
    let mut shock = ShockBoundarySlab64::new(
        1,
        [0, 0, 1000],
        [0, 32768, 0],
        20, // 20 mm boundary layer
        5000, // 5000 N peak impulse
    );

    // Speed: 680,000 mm/s = 680 m/s = 2.0 Mach (severe shock)
    let (p_ratio, buffet_n) = compute_shock_boundary_interaction(&mut shock, 680_000);

    assert!(p_ratio >= 78643, "Severe shock must exceed P2/P1 1.20 threshold");
    assert!(shock.flags & SHOCK_BL_SEPARATED != 0, "Must trigger SBLI separation");
    assert!(shock.flags & SHOCK_BL_BUFFETING != 0, "Must trigger buffeting oscillation");
    assert!(shock.separation_distance_mm > 0, "Separation bubble detachment must be > 0mm");
    assert!(buffet_n > 0, "Buffeting force must be positive");
}

#[test]
fn test_mecha_turbulence_engine_simulation_stepping() {
    let mut engine = MechaTurbulenceEngine::<16, 8>::new();

    let cell = TurbulenceCellSlab64::new(1, [0, 500, 0], [1200, -800, 400]);
    let shock = ShockBoundarySlab64::new(2, [0, 0, 1500], [0, 32768, 0], 15, 3000);

    engine.register_cell(cell);
    engine.register_shock(shock);

    let (evaluated_cells, _separated) = engine.step_simulation(16384);
    assert_eq!(evaluated_cells, 1, "Must evaluate 1 active turbulence cell");
    assert_eq!(engine.tick, 1, "Tick must advance to 1");
}

#[test]
fn test_mecha_turbulence_consensus_hash() {
    let mut engine1 = MechaTurbulenceEngine::<8, 4>::new();
    let mut engine2 = MechaTurbulenceEngine::<8, 4>::new();

    let cell = TurbulenceCellSlab64::new(1, [100, 200, 300], [500, 1000, -200]);
    let shock = ShockBoundarySlab64::new(2, [0, 100, 200], [0, 32768, 0], 10, 4000);

    engine1.register_cell(cell);
    engine1.register_shock(shock);

    engine2.register_cell(cell);
    engine2.register_shock(shock);

    for _ in 0..10 {
        engine1.step_simulation(16384);
        engine2.step_simulation(16384);
    }

    let hash1 = engine1.compute_consensus_hash();
    let hash2 = engine2.compute_consensus_hash();

    assert_eq!(hash1, hash2, "Consensus hash must match exactly across identical runs");
}
