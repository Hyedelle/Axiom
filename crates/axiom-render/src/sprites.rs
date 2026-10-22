//! 16x16 Pixel Sprite Atlas loaded zero-copy from build-time binary slab.
//!
//! Sprites are baked by `build.rs` into a 64-byte aligned binary slab (`sprites.atlas`)
//! with a 64-byte `AtlasHeader` followed by 88 x 1024-byte (`[u32; 256]`) sprites.

pub const SPRITE_SIZE: usize = 16;
pub const PIXELS_PER_SPRITE: usize = SPRITE_SIZE * SPRITE_SIZE; // 256

pub const SPRITE_KIND_EMPTY: u32 = 0;
pub const SPRITE_KIND_DEPOSIT: u32 = 1;
pub const SPRITE_KIND_DRILL: u32 = 2;
pub const SPRITE_KIND_BELT: u32 = 3;
pub const SPRITE_KIND_INSERTER: u32 = 4;
pub const SPRITE_KIND_SMELTER: u32 = 5;
pub const SPRITE_KIND_CHEST: u32 = 6;
pub const SPRITE_KIND_WALL: u32 = 7;
pub const SPRITE_KIND_TURRET: u32 = 8;
pub const SPRITE_KIND_BASE_HEART: u32 = 9;
pub const SPRITE_KIND_MONSTER: u32 = 10;
pub const SPRITE_KIND_FAST_BELT: u32 = 11;
pub const SPRITE_KIND_FILTER_INSERTER: u32 = 12;
pub const SPRITE_KIND_LASER_TURRET: u32 = 13;

pub const DIR_NORTH: u32 = 0;
pub const DIR_EAST: u32 = 1;
pub const DIR_SOUTH: u32 = 2;
pub const DIR_WEST: u32 = 3;

pub const ATLAS_MAGIC: u64 = 0x41544C415358414D; // "AXMATLAS"
pub const SPRITE_COUNT: usize = 88;
pub const HEADER_SIZE: usize = 64;
pub const BYTES_PER_SPRITE: usize = 1024;
pub const TOTAL_ATLAS_BYTES: usize = HEADER_SIZE + SPRITE_COUNT * BYTES_PER_SPRITE; // 90,176 bytes

#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasHeader {
    pub magic: u64,
    pub version: u32,
    pub sprite_count: u32,
    pub sprite_size: u32,
    pub bytes_per_sprite: u32,
    pub checksum: u64,
    pub _reserved: [u8; 32],
}

#[repr(C, align(64))]
struct AlignedAtlas([u8; TOTAL_ATLAS_BYTES]);

static BAKED_ATLAS: AlignedAtlas = AlignedAtlas(*include_bytes!(concat!(env!("OUT_DIR"), "/sprites.atlas")));

pub static EMPTY_SPRITE: [u32; 256] = [0u32; 256];

// Indices inside the baked binary atlas
pub const IDX_EMPTY: usize = 0;
pub const IDX_BELT_BASE: usize = 1;             // 1..=16 (4 dirs x 4 frames)
pub const IDX_DRILL_BASE: usize = 17;           // 17..=32 (4 dirs x 4 frames)
pub const IDX_SMELTER_BASE: usize = 33;         // 33..=36 (4 frames)
pub const IDX_CHEST: usize = 37;                // 37
pub const IDX_WALL: usize = 38;                 // 38
pub const IDX_TURRET_BASE: usize = 39;          // 39..=42 (4 dirs)
pub const IDX_HEART_BASE: usize = 43;           // 43..=46 (4 frames)
pub const IDX_DEPOSIT: usize = 47;              // 47
pub const IDX_GOBLIN_BASE: usize = 48;          // 48..=51 (4 frames)
pub const IDX_INSERTER_BASE: usize = 52;        // 52..=55 (4 dirs)
pub const IDX_FAST_BELT_BASE: usize = 56;       // 56..=71 (4 dirs x 4 frames)
pub const IDX_FILTER_INSERTER_BASE: usize = 72; // 72..=75 (4 dirs)
pub const IDX_LASER_TURRET_BASE: usize = 76;    // 76..=79 (4 dirs)
pub const IDX_GOLEM_BASE: usize = 80;           // 80..=83 (4 frames)
pub const IDX_RUNNER_BASE: usize = 84;          // 84..=87 (4 frames)

/// Returns a reference to the 64-byte atlas header.
#[inline(always)]
pub fn get_atlas_header() -> &'static AtlasHeader {
    unsafe {
        &*(BAKED_ATLAS.0.as_ptr() as *const AtlasHeader)
    }
}

/// Zero-copy, 64-byte L1 cache-aligned lookup of a 16x16 sprite slice.
#[inline(always)]
pub fn get_sprite_by_index(idx: usize) -> &'static [u32; 256] {
    if idx >= SPRITE_COUNT {
        return &EMPTY_SPRITE;
    }
    let offset = HEADER_SIZE + idx * BYTES_PER_SPRITE;
    unsafe {
        &*(BAKED_ATLAS.0.as_ptr().add(offset) as *const [u32; 256])
    }
}

/// Returns the sprite for a belt given direction and tick.
#[inline(always)]
pub fn get_belt_sprite(direction: u32, tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    let frame_idx = (tick as usize / 2) % 4;
    get_sprite_by_index(IDX_BELT_BASE + dir_idx * 4 + frame_idx)
}

/// Returns the sprite for a fast conveyor belt given direction and tick.
#[inline(always)]
pub fn get_fast_belt_sprite(direction: u32, tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    let frame_idx = (tick as usize) % 4;
    get_sprite_by_index(IDX_FAST_BELT_BASE + dir_idx * 4 + frame_idx)
}

/// Returns the sprite for a mining drill given direction and tick.
#[inline(always)]
pub fn get_drill_sprite(direction: u32, tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    let frame_idx = (tick as usize / 2) % 4;
    get_sprite_by_index(IDX_DRILL_BASE + dir_idx * 4 + frame_idx)
}

/// Returns the sprite for a smelter furnace given tick.
#[inline(always)]
pub fn get_smelter_sprite(tick: u64) -> &'static [u32; 256] {
    let frame_idx = (tick as usize / 3) % 4;
    get_sprite_by_index(IDX_SMELTER_BASE + frame_idx)
}

/// Returns the sprite for an output storage chest.
#[inline(always)]
pub fn get_chest_sprite() -> &'static [u32; 256] {
    get_sprite_by_index(IDX_CHEST)
}

/// Returns the sprite for a defense wall.
#[inline(always)]
pub fn get_wall_sprite() -> &'static [u32; 256] {
    get_sprite_by_index(IDX_WALL)
}

/// Returns the sprite for a defense turret given direction.
#[inline(always)]
pub fn get_turret_sprite(direction: u32) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    get_sprite_by_index(IDX_TURRET_BASE + dir_idx)
}

/// Returns the sprite for a laser turret given direction.
#[inline(always)]
pub fn get_laser_turret_sprite(direction: u32) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    get_sprite_by_index(IDX_LASER_TURRET_BASE + dir_idx)
}

/// Returns the sprite for the base heart crystal given tick.
#[inline(always)]
pub fn get_heart_sprite(tick: u64) -> &'static [u32; 256] {
    let frame_idx = (tick as usize / 6) % 4;
    get_sprite_by_index(IDX_HEART_BASE + frame_idx)
}

/// Returns the sprite for an ore deposit bedrock.
#[inline(always)]
pub fn get_deposit_sprite(_tick: u64) -> &'static [u32; 256] {
    get_sprite_by_index(IDX_DEPOSIT)
}

/// Returns the sprite for a standard goblin monster given tick.
#[inline(always)]
pub fn get_monster_sprite(tick: u64) -> &'static [u32; 256] {
    let frame_idx = (tick as usize / 4) % 4;
    get_sprite_by_index(IDX_GOBLIN_BASE + frame_idx)
}

/// Returns the sprite for a monster given its archetype kind and tick.
#[inline(always)]
pub fn get_monster_sprite_by_kind(kind: u8, tick: u64) -> &'static [u32; 256] {
    match kind {
        1 => {
            // Golem (Stone Titan)
            let frame_idx = (tick as usize / 4) % 4;
            get_sprite_by_index(IDX_GOLEM_BASE + frame_idx)
        }
        2 => {
            // Runner (Fast Scout)
            let frame_idx = (tick as usize / 2) % 4;
            get_sprite_by_index(IDX_RUNNER_BASE + frame_idx)
        }
        _ => {
            // Standard Goblin
            let frame_idx = (tick as usize / 4) % 4;
            get_sprite_by_index(IDX_GOBLIN_BASE + frame_idx)
        }
    }
}

/// Returns the sprite for an inserter arm given direction and tick.
#[inline(always)]
pub fn get_inserter_sprite(direction: u32, _tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    get_sprite_by_index(IDX_INSERTER_BASE + dir_idx)
}

/// Returns the sprite for an item-selective filter inserter given direction.
#[inline(always)]
pub fn get_filter_inserter_sprite(direction: u32, _tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    get_sprite_by_index(IDX_FILTER_INSERTER_BASE + dir_idx)
}

/// Universal structure sprite resolver matching `kind_id` and `direction`.
pub fn render_structure_sprite(kind_id: u32, direction: u32, tick: u64) -> &'static [u32; 256] {
    match kind_id {
        SPRITE_KIND_EMPTY => &EMPTY_SPRITE,
        SPRITE_KIND_DEPOSIT => get_deposit_sprite(tick),
        SPRITE_KIND_DRILL => get_drill_sprite(direction, tick),
        SPRITE_KIND_BELT => get_belt_sprite(direction, tick),
        SPRITE_KIND_INSERTER => get_inserter_sprite(direction, tick),
        SPRITE_KIND_SMELTER => get_smelter_sprite(tick),
        SPRITE_KIND_CHEST => get_chest_sprite(),
        SPRITE_KIND_WALL => get_wall_sprite(),
        SPRITE_KIND_TURRET => get_turret_sprite(direction),
        SPRITE_KIND_BASE_HEART => get_heart_sprite(tick),
        SPRITE_KIND_MONSTER => get_monster_sprite(tick),
        SPRITE_KIND_FAST_BELT => get_fast_belt_sprite(direction, tick),
        SPRITE_KIND_FILTER_INSERTER => get_filter_inserter_sprite(direction, tick),
        SPRITE_KIND_LASER_TURRET => get_laser_turret_sprite(direction),
        _ => &EMPTY_SPRITE,
    }
}

/// Alias for `render_structure_sprite`.
#[inline(always)]
pub fn get_animated_sprite(kind_id: u32, direction: u32, tick: u64) -> &'static [u32; 256] {
    render_structure_sprite(kind_id, direction, tick)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baked_atlas_binary_integrity() {
        let header = get_atlas_header();
        assert_eq!(header.magic, ATLAS_MAGIC, "Atlas magic header mismatch");
        assert_eq!(header.version, 1, "Atlas version mismatch");
        assert_eq!(header.sprite_count, SPRITE_COUNT as u32, "Atlas sprite count mismatch");
        assert_eq!(header.sprite_size, 16, "Atlas sprite size must be 16x16");
        assert_eq!(header.bytes_per_sprite, 1024, "Sprite slice must be 1024 bytes");
        assert_ne!(header.checksum, 0, "Checksum must be non-zero");

        // Verify all 88 sprites are accessible and 64-byte aligned in memory
        for idx in 0..SPRITE_COUNT {
            let sprite = get_sprite_by_index(idx);
            assert_eq!(sprite.len(), 256);
            let ptr = sprite.as_ptr() as usize;
            assert_eq!(ptr % 64, 0, "Sprite index {} is not 64-byte aligned (ptr=0x{:X})", idx, ptr);
        }

        // Out of bounds falls back to EMPTY_SPRITE
        let oob = get_sprite_by_index(999);
        assert_eq!(oob, &EMPTY_SPRITE);
    }

    #[test]
    fn test_sprite_dimensions_and_alpha_integrity() {
        // 1. Verify dimensions
        assert_eq!(EMPTY_SPRITE.len(), 256);
        assert_eq!(get_chest_sprite().len(), 256);
        assert_eq!(get_wall_sprite().len(), 256);
        assert_eq!(get_deposit_sprite(0).len(), 256);

        // 2. Verify all empty pixels are zero alpha
        for &px in EMPTY_SPRITE.iter() {
            assert_eq!(px >> 24, 0);
        }

        // 3. Verify structures have non-zero alpha pixels
        let test_sprites = [
            get_chest_sprite(),
            get_wall_sprite(),
            get_deposit_sprite(0),
            get_belt_sprite(DIR_NORTH, 0),
            get_drill_sprite(DIR_NORTH, 0),
            get_smelter_sprite(0),
            get_turret_sprite(DIR_NORTH),
            get_heart_sprite(0),
            get_monster_sprite(0),
            get_inserter_sprite(DIR_NORTH, 0),
            get_fast_belt_sprite(DIR_NORTH, 0),
            get_filter_inserter_sprite(DIR_NORTH, 0),
            get_laser_turret_sprite(DIR_NORTH),
            get_monster_sprite_by_kind(1, 0), // Golem
            get_monster_sprite_by_kind(2, 0), // Runner
        ];

        for sprite in test_sprites {
            assert_eq!(sprite.len(), 256);
            let opaque_count = sprite.iter().filter(|&&p| (p >> 24) > 0).count();
            assert!(
                opaque_count >= 20,
                "Sprite should have sufficient visible pixels, found {}",
                opaque_count
            );
        }

        // 4. Verify helper resolver works for all kinds and directions
        for kind in 0..=13 {
            for dir in 0..4 {
                for tick in [0u64, 1, 5, 12, 100] {
                    let s = render_structure_sprite(kind, dir, tick);
                    assert_eq!(s.len(), 256);
                    let alias_s = get_animated_sprite(kind, dir, tick);
                    assert_eq!(s, alias_s);
                }
            }
        }
    }
}
