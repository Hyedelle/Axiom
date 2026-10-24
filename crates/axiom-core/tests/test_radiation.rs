//! Unit tests for Axiom 340 Radiative Transfer Equation (RTE) & Particle Beam Ablation Physics Engine.

use axiom_core::radiation::*;

#[test]
fn test_radiation_slab_sizes_and_alignments() {
    assert_eq!(core::mem::size_of::<RadiationCellSlab64>(), 64);
    assert_eq!(core::mem::align_of::<RadiationCellSlab64>(), 64);

    assert_eq!(core::mem::size_of::<ParticleBeamSlab64>(), 64);
    assert_eq!(core::mem::align_of::<ParticleBeamSlab64>(), 64);

    assert_eq!(core::mem::size_of::<RadiationEventReport>(), 40);
}

#[test]
fn test_radiation_event_report_lossless_roundtrip() {
    let report = RadiationEventReport {
        pos_xyz: [1500, -800, 2400],
        dir_xyz: [0, 0, 32768], // Q1.15 normalized forward vector
        flux_or_energy_mj: 450_000,
        optical_depth_or_temp: 1200,
        penetration_depth_mm: 25,
        wavelength_or_cell_id: 1064,
        emitter_id: 1,
        target_id: 2,
        flags: BEAM_FLAG_ACTIVE | BEAM_FLAG_ABLATING | BEAM_FLAG_PENETRATED,
        event_kind: RAD_EVENT_KIND_BEAM,
    };

    let encoded = report.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = RadiationEventReport::decode(&encoded);
    assert_eq!(report, decoded);
}

#[test]
fn test_stefan_boltzmann_integer_radiation() {
    // 0 Kelvin -> 0 W/m²
    assert_eq!(compute_stefan_boltzmann_emission_flux(0), 0);

    // 300 Kelvin -> (3^4 * 567) / 100 = 81 * 567 / 100 = 459 W/m²
    let flux_300 = compute_stefan_boltzmann_emission_flux(300);
    assert_eq!(flux_300, 459);

    // 1000 Kelvin -> (10^4 * 567) / 100 = 10000 * 567 / 100 = 56,700 W/m² (56.7 kW/m²)
    let flux_1000 = compute_stefan_boltzmann_emission_flux(1000);
    assert_eq!(flux_1000, 56700);

    // Monotonic increase check
    assert!(compute_stefan_boltzmann_emission_flux(2000) > flux_1000);
}

#[test]
fn test_beer_lambert_pade_transmittance() {
    // Optical depth tau = 0 -> Transmittance 100% (65536)
    assert_eq!(compute_beer_lambert_transmittance_q16(0), 65536);

    // Optical depth tau = 1.0 (65536) -> ~0.375 (exact exp(-1) ~ 0.368)
    let t_1 = compute_beer_lambert_transmittance_q16(65536);
    assert!(t_1 > 20000 && t_1 < 30000, "Transmittance at tau=1.0 should be near 24576, got {}", t_1);

    // Optical depth tau = 3.0 (196608) -> Transmittance monotonically decreases
    let t_3 = compute_beer_lambert_transmittance_q16(196608);
    assert!(t_3 < t_1, "Transmittance must decrease with optical depth");
    assert!(t_3 > 0, "Transmittance must remain strictly positive");
}

#[test]
fn test_particle_beam_armor_ablation_and_penetration() {
    let mut beam = ParticleBeamSlab64::new(
        1,
        2,
        [0, 0, 0],
        [0, 0, 32768],
        500_000, // 500,000 mJ = 500 J pulse
        1064,    // 1064nm Nd:YAG
        2,       // 2 mrad collimated divergence
    );

    assert!(beam.flags & BEAM_FLAG_COLLIMATED != 0);

    // Fire at 1000 mm target through clear air
    let pen = compute_beam_ablation_penetration(
        &mut beam,
        1000,
        0, // Clear air
        TITANIUM_ABLATION_HEAT_MJ_MM3,
    );

    assert!(pen > 0, "Penetration depth must be positive for 500J pulse, got {}", pen);
    assert!(beam.flags & BEAM_FLAG_ABLATING != 0, "Ablation flag must be active");

    // Super high energy pulse (5,000,000 mJ = 5 kJ) to breakthrough 20mm plate
    let mut heavy_beam = ParticleBeamSlab64::new(
        1,
        2,
        [0, 0, 0],
        [0, 0, 32768],
        5_000_000,
        1064,
        1,
    );
    let pen_heavy = compute_beam_ablation_penetration(
        &mut heavy_beam,
        1000,
        0,
        TITANIUM_ABLATION_HEAT_MJ_MM3,
    );
    assert!(pen_heavy >= 20, "Heavy beam should breach 20mm plate, got {}", pen_heavy);
    assert!(heavy_beam.flags & BEAM_FLAG_PENETRATED != 0, "Penetrated flag must be active");
}

#[test]
fn test_mecha_radiation_engine_simulation_and_consensus_hash() {
    let mut engine1 = MechaRadiationEngine::<16, 8>::new();
    let mut engine2 = MechaRadiationEngine::<16, 8>::new();

    let cell = RadiationCellSlab64::new(1, [0, 500, 0], 1200);
    let beam = ParticleBeamSlab64::new(1, 2, [0, 0, 0], [0, 0, 32768], 1_000_000, 1064, 2);

    engine1.register_cell(cell);
    engine1.register_beam(beam);

    engine2.register_cell(cell);
    engine2.register_beam(beam);

    for _ in 0..10 {
        let (cells1, beams1) = engine1.step_simulation(0);
        let (cells2, beams2) = engine2.step_simulation(0);
        assert_eq!(cells1, cells2);
        assert_eq!(beams1, beams2);
    }

    assert!(engine1.total_emitted_energy_w > 0);
    assert!(engine1.total_penetration_depth_mm > 0);

    let h1 = engine1.compute_consensus_hash();
    let h2 = engine2.compute_consensus_hash();
    assert_eq!(h1, h2, "Consensus hash across deterministic runs must be identical");
}
