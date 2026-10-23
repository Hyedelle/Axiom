//! Test suite for 3D Voxel Destructible Arena Autonomous Championship Director.

use axiom_game::tournament::TournamentFighter;
use axiom_game::voxel_tournament::{VoxelChampionshipDirector, VoxelChampionshipTelemetry};
use core::mem::size_of;

#[test]
fn test_voxel_championship_telemetry_l1_alignment() {
    assert_eq!(size_of::<VoxelChampionshipTelemetry>(), 64);
    assert_eq!(core::mem::align_of::<VoxelChampionshipTelemetry>(), 64);
}

#[test]
fn test_voxel_championship_director_initialization() {
    let f0 = TournamentFighter::new(0, 1, b"ELPIS");
    let f1 = TournamentFighter::new(1, 4, b"TALOS");
    let f2 = TournamentFighter::new(2, 2, b"THESEUS");
    let f3 = TournamentFighter::new(3, 3, b"AETHER");

    let director = VoxelChampionshipDirector::new(f0, f1, f2, f3, 0x1234_5678);

    assert_eq!(director.tournament.matches.len(), 7);
    assert_eq!(director.craters_carved, 0);
    assert_eq!(director.debris_count, 0);
    assert!(director.arena.total_solid > 0);

    let telem = director.telemetry();
    assert_eq!(telem.round, 0);
    assert_eq!(telem.fighter_a_id, 0);
    assert_eq!(telem.fighter_b_id, 1);
}

#[test]
fn test_voxel_championship_deterministic_run_to_crown_champion() {
    let f0 = TournamentFighter::new(0, 1, b"ELPIS");
    let f1 = TournamentFighter::new(1, 4, b"TALOS");
    let f2 = TournamentFighter::new(2, 2, b"THESEUS");
    let f3 = TournamentFighter::new(3, 3, b"AETHER");

    let mut director = VoxelChampionshipDirector::new(f0, f1, f2, f3, 0x9876_5432);

    let mut ticks = 0;
    let mut grand_champion_crowned = false;

    while ticks < 10000 {
        ticks += 1;
        let event = director.step();
        if event == 3 {
            grand_champion_crowned = true;
            break;
        }
    }

    assert!(grand_champion_crowned, "Tournament must crown a grand champion within 10000 ticks");
    assert!(director.tournament.champion.is_some());
    assert!(director.craters_carved > 0, "Autonomous combat must carve craters in the voxel arena");
    assert!(director.arena.damaged_count > 0, "Arena must accumulate voxel destruction");
    assert!(director.shards.total_spawned > 0, "Shards must have been spawned during championship combat");

    let telem = director.telemetry();
    assert_ne!(telem.champion_id, 0xFF);
    assert!(telem.craters_carved > 0);
    assert!(telem.damaged_voxels > 0);
    assert_ne!(telem.state_hash, 0);
}

#[test]
fn test_voxel_championship_state_hash_determinism() {
    let f0 = TournamentFighter::new(0, 1, b"ELPIS");
    let f1 = TournamentFighter::new(1, 4, b"TALOS");
    let f2 = TournamentFighter::new(2, 2, b"THESEUS");
    let f3 = TournamentFighter::new(3, 3, b"AETHER");

    let mut d1 = VoxelChampionshipDirector::new(f0, f1, f2, f3, 0xCAFE_BABE);
    let mut d2 = VoxelChampionshipDirector::new(f0, f1, f2, f3, 0xCAFE_BABE);

    for _ in 0..150 {
        d1.step();
        d2.step();
    }

    assert_eq!(d1.compute_consensus_hash(), d2.compute_consensus_hash());
    assert_eq!(d1.craters_carved, d2.craters_carved);
    assert_eq!(d1.arena.damaged_count, d2.arena.damaged_count);
    assert_eq!(d1.arena.total_solid, d2.arena.total_solid);
    assert_eq!(d1.telemetry(), d2.telemetry());
}
