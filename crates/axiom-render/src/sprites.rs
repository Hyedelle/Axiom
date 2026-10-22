//! 16x16 Pixel Sprite Atlas and Deterministic Animation Frames.
//!
//! Provides compile-time bitmap tables (`[u32; 256]` in 32-bit ARGB format)
//! for factory structures, defense emplacements, and monsters.

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

pub const DIR_NORTH: u32 = 0;
pub const DIR_EAST: u32 = 1;
pub const DIR_SOUTH: u32 = 2;
pub const DIR_WEST: u32 = 3;

/// Parses a 16x16 character grid into a 256-word ARGB pixel array at compile time.
pub const fn parse_sprite(rows: [&str; 16], pal: &[(u8, u32)]) -> [u32; 256] {
    let mut out = [0u32; 256];
    let mut y = 0;
    while y < 16 {
        let row_bytes = rows[y].as_bytes();
        let mut x = 0;
        while x < 16 {
            let ch = row_bytes[x];
            let mut color = 0u32;
            let mut i = 0;
            while i < pal.len() {
                if pal[i].0 == ch {
                    color = pal[i].1;
                    break;
                }
                i += 1;
            }
            out[y * 16 + x] = color;
            x += 1;
        }
        y += 1;
    }
    out
}

pub static EMPTY_SPRITE: [u32; 256] = [0u32; 256];

// ===========================================================================
// 1. Conveyor Belts (4 Directions x 4 Animation Frames)
// ===========================================================================
const BELT_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'B', 0xFF1E293B), // Dark Rail
    (b'g', 0xFF475569), // Rail Highlight
    (b'b', 0xFF334155), // Belt Surface
    (b'r', 0xFF273548), // Roller groove
    (b'o', 0xFFD97706), // Chevron Shadow
    (b'y', 0xFFF59E0B), // Chevron Amber
    (b'Y', 0xFFFBBF24), // Chevron Bright Tip
];

// Belt North (Frames 0..3: Chevrons '^' scrolling upward)
const BELT_N_F0: [u32; 256] = parse_sprite([
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
], BELT_PAL);

const BELT_N_F1: [u32; 256] = parse_sprite([
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
], BELT_PAL);

const BELT_N_F2: [u32; 256] = parse_sprite([
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
], BELT_PAL);

const BELT_N_F3: [u32; 256] = parse_sprite([
    "BgbbbbbbbbbbbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbyyyyyybbbgB",
    "BgbyyyoooooyybgB",
    "BgyyoobbbbooyygB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
], BELT_PAL);

// Belt East (Frames 0..3: Chevrons '>' scrolling eastward)
const BELT_E_F0: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "bbyybbbbbbbyybbb",
    "bbyyybbbbbbyyybb",
    "bbooyybbbbbooyyb",
    "bbbbooybbbbbbooy",
    "bbbbboYYbbbbbboY",
    "rrrrroYYrrrrrroY",
    "bbbbboYYbbbbbboY",
    "bbbbooybbbbbbooy",
    "bbooyybbbbbooyyb",
    "bbyyybbbbbbyyybb",
    "bbyybbbbbbbyybbb",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

const BELT_E_F1: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "bbbbyybbbbbbbyyb",
    "bbbbyyybbbbbbyyy",
    "bbbooyybbbbbooyy",
    "bbbbbooybbbbbboo",
    "YbbbbboYYbbbbbbo",
    "YrbbbboYYrrrrrro",
    "YbbbbboYYbbbbbbo",
    "bbbbbooybbbbbboo",
    "bbbooyybbbbbooyy",
    "bbbbyyybbbbbbyyy",
    "bbbbyybbbbbbbyyb",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

const BELT_E_F2: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "bbbbbbyybbbbbbby",
    "bbbbbbyyybbbbbby",
    "ybbbbbooyybbbbbo",
    "yybbbbbbooybbbbb",
    "oYYbbbbbboYYbbbb",
    "oYYrrrrrroYYrrrr",
    "oYYbbbbbboYYbbbb",
    "yybbbbbbooybbbbb",
    "ybbbbbooyybbbbbo",
    "bbbbbbyyybbbbbby",
    "bbbbbbyybbbbbbby",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

const BELT_E_F3: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "ybbbbbbbyybbbbbb",
    "yybbbbbbyyybbbbb",
    "oyybbbbbooyybbbb",
    "booybbbbbbooybbb",
    "bboYYbbbbbboYYbb",
    "rroYYrrrrrroYYrr",
    "bboYYbbbbbboYYbb",
    "booybbbbbbooybbb",
    "oyybbbbbooyybbbb",
    "yybbbbbbyyybbbbb",
    "ybbbbbbbyybbbbbb",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

// Belt South (Frames 0..3: Chevrons 'v' scrolling downward)
const BELT_S_F0: [u32; 256] = parse_sprite([
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
], BELT_PAL);

const BELT_S_F1: [u32; 256] = parse_sprite([
    "BgbbbbYYYYbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
], BELT_PAL);

const BELT_S_F2: [u32; 256] = parse_sprite([
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
    "BgbbbbYYYYbbbbgB",
], BELT_PAL);

const BELT_S_F3: [u32; 256] = parse_sprite([
    "BgbbbbbbbbbbbbgB",
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
    "BgyyoobbbbooyygB",
    "BgbyyyoooooyybgB",
    "BgbbbyyyyyybbbgB",
    "BgbbbbYYYYbbbbgB",
    "BgbbbbbbbbbbbbgB",
    "BgbrrrrrrrrrrbgB",
], BELT_PAL);

// Belt West (Frames 0..3: Chevrons '<' scrolling westward)
const BELT_W_F0: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "bbbyybbbbbbbyybb",
    "bbyyybbbbbbyyybb",
    "byyoobbbbbyyoobb",
    "yooabbbbbbyooabb",
    "YoobbbbbbYoobbbb",
    "YoorrrrrrYoorrrr",
    "YoobbbbbbYoobbbb",
    "yooabbbbbbyooabb",
    "byyoobbbbbyyoobb",
    "bbyyybbbbbbyyybb",
    "bbbyybbbbbbbyybb",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

const BELT_W_F1: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "bbyybbbbbbbyybbb",
    "byyybbbbbbyyybbb",
    "yyoobbbbbyyoobbb",
    "ooabbbbbbyooabbb",
    "oobbbbbbYoobbbbb",
    "oorrrrrrYoorrrrr",
    "oobbbbbbYoobbbbb",
    "ooabbbbbbyooabbb",
    "yyoobbbbbyyoobbb",
    "byyybbbbbbyyybbb",
    "bbyybbbbbbbyybbb",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

const BELT_W_F2: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "byybbbbbbbyybbbb",
    "yyybbbbbbyyybbbb",
    "yoobbbbbyyoobbbb",
    "oabbbbbbyooabbbb",
    "obbbbbbYoobbbbbb",
    "orrrrrrYoorrrrrr",
    "obbbbbbYoobbbbbb",
    "oabbbbbbyooabbbb",
    "yoobbbbbyyoobbbb",
    "yyybbbbbbyyybbbb",
    "byybbbbbbbyybbbb",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

const BELT_W_F3: [u32; 256] = parse_sprite([
    "BBBBBBBBBBBBBBBB",
    "gggggggggggggggg",
    "yybbbbbbbyybbbbb",
    "yybbbbbbyyybbbbb",
    "oobbbbbyyoobbbbb",
    "abbbbbbyooabbbbb",
    "bbbbbbYoobbbbbbb",
    "rrrrrrYoorrrrrrr",
    "bbbbbbYoobbbbbbb",
    "abbbbbbyooabbbbb",
    "oobbbbbyyoobbbbb",
    "yybbbbbbyyybbbbb",
    "yybbbbbbbyybbbbb",
    "bbbbbbbbbbbbbbbb",
    "gggggggggggggggg",
    "BBBBBBBBBBBBBBBB",
], BELT_PAL);

pub static BELT_FRAMES: [[[u32; 256]; 4]; 4] = [
    [BELT_N_F0, BELT_N_F1, BELT_N_F2, BELT_N_F3], // North
    [BELT_E_F0, BELT_E_F1, BELT_E_F2, BELT_E_F3], // East
    [BELT_S_F0, BELT_S_F1, BELT_S_F2, BELT_S_F3], // South
    [BELT_W_F0, BELT_W_F1, BELT_W_F2, BELT_W_F3], // West
];

// ===========================================================================
// 2. Mining Drill (4 Directions x 4 Rotating Bit Frames)
// ===========================================================================
const DRILL_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'm', 0xFF475569), // Metallic Frame
    (b'r', 0xFF94A3B8), // Rivet / Highlight
    (b'd', 0xFF1E293B), // Dark Chamber
    (b'c', 0xFF0284C7), // Drill Bit Dark Cyan
    (b'C', 0xFF38BDF8), // Drill Bit Bright Cyan
    (b'w', 0xFFE0F2FE), // Center Pivot Glint
    (b'y', 0xFFD97706), // Nozzle Dark Amber
    (b'Y', 0xFFF59E0B), // Nozzle Bright Amber
];

// Directional Drill Frames (Center rotating cross bit across 4 frames)
// North Chute
const DRILL_N_F0: [u32; 256] = parse_sprite([
    "mrmmYYYmYYYmmrmm",
    "rdddyyydyyydddrr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mdddddcCCddddddm",
    "mdddddcCCddddddm",
    "mddCCCwwCCwCCddm",
    "mddcccwwccwccddm",
    "mdddddcCCddddddm",
    "mdddddcCCddddddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rddddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_N_F1: [u32; 256] = parse_sprite([
    "mrmmYYYmYYYmmrmm",
    "rdddyyydyyydddrr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mddCcddddddcCddm",
    "mdddCcddddcCdddm",
    "mddddCcwwcCddddm",
    "mddddcCwwCcddddm",
    "mdddCcddddcCdddm",
    "mddCcddddddcCddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rddddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_N_F2: [u32; 256] = parse_sprite([
    "mrmmYYYmYYYmmrmm",
    "rdddyyydyyydddrr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mddddddcCCdddddm",
    "mddddddcCCdddddm",
    "mddCCCwwCCwCCddm",
    "mddcccwwccwccddm",
    "mdddddcCCddddddm",
    "mdddddcCCddddddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rddddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_N_F3: [u32; 256] = parse_sprite([
    "mrmmYYYmYYYmmrmm",
    "rdddyyydyyydddrr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mddddcCddddCcddm",
    "mdddCcddddcCdddm",
    "mddddCcwwcCddddm",
    "mddddcCwwCcddddm",
    "mdddCcddddcCdddm",
    "mddcCddddddCcddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rddddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

// East Chute
const DRILL_E_F0: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rdddddddddddddrr",
    "mdddddddddddddyY",
    "mddmmmddddmmmdyY",
    "mddmrrddddrrmdmY",
    "mdddddcCCdddddmY",
    "mdddddcCCdddddmY",
    "mddCCCwwCCwCCdmY",
    "mddcccwwccwccdmY",
    "mdddddcCCdddddmY",
    "mdddddcCCdddddmY",
    "mddmrrddddrrmdmY",
    "mddmmmddddmmmdyY",
    "mdddddddddddddyY",
    "rdddddddddddddrr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_E_F1: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rdddddddddddddrr",
    "mdddddddddddddyY",
    "mddmmmddddmmmdyY",
    "mddmrrddddrrmdmY",
    "mddCcddddddcCdmy",
    "mdddCcddddcCdymy",
    "mddddCcwwcCdymyy",
    "mddddcCwwCcdymyy",
    "mdddCcddddcCdymy",
    "mddCcddddddcCdmy",
    "mddmrrddddrrmdmY",
    "mddmmmddddmmmdyY",
    "mdddddddddddddyY",
    "rdddddddddddddrr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_E_F2: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rdddddddddddddrr",
    "mdddddddddddddyY",
    "mddmmmddddmmmdyY",
    "mddmrrddddrrmdmY",
    "mddddddcCCddddmy",
    "mddddddcCCddddmy",
    "mddCCCwwCCwCCdmy",
    "mddcccwwccwccdmy",
    "mdddddcCCdddddmy",
    "mdddddcCCdddddmy",
    "mddmrrddddrrmdmY",
    "mddmmmddddmmmdyY",
    "mdddddddddddddyY",
    "rdddddddddddddrr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_E_F3: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rdddddddddddddrr",
    "mdddddddddddddyY",
    "mddmmmddddmmmdyY",
    "mddmrrddddrrmdmY",
    "mddddcCddddCcdmy",
    "mdddCcddddcCdymy",
    "mddddCcwwcCdymyy",
    "mddddcCwwCcdymyy",
    "mdddCcddddcCdymy",
    "mddcCddddddCcdmy",
    "mddmrrddddrrmdmY",
    "mddmmmddddmmmdyY",
    "mdddddddddddddyY",
    "rdddddddddddddrr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

// South Chute
const DRILL_S_F0: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rddddddddddddddr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mdddddcCCddddddm",
    "mdddddcCCddddddm",
    "mddCCCwwCCwCCddm",
    "mddcccwwccwccddm",
    "mdddddcCCddddddm",
    "mdddddcCCddddddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rdddyyydyyydddrr",
    "mrmmYYYmYYYmmrmm",
], DRILL_PAL);

const DRILL_S_F1: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rddddddddddddddr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mddCcddddddcCddm",
    "mdddCcddddcCdddm",
    "mddddCcwwcCddddm",
    "mddddcCwwCcddddm",
    "mdddCcddddcCdddm",
    "mddCcddddddcCddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rdddyyydyyydddrr",
    "mrmmYYYmYYYmmrmm",
], DRILL_PAL);

const DRILL_S_F2: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rddddddddddddddr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mddddddcCCdddddm",
    "mddddddcCCdddddm",
    "mddCCCwwCCwCCddm",
    "mddcccwwccwccddm",
    "mdddddcCCddddddm",
    "mdddddcCCddddddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rdddyyydyyydddrr",
    "mrmmYYYmYYYmmrmm",
], DRILL_PAL);

const DRILL_S_F3: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rddddddddddddddr",
    "mddddddddddddddm",
    "mddmmmddddmmmddm",
    "mddmrrddddrrmddm",
    "mddddcCddddCcddm",
    "mdddCcddddcCdddm",
    "mddddCcwwcCddddm",
    "mddddcCwwCcddddm",
    "mdddCcddddcCdddm",
    "mddcCddddddCcddm",
    "mddmrrddddrrmddm",
    "mddmmmddddmmmddm",
    "mddddddddddddddm",
    "rdddyyydyyydddrr",
    "mrmmYYYmYYYmmrmm",
], DRILL_PAL);

// West Chute
const DRILL_W_F0: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rrdddddddddddddr",
    "Yydddddddddddddm",
    "Yydmmmddddmmmddm",
    "Ymdmrrddddrrmddm",
    "YmddddcCCddddddm",
    "YmddddcCCddddddm",
    "YmddCCCwwCCwCCdm",
    "Ymddcccwwccwccdm",
    "YmddddcCCddddddm",
    "YmddddcCCddddddm",
    "Ymdmrrddddrrmddm",
    "Yydmmmddddmmmddm",
    "Yydddddddddddddm",
    "rrdddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_W_F1: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rrdddddddddddddr",
    "Yydddddddddddddm",
    "Yydmmmddddmmmddm",
    "Ymdmrrddddrrmddm",
    "ymdCcddddddcCddm",
    "ymddCcddddcCdddm",
    "yymddCcwwcCddddm",
    "yymddcCwwCcddddm",
    "ymddCcddddcCdddm",
    "ymdCcddddddcCddm",
    "Ymdmrrddddrrmddm",
    "Yydmmmddddmmmddm",
    "Yydddddddddddddm",
    "rrdddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_W_F2: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rrdddddddddddddr",
    "Yydddddddddddddm",
    "Yydmmmddddmmmddm",
    "Ymdmrrddddrrmddm",
    "ymdddddcCCdddddm",
    "ymdddddcCCdddddm",
    "ymddCCCwwCCwCCdm",
    "ymddcccwwccwccdm",
    "ymdddddcCCdddddm",
    "ymdddddcCCdddddm",
    "Ymdmrrddddrrmddm",
    "Yydmmmddddmmmddm",
    "Yydddddddddddddm",
    "rrdddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

const DRILL_W_F3: [u32; 256] = parse_sprite([
    "mrmmmmmmmmmmmmrm",
    "rrdddddddddddddr",
    "Yydddddddddddddm",
    "Yydmmmddddmmmddm",
    "Ymdmrrddddrrmddm",
    "ymddddcCddddCcdd",
    "ymdddCcddddcCddd",
    "yymdddCcwwcCdddd",
    "yymdddcCwwCcdddd",
    "ymdddCcddddcCddd",
    "ymddcCddddddCcdd",
    "Ymdmrrddddrrmddm",
    "Yydmmmddddmmmddm",
    "Yydddddddddddddm",
    "rrdddddddddddddr",
    "mrmmmmmmmmmmmmrm",
], DRILL_PAL);

pub static DRILL_FRAMES: [[[u32; 256]; 4]; 4] = [
    [DRILL_N_F0, DRILL_N_F1, DRILL_N_F2, DRILL_N_F3],
    [DRILL_E_F0, DRILL_E_F1, DRILL_E_F2, DRILL_E_F3],
    [DRILL_S_F0, DRILL_S_F1, DRILL_S_F2, DRILL_S_F3],
    [DRILL_W_F0, DRILL_W_F1, DRILL_W_F2, DRILL_W_F3],
];

// ===========================================================================
// 3. Smelter Furnace (4 Animated Fire Frames)
// ===========================================================================
const SMELTER_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'k', 0xFF0F172A), // Dark Vent Opening
    (b'd', 0xFF1E293B), // Dark Stone Block
    (b's', 0xFF334155), // Mid Stone Block
    (b'f', 0xFF475569), // Furnace Arch Stone
    (b'r', 0xFF450A0A), // Dark Hearth Recess
    (b'o', 0xFFEA580C), // Dark Orange Flame
    (b'O', 0xFFF97316), // Bright Orange Flame
    (b'y', 0xFFFBBF24), // Amber Flare
    (b'Y', 0xFFFEF08A), // White-Hot Yellow Flame Tip
];

const SMELTER_F0: [u32; 256] = parse_sprite([
    "..sddkkkkkkdds..",
    ".sdddkkkkkkddds.",
    "sddddssssssdddds",
    "sddssssssssssdds",
    "sdssssssssssssds",
    "sdssfssssssfsdds",
    "sdsfffffffffssds",
    "sdsffrrrrrrffsds",
    "sdsffrooyorffsds",
    "sdsffroyyyOffsds",
    "sdsffroyYYOffsds",
    "sdsffrOYyOYrfsds",
    "sdssfroyoyOffsds",
    "sddssffffffssdds",
    "sddddssssssdddds",
    ".sdddddddddddds.",
], SMELTER_PAL);

const SMELTER_F1: [u32; 256] = parse_sprite([
    "..sddkkkkkkdds..",
    ".sdddkkkkkkddds.",
    "sddddssssssdddds",
    "sddssssssssssdds",
    "sdssssssssssssds",
    "sdssfssssssfsdds",
    "sdsfffffffffssds",
    "sdsffrrYYrrffsds",
    "sdsffrOYYoyffsds",
    "sdsffroyYYOffsds",
    "sdsffroyyyOffsds",
    "sdsffrOYyyYrfsds",
    "sdssfroyOyOffsds",
    "sddssffffffssdds",
    "sddddssssssdddds",
    ".sdddddddddddds.",
], SMELTER_PAL);

const SMELTER_F2: [u32; 256] = parse_sprite([
    "..sddkkkkkkdds..",
    ".sdddkkkkkkddds.",
    "sddddssssssdddds",
    "sddssssssssssdds",
    "sdssssssssssssds",
    "sdssfssssssfsdds",
    "sdsfffffffffssds",
    "sdsffrYYYYrffsds",
    "sdsffroYYYYOffsd",
    "sdsffroyYYyOffsd",
    "sdsffroYYYYOffsd",
    "sdsffrOYyyOYrfsd",
    "sdssfroyyyOffsds",
    "sddssffffffssdds",
    "sddddssssssdddds",
    ".sdddddddddddds.",
], SMELTER_PAL);

const SMELTER_F3: [u32; 256] = parse_sprite([
    "..sddkkkkkkdds..",
    ".sdddkkkkkkddds.",
    "sddddssssssdddds",
    "sddssssssssssdds",
    "sdssssssssssssds",
    "sdssfssssssfsdds",
    "sdsfffffffffssds",
    "sdsffrryyrrffsds",
    "sdsffroyYYOffsds",
    "sdsffroYYYYOffsd",
    "sdsffrOYYYYrffsd",
    "sdsffroyYYOffsds",
    "sdssfroyoyOffsds",
    "sddssffffffssdds",
    "sddddssssssdddds",
    ".sdddddddddddds.",
], SMELTER_PAL);

pub static SMELTER_FRAMES: [[u32; 256]; 4] = [
    SMELTER_F0, SMELTER_F1, SMELTER_F2, SMELTER_F3,
];

// ===========================================================================
// 4. Output Chest (Reinforced Wooden Crate with Brass Lock)
// ===========================================================================
const CHEST_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'm', 0xFF475569), // Metal Corner Bracket
    (b'M', 0xFF94A3B8), // Metal Rivet Highlight
    (b'b', 0xFF334155), // Iron Reinforcing Band
    (b's', 0xFF451A03), // Dark Plank Seam
    (b'w', 0xFF78350F), // Dark Mahogany Wood
    (b'W', 0xFF92400E), // Medium Oak Wood
    (b'l', 0xFFB45309), // Light Wood Highlight
    (b'g', 0xFFD97706), // Brass Lock Base
    (b'G', 0xFFFBBF24), // Golden Lock Glint
];

pub static CHEST_SPRITE: [u32; 256] = parse_sprite([
    "MmmmmmmmmmmmmmmM",
    "mMMblllllllbbMMm",
    "mbblWWWWWWWWlbbm",
    "mbbwWWWWWWWWwbbm",
    "mbbssssssssssbbm",
    "mbbllllllllllbbm",
    "mbbWWWWggWWWWbbm",
    "mbbwwwwgGwwwwbbm",
    "mbbWWWWggWWWWbbm",
    "mbbssssssssssbbm",
    "mbbllllllllllbbm",
    "mbbWWWWWWWWWWbbm",
    "mbbwwwwwwwwwwbbm",
    "mbbWWWWWWWWWWbbm",
    "mMMbssssssssbMMm",
    "MmmmmmmmmmmmmmmM",
], CHEST_PAL);

// ===========================================================================
// 5. Defense Wall (Crenelated Stone Brick)
// ===========================================================================
const WALL_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'd', 0xFF1E293B), // Deep Mortar Shadow
    (b'm', 0xFF334155), // Stone Mortar
    (b's', 0xFF64748B), // Slate Stone Brick
    (b'S', 0xFF94A3B8), // Brick Top Highlight
];

pub static WALL_SPRITE: [u32; 256] = parse_sprite([
    "SSSS..SSSS..SSSS",
    "ssss..ssss..ssss",
    "sssd..sssd..sssd",
    "mmmmddmmmmddmmmm",
    "SSSSSSSSmSSSSSSS",
    "ssssssssmsssssss",
    "ssssssssmsssssss",
    "mmmmmmmmdmmmmmmm",
    "SSSdSSSSSSSSmSSS",
    "sssdssssssssmsss",
    "sssdssssssssmsss",
    "mmmmdmmmmmmmdmmm",
    "SSSSSSSSmSSSSSSS",
    "ssssssssmsssssss",
    "ssssssssmsssssss",
    "mmmmmmmmdmmmmmmm",
], WALL_PAL);

// ===========================================================================
// 6. Turret (Circular Armored Base with Directional Twin Cannons)
// ===========================================================================
const TURRET_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'b', 0xFF334155), // Outer Armored Base
    (b'B', 0xFF475569), // Base Highlight
    (b'r', 0xFF64748B), // Outer Ring Rivet
    (b'd', 0xFF1E293B), // Dome Interior Dark
    (b'c', 0xFF0284C7), // Gun Barrel Cyan
    (b'C', 0xFF38BDF8), // Gun Barrel Highlight
    (b'k', 0xFF0F172A), // Muzzle Bore
    (b'y', 0xFFFDE047), // Ammo Status Indicator
];

// Turret North
const TURRET_N: [u32; 256] = parse_sprite([
    "......k..k......",
    "......c..c......",
    "....rcC..Cc.....",
    "...bBcCc.cCcB...",
    "..bB..c..c..Bb..",
    ".bB...c..c...Bb.",
    ".Br...cCwwCc..rB",
    "bB..cCCwwCCc..Bb",
    "bB.dcCCwwCCcd.Bb",
    "bB..dddddddd..Bb",
    ".Br.dddddddd.rB.",
    ".bB.dddddddd.Bb.",
    "..bB.dyyyyd.Bb..",
    "...bBddddddBb...",
    "....rbbbbbr.....",
    "................",
], TURRET_PAL);

// Turret East
const TURRET_E: [u32; 256] = parse_sprite([
    "....rbbbbbr.....",
    "...bBddddddBb...",
    "..bB.dddddd.Bb..",
    ".bB..dddddd..Bb.",
    ".Br..ddcCCd...rB",
    "bB..ddcCCCccCCck",
    "bB..ddcCCCwwCC..",
    "bB.dyycCCCwwCC..",
    "bB.dyycCCCwwCC..",
    "bB..ddcCCCwwCC..",
    "bB..ddcCCCccCCck",
    ".Br..ddcCCd...rB",
    ".bB..dddddd..Bb.",
    "..bB.dddddd.Bb..",
    "...bBddddddBb...",
    "....rbbbbbr.....",
], TURRET_PAL);

// Turret South
const TURRET_S: [u32; 256] = parse_sprite([
    "....rbbbbbr.....",
    "...bBddddddBb...",
    "..bB.dyyyyd.Bb..",
    ".bB.dddddddd.Bb.",
    ".Br.dddddddd.rB.",
    "bB..dddddddd..Bb",
    "bB.dcCCwwCCcd.Bb",
    "bB..cCCwwCCc..Bb",
    ".Br...cCwwCc..rB",
    ".bB...c..c...Bb.",
    "..bB..c..c..Bb..",
    "...bBcCc.cCcB...",
    "....rcC..Cc.....",
    "......c..c......",
    "......k..k......",
    "................",
], TURRET_PAL);

// Turret West
const TURRET_W: [u32; 256] = parse_sprite([
    "....rbbbbbr.....",
    "...bBddddddBb...",
    "..bB.dddddd.Bb..",
    ".bB..dddddd..Bb.",
    ".Br...dCCcd..rB.",
    "kcCCccCCCcd..bB.",
    "..CCwwCCCcd..bB.",
    "..CCwwCCCcyydbB.",
    "..CCwwCCCcyydbB.",
    "..CCwwCCCcd..bB.",
    "kcCCccCCCcd..bB.",
    ".Br...dCCcd..rB.",
    ".bB..dddddd..Bb.",
    "..bB.dddddd.Bb..",
    "...bBddddddBb...",
    "....rbbbbbr.....",
], TURRET_PAL);

pub static TURRET_SPRITES: [[u32; 256]; 4] = [
    TURRET_N, TURRET_E, TURRET_S, TURRET_W,
];

// ===========================================================================
// 7. Base Heart (4 Pulsing Crystal Core Frames)
// ===========================================================================
const HEART_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'k', 0xFF0F172A), // Void Socket Border
    (b'd', 0xFF1E293B), // Socket Stone Frame
    (b'r', 0xFF0891B2), // Runic Energy Conduit
    (b'c', 0xFF06B6D4), // Crystal Body Cyan
    (b'C', 0xFF38BDF8), // Radiant Energy Cyan
    (b'e', 0xFF67E8F9), // Pulse Surge Wave
    (b'w', 0xFFE0F2FE), // White Flare
    (b'W', 0xFFFFFFFF), // Pure White Core
];

// Calm glow
const HEART_F0: [u32; 256] = parse_sprite([
    "..kkddddddddkk..",
    ".kdrrrrrrrrrrdk.",
    "kdrc........crdk",
    "kdr..........rdk",
    "dd............dd",
    "dd....cCCc....dd",
    "dd...cCCCCc...dd",
    "dr..cCCwwCCc..rd",
    "dr..cCCwwCCc..rd",
    "dd...cCCCCc...dd",
    "dd....cCCc....dd",
    "dd............dd",
    "kdr..........rdk",
    "kdrc........crdk",
    ".kdrrrrrrrrrrdk.",
    "..kkddddddddkk..",
], HEART_PAL);

// Expanding wave
const HEART_F1: [u32; 256] = parse_sprite([
    "..kkddddddddkk..",
    ".kdrrrrrrrrrrdk.",
    "kdree......eerdk",
    "kdre........erdk",
    "dd...eCCCCe...dd",
    "dd..eCCeeCCe..dd",
    "de.eCCwwwwCCe.ed",
    "dr.eCCwWWwCCe.rd",
    "dr.eCCwWWwCCe.rd",
    "de.eCCwwwwCCe.ed",
    "dd..eCCeeCCe..dd",
    "dd...eCCCCe...dd",
    "kdre........erdk",
    "kdree......eerdk",
    ".kdrrrrrrrrrrdk.",
    "..kkddddddddkk..",
], HEART_PAL);

// Peak brilliance
const HEART_F2: [u32; 256] = parse_sprite([
    "..kkddddddddkk..",
    ".kdrrrrrrrrrrdk.",
    "kdrWeeWWWWeeWrdk",
    "kdrW...ee...Wrdk",
    "ddW..eCCCCe..Wdd",
    "dd..eCCWWCCe..dd",
    "dWeeCCWWWWCCeeWd",
    "drWeCCWWWWCCeWrd",
    "drWeCCWWWWCCeWrd",
    "dWeeCCWWWWCCeeWd",
    "dd..eCCWWCCe..dd",
    "ddW..eCCCCe..Wdd",
    "kdrW...ee...Wrdk",
    "kdrWeeWWWWeeWrdk",
    ".kdrrrrrrrrrrdk.",
    "..kkddddddddkk..",
], HEART_PAL);

// Discharging pulse
const HEART_F3: [u32; 256] = parse_sprite([
    "..kkddddddddkk..",
    ".kdrrrrrrrrrrdk.",
    "kdrc..eeee..crdk",
    "kdree......eerdk",
    "dd...eCCCCe...dd",
    "dd..cCCeeCCc..dd",
    "dd.cCCewweCCc.dd",
    "dr.cCCwWWwCCc.rd",
    "dr.cCCwWWwCCc.rd",
    "dd.cCCewweCCc.dd",
    "dd..cCCeeCCc..dd",
    "dd...eCCCCe...dd",
    "kdree......eerdk",
    "kdrc..eeee..crdk",
    ".kdrrrrrrrrrrdk.",
    "..kkddddddddkk..",
], HEART_PAL);

pub static HEART_FRAMES: [[u32; 256]; 4] = [
    HEART_F0, HEART_F1, HEART_F2, HEART_F3,
];

// ===========================================================================
// 8. Ore Deposit (Cracked Bedrock with Sparkling Metallic Facets)
// ===========================================================================
const ORE_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'k', 0xFF0F172A), // Deep Rock Fissure
    (b'd', 0xFF1E293B), // Ground Bedrock
    (b'r', 0xFF334155), // Rough Stone Texture
    (b'o', 0xFF64748B), // Dark Ore Vein
    (b'O', 0xFF94A3B8), // Mid Iron Facet
    (b's', 0xFFCBD5E1), // Bright Mineral Edge
    (b'S', 0xFFE2E8F0), // Gleaming Sparkle
    (b'w', 0xFFFFFFFF), // White Specular Glint
];

pub static ORE_DEPOSIT: [u32; 256] = parse_sprite([
    "ddddrdddkkddddrd",
    "drrddkkddkrrrddd",
    "dddkkkdOOskkddrd",
    "ddkkddoOOSSwkddd",
    "dkkdrdoOOSswdkrd",
    "kkrdddoOOskkdddd",
    "kdddkkddkddrkddd",
    "drdkkdddkrkkddrd",
    "dddkddoOskkdrddd",
    "drkkdoOOSSwkddrd",
    "dkkdooOOSswddrdd",
    "kkdddoOskkkddrdd",
    "kddrkddkdrkkdddd",
    "drddrkkdddkkdrrd",
    "ddrdddkkddkkdddd",
    "rddddrddkkdddddr",
], ORE_PAL);

// ===========================================================================
// 9. Monster Goblin (Red Horned Fiend with Glowing Piercing Eyes)
// ===========================================================================
const GOBLIN_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'h', 0xFF450A0A), // Horn Dark Base
    (b'H', 0xFF7F1D1D), // Horn Mid Maroon
    (b'd', 0xFF991B1B), // Dark Skin Shading
    (b'b', 0xFFB91C1C), // Goblin Body Red
    (b'B', 0xFFDC2626), // Mid Crimson
    (b'r', 0xFFEF4444), // Bright Red Highlight
    (b'y', 0xFFFEF08A), // Glowing Yellow Eyes
    (b'k', 0xFF1C1917), // Claw / Talon Dark
];

// Goblin Frame 0: Left stride forward
const GOBLIN_F0: [u32; 256] = parse_sprite([
    "..Hh........hH..",
    "..HHh......hHH..",
    "...HHh.dd.hHH...",
    "....HbbddbbH....",
    "....bbbrrbbb....",
    "...bbbyrrybbb...",
    "...dbbkkkkbbd...",
    "....ddbbbbdd....",
    "..rbbBBBBBBbbr..",
    ".rrdbBBBBBBbdrr.",
    ".k.ddBBBBBBdd.k.",
    "....dbbddbbd....",
    "....dbb..bbd....",
    "...kbb....bbk...",
    "..kbb......k....",
    "................",
], GOBLIN_PAL);

// Goblin Frame 1: Ready stance
const GOBLIN_F1: [u32; 256] = parse_sprite([
    "..Hh........hH..",
    "..HHh......hHH..",
    "...HHh.dd.hHH...",
    "....HbbddbbH....",
    "....bbbrrbbb....",
    "...bbbyrrybbb...",
    "...dbbkkkkbbd...",
    "....ddbbbbdd....",
    "..rbbBBBBBBbbr..",
    ".rrdbBBBBBBbdrr.",
    ".k.ddBBBBBBdd.k.",
    "....dbbddbbd....",
    "....dbb..bbd....",
    "...kbb....bbk...",
    "...kb......bk...",
    "................",
], GOBLIN_PAL);

// Goblin Frame 2: Right stride forward
const GOBLIN_F2: [u32; 256] = parse_sprite([
    "..Hh........hH..",
    "..HHh......hHH..",
    "...HHh.dd.hHH...",
    "....HbbddbbH....",
    "....bbbrrbbb....",
    "...bbbyrrybbb...",
    "...dbbkkkkbbd...",
    "....ddbbbbdd....",
    "..rbbBBBBBBbbr..",
    ".rrdbBBBBBBbdrr.",
    ".k.ddBBBBBBdd.k.",
    "....dbbddbbd....",
    "....dbb..bbd....",
    "...kbb....bbk...",
    "....k......bbk..",
    "................",
], GOBLIN_PAL);

// Goblin Frame 3: Breathing ready stance
const GOBLIN_F3: [u32; 256] = parse_sprite([
    "..Hh........hH..",
    "..HHh......hHH..",
    "...HHh.dd.hHH...",
    "....HbbddbbH....",
    "....bbbrrbbb....",
    "...bbbyrrybbb...",
    "...dbbkkkkbbd...",
    "....ddbbbbdd....",
    "..rbbBBBBBBbbr..",
    ".rrdbBBBBBBbdrr.",
    ".k.ddBBBBBBdd.k.",
    "....dbbddbbd....",
    "....dbb..bbd....",
    "...kbb....bbk...",
    "...kb......bk...",
    "................",
], GOBLIN_PAL);

pub static MONSTER_FRAMES: [[u32; 256]; 4] = [
    GOBLIN_F0, GOBLIN_F1, GOBLIN_F2, GOBLIN_F3,
];

// ===========================================================================
// 10. Robotic Inserter (Mechanical Articulated Claw)
// ===========================================================================
const INSERTER_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'v', 0xFF4C1D95), // Deep Violet Mount
    (b'V', 0xFF6D28D9), // Violet Body
    (b'p', 0xFF8B5CF6), // Pivot Joint
    (b'a', 0xFFA78BFA), // Robotic Arm
    (b'c', 0xFFC4B5FD), // Claw Gripper
    (b'C', 0xFFDDD6FE), // Claw Tip
];

// Inserter North
const INSERTER_N: [u32; 256] = parse_sprite([
    "......C..C......",
    "......c..c......",
    ".......cc.......",
    ".......aa.......",
    ".......aa.......",
    ".......aa.......",
    ".......aa.......",
    "......pppp......",
    ".....pVVVVp.....",
    "....vVVVVVVv....",
    "...vvVVVVVVvv...",
    "...vvvVVVVvvv...",
    "....vvvvvvvv....",
    "................",
    "................",
    "................",
], INSERTER_PAL);

// Inserter East
const INSERTER_E: [u32; 256] = parse_sprite([
    "................",
    "....vvvv........",
    "...vvVVvv.......",
    "..vvVVVVvv......",
    ".vVVVVVVVVp.....",
    ".vVVVVpppppaaaaa",
    ".vVVVVpppppaacCC",
    ".vVVVVpppppaacCC",
    ".vVVVVpppppaaaaa",
    ".vVVVVVVVVp.....",
    "..vvVVVVvv......",
    "...vvVVvv.......",
    "....vvvv........",
    "................",
    "................",
    "................",
], INSERTER_PAL);

// Inserter South
const INSERTER_S: [u32; 256] = parse_sprite([
    "................",
    "................",
    "................",
    "....vvvvvvvv....",
    "...vvvVVVVvvv...",
    "...vvVVVVVVvv...",
    "....vVVVVVVv....",
    ".....pVVVVp.....",
    "......pppp......",
    ".......aa.......",
    ".......aa.......",
    ".......aa.......",
    ".......aa.......",
    ".......cc.......",
    "......c..c......",
    "......C..C......",
], INSERTER_PAL);

// Inserter West
const INSERTER_W: [u32; 256] = parse_sprite([
    "................",
    "........vvvv....",
    ".......vvVVvv...",
    "......vvVVVVvv..",
    ".....pVVVVVVVVv.",
    "aaaaapppppVVVVv.",
    "CCcaapppppVVVVv.",
    "CCcaapppppVVVVv.",
    "aaaaapppppVVVVv.",
    ".....pVVVVVVVVv.",
    "......vvVVVVvv..",
    ".......vvVVvv...",
    "........vvvv....",
    "................",
    "................",
    "................",
], INSERTER_PAL);

pub static INSERTER_SPRITES: [[u32; 256]; 4] = [
    INSERTER_N, INSERTER_E, INSERTER_S, INSERTER_W,
];

// ===========================================================================
// Public Lookup Helpers
// ===========================================================================

/// Returns the sprite for a belt given direction and tick.
#[inline(always)]
pub fn get_belt_sprite(direction: u32, tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    let frame_idx = (tick as usize / 2) % 4;
    &BELT_FRAMES[dir_idx][frame_idx]
}

/// Returns the sprite for a mining drill given direction and tick.
#[inline(always)]
pub fn get_drill_sprite(direction: u32, tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    let frame_idx = (tick as usize / 2) % 4;
    &DRILL_FRAMES[dir_idx][frame_idx]
}

/// Returns the sprite for a smelter furnace given tick.
#[inline(always)]
pub fn get_smelter_sprite(tick: u64) -> &'static [u32; 256] {
    let frame_idx = (tick as usize / 3) % 4;
    &SMELTER_FRAMES[frame_idx]
}

/// Returns the sprite for a chest.
#[inline(always)]
pub const fn get_chest_sprite() -> &'static [u32; 256] {
    &CHEST_SPRITE
}

/// Returns the sprite for a defense wall.
#[inline(always)]
pub const fn get_wall_sprite() -> &'static [u32; 256] {
    &WALL_SPRITE
}

/// Returns the sprite for a defense turret given direction.
#[inline(always)]
pub fn get_turret_sprite(direction: u32) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    &TURRET_SPRITES[dir_idx]
}

/// Returns the sprite for the base heart given tick.
#[inline(always)]
pub fn get_heart_sprite(tick: u64) -> &'static [u32; 256] {
    let frame_idx = (tick as usize / 6) % 4;
    &HEART_FRAMES[frame_idx]
}

/// Returns the sprite for an ore deposit.
#[inline(always)]
pub const fn get_deposit_sprite(_tick: u64) -> &'static [u32; 256] {
    &ORE_DEPOSIT
}

/// Returns the sprite for a monster given tick.
#[inline(always)]
pub fn get_monster_sprite(tick: u64) -> &'static [u32; 256] {
    let frame_idx = (tick as usize / 4) % 4;
    &MONSTER_FRAMES[frame_idx]
}

/// Returns the sprite for an inserter arm given direction and tick.
#[inline(always)]
pub fn get_inserter_sprite(direction: u32, _tick: u64) -> &'static [u32; 256] {
    let dir_idx = (direction as usize) % 4;
    &INSERTER_SPRITES[dir_idx]
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
    fn test_sprite_dimensions_and_alpha_integrity() {
        // 1. Verify dimensions
        assert_eq!(EMPTY_SPRITE.len(), 256);
        assert_eq!(CHEST_SPRITE.len(), 256);
        assert_eq!(WALL_SPRITE.len(), 256);
        assert_eq!(ORE_DEPOSIT.len(), 256);

        // 2. Verify all empty pixels are zero alpha
        for &px in EMPTY_SPRITE.iter() {
            assert_eq!(px >> 24, 0);
        }

        // 3. Verify structures have non-zero alpha pixels
        let test_sprites = [
            &CHEST_SPRITE,
            &WALL_SPRITE,
            &ORE_DEPOSIT,
            &BELT_FRAMES[0][0],
            &DRILL_FRAMES[0][0],
            &SMELTER_FRAMES[0],
            &TURRET_SPRITES[0],
            &HEART_FRAMES[0],
            &MONSTER_FRAMES[0],
            &INSERTER_SPRITES[0],
        ];

        for sprite in test_sprites {
            assert_eq!(sprite.len(), 256);
            let opaque_count = sprite.iter().filter(|&&p| (p >> 24) > 0).count();
            assert!(
                opaque_count >= 30,
                "Sprite should have sufficient visible pixels, found {}",
                opaque_count
            );
        }

        // 4. Verify helper resolver works for all kinds and directions
        for kind in 0..=10 {
            for dir in 0..4 {
                for tick in [0u64, 1, 5, 12, 100] {
                    let s = render_structure_sprite(kind, dir, tick);
                    assert_eq!(s.len(), 256);
                    let alias_s = get_animated_sprite(kind, dir, tick);
                    assert_eq!(s, alias_s);
                }
            }
        }

        // 5. Verify animation frames cycle across ticks
        let belt_f0 = render_structure_sprite(SPRITE_KIND_BELT, DIR_NORTH, 0);
        let belt_f1 = render_structure_sprite(SPRITE_KIND_BELT, DIR_NORTH, 2);
        assert_ne!(belt_f0, belt_f1);

        let heart_f0 = render_structure_sprite(SPRITE_KIND_BASE_HEART, 0, 0);
        let heart_f1 = render_structure_sprite(SPRITE_KIND_BASE_HEART, 0, 6);
        assert_ne!(heart_f0, heart_f1);

        let smelter_f0 = render_structure_sprite(SPRITE_KIND_SMELTER, 0, 0);
        let smelter_f1 = render_structure_sprite(SPRITE_KIND_SMELTER, 0, 3);
        assert_ne!(smelter_f0, smelter_f1);

        let monster_f0 = get_monster_sprite(0);
        let monster_f2 = get_monster_sprite(8);
        assert_ne!(monster_f0, monster_f2);
    }
}
