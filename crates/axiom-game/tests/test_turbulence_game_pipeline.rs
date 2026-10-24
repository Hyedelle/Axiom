//! Integration test for Mecha Atmospheric Turbulence Pipeline within NetworkMatchDirector (T-256)

use axiom_core::turbulence::{
    MechaTurbulenceEngine, ShockBoundarySlab64, TurbulenceCellSlab64, TurbulenceEventReport,
    EVENT_SHOCK_BOUNDARY, EVENT_TURBULENCE_VORTEX, SHOCK_BL_BUFFETING, SHOCK_BL_SEPARATED,
    SHOCK_BL_SUPERSONIC, TURB_EVENT_KIND_SHOCK_BL, TURB_EVENT_KIND_VORTEX,
    TURB_FLAG_ACTIVE, TURB_FLAG_VORTEX_CORE,
};
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_turbulence_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaTurbulenceEngine::<16, 4>::new();

    let cell = TurbulenceCellSlab64::new(0, [0, 0, 1000], [1500, 2000, 0]);
    engine.register_cell(cell);

    let report = TurbulenceEventReport {
        pos_xyz: [0, 0, 1000],
        vector_xyz: [1500, 2000, 0],
        impulse_or_force_n: 180,
        tke_or_pressure_ratio_q16: 35000,
        mach_or_freq_q8: 2560,
        bl_thickness_or_cell_id: 0,
        emitter_id: 1,
        target_id: 0,
        flags: TURB_FLAG_ACTIVE | TURB_FLAG_VORTEX_CORE,
        event_kind: TURB_EVENT_KIND_VORTEX,
    };

    let ok = director.step_turbulence(&mut engine, 32768, Some(&report));
    assert!(ok, "step_turbulence must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_TURBULENCE_VORTEX);
    assert_eq!(
        event.channel_mask,
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 0);

    let decoded = TurbulenceEventReport::decode(&event.payload);
    assert_eq!(decoded.emitter_id, 1);
    assert_eq!(decoded.impulse_or_force_n, 180);
}

#[test]
fn test_network_match_director_turbulence_jobs_and_shock_boundary() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaTurbulenceEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..4 {
        let cell = TurbulenceCellSlab64::new(
            i as u8,
            [i as i32 * 1000, 0, 500],
            [1200, 1800, 0],
        );
        engine.register_cell(cell);
    }

    let shock = ShockBoundarySlab64::new(
        1,
        [0, 2000, 1000],
        [0, 65536, 0],
        12,
        3500,
    );
    engine.register_shock(shock);

    let report = TurbulenceEventReport {
        pos_xyz: [0, 2000, 1000],
        vector_xyz: [0, 65536, 0],
        impulse_or_force_n: 3200,
        tke_or_pressure_ratio_q16: 88000,
        mach_or_freq_q8: 384, // 1.5 Mach
        bl_thickness_or_cell_id: 12,
        emitter_id: 2,
        target_id: 1,
        flags: SHOCK_BL_SUPERSONIC | SHOCK_BL_SEPARATED | SHOCK_BL_BUFFETING,
        event_kind: TURB_EVENT_KIND_SHOCK_BL,
    };

    let ok = director.step_turbulence_with_jobs(
        &mut engine,
        32768,
        Some(&report),
        &mut job_system,
        &mut coordinator,
    );

    assert!(ok, "step_turbulence_with_jobs must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1);

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_SHOCK_BOUNDARY);
    assert_eq!(event.source_id, 2);
    assert_eq!(event.target_id, 1);

    let decoded = TurbulenceEventReport::decode(&event.payload);
    assert_eq!(decoded.flags & SHOCK_BL_SEPARATED, SHOCK_BL_SEPARATED);
    assert_eq!(decoded.impulse_or_force_n, 3200);
}
