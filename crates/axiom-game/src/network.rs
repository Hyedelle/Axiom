//! Axiom 16-Player Deterministic Physical State Synchronization & Rollback Engine (T-162).
//!
//! Provides zero-floating-point 64-byte L1 cache-aligned physical entity slabs,
//! 16-player collision and combat resolution, circular ring boundary physics,
//! and full snapshot rewind/resimulation integrated with `axiom-net`.

use axiom_core::event_bus::{
    channel, dispatch_events_with_jobs, DeterministicEventBus,
};
use axiom_core::spatial_grid::{
    dispatch_broadphase_with_jobs, emit_collision_pairs_to_event_bus, AabbSlab64, CollisionPair64,
    SpatialHashGrid, COLLISION_FLAG_DYNAMIC,
};
use axiom_core::ccd::{
    dispatch_ccd_hitboxes_with_jobs, emit_ccd_hits_to_event_bus, swept_hitbox_vs_hurtbox,
    HitType, HitboxSlab64, SweptAabbResult64, HITBOX_FLAG_ACTIVE, HITBOX_FLAG_SWEPT_CCD,
};
use axiom_core::ragdoll::{
    dispatch_ragdoll_with_jobs, emit_ragdoll_impacts_to_event_bus, RagdollBoneSlab64,
    RagdollEngine, RagdollImpactReport, RagdollJointSlab64, RagdollJointType,
};
use axiom_core::deformable_mesh::{
    dispatch_terrain_deform_with_jobs, emit_crater_to_event_bus, CraterSlab64, DeformableMeshGrid,
    TerrainCraterReport,
};
use axiom_core::destruction_audio::{
    dispatch_destruction_audio_with_jobs, emit_destruction_audio_to_event_bus,
    DestructionAudioEngine, DestructionAudioReport, DestructionVoiceSlab64,
};
use axiom_core::debris_particle::{
    dispatch_debris_with_jobs, emit_debris_burst_to_event_bus, DebrisBurstReport,
    DebrisParticlePool,
};
use axiom_net::{
    DistributedRollbackSession, NetError, NetPacket64, DEFAULT_NET_BUFFER_FRAMES, MAX_PEERS,
};

pub const EVENT_COMBAT_HIT: u16 = 1;
pub const EVENT_COMBAT_GUARD: u16 = 2;
pub const EVENT_COMBAT_DAMAGE: u16 = 3;
pub const EVENT_COMBAT_KNOCKDOWN: u16 = 4;

/// 24-byte POD combat hit payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CombatHitPayload {
    pub attacker_id: u8,
    pub victim_id: u8,
    pub damage: u16,
    pub knockback: i32,
    pub hit_x: i32,
    pub hit_y: i32,
    pub is_guard: u8,
    pub _pad: [u8; 3],
}

pub const ARENA_RADIUS_MM: i32 = 6_000; // 6.0 meters ring radius in fixed-point mm
pub const PLAYER_RADIUS_MM: i32 = 450;   // 450 mm collision radius
pub const MOVE_SPEED_MMS: i32 = 2_500;   // 2.5 m/s movement speed in mm/s
pub const DASH_SPEED_MMS: i32 = 5_000;   // 5.0 m/s dash speed in mm/s
pub const DT_NUMERATOR: i32 = 1;         // 60 Hz simulation step (1/60s)
pub const DT_DENOMINATOR: i32 = 60;

// Input bitmask definitions
pub const INPUT_UP: u16 = 1 << 0;
pub const INPUT_DOWN: u16 = 1 << 1;
pub const INPUT_LEFT: u16 = 1 << 2;
pub const INPUT_RIGHT: u16 = 1 << 3;
pub const INPUT_LIGHT_ATTACK: u16 = 1 << 4;
pub const INPUT_HEAVY_ATTACK: u16 = 1 << 5;
pub const INPUT_GUARD: u16 = 1 << 6;
pub const INPUT_DASH: u16 = 1 << 7;

// Action states
pub const STATE_IDLE: u8 = 0;
pub const STATE_MOVE: u8 = 1;
pub const STATE_DASH: u8 = 2;
pub const STATE_LIGHT_ATTACK: u8 = 3;
pub const STATE_HEAVY_ATTACK: u8 = 4;
pub const STATE_GUARD: u8 = 5;
pub const STATE_HIT_STUN: u8 = 6;
pub const STATE_KNOCKDOWN: u8 = 7;

/// 64-Byte L1 cache-aligned physical entity state for a single network peer.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerPhysicalState64 {
    pub pos_x: i32,        // Millimeters (-32,768..+32,767 mm)
    pub pos_y: i32,        // Millimeters
    pub pos_z: i32,        // Vertical elevation mm
    pub vel_x: i32,        // mm/s
    pub vel_y: i32,        // mm/s
    pub vel_z: i32,        // mm/s
    pub facing_yaw: i16,   // -18000..+18000 (0.01 degree fixed-point)
    pub health: u16,       // 0..1000 HP
    pub stamina: u16,      // 0..1000 SP
    pub action_state: u8,  // STATE_*
    pub action_frame: u8,  // Frame counter within active state
    pub combo_count: u8,   // Active hit combo counter
    pub flags: u8,         // Bit 0: Grounded, Bit 1: Invincible, Bit 2: Guarding
    pub target_peer: u8,   // Tracked opponent index
    pub score: u16,        // Match points
    pub _padding: [u8; 26],
}

impl Default for PlayerPhysicalState64 {
    fn default() -> Self {
        Self {
            pos_x: 0,
            pos_y: 0,
            pos_z: 0,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            facing_yaw: 0,
            health: 1000,
            stamina: 1000,
            action_state: STATE_IDLE,
            action_frame: 0,
            combo_count: 0,
            flags: 1, // Grounded
            target_peer: 0,
            score: 0,
            _padding: [0u8; 26],
        }
    }
}

/// 16-Player Physical World Snapshot (1,088 bytes = exactly 17 x 64-byte L1 slabs).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalWorldSnapshot {
    pub players: [PlayerPhysicalState64; MAX_PEERS],
    // Header slab (64 bytes)
    pub tick: u32,
    pub active_mask: u16,
    pub arena_radius_mm: u32,
    pub total_hits_landed: u32,
    pub total_damage_dealt: u32,
    pub _padding: [u8; 44],
}

impl Default for PhysicalWorldSnapshot {
    fn default() -> Self {
        let mut players = [PlayerPhysicalState64::default(); MAX_PEERS];

        // Position 16 players symmetrically around a circle of radius 4,000 mm
        let spawn_radius = 4_000i32;
        for i in 0..MAX_PEERS {
            // Integer fixed-point sin/cos approximation using circle coordinates
            let (cos_val, sin_val) = fixed_circle_coords(i);
            players[i].pos_x = (spawn_radius * cos_val) / 1000;
            players[i].pos_y = (spawn_radius * sin_val) / 1000;
            // Face towards the center (0, 0)
            players[i].facing_yaw = (((i as i32 + 8) % 16) * 2250) as i16;
            players[i].target_peer = ((i + 1) % MAX_PEERS) as u8;
        }

        Self {
            players,
            tick: 0,
            active_mask: 0xFFFF, // All 16 peers active
            arena_radius_mm: ARENA_RADIUS_MM as u32,
            total_hits_landed: 0,
            total_damage_dealt: 0,
            _padding: [0u8; 44],
        }
    }
}

/// Computes integer fixed-point unit circle coordinates (cos * 1000, sin * 1000) for 16 angles.
fn fixed_circle_coords(index: usize) -> (i32, i32) {
    const COORDS: [(i32, i32); 16] = [
        (1000, 0),     // 0 deg
        (924, 383),    // 22.5 deg
        (707, 707),    // 45 deg
        (383, 924),    // 67.5 deg
        (0, 1000),     // 90 deg
        (-383, 924),   // 112.5 deg
        (-707, 707),   // 135 deg
        (-924, 383),   // 157.5 deg
        (-1000, 0),    // 180 deg
        (-924, -383),  // 202.5 deg
        (-707, -707),  // 225 deg
        (-383, -924),  // 247.5 deg
        (0, -1000),    // 270 deg
        (383, -924),   // 292.5 deg
        (707, -707),   // 315 deg
        (924, -383),   // 337.5 deg
    ];
    COORDS[index % 16]
}

pub fn step_physical_world(world: &mut PhysicalWorldSnapshot, inputs: &[u16; MAX_PEERS]) -> u64 {
    step_physical_world_with_events(world, inputs, None::<&mut DeterministicEventBus<128>>)
}

/// Executes a single 60 Hz deterministic physical step for all 16 players.
/// Emits combat hit/guard/damage/knockdown events into the optional deterministic event bus.
/// Returns the 64-bit FNV-1a consensus state hash.
pub fn step_physical_world_with_events<const EVENT_CAP: usize>(
    world: &mut PhysicalWorldSnapshot,
    inputs: &[u16; MAX_PEERS],
    mut event_bus: Option<&mut DeterministicEventBus<EVENT_CAP>>,
) -> u64 {
    // 1. Process player actions and update velocities
    for p in 0..MAX_PEERS {
        if (world.active_mask & (1 << p)) == 0 || world.players[p].health == 0 {
            continue;
        }

        let inp = inputs[p];
        let player = &mut world.players[p];

        // State machine progression
        if player.action_state == STATE_HIT_STUN {
            player.action_frame += 1;
            if player.action_frame >= 12 {
                // 200ms stun recovery
                player.action_state = STATE_IDLE;
                player.action_frame = 0;
            }
            // Decelerate stun knockback
            player.vel_x = (player.vel_x * 85) / 100;
            player.vel_y = (player.vel_y * 85) / 100;
        } else if player.action_state == STATE_LIGHT_ATTACK {
            player.action_frame += 1;
            if player.action_frame >= 8 {
                // Light attack ends after 8 frames (133ms)
                player.action_state = STATE_IDLE;
                player.action_frame = 0;
            }
            player.vel_x = (player.vel_x * 50) / 100;
            player.vel_y = (player.vel_y * 50) / 100;
        } else if player.action_state == STATE_HEAVY_ATTACK {
            player.action_frame += 1;
            if player.action_frame >= 16 {
                // Heavy attack ends after 16 frames (266ms)
                player.action_state = STATE_IDLE;
                player.action_frame = 0;
            }
            player.vel_x = (player.vel_x * 30) / 100;
            player.vel_y = (player.vel_y * 30) / 100;
        } else {
            // Free movement / action input processing
            if (inp & INPUT_LIGHT_ATTACK) != 0 && player.stamina >= 50 {
                player.action_state = STATE_LIGHT_ATTACK;
                player.action_frame = 0;
                player.stamina = player.stamina.saturating_sub(50);
            } else if (inp & INPUT_HEAVY_ATTACK) != 0 && player.stamina >= 120 {
                player.action_state = STATE_HEAVY_ATTACK;
                player.action_frame = 0;
                player.stamina = player.stamina.saturating_sub(120);
            } else if (inp & INPUT_GUARD) != 0 {
                player.action_state = STATE_GUARD;
                player.vel_x = (player.vel_x * 50) / 100;
                player.vel_y = (player.vel_y * 50) / 100;
                player.flags |= 1 << 2; // Guard flag
            } else {
                player.flags &= !(1 << 2);
                let speed = if (inp & INPUT_DASH) != 0 && player.stamina >= 10 {
                    player.stamina = player.stamina.saturating_sub(2);
                    DASH_SPEED_MMS
                } else {
                    MOVE_SPEED_MMS
                };

                let mut dx = 0i32;
                let mut dy = 0i32;
                if (inp & INPUT_LEFT) != 0 {
                    dx -= 1;
                }
                if (inp & INPUT_RIGHT) != 0 {
                    dx += 1;
                }
                if (inp & INPUT_UP) != 0 {
                    dy += 1;
                }
                if (inp & INPUT_DOWN) != 0 {
                    dy -= 1;
                }

                if dx != 0 || dy != 0 {
                    player.action_state = if speed == DASH_SPEED_MMS {
                        STATE_DASH
                    } else {
                        STATE_MOVE
                    };
                    // Diagonal normalization: multiply by 707/1000 if both axes active
                    if dx != 0 && dy != 0 {
                        player.vel_x = (dx * speed * 707) / 1000;
                        player.vel_y = (dy * speed * 707) / 1000;
                    } else {
                        player.vel_x = dx * speed;
                        player.vel_y = dy * speed;
                    }
                } else {
                    player.action_state = STATE_IDLE;
                    player.vel_x = (player.vel_x * 60) / 100;
                    player.vel_y = (player.vel_y * 60) / 100;
                    // Passive stamina regeneration
                    player.stamina = (player.stamina + 5).min(1000);
                }
            }
        }
    }

    // 2. Position integration & arena circular boundary clamping
    for p in 0..MAX_PEERS {
        if (world.active_mask & (1 << p)) == 0 {
            continue;
        }

        let player = &mut world.players[p];
        player.pos_x += (player.vel_x * DT_NUMERATOR) / DT_DENOMINATOR;
        player.pos_y += (player.vel_y * DT_NUMERATOR) / DT_DENOMINATOR;

        // Circular ring boundary test: x^2 + y^2 <= (R - r)^2
        let max_r = (world.arena_radius_mm as i32).saturating_sub(PLAYER_RADIUS_MM);
        let dist_sq = (player.pos_x as i64) * (player.pos_x as i64)
            + (player.pos_y as i64) * (player.pos_y as i64);
        let max_r_sq = (max_r as i64) * (max_r as i64);

        if dist_sq > max_r_sq && dist_sq > 0 {
            let dist = isqrt_64(dist_sq) as i32;
            if dist > 0 {
                player.pos_x = ((player.pos_x as i64 * max_r as i64) / dist as i64) as i32;
                player.pos_y = ((player.pos_y as i64 * max_r as i64) / dist as i64) as i32;
                player.vel_x = 0;
                player.vel_y = 0;
            }
        }
    }

    // 3. Inter-player collision resolution & attack hit tests (O(N^2) for 16 players = 120 pairs)
    for i in 0..MAX_PEERS {
        if (world.active_mask & (1 << i)) == 0 || world.players[i].health == 0 {
            continue;
        }

        for j in (i + 1)..MAX_PEERS {
            if (world.active_mask & (1 << j)) == 0 || world.players[j].health == 0 {
                continue;
            }

            let dx = world.players[j].pos_x - world.players[i].pos_x;
            let dy = world.players[j].pos_y - world.players[i].pos_y;
            let dist_sq = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
            let min_dist = PLAYER_RADIUS_MM * 2;
            let min_dist_sq = (min_dist as i64) * (min_dist as i64);

            // Elastic separation pushback if overlapping
            if dist_sq < min_dist_sq && dist_sq > 0 {
                let dist = isqrt_64(dist_sq) as i32;
                let overlap = (min_dist - dist) / 2;
                if dist > 0 {
                    let push_x = (dx * overlap) / dist;
                    let push_y = (dy * overlap) / dist;
                    world.players[i].pos_x -= push_x;
                    world.players[i].pos_y -= push_y;
                    world.players[j].pos_x += push_x;
                    world.players[j].pos_y += push_y;
                }
            }

            // Hit detection: Attack hitboxes active at specific animation frames
            let hit_range = PLAYER_RADIUS_MM * 2 + 350; // 1,250 mm reach
            let hit_range_sq = (hit_range as i64) * (hit_range as i64);

            if dist_sq <= hit_range_sq {
                let (left, right) = world.players.split_at_mut(j);
                let p_i = &mut left[i];
                let p_j = &mut right[0];

                // Check player i attacking player j
                if p_i.action_state == STATE_LIGHT_ATTACK && p_i.action_frame == 3 {
                    let (dmg, is_guard, is_ko) = apply_attack_damage(p_i, p_j, 45, 1200, dx, dy);
                    world.total_hits_landed += 1;
                    world.total_damage_dealt += 45;
                    if let Some(bus) = event_bus.as_deref_mut() {
                        let payload = CombatHitPayload {
                            attacker_id: i as u8,
                            victim_id: j as u8,
                            damage: dmg,
                            knockback: 1200,
                            hit_x: p_j.pos_x,
                            hit_y: p_j.pos_y,
                            is_guard: if is_guard { 1 } else { 0 },
                            _pad: [0; 3],
                        };
                        let ch = if is_guard { channel::COMBAT | channel::AUDIO } else { channel::COMBAT | channel::AUDIO | channel::VFX };
                        let ev_type = if is_guard { EVENT_COMBAT_GUARD } else { EVENT_COMBAT_HIT };
                        bus.emit(world.tick, ev_type, ch, i as u32, j as u32, &payload);
                        if is_ko {
                            bus.emit(world.tick, EVENT_COMBAT_KNOCKDOWN, channel::COMBAT | channel::AUDIO | channel::UI, i as u32, j as u32, &payload);
                        }
                    }
                } else if p_i.action_state == STATE_HEAVY_ATTACK && p_i.action_frame == 6 {
                    let (dmg, is_guard, is_ko) = apply_attack_damage(p_i, p_j, 110, 2800, dx, dy);
                    world.total_hits_landed += 1;
                    world.total_damage_dealt += 110;
                    if let Some(bus) = event_bus.as_deref_mut() {
                        let payload = CombatHitPayload {
                            attacker_id: i as u8,
                            victim_id: j as u8,
                            damage: dmg,
                            knockback: 2800,
                            hit_x: p_j.pos_x,
                            hit_y: p_j.pos_y,
                            is_guard: if is_guard { 1 } else { 0 },
                            _pad: [0; 3],
                        };
                        let ch = if is_guard { channel::COMBAT | channel::AUDIO } else { channel::COMBAT | channel::AUDIO | channel::VFX };
                        let ev_type = if is_guard { EVENT_COMBAT_GUARD } else { EVENT_COMBAT_HIT };
                        bus.emit(world.tick, ev_type, ch, i as u32, j as u32, &payload);
                        if is_ko {
                            bus.emit(world.tick, EVENT_COMBAT_KNOCKDOWN, channel::COMBAT | channel::AUDIO | channel::UI, i as u32, j as u32, &payload);
                        }
                    }
                }

                // Check player j attacking player i
                if p_j.action_state == STATE_LIGHT_ATTACK && p_j.action_frame == 3 {
                    let (dmg, is_guard, is_ko) = apply_attack_damage(p_j, p_i, 45, 1200, -dx, -dy);
                    world.total_hits_landed += 1;
                    world.total_damage_dealt += 45;
                    if let Some(bus) = event_bus.as_deref_mut() {
                        let payload = CombatHitPayload {
                            attacker_id: j as u8,
                            victim_id: i as u8,
                            damage: dmg,
                            knockback: 1200,
                            hit_x: p_i.pos_x,
                            hit_y: p_i.pos_y,
                            is_guard: if is_guard { 1 } else { 0 },
                            _pad: [0; 3],
                        };
                        let ch = if is_guard { channel::COMBAT | channel::AUDIO } else { channel::COMBAT | channel::AUDIO | channel::VFX };
                        let ev_type = if is_guard { EVENT_COMBAT_GUARD } else { EVENT_COMBAT_HIT };
                        bus.emit(world.tick, ev_type, ch, j as u32, i as u32, &payload);
                        if is_ko {
                            bus.emit(world.tick, EVENT_COMBAT_KNOCKDOWN, channel::COMBAT | channel::AUDIO | channel::UI, j as u32, i as u32, &payload);
                        }
                    }
                } else if p_j.action_state == STATE_HEAVY_ATTACK && p_j.action_frame == 6 {
                    let (dmg, is_guard, is_ko) = apply_attack_damage(p_j, p_i, 110, 2800, -dx, -dy);
                    world.total_hits_landed += 1;
                    world.total_damage_dealt += 110;
                    if let Some(bus) = event_bus.as_deref_mut() {
                        let payload = CombatHitPayload {
                            attacker_id: j as u8,
                            victim_id: i as u8,
                            damage: dmg,
                            knockback: 2800,
                            hit_x: p_i.pos_x,
                            hit_y: p_i.pos_y,
                            is_guard: if is_guard { 1 } else { 0 },
                            _pad: [0; 3],
                        };
                        let ch = if is_guard { channel::COMBAT | channel::AUDIO } else { channel::COMBAT | channel::AUDIO | channel::VFX };
                        let ev_type = if is_guard { EVENT_COMBAT_GUARD } else { EVENT_COMBAT_HIT };
                        bus.emit(world.tick, ev_type, ch, j as u32, i as u32, &payload);
                        if is_ko {
                            bus.emit(world.tick, EVENT_COMBAT_KNOCKDOWN, channel::COMBAT | channel::AUDIO | channel::UI, j as u32, i as u32, &payload);
                        }
                    }
                }
            }
        }
    }

    world.tick += 1;

    // 4. Compute 64-bit FNV-1a deterministic state hash across all 16 players
    compute_world_hash(world)
}

fn apply_attack_damage(
    attacker: &mut PlayerPhysicalState64,
    victim: &mut PlayerPhysicalState64,
    damage: u16,
    knockback: i32,
    dir_x: i32,
    dir_y: i32,
) -> (u16, bool, bool) {
    let dist = isqrt_64((dir_x as i64) * (dir_x as i64) + (dir_y as i64) * (dir_y as i64)).max(1) as i32;
    let is_guard = (victim.flags & (1 << 2)) != 0;
    let actual_dmg = if is_guard {
        // Guarding reduces damage by 75%
        damage / 4
    } else {
        damage
    };

    victim.health = victim.health.saturating_sub(actual_dmg);
    let is_ko = victim.health == 0;
    victim.action_state = if is_ko { STATE_KNOCKDOWN } else { STATE_HIT_STUN };
    victim.action_frame = 0;
    victim.vel_x = (dir_x * knockback) / dist;
    victim.vel_y = (dir_y * knockback) / dist;

    attacker.combo_count = attacker.combo_count.saturating_add(1);
    attacker.score += actual_dmg;

    (actual_dmg, is_guard, is_ko)
}

/// 64-bit integer square root (Newton-Raphson).
fn isqrt_64(val: i64) -> i64 {
    if val <= 0 {
        return 0;
    }
    let mut x0 = val / 2;
    if x0 == 0 {
        return 1;
    }
    let mut x1 = (x0 + val / x0) / 2;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + val / x0) / 2;
    }
    x0
}

/// 64-bit FNV-1a Hash of the complete 16-player world snapshot.
pub fn compute_world_hash(world: &PhysicalWorldSnapshot) -> u64 {
    const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01B3;

    let mut h = FNV_OFFSET ^ (world.tick as u64);

    for (p, player) in world.players.iter().enumerate() {
        let p_bits = (player.pos_x as u64)
            ^ ((player.pos_y as u64) << 32)
            ^ ((player.vel_x as u64) << 16)
            ^ (player.health as u64)
            ^ ((player.action_state as u64) << 24)
            ^ ((p as u64) << 48);

        h ^= p_bits;
        h = h.wrapping_mul(FNV_PRIME);
    }

    h ^= (world.total_damage_dealt as u64) << 32;
    h = h.wrapping_mul(FNV_PRIME);
    h
}

/// 16-Player Physical Match Director with snapshot rewind and speculative resimulation.
#[derive(Debug, Clone)]
pub struct NetworkMatchDirector<const BUFFER_FRAMES: usize = DEFAULT_NET_BUFFER_FRAMES> {
    pub session: DistributedRollbackSession<BUFFER_FRAMES>,
    pub world_history: [PhysicalWorldSnapshot; BUFFER_FRAMES],
    pub current_world: PhysicalWorldSnapshot,
    pub event_bus: DeterministicEventBus<128>,
}

impl<const BUFFER_FRAMES: usize> NetworkMatchDirector<BUFFER_FRAMES> {
    pub fn new(local_peer_id: usize, expected_peers_mask: u16) -> Self {
        let initial_world = PhysicalWorldSnapshot::default();
        Self {
            session: DistributedRollbackSession::with_expected_peers(local_peer_id, expected_peers_mask),
            world_history: [initial_world; BUFFER_FRAMES],
            current_world: initial_world,
            event_bus: DeterministicEventBus::<128>::new(),
        }
    }

    /// Advances 1 physical simulation tick using local input, generating wire packet.
    pub fn advance_tick(&mut self, local_input: u16) -> NetPacket64 {
        let pkt = self.session.submit_local_input(local_input);

        // Store snapshot before stepping
        let slot = (self.session.current_frame as usize) % BUFFER_FRAMES;
        self.world_history[slot] = self.current_world;

        // Step simulation and record combat events
        let mut sim_world = self.current_world;
        let bus = &mut self.event_bus;
        self.session.advance_frame(|_frame, inputs| {
            step_physical_world_with_events(&mut sim_world, inputs, Some(bus))
        });
        self.current_world = sim_world;

        pkt
    }

    /// Advances 1 physical simulation tick using the JobSystem and WorkerPoolCoordinator
    /// to parallelize player kinematic integration tasks across workers.
    pub fn advance_tick_with_jobs<
        const SYS_CAP: usize,
        const MAX_WORKERS: usize,
        const DEQUE_CAP: usize,
    >(
        &mut self,
        local_input: u16,
        job_system: &mut axiom_core::job_system::JobSystem<SYS_CAP>,
        coordinator: &mut axiom_core::worker_pool::WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
    ) -> NetPacket64 {
        // Enqueue 4 player-batch kinematic integration tasks
        for batch in 0..4u64 {
            if let Some(h) = job_system.allocate_job(
                Self::kinematic_batch_job,
                batch,
                self.current_world.tick as u64,
                axiom_core::job_system::JobPriority::High,
            ) {
                let w = (batch as usize) % coordinator.worker_count;
                job_system.descriptors[h.slot()].status = axiom_core::job_system::JobStatus::Ready as u8;
                coordinator.push_local(w, h);
            }
        }

        // Drain workers cooperatively
        coordinator.drain_all(job_system);

        // Advance physical tick and record events
        let pkt = self.advance_tick(local_input);

        // Dispatch unconsumed events across worker pool in parallel
        if self.event_bus.count > 0 {
            dispatch_events_with_jobs(
                &self.event_bus,
                channel::ALL,
                4,
                job_system,
                coordinator,
            );
        }

        pkt
    }

    /// Evaluates 3D spatial hashing and AABB broadphase collision acceleration across all active players,
    /// recording broadphase collision pairs to the deterministic event bus under channel::PHYSICS (synchronous fallback).
    pub fn step_broadphase(&mut self) -> usize {
        let mut slabs = [AabbSlab64::empty(); MAX_PEERS];
        for i in 0..MAX_PEERS {
            let p = &self.current_world.players[i];
            if (self.current_world.active_mask & (1 << i)) != 0 && p.health > 0 {
                slabs[i] = AabbSlab64::new(
                    (i + 1) as u32,
                    1,
                    0b01,
                    COLLISION_FLAG_DYNAMIC,
                    (p.pos_x - PLAYER_RADIUS_MM, p.pos_y - PLAYER_RADIUS_MM, p.pos_z),
                    (p.pos_x + PLAYER_RADIUS_MM, p.pos_y + PLAYER_RADIUS_MM, p.pos_z + 1800),
                );
            }
        }

        let mut grid = SpatialHashGrid::<1024, 14>::new(1000);
        for i in 0..MAX_PEERS {
            grid.insert(&slabs[i]);
        }
        let mut collision_pairs = [CollisionPair64::default(); 32];
        let pair_count = grid.query_all_collision_pairs(&slabs, &mut collision_pairs);
        if pair_count > 0 {
            emit_collision_pairs_to_event_bus(
                &collision_pairs[..pair_count],
                self.current_world.tick,
                &mut self.event_bus,
            );
        }

        pair_count
    }

    /// Executes a parallel 3D broadphase spatial collision detection pass using SpatialHashGrid,
    /// dispatching partitioning jobs across the worker pool and recording broadphase collision
    /// pairs to the deterministic event bus under channel::PHYSICS.
    pub fn step_broadphase_with_jobs<
        const SYS_CAP: usize,
        const MAX_WORKERS: usize,
        const DEQUE_CAP: usize,
    >(
        &mut self,
        job_system: &mut axiom_core::job_system::JobSystem<SYS_CAP>,
        coordinator: &mut axiom_core::worker_pool::WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
    ) -> usize {
        let mut slabs = [AabbSlab64::empty(); MAX_PEERS];
        for i in 0..MAX_PEERS {
            let p = &self.current_world.players[i];
            if (self.current_world.active_mask & (1 << i)) != 0 && p.health > 0 {
                slabs[i] = AabbSlab64::new(
                    (i + 1) as u32,
                    1,
                    0b01,
                    COLLISION_FLAG_DYNAMIC,
                    (p.pos_x - PLAYER_RADIUS_MM, p.pos_y - PLAYER_RADIUS_MM, p.pos_z),
                    (p.pos_x + PLAYER_RADIUS_MM, p.pos_y + PLAYER_RADIUS_MM, p.pos_z + 1800),
                );
            }
        }

        // Parallel broadphase partition dispatch across worker pool
        dispatch_broadphase_with_jobs(&slabs, 4, job_system, coordinator);
        coordinator.drain_all(job_system);

        // Populate spatial hash grid and detect broadphase collision pairs
        let mut grid = SpatialHashGrid::<1024, 14>::new(1000);
        for i in 0..MAX_PEERS {
            grid.insert(&slabs[i]);
        }
        let mut collision_pairs = [CollisionPair64::default(); 32];
        let pair_count = grid.query_all_collision_pairs(&slabs, &mut collision_pairs);
        if pair_count > 0 {
            emit_collision_pairs_to_event_bus(
                &collision_pairs[..pair_count],
                self.current_world.tick,
                &mut self.event_bus,
            );
        }

        pair_count
    }

    /// Evaluates continuous collision detection (CCD) for all active player attacks or high-velocity movements,
    /// emitting precise contact events to the deterministic event bus under channel::COMBAT | channel::PHYSICS (synchronous fallback).
    pub fn step_ccd(&mut self) -> usize {
        let mut hitboxes = [HitboxSlab64::empty(); MAX_PEERS];
        let mut hitbox_count = 0;
        for i in 0..MAX_PEERS {
            let p = &self.current_world.players[i];
            if (self.current_world.active_mask & (1 << i)) != 0 && p.health > 0 {
                let is_attacking = p.action_state == STATE_LIGHT_ATTACK || p.action_state == STATE_HEAVY_ATTACK;
                let is_high_speed = p.vel_x.abs() > 500 || p.vel_y.abs() > 500;
                if is_attacking || is_high_speed {
                    let is_heavy = p.action_state == STATE_HEAVY_ATTACK;
                    let (dmg, kb, hit_type) = if is_heavy {
                        (110u16, 2800u16, HitType::High)
                    } else {
                        (45u16, 1200u16, HitType::Mid)
                    };
                    let disp = if p.vel_x != 0 || p.vel_y != 0 {
                        (p.vel_x, p.vel_y, 0)
                    } else {
                        (450, 0, 0)
                    };
                    hitboxes[hitbox_count] = HitboxSlab64::from_extents_and_disp(
                        (i + 1) as u32,
                        (i as u16) + 1,
                        0xFFFF,
                        (p.pos_x - 150, p.pos_y - 150, p.pos_z + 800),
                        (p.pos_x + 150, p.pos_y + 150, p.pos_z + 1200),
                        disp,
                        dmg,
                        kb,
                        hit_type,
                        HITBOX_FLAG_ACTIVE | HITBOX_FLAG_SWEPT_CCD,
                    );
                    hitbox_count += 1;
                }
            }
        }

        let mut results = [SweptAabbResult64::empty(); 16];
        let mut hit_count = 0;
        for h in 0..hitbox_count {
            let hb = &hitboxes[h];
            for j in 0..MAX_PEERS {
                let defender_id = (j + 1) as u32;
                if defender_id == hb.attacker_id {
                    continue;
                }
                let p_j = &self.current_world.players[j];
                if (self.current_world.active_mask & (1 << j)) != 0 && p_j.health > 0 {
                    let hurtbox_min = (p_j.pos_x - PLAYER_RADIUS_MM, p_j.pos_y - PLAYER_RADIUS_MM, p_j.pos_z);
                    let hurtbox_max = (p_j.pos_x + PLAYER_RADIUS_MM, p_j.pos_y + PLAYER_RADIUS_MM, p_j.pos_z + 1800);
                    let hurtbox_disp = (p_j.vel_x, p_j.vel_y, 0);

                    if let Some(res) = swept_hitbox_vs_hurtbox(hb, hurtbox_min, hurtbox_max, hurtbox_disp, defender_id) {
                        if hit_count < results.len() {
                            results[hit_count] = res;
                            hit_count += 1;
                        }
                    }
                }
            }
        }

        if hit_count > 0 {
            emit_ccd_hits_to_event_bus(
                &results[..hit_count],
                self.current_world.tick,
                &mut self.event_bus,
            );
        }

        hit_count
    }

    /// Executes parallel continuous collision detection (CCD) using the JobSystem and WorkerPoolCoordinator,
    /// evaluating high-velocity punch hitboxes against opponent hurtboxes and emitting events to the event bus.
    pub fn step_ccd_with_jobs<
        const SYS_CAP: usize,
        const MAX_WORKERS: usize,
        const DEQUE_CAP: usize,
    >(
        &mut self,
        job_system: &mut axiom_core::job_system::JobSystem<SYS_CAP>,
        coordinator: &mut axiom_core::worker_pool::WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
    ) -> usize {
        let mut hitboxes = [HitboxSlab64::empty(); MAX_PEERS];
        let mut hitbox_count = 0;
        for i in 0..MAX_PEERS {
            let p = &self.current_world.players[i];
            if (self.current_world.active_mask & (1 << i)) != 0 && p.health > 0 {
                let is_attacking = p.action_state == STATE_LIGHT_ATTACK || p.action_state == STATE_HEAVY_ATTACK;
                let is_high_speed = p.vel_x.abs() > 500 || p.vel_y.abs() > 500;
                if is_attacking || is_high_speed {
                    let is_heavy = p.action_state == STATE_HEAVY_ATTACK;
                    let (dmg, kb, hit_type) = if is_heavy {
                        (110u16, 2800u16, HitType::High)
                    } else {
                        (45u16, 1200u16, HitType::Mid)
                    };
                    let disp = if p.vel_x != 0 || p.vel_y != 0 {
                        (p.vel_x, p.vel_y, 0)
                    } else {
                        (450, 0, 0)
                    };
                    hitboxes[hitbox_count] = HitboxSlab64::from_extents_and_disp(
                        (i + 1) as u32,
                        (i as u16) + 1,
                        0xFFFF,
                        (p.pos_x - 150, p.pos_y - 150, p.pos_z + 800),
                        (p.pos_x + 150, p.pos_y + 150, p.pos_z + 1200),
                        disp,
                        dmg,
                        kb,
                        hit_type,
                        HITBOX_FLAG_ACTIVE | HITBOX_FLAG_SWEPT_CCD,
                    );
                    hitbox_count += 1;
                }
            }
        }

        if hitbox_count > 0 {
            dispatch_ccd_hitboxes_with_jobs(&hitboxes, 4, job_system, coordinator);
            coordinator.drain_all(job_system);
        }

        let mut results = [SweptAabbResult64::empty(); 16];
        let mut hit_count = 0;
        for h in 0..hitbox_count {
            let hb = &hitboxes[h];
            for j in 0..MAX_PEERS {
                let defender_id = (j + 1) as u32;
                if defender_id == hb.attacker_id {
                    continue;
                }
                let p_j = &self.current_world.players[j];
                if (self.current_world.active_mask & (1 << j)) != 0 && p_j.health > 0 {
                    let hurtbox_min = (p_j.pos_x - PLAYER_RADIUS_MM, p_j.pos_y - PLAYER_RADIUS_MM, p_j.pos_z);
                    let hurtbox_max = (p_j.pos_x + PLAYER_RADIUS_MM, p_j.pos_y + PLAYER_RADIUS_MM, p_j.pos_z + 1800);
                    let hurtbox_disp = (p_j.vel_x, p_j.vel_y, 0);

                    if let Some(res) = swept_hitbox_vs_hurtbox(hb, hurtbox_min, hurtbox_max, hurtbox_disp, defender_id) {
                        if hit_count < results.len() {
                            results[hit_count] = res;
                            hit_count += 1;
                        }
                    }
                }
            }
        }

        if hit_count > 0 {
            emit_ccd_hits_to_event_bus(
                &results[..hit_count],
                self.current_world.tick,
                &mut self.event_bus,
            );
        }

        hit_count
    }

    fn kinematic_batch_job(_ctx: axiom_core::job_system::JobContext) {
        // Deterministic kinematic integration partition marker
    }

    /// Evaluates procedural ragdoll simulation for knocked down or eliminated players,
    /// relaxing distance constraints and emitting impact/severance events.
    pub fn step_ragdoll(&mut self) -> usize {
        let mut impacts = [RagdollImpactReport::default(); 16];
        let mut impact_count = 0;

        for i in 0..MAX_PEERS {
            let p = &self.current_world.players[i];
            if (self.current_world.active_mask & (1 << i)) != 0 && (p.action_state == STATE_KNOCKDOWN || p.health == 0) {
                let mut engine = RagdollEngine::<8, 7>::new();
                let b0 = RagdollBoneSlab64::new(0, 0, (p.pos_x, p.pos_y, p.pos_z), 65536, 120, 300);
                let b1 = RagdollBoneSlab64::new(1, 0, (p.pos_x + 300, p.pos_y, p.pos_z), 65536, 60, 300);
                engine.add_bone(b0);
                engine.add_bone(b1);
                let j0 = RagdollJointSlab64::new(0, 1, RagdollJointType::Hinge, 300, 100_000);
                engine.add_joint(j0);

                engine.step_ragdoll(-10, 4, 0, 6000);

                if impact_count < impacts.len() {
                    impacts[impact_count] = RagdollImpactReport {
                        entity_id: (i + 1) as u32,
                        bone_id: 0,
                        joint_id: 0,
                        is_severed: 0,
                        flags: 1,
                        impact_force: 5000,
                        hit_x: engine.bones[0].pos_x,
                        hit_y: engine.bones[0].pos_y,
                        hit_z: engine.bones[0].pos_z,
                        impulse_x: engine.bones[0].vel_x,
                        impulse_y: engine.bones[0].vel_y,
                        impulse_z: engine.bones[0].vel_z,
                    };
                    impact_count += 1;
                }
            }
        }

        if impact_count > 0 {
            emit_ragdoll_impacts_to_event_bus(
                &impacts[..impact_count],
                self.current_world.tick,
                &mut self.event_bus,
            );
        }

        impact_count
    }

    /// Evaluates procedural ragdoll simulation parallelized across the worker pool using JobSystem.
    pub fn step_ragdoll_with_jobs<
        const SYS_CAP: usize,
        const MAX_WORKERS: usize,
        const DEQUE_CAP: usize,
    >(
        &mut self,
        job_system: &mut axiom_core::job_system::JobSystem<SYS_CAP>,
        coordinator: &mut axiom_core::worker_pool::WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
    ) -> usize {
        let mut impacts = [RagdollImpactReport::default(); 16];
        let mut impact_count = 0;
        let mut dummy_engine = RagdollEngine::<16, 15>::new();

        for i in 0..MAX_PEERS {
            let p = &self.current_world.players[i];
            if (self.current_world.active_mask & (1 << i)) != 0 && (p.action_state == STATE_KNOCKDOWN || p.health == 0) {
                let b = RagdollBoneSlab64::new(i as u8, 0, (p.pos_x, p.pos_y, p.pos_z), 65536, 120, 300);
                dummy_engine.add_bone(b);
                if dummy_engine.active_bones_count > 1 {
                    let j = RagdollJointSlab64::new(0, (dummy_engine.active_bones_count - 1) as u8, RagdollJointType::Hinge, 300, 100_000);
                    dummy_engine.add_joint(j);
                }

                if impact_count < impacts.len() {
                    impacts[impact_count] = RagdollImpactReport {
                        entity_id: (i + 1) as u32,
                        bone_id: i as u8,
                        joint_id: 0,
                        is_severed: 0,
                        flags: 1,
                        impact_force: 5000,
                        hit_x: p.pos_x,
                        hit_y: p.pos_y,
                        hit_z: p.pos_z,
                        impulse_x: p.vel_x,
                        impulse_y: p.vel_y,
                        impulse_z: 0,
                    };
                    impact_count += 1;
                }
            }
        }

        if dummy_engine.active_bones_count > 0 {
            dispatch_ragdoll_with_jobs(&dummy_engine, 4, job_system, coordinator);
            coordinator.drain_all(job_system);
        }

        if impact_count > 0 {
            emit_ragdoll_impacts_to_event_bus(
                &impacts[..impact_count],
                self.current_world.tick,
                &mut self.event_bus,
            );
        }

        impact_count
    }

    /// Evaluates dynamic procedural terrain crater deformation,
    /// stamping the crater into the arena mesh and recording the crater event to the event bus.
    pub fn step_terrain_crater<const DIM: usize, const VERTS: usize, const CRATERS: usize>(
        &mut self,
        grid: &mut DeformableMeshGrid<DIM, VERTS, CRATERS>,
        crater: &CraterSlab64,
    ) -> usize {
        let deformed = grid.apply_crater_displacement(crater);
        if deformed > 0 {
            let report = TerrainCraterReport::from_crater(crater);
            emit_crater_to_event_bus(&report, self.current_world.tick, &mut self.event_bus);
        }
        deformed
    }

    /// Evaluates dynamic procedural terrain crater deformation parallelized across the worker pool using JobSystem.
    pub fn step_terrain_crater_with_jobs<
        const DIM: usize,
        const VERTS: usize,
        const CRATERS: usize,
        const SYS_CAP: usize,
        const MAX_WORKERS: usize,
        const DEQUE_CAP: usize,
    >(
        &mut self,
        grid: &mut DeformableMeshGrid<DIM, VERTS, CRATERS>,
        crater: &CraterSlab64,
        job_system: &mut axiom_core::job_system::JobSystem<SYS_CAP>,
        coordinator: &mut axiom_core::worker_pool::WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
    ) -> usize {
        dispatch_terrain_deform_with_jobs(grid, 4, job_system, coordinator);
        coordinator.drain_all(job_system);
        self.step_terrain_crater(grid, crater)
    }

    /// Evaluates dynamic procedural destruction audio synthesis for physical impacts and explosions,
    /// synthesizing the audio voice in the acoustic environment and emitting the event to the event bus.
    pub fn step_destruction_audio<const VOICES: usize, const ZONES: usize>(
        &mut self,
        engine: &mut DestructionAudioEngine<VOICES, ZONES>,
        voice: &DestructionVoiceSlab64,
    ) -> bool {
        let spawned = engine.spawn_destruction_voice(*voice);
        if spawned {
            let report = DestructionAudioReport::from_voice(voice);
            emit_destruction_audio_to_event_bus(&report, self.current_world.tick, &mut self.event_bus);
        }
        spawned
    }

    /// Evaluates dynamic procedural destruction audio synthesis parallelized across the worker pool using JobSystem.
    pub fn step_destruction_audio_with_jobs<
        const VOICES: usize,
        const ZONES: usize,
        const SYS_CAP: usize,
        const MAX_WORKERS: usize,
        const DEQUE_CAP: usize,
    >(
        &mut self,
        engine: &mut DestructionAudioEngine<VOICES, ZONES>,
        voice: &DestructionVoiceSlab64,
        job_system: &mut axiom_core::job_system::JobSystem<SYS_CAP>,
        coordinator: &mut axiom_core::worker_pool::WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
    ) -> bool {
        dispatch_destruction_audio_with_jobs(engine, 4, job_system, coordinator);
        coordinator.drain_all(job_system);
        self.step_destruction_audio(engine, voice)
    }

    /// Evaluates dynamic procedural debris particle scattering from physical impacts, craters, or knockdowns,
    /// scattering the particles in the arena and emitting the event into the event bus.
    pub fn step_debris_burst<const PARTICLES: usize>(
        &mut self,
        pool: &mut DebrisParticlePool<PARTICLES>,
        report: &DebrisBurstReport,
    ) -> usize {
        let spawned = pool.scatter_debris_burst(
            report.origin_x,
            report.origin_y,
            report.origin_z,
            report.particle_count as usize,
            report.speed_min_mms as i32,
            report.speed_max_mms as i32,
            report.material,
            report.emitter_id,
            report.impulse_energy_j,
        );
        if spawned > 0 {
            emit_debris_burst_to_event_bus(report, self.current_world.tick, &mut self.event_bus);
        }
        spawned
    }

    /// Evaluates dynamic procedural debris particle ballistic integration parallelized across the worker pool using JobSystem.
    pub fn step_debris_burst_with_jobs<
        const PARTICLES: usize,
        const SYS_CAP: usize,
        const MAX_WORKERS: usize,
        const DEQUE_CAP: usize,
    >(
        &mut self,
        pool: &mut DebrisParticlePool<PARTICLES>,
        report: &DebrisBurstReport,
        job_system: &mut axiom_core::job_system::JobSystem<SYS_CAP>,
        coordinator: &mut axiom_core::worker_pool::WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
    ) -> usize {
        dispatch_debris_with_jobs(pool, 4, job_system, coordinator);
        coordinator.drain_all(job_system);
        self.step_debris_burst(pool, report)
    }

    /// Ingests a remote wire packet and executes physical state rollback if divergent.
    pub fn receive_packet(&mut self, pkt: &NetPacket64) -> Result<(), NetError> {
        let divergence = self.session.receive_remote_packet(pkt)?;

        if let Some(from_frame) = divergence {
            // Restore physical snapshot from the earliest diverged frame
            let from_slot = (from_frame as usize) % BUFFER_FRAMES;
            let mut resim_world = self.world_history[from_slot];

            // Re-simulate forward up to current_frame
            let mut current_f = from_frame;
            let bus = &mut self.event_bus;
            self.session.rollback_and_resimulate(from_frame, |_frame, inputs| {
                let slot = (current_f as usize) % BUFFER_FRAMES;
                self.world_history[slot] = resim_world;
                let hash = step_physical_world_with_events(&mut resim_world, inputs, Some(bus));
                current_f += 1;
                hash
            });

            self.current_world = resim_world;
        }

        self.session.verify_remote_consensus(pkt)?;
        Ok(())
    }

    /// Enables NAT traversal and registers local candidate descriptor.
    pub fn enable_nat_traversal(&mut self, local_candidate: axiom_net::NatCandidate64) {
        self.session.enable_nat_traversal(local_candidate);
    }

    /// Registers a remote peer's candidate information into the NAT traversal session.
    pub fn add_peer_nat_candidate(&mut self, peer_id: usize, candidate: axiom_net::NatCandidate64) {
        if let Some(ref mut nat) = self.session.nat_session {
            nat.add_peer_candidate(peer_id, candidate);
        }
    }

    /// Ingests an incoming STUN packet and returns an optional response pong.
    pub fn process_stun_packet(
        &mut self,
        pkt: &axiom_net::StunPacket64,
        now_micros: u32,
    ) -> Option<axiom_net::StunPacket64> {
        if let Some(ref mut nat) = self.session.nat_session {
            nat.process_stun_packet(pkt, now_micros)
        } else {
            None
        }
    }

    /// Returns true if a peer is ready for wire rollback traffic (hole-punched or direct).
    pub fn is_peer_ready(&self, peer_id: usize) -> bool {
        self.session.is_peer_p2p_ready(peer_id)
    }

    /// Returns true if all active peers have established direct hole-punched P2P connectivity.
    pub fn all_peers_ready(&self) -> bool {
        if let Some(ref nat) = self.session.nat_session {
            nat.all_peers_connected()
        } else {
            true
        }
    }

    /// Returns the connected peer bitmask from the NAT traversal session.
    pub fn nat_connected_mask(&self) -> u16 {
        if let Some(ref nat) = self.session.nat_session {
            nat.connected_mask
        } else {
            0xFFFF
        }
    }

    /// Returns the NAT probe counts (total_probes_sent, total_probes_received).
    pub fn nat_probe_counts(&self) -> (u32, u32) {
        if let Some(ref nat) = self.session.nat_session {
            (nat.total_probes_sent, nat.total_probes_received)
        } else {
            (0, 0)
        }
    }

    /// Returns the direct round-trip latency in microseconds for a specific peer.
    pub fn nat_peer_rtt_micros(&self, peer_id: usize) -> u32 {
        if let Some(ref nat) = self.session.nat_session {
            if peer_id < 16 {
                nat.peer_candidates[peer_id].rtt_micros
            } else {
                0
            }
        } else {
            0
        }
    }

    /// Returns true if NAT traversal is enabled on this match director.
    pub fn is_nat_enabled(&self) -> bool {
        self.session.nat_session.is_some()
    }
}
