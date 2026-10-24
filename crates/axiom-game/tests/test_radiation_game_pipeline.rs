//! Integration test for Mecha Radiative Transfer & Particle Beam Pipeline within NetworkMatchDirector (T-262)

use axiom_core::radiation::{
    MechaRadiationEngine, ParticleBeamSlab64, RadiationCellSlab64, RadiationEventReport,
    BEAM_FLAG_ABLATING, BEAM_FLAG_ACTIVE, BEAM_FLAG_PENETRATED, EVENT_BEAM_PENETRATION,
    EVENT_RADIATION_BURST, RAD_EVENT_KIND_BEAM, RAD_EVENT_KIND_BURST, RAD_FLAG_ACTIVE,
    RAD_FLAG_INCANDESCENT,
};
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_radiation_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaRadiationEngine::<16, 4>::new();

    let cell = RadiationCellSlab64::new(0, [0, 0, 1000], 1500);
    engine.register_cell(cell);

    let report = RadiationEventReport {
        pos_xyz: [0, 0, 1000],
        dir_xyz: [0, 0, 32768],
        flux_or_energy_mj: 60_000,
        optical_depth_or_temp: 1500,
        penetration_depth_mm: 0,
        wavelength_or_cell_id: 0,
        emitter_id: 1,
        target_id: 0,
        flags: RAD_FLAG_ACTIVE | RAD_FLAG_INCANDESCENT,
        event_kind: RAD_EVENT_KIND_BURST,
    };

    let ok = director.step_radiation(&mut engine, 0, Some(&report));
    assert!(ok, "step_radiation must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_RADIATION_BURST);
    assert_eq!(
        event.channel_mask,
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 0);

    let decoded = RadiationEventReport::decode(&event.payload);
    assert_eq!(decoded.emitter_id, 1);
    assert_eq!(decoded.flux_or_energy_mj, 60_000);
}

#[test]
fn test_network_match_director_radiation_jobs_and_beam_penetration() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaRadiationEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..4 {
        let cell = RadiationCellSlab64::new(
            i as u8,
            [i as i32 * 1000, 0, 500],
            1200 + (i as u32 * 100),
        );
        engine.register_cell(cell);
    }

    let beam = ParticleBeamSlab64::new(
        1,
        2,
        [0, 0, 0],
        [0, 0, 32768],
        5_000_000,
        1064,
        2,
    );
    engine.register_beam(beam);

    let report = RadiationEventReport {
        pos_xyz: [0, 0, 2000],
        dir_xyz: [0, 0, 32768],
        flux_or_energy_mj: 5_000_000,
        optical_depth_or_temp: 65536,
        penetration_depth_mm: 24,
        wavelength_or_cell_id: 1064,
        emitter_id: 1,
        target_id: 2,
        flags: BEAM_FLAG_ACTIVE | BEAM_FLAG_ABLATING | BEAM_FLAG_PENETRATED,
        event_kind: RAD_EVENT_KIND_BEAM,
    };

    let ok = director.step_radiation_with_jobs(
        &mut engine,
        0,
        Some(&report),
        &mut job_system,
        &mut coordinator,
    );

    assert!(ok, "step_radiation_with_jobs must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1);

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_BEAM_PENETRATION);
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 2);

    let decoded = RadiationEventReport::decode(&event.payload);
    assert!(decoded.flags & BEAM_FLAG_PENETRATED != 0);
    assert_eq!(decoded.penetration_depth_mm, 24);
}
