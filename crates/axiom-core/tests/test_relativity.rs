use axiom_core::event_bus::DeterministicEventBus;
use axiom_core::job_system::JobSystem;
use axiom_core::relativity::*;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_relativity_slab_sizes_and_alignments() {
    assert_eq!(core::mem::size_of::<SpacetimeMetricSlab64>(), 64);
    assert_eq!(core::mem::align_of::<SpacetimeMetricSlab64>(), 64);

    assert_eq!(core::mem::size_of::<GravitationalLensSlab64>(), 64);
    assert_eq!(core::mem::align_of::<GravitationalLensSlab64>(), 64);

    assert_eq!(core::mem::size_of::<RelativityEventReport>(), 40);
}

#[test]
fn test_schwarzschild_metric_and_time_dilation() {
    let mut cell = SpacetimeMetricSlab64 {
        center_xyz: [0, 0, 0],
        rs_schwarzschild_mm: 100,
        mass_kg: 6_740_000_000_000_000_000,
        time_dilation_q16: 65536,
        curvature_scalar_q16: 0,
        metric_g00_q16: -65536,
        metric_grr_q16: 65536,
        field_radius_mm: 50_000,
        entity_id: 1,
        flags: METRIC_FLAG_ACTIVE,
        reserved: [0u8; 18],
    };

    // Query at r = 1000 mm (outside horizon)
    let (g00, grr, dilation) = compute_spacetime_metric(&mut cell, [1000, 0, 0]);
    // g00 = -(1 - 100/1000) = -0.90 -> in Q16: -58982
    assert!(g00 < 0 && g00 > -65536);
    // grr = 1000 / 900 = 1.111 -> in Q16: ~72817
    assert!(grr > 65536);
    // Dilation = sqrt(900/1000) = sqrt(0.9) ~ 0.9486 -> in Q16: ~62172
    assert!(dilation < 65536 && dilation > 50000);

    // Query at r = 50 mm (inside event horizon)
    let (g00_h, grr_h, dilation_h) = compute_spacetime_metric(&mut cell, [50, 0, 0]);
    assert_eq!(g00_h, 0);
    assert!(grr_h > 65536 * 100);
    assert_eq!(dilation_h, 0);
    assert!((cell.flags & METRIC_FLAG_EVENT_HORIZON) != 0);
}

#[test]
fn test_geodesic_ray_deflection_and_photon_sphere() {
    let mut lens = GravitationalLensSlab64 {
        origin_xyz: [0, 0, 0],
        deflection_angle_mrad: 0,
        impact_param_b_mm: 0,
        warp_factor_q16: 65536,
        event_horizon_r_mm: 100,
        shear_distortion_q16: 0,
        einstein_ring_mrad: 0,
        redshift_factor_q16: 0,
        emitter_id: 1,
        target_id: 2,
        flags: LENS_FLAG_ACTIVE,
        reserved: [0u8; 21],
    };

    // Probing ray passing along +X at Y = 500 mm (impact parameter b ~ 500 mm)
    let ray_pos = [0, 500, 0];
    let ray_dir = [32768, 0, 0]; // Normalized in Q1.15

    let (bent_dir, deflected) = compute_geodesic_deflection(&mut lens, ray_pos, ray_dir, 100);
    assert!(deflected);
    assert!(lens.deflection_angle_mrad > 0);
    // Deflection angle ~ (2000 * 100) / 500 = 400 mrad
    assert_eq!(lens.deflection_angle_mrad, 400);
    // Bent direction should bend towards center (negative Y shift)
    assert!(bent_dir[1] < ray_dir[1]);

    // Probing ray passing inside critical photon capture radius: b = 200 mm <= b_crit (259 mm)
    let ray_pos_crit = [0, 200, 0];
    let (captured_dir, captured) = compute_geodesic_deflection(&mut lens, ray_pos_crit, ray_dir, 100);
    assert!(captured);
    assert_eq!(captured_dir, [0, 0, 0]);
    assert!((lens.flags & LENS_FLAG_PHOTON_SPHERE) != 0);
}

#[test]
fn test_relativity_event_report_encode_decode_roundtrip() {
    let report = RelativityEventReport {
        pos_xyz: [1234, -5678, 9012],
        deflected_dir_xyz: [32000, -1000, 500],
        deflection_angle_mrad: 450,
        time_dilation_q16: 58000,
        warp_or_shear_q16: 72000,
        emitter_id: 42,
        target_id: 99,
        flags: 0b0000_0111,
        event_kind: RELATIVITY_EVENT_KIND_LENS,
    };

    let encoded = report.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = RelativityEventReport::decode(encoded);
    assert_eq!(report, decoded);
}

#[test]
fn test_mecha_relativity_engine_simulation_and_consensus_hash() {
    let mut engine = MechaRelativityEngine::<16, 8>::new();
    assert_eq!(engine.active_cells_count, 0);
    assert_eq!(engine.active_lenses_count, 0);

    let cell = SpacetimeMetricSlab64 {
        center_xyz: [0, 0, 0],
        rs_schwarzschild_mm: 50,
        mass_kg: 3_370_000_000_000_000_000,
        time_dilation_q16: 65536,
        curvature_scalar_q16: 0,
        metric_g00_q16: -65536,
        metric_grr_q16: 65536,
        field_radius_mm: 20_000,
        entity_id: 1,
        flags: METRIC_FLAG_ACTIVE,
        reserved: [0u8; 18],
    };
    let lens = GravitationalLensSlab64 {
        origin_xyz: [0, 0, 0],
        deflection_angle_mrad: 0,
        impact_param_b_mm: 0,
        warp_factor_q16: 65536,
        event_horizon_r_mm: 50,
        shear_distortion_q16: 0,
        einstein_ring_mrad: 0,
        redshift_factor_q16: 0,
        emitter_id: 1,
        target_id: 2,
        flags: LENS_FLAG_ACTIVE,
        reserved: [0u8; 21],
    };

    assert_eq!(engine.add_cell(cell), Some(0));
    assert_eq!(engine.add_lens(lens), Some(0));

    let hash0 = engine.compute_consensus_hash();

    let (deflected, dilated) = engine.step_simulation(1);
    assert_eq!(deflected, 1);
    assert_eq!(dilated, 1);

    let hash1 = engine.compute_consensus_hash();
    assert_ne!(hash0, hash1);
}

#[test]
fn test_relativity_event_bus_and_job_dispatch() {
    let mut bus = DeterministicEventBus::<64>::new();
    let report = RelativityEventReport {
        pos_xyz: [1000, 2000, 3000],
        deflected_dir_xyz: [32000, 0, 0],
        deflection_angle_mrad: 250,
        time_dilation_q16: 60000,
        warp_or_shear_q16: 65536,
        emitter_id: 1,
        target_id: 2,
        flags: 0,
        event_kind: RELATIVITY_EVENT_KIND_LENS,
    };

    let emitted = emit_relativity_to_event_bus(&report, 100, &mut bus);
    assert!(emitted);
    assert!(bus.peek().is_some());

    let mut engine = MechaRelativityEngine::<16, 8>::new();
    for i in 0..4 {
        let cell = SpacetimeMetricSlab64 {
            center_xyz: [i * 1000, 0, 0],
            rs_schwarzschild_mm: 50,
            mass_kg: 3_370_000_000_000_000_000,
            time_dilation_q16: 65536,
            curvature_scalar_q16: 0,
            metric_g00_q16: -65536,
            metric_grr_q16: 65536,
            field_radius_mm: 20_000,
            entity_id: i as u8,
            flags: METRIC_FLAG_ACTIVE,
            reserved: [0u8; 18],
        };
        engine.add_cell(cell);
    }

    let mut job_system = JobSystem::<32>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let dispatched = dispatch_relativity_with_jobs(&engine, 2, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 2);
}
