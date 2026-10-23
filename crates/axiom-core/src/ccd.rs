//! # Continuous Collision Detection (CCD) & Nightmare Boxing Hitbox Physics (`axiom-core::ccd`)
//!
//! Provides zero-float, 64-byte L1 cache-aligned swept continuous collision detection
//! for high-velocity projectiles, mecha rocket punches, and extreme hitboxes.
//!
//! ## Architectural Guarantees
//! - **0-Float in Core**: All coordinates and linear displacements in integer millimeters (`mm: i32`).
//!   Time of Impact (TOI) normalized in $Q16.16$ fixed-point arithmetic (`0 = 0.0`, `65536 = 1.0`).
//! - **64B L1 Cache Alignment**: Both [`HitboxSlab64`] and [`SweptAabbResult64`] are `#[repr(C, align(64))]`
//!   with `size_of == 64` and zero compiler-inserted implicit padding.
//! - **Zero-Allocation**: Analytical closed-form Minkowski Sum + Slab Raycast in $O(1)$ constant time.
//! - **Tunneling Prevention**: Detects sub-frame contact even when velocity exceeds 30 m/s (>500 mm/tick).

use core::mem::{align_of, size_of};

/// Hitbox physics flag bitmasks
pub const HITBOX_FLAG_ACTIVE: u8      = 1 << 0;
pub const HITBOX_FLAG_SWEPT_CCD: u8   = 1 << 1;
pub const HITBOX_FLAG_PIERCING: u8    = 1 << 2;
pub const HITBOX_FLAG_COUNTER_HIT: u8 = 1 << 3;
pub const HITBOX_FLAG_GUARD_BREAK: u8 = 1 << 4;

/// 1.0 in Q16.16 fixed-point representation
pub const TOI_ONE_Q16: u32 = 65536;

/// Hit height/type classification
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitType {
    #[default]
    High = 0,
    Mid = 1,
    Low = 2,
    Unblockable = 3,
}

/// 6-Directional Discrete Unit Normal Vector
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContactNormal6 {
    #[default]
    None = 0,
    NegX = 1, // [-1,  0,  0]
    PosX = 2, // [ 1,  0,  0]
    NegY = 3, // [ 0, -1,  0]
    PosY = 4, // [ 0,  1,  0]
    NegZ = 5, // [ 0,  0, -1]
    PosZ = 6, // [ 0,  0,  1]
}

impl ContactNormal6 {
    /// Returns discrete (x, y, z) direction tuple in {-1, 0, 1}.
    #[inline(always)]
    pub const fn to_vector(self) -> (i8, i8, i8) {
        match self {
            Self::None => (0, 0, 0),
            Self::NegX => (-1, 0, 0),
            Self::PosX => (1, 0, 0),
            Self::NegY => (0, -1, 0),
            Self::PosY => (0, 1, 0),
            Self::NegZ => (0, 0, -1),
            Self::PosZ => (0, 0, 1),
        }
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Boxing Hitbox Slab.
///
/// Natural alignment layout:
/// - 8B target_mask
/// - 4B * 11 = 44B (attacker_id, generation, swept_min_xyz, swept_max_xyz, velocity_xyz)
/// - 2B * 4 = 8B (damage, knockback, active_start_tick, active_end_tick)
/// - 2B hitbox_id, 2B flags, 1B hit_type, 3B reserved padding
/// Total: 8 + 44 + 8 + 2 + 2 + 1 + 3 = 64 bytes exactly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct HitboxSlab64 {
    /// 8B: Target entity layer / hurtbox mask
    pub target_mask: u64,
    /// 4B: Originating attacking entity handle ID (0 = inactive)
    pub attacker_id: u32,
    /// 4B: ABA prevention generation counter
    pub generation: u32,
    /// 4B: Swept AABB Min X in integer millimeters
    pub swept_min_x: i32,
    /// 4B: Swept AABB Min Y in integer millimeters
    pub swept_min_y: i32,
    /// 4B: Swept AABB Min Z in integer millimeters
    pub swept_min_z: i32,
    /// 4B: Swept AABB Max X in integer millimeters
    pub swept_max_x: i32,
    /// 4B: Swept AABB Max Y in integer millimeters
    pub swept_max_y: i32,
    /// 4B: Swept AABB Max Z in integer millimeters
    pub swept_max_z: i32,
    /// 4B: Linear velocity/displacement X in mm per tick
    pub velocity_x: i32,
    /// 4B: Linear velocity/displacement Y in mm per tick
    pub velocity_y: i32,
    /// 4B: Linear velocity/displacement Z in mm per tick
    pub velocity_z: i32,
    /// 2B: Base damage payload
    pub damage: u16,
    /// 2B: Knockback impulse magnitude
    pub knockback: u16,
    /// 2B: Active start tick index
    pub active_start_tick: u16,
    /// 2B: Active end tick index
    pub active_end_tick: u16,
    /// 2B: Hitbox identifier (fist_l, fist_r, headbutt, etc.)
    pub hitbox_id: u16,
    /// 1B: Hitbox flags (`HITBOX_FLAG_*`)
    pub flags: u8,
    /// 1B: Hit height classification (`HitType`)
    pub hit_type: u8,
}

const _: () = assert!(size_of::<HitboxSlab64>() == 64);
const _: () = assert!(align_of::<HitboxSlab64>() == 64);

impl Default for HitboxSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl HitboxSlab64 {
    /// Creates a blank inactive hitbox slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            target_mask: 0,
            attacker_id: 0,
            generation: 0,
            swept_min_x: 0,
            swept_min_y: 0,
            swept_min_z: 0,
            swept_max_x: 0,
            swept_max_y: 0,
            swept_max_z: 0,
            velocity_x: 0,
            velocity_y: 0,
            velocity_z: 0,
            damage: 0,
            knockback: 0,
            active_start_tick: 0,
            active_end_tick: 0,
            hitbox_id: 0,
            flags: 0,
            hit_type: 0,
        }
    }

    /// Creates an active hitbox slab from local extents and tick displacement.
    pub fn from_extents_and_disp(
        attacker_id: u32,
        hitbox_id: u16,
        target_mask: u64,
        start_min: (i32, i32, i32),
        start_max: (i32, i32, i32),
        disp: (i32, i32, i32),
        damage: u16,
        knockback: u16,
        hit_type: HitType,
        flags: u8,
    ) -> Self {
        let swept_min_x = start_min.0.min(start_min.0 + disp.0);
        let swept_min_y = start_min.1.min(start_min.1 + disp.1);
        let swept_min_z = start_min.2.min(start_min.2 + disp.2);

        let swept_max_x = start_max.0.max(start_max.0 + disp.0);
        let swept_max_y = start_max.1.max(start_max.1 + disp.1);
        let swept_max_z = start_max.2.max(start_max.2 + disp.2);

        Self {
            target_mask,
            attacker_id,
            generation: 1,
            swept_min_x,
            swept_min_y,
            swept_min_z,
            swept_max_x,
            swept_max_y,
            swept_max_z,
            velocity_x: disp.0,
            velocity_y: disp.1,
            velocity_z: disp.2,
            damage,
            knockback,
            active_start_tick: 0,
            active_end_tick: 65535,
            hitbox_id,
            flags: flags | HITBOX_FLAG_ACTIVE | HITBOX_FLAG_SWEPT_CCD,
            hit_type: hit_type as u8,
        }
    }

    /// Returns true if this hitbox slab is currently active.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.attacker_id != 0 && (self.flags & HITBOX_FLAG_ACTIVE) != 0
    }

    /// Checks if this swept hitbox overlaps a target bounding box in 3D.
    #[inline(always)]
    pub fn intersects_swept(&self, target_min: (i32, i32, i32), target_max: (i32, i32, i32)) -> bool {
        self.swept_min_x <= target_max.0
            && self.swept_max_x >= target_min.0
            && self.swept_min_y <= target_max.1
            && self.swept_max_y >= target_min.1
            && self.swept_min_z <= target_max.2
            && self.swept_max_z >= target_min.2
    }
}

/// Exact 64-Byte L1 Cache Line Aligned CCD Hit Resolution Result Slab.
///
/// Natural alignment layout:
/// - 8B pair_key, 8B user_data
/// - 4B toi, 4B hit_x, 4B hit_y, 4B hit_z, 4B attacker_id, 4B victim_id = 24B
/// - 2B hitbox_id, 2B damage, 2B knockback, 2B flags = 8B
/// - 1B normal_x, 1B normal_y, 1B normal_z, 1B has_hit = 4B
/// - 12B reserved padding
/// Total: 16 + 24 + 8 + 4 + 12 = 64 bytes exactly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct SweptAabbResult64 {
    /// 8B: Deduplicated canonical pair key: ((min(att, vic) as u64) << 32) | (max(att, vic) as u64)
    pub pair_key: u64,
    /// 8B: Strike metadata / user data
    pub user_data: u64,
    /// 4B: Time of Impact in Q16.16 (0 = 0.0, 65536 = 1.0)
    pub toi: u32,
    /// 4B: Exact contact point X in integer mm
    pub hit_x: i32,
    /// 4B: Exact contact point Y in integer mm
    pub hit_y: i32,
    /// 4B: Exact contact point Z in integer mm
    pub hit_z: i32,
    /// 4B: Attacker entity ID
    pub attacker_id: u32,
    /// 4B: Victim/Defender entity ID
    pub victim_id: u32,
    /// 2B: Hitbox ID
    pub hitbox_id: u16,
    /// 2B: Applied damage
    pub damage: u16,
    /// 2B: Applied knockback
    pub knockback: u16,
    /// 2B: Physics & combat flags
    pub flags: u16,
    /// 1B: Contact surface normal X (-1, 0, 1)
    pub normal_x: i8,
    /// 1B: Contact surface normal Y (-1, 0, 1)
    pub normal_y: i8,
    /// 1B: Contact surface normal Z (-1, 0, 1)
    pub normal_z: i8,
    /// 1B: Discrete hit flag (1 = hit detected, 0 = no hit)
    pub has_hit: u8,
    /// 12B: Cache line alignment padding
    pub _reserved: [u8; 12],
}

const _: () = assert!(size_of::<SweptAabbResult64>() == 64);
const _: () = assert!(align_of::<SweptAabbResult64>() == 64);

impl Default for SweptAabbResult64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl SweptAabbResult64 {
    /// Creates a blank empty result slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            pair_key: 0,
            user_data: 0,
            toi: 0,
            hit_x: 0,
            hit_y: 0,
            hit_z: 0,
            attacker_id: 0,
            victim_id: 0,
            hitbox_id: 0,
            damage: 0,
            knockback: 0,
            flags: 0,
            normal_x: 0,
            normal_y: 0,
            normal_z: 0,
            has_hit: 0,
            _reserved: [0; 12],
        }
    }

    /// Returns true if a valid hit occurred.
    #[inline(always)]
    pub fn has_hit(&self) -> bool {
        self.has_hit != 0
    }
}

/// Closed-form Raycast vs 3D AABB intersection in pure Q16.16 fixed-point arithmetic.
///
/// Uses Kay-Kajiya slab algorithm with 64-bit integer arithmetic to prevent overflow.
/// Returns `Some((toi_q16, normal))` if hit occurs within `[0, 65536]`, otherwise `None`.
#[inline]
pub fn raycast_aabb_toi(
    ray_origin: (i32, i32, i32),
    ray_disp: (i32, i32, i32),
    box_min: (i32, i32, i32),
    box_max: (i32, i32, i32),
) -> Option<(u32, ContactNormal6)> {
    let mut t_enter = i64::MIN;
    let mut t_exit = i64::MAX;
    let mut hit_normal = ContactNormal6::None;

    // Macro for zero-float integer Kay-Kajiya slab evaluation
    macro_rules! test_axis {
        ($r:expr, $d:expr, $b_min:expr, $b_max:expr, $neg_norm:expr, $pos_norm:expr) => {{
            let r = $r as i64;
            let d = $d as i64;
            let b_min = $b_min as i64;
            let b_max = $b_max as i64;

            if d == 0 {
                if r < b_min || r > b_max {
                    return None;
                }
            } else {
                let (t1, t2, norm) = if d > 0 {
                    (
                        ((b_min - r) << 16) / d,
                        ((b_max - r) << 16) / d,
                        $neg_norm,
                    )
                } else {
                    (
                        ((b_max - r) << 16) / d,
                        ((b_min - r) << 16) / d,
                        $pos_norm,
                    )
                };

                if t1 > t_enter {
                    t_enter = t1;
                    hit_normal = norm;
                }
                if t2 < t_exit {
                    t_exit = t2;
                }

                if t_enter > t_exit {
                    return None;
                }
            }
        }};
    }

    test_axis!(ray_origin.0, ray_disp.0, box_min.0, box_max.0, ContactNormal6::NegX, ContactNormal6::PosX);
    test_axis!(ray_origin.1, ray_disp.1, box_min.1, box_max.1, ContactNormal6::NegY, ContactNormal6::PosY);
    test_axis!(ray_origin.2, ray_disp.2, box_min.2, box_max.2, ContactNormal6::NegZ, ContactNormal6::PosZ);

    if t_exit < 0 || t_enter > (TOI_ONE_Q16 as i64) {
        return None;
    }

    let final_toi = if t_enter < 0 { 0 } else { t_enter as u32 };
    Some((final_toi, hit_normal))
}

/// Continuous Collision Detection (CCD) between moving Box A and moving Box B.
///
/// Uses Minkowski Sum to reduce moving AABB vs moving AABB to Ray vs Expanded AABB.
/// Returns exact sub-frame Time of Impact ($Q16.16$), contact normal, and hit coordinates.
#[inline]
pub fn swept_aabb_toi(
    box_a_min: (i32, i32, i32),
    box_a_max: (i32, i32, i32),
    box_a_disp: (i32, i32, i32),
    box_b_min: (i32, i32, i32),
    box_b_max: (i32, i32, i32),
    box_b_disp: (i32, i32, i32),
) -> Option<SweptAabbResult64> {
    // Relative displacement of Box A with respect to Box B
    let rel_disp = (
        box_a_disp.0 - box_b_disp.0,
        box_a_disp.1 - box_b_disp.1,
        box_a_disp.2 - box_b_disp.2,
    );

    // Box A extents
    let extent_a_x = box_a_max.0 - box_a_min.0;
    let extent_a_y = box_a_max.1 - box_a_min.1;
    let extent_a_z = box_a_max.2 - box_a_min.2;

    // Minkowski expanded Box B
    let exp_b_min = (
        box_b_min.0 - extent_a_x,
        box_b_min.1 - extent_a_y,
        box_b_min.2 - extent_a_z,
    );
    let exp_b_max = box_b_max;

    let (toi, normal) = raycast_aabb_toi(box_a_min, rel_disp, exp_b_min, exp_b_max)?;
    let (nx, ny, nz) = normal.to_vector();

    // Compute exact contact positions at t = TOI
    let toi_i64 = toi as i64;
    let pos_a_x = box_a_min.0 as i64 + ((box_a_disp.0 as i64 * toi_i64) >> 16);
    let pos_a_y = box_a_min.1 as i64 + ((box_a_disp.1 as i64 * toi_i64) >> 16);
    let pos_a_z = box_a_min.2 as i64 + ((box_a_disp.2 as i64 * toi_i64) >> 16);

    let pos_b_x = box_b_min.0 as i64 + ((box_b_disp.0 as i64 * toi_i64) >> 16);
    let pos_b_y = box_b_min.1 as i64 + ((box_b_disp.1 as i64 * toi_i64) >> 16);
    let pos_b_z = box_b_min.2 as i64 + ((box_b_disp.2 as i64 * toi_i64) >> 16);

    let hit_x = if nx != 0 {
        if nx < 0 { pos_b_x as i32 } else { (pos_b_x + (box_b_max.0 - box_b_min.0) as i64) as i32 }
    } else {
        ((pos_a_x + pos_b_x) / 2) as i32
    };

    let hit_y = if ny != 0 {
        if ny < 0 { pos_b_y as i32 } else { (pos_b_y + (box_b_max.1 - box_b_min.1) as i64) as i32 }
    } else {
        ((pos_a_y + pos_b_y) / 2) as i32
    };

    let hit_z = if nz != 0 {
        if nz < 0 { pos_b_z as i32 } else { (pos_b_z + (box_b_max.2 - box_b_min.2) as i64) as i32 }
    } else {
        ((pos_a_z + pos_b_z) / 2) as i32
    };

    let res = SweptAabbResult64 {
        pair_key: 0,
        user_data: 0,
        toi,
        hit_x,
        hit_y,
        hit_z,
        attacker_id: 0,
        victim_id: 0,
        hitbox_id: 0,
        damage: 0,
        knockback: 0,
        flags: 0,
        normal_x: nx,
        normal_y: ny,
        normal_z: nz,
        has_hit: 1,
        _reserved: [0u8; 12],
    };

    Some(res)
}

/// Evaluates a swept hitbox against a defender's hurtbox.
pub fn swept_hitbox_vs_hurtbox(
    hitbox: &HitboxSlab64,
    hurtbox_min: (i32, i32, i32),
    hurtbox_max: (i32, i32, i32),
    hurtbox_disp: (i32, i32, i32),
    victim_id: u32,
) -> Option<SweptAabbResult64> {
    if !hitbox.is_active() || hitbox.attacker_id == victim_id {
        return None;
    }

    // Hitbox start extent before displacement
    let start_min = (
        hitbox.swept_min_x.min(hitbox.swept_max_x - hitbox.velocity_x.abs()),
        hitbox.swept_min_y.min(hitbox.swept_max_y - hitbox.velocity_y.abs()),
        hitbox.swept_min_z.min(hitbox.swept_max_z - hitbox.velocity_z.abs()),
    );
    let start_max = (
        start_min.0 + (hitbox.swept_max_x - hitbox.swept_min_x - hitbox.velocity_x.abs()).max(100),
        start_min.1 + (hitbox.swept_max_y - hitbox.swept_min_y - hitbox.velocity_y.abs()).max(100),
        start_min.2 + (hitbox.swept_max_z - hitbox.swept_min_z - hitbox.velocity_z.abs()).max(100),
    );

    let disp_a = (hitbox.velocity_x, hitbox.velocity_y, hitbox.velocity_z);

    let mut res = swept_aabb_toi(start_min, start_max, disp_a, hurtbox_min, hurtbox_max, hurtbox_disp)?;
    res.attacker_id = hitbox.attacker_id;
    res.victim_id = victim_id;
    res.hitbox_id = hitbox.hitbox_id;
    res.damage = hitbox.damage;
    res.knockback = hitbox.knockback;
    res.flags = hitbox.flags as u16;
    let min_id = hitbox.attacker_id.min(victim_id) as u64;
    let max_id = hitbox.attacker_id.max(victim_id) as u64;
    res.pair_key = (min_id << 32) | max_id;

    Some(res)
}

/// Computes a guaranteed-deterministic 64-bit FNV-1a checksum over CCD results.
pub fn compute_ccd_consensus_hash(results: &[SweptAabbResult64]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for res in results {
        if res.has_hit() {
            let bytes = [
                res.toi.to_le_bytes(),
                res.hit_x.to_le_bytes(),
                res.hit_y.to_le_bytes(),
                res.hit_z.to_le_bytes(),
            ];
            for chunk in &bytes {
                for &b in chunk {
                    hash ^= b as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
            }
            hash ^= res.pair_key;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}
