//! Integration test for Mecha Metamaterial Acousto-Optics & Cloaking Pipeline within NetworkMatchDirector (T-280)

use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::metamaterial::{
    AcoustoOpticCellSlab64, CloakingFieldSlab64, MechaMetamaterialEngine,
    MetamaterialEventReport, AO_FLAG_ACTIVE, AO_FLAG_DEFLECTING, CLOAK_FLAG_ACTIVE,
    CLOAK_FLAG_BENDING, EVENT_BRILLOUIN_SCATTER, EVENT_OPTICAL_CLOAK, META_EVENT_KIND_CLOAK,
    META_EVENT_KIND_SCATTER,
};
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_metamaterial_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaMetamaterialEngine::<16, 8>::new();

    let cell = AcoustoOpticCellSlab64::new(0, [0, 0, 100], 4_200_000, 100_000, 1064);
    engine.register_cell(cell);

    let report = MetamaterialEventReport {
        pos_xyz: [0, 0, 100],
        deflection_dir_xyz: [32000, 1000, 0],
        bragg_angle_mrad: 12,
        diffraction_eff_or_grad_q16: 45000,
        cloak_efficiency_pct: 95,
        laser_wavelength_nm: 1064,
        emitter_id: 1,
        target_id: 0,
        flags: CLOAK_FLAG_ACTIVE | CLOAK_FLAG_BENDING,
        event_kind: META_EVENT_KIND_CLOAK,
    };

    let ok = director.step_metamaterial(&mut engine, 10, Some(&report));
    assert!(ok, "step_metamaterial must return true");
    assert_eq!(engine.tick, 10);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_OPTICAL_CLOAK);
    assert_eq!(
        event.channel_mask,
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 0);

    let decoded = MetamaterialEventReport::decode(event.payload);
    assert_eq!(decoded.emitter_id, 1);
    assert_eq!(decoded.target_id, 0);
    assert_eq!(decoded.bragg_angle_mrad, 12);
    assert_eq!(decoded.cloak_efficiency_pct, 95);
}

#[test]
fn test_network_match_director_metamaterial_jobs_and_scatter() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaMetamaterialEngine::<16, 8>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let cell = AcoustoOpticCellSlab64::new(1, [100, 200, 50], 4_200_000, 100_000, 532);
    let field = CloakingFieldSlab64::new(2, 1, [100, 200, 100], 1000, 2000);

    engine.register_cell(cell);
    engine.register_field(field);

    let scatter_report = MetamaterialEventReport {
        pos_xyz: [100, 200, 50],
        deflection_dir_xyz: [31500, 2500, 0],
        bragg_angle_mrad: 25,
        diffraction_eff_or_grad_q16: 60000,
        cloak_efficiency_pct: 0,
        laser_wavelength_nm: 532,
        emitter_id: 2,
        target_id: 1,
        flags: AO_FLAG_ACTIVE | AO_FLAG_DEFLECTING,
        event_kind: META_EVENT_KIND_SCATTER,
    };

    let ok = director.step_metamaterial_with_jobs(
        &mut engine,
        15,
        Some(&scatter_report),
        &mut job_system,
        &mut coordinator,
    );

    assert!(ok, "step_metamaterial_with_jobs must return true");
    assert_eq!(director.event_bus.count, 1);

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_BRILLOUIN_SCATTER);
    assert_eq!(event.source_id, 2);
    assert_eq!(event.target_id, 1);

    let decoded = MetamaterialEventReport::decode(event.payload);
    assert_eq!(decoded.bragg_angle_mrad, 25);
    assert_eq!(decoded.laser_wavelength_nm, 532);
    assert_eq!(decoded.event_kind, META_EVENT_KIND_SCATTER);
}
