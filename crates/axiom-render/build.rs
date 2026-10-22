// ==============================================================================
// Axiom Render — Build-Time Binary Sprite Atlas Baker (build.rs)
// ==============================================================================
//! Bakes 16x16 pixel sprites into a 64-byte aligned binary slab (`sprites.atlas`).
//!
//! Format:
//! - 64B AtlasHeader (magic 0x41544C415358414D, version, sprite_count, checksum)
//! - 56 x 1024-byte sprite slices ([u32; 256] little-endian ARGB pixels)

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

const fn parse_sprite(rows: [&str; 16], pal: &[(u8, u32)]) -> [u32; 256] {
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

fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// -----------------------------------------------------------------------------
// Color Palettes
// -----------------------------------------------------------------------------
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

const WALL_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'd', 0xFF1E293B), // Deep Mortar Shadow
    (b'm', 0xFF334155), // Stone Mortar
    (b's', 0xFF64748B), // Slate Stone Brick
    (b'S', 0xFF94A3B8), // Brick Top Highlight
];

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

const INSERTER_PAL: &[(u8, u32)] = &[
    (b'.', 0x00000000), // Transparent
    (b'v', 0xFF4C1D95), // Deep Violet Mount
    (b'V', 0xFF6D28D9), // Violet Body
    (b'p', 0xFF8B5CF6), // Pivot Joint
    (b'a', 0xFFA78BFA), // Robotic Arm
    (b'c', 0xFFC4B5FD), // Claw Gripper
    (b'C', 0xFFDDD6FE), // Claw Tip
];

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest_path = Path::new(&out_dir).join("sprites.atlas");

    // 0: EMPTY
    let empty_sprite = [0u32; 256];

    // 1..4: Belt North
    let belt_n_f0 = parse_sprite([
        "BgbbbbYYYYbbbbgB", "BgbbbyyyyyybbbgB", "BgbyyyoooooyybgB", "BgyyoobbbbooyygB",
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgbbbbYYYYbbbbgB",
        "BgbbbyyyyyybbbgB", "BgbyyyoooooyybgB", "BgyyoobbbbooyygB", "BgbbbbbbbbbbbbgB",
        "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB",
    ], BELT_PAL);
    let belt_n_f1 = parse_sprite([
        "BgbyyyoooooyybgB", "BgyyoobbbbooyygB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB",
        "BgbbbbbbbbbbbbgB", "BgbbbbYYYYbbbbgB", "BgbbbyyyyyybbbgB", "BgbyyyoooooyybgB",
        "BgyyoobbbbooyygB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB",
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbYYYYbbbbgB", "BgbbbyyyyyybbbgB",
    ], BELT_PAL);
    let belt_n_f2 = parse_sprite([
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgbbbbYYYYbbbbgB",
        "BgbbbyyyyyybbbgB", "BgbyyyoooooyybgB", "BgyyoobbbbooyygB", "BgbbbbbbbbbbbbgB",
        "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB",
        "BgbbbbYYYYbbbbgB", "BgbbbyyyyyybbbgB", "BgbyyyoooooyybgB", "BgyyoobbbbooyygB",
    ], BELT_PAL);
    let belt_n_f3 = parse_sprite([
        "BgbbbbbbbbbbbbgB", "BgbbbbYYYYbbbbgB", "BgbbbyyyyyybbbgB", "BgbyyyoooooyybgB",
        "BgyyoobbbbooyygB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB",
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbYYYYbbbbgB", "BgbbbyyyyyybbbgB",
        "BgbyyyoooooyybgB", "BgyyoobbbbooyygB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB",
    ], BELT_PAL);

    // 5..8: Belt East
    let belt_e_f0 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "bbyybbbbbbbyybbb", "bbyyybbbbbbyyybb",
        "bbooyybbbbbooyyb", "bbbbooybbbbbbooy", "bbbbboYYbbbbbboY", "rrrrroYYrrrrrroY",
        "bbbbboYYbbbbbboY", "bbbbooybbbbbbooy", "bbooyybbbbbooyyb", "bbyyybbbbbbyyybb",
        "bbyybbbbbbbyybbb", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);
    let belt_e_f1 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "bbbbyybbbbbbbyyb", "bbbbyyybbbbbbyyy",
        "bbbooyybbbbbooyy", "bbbbbooybbbbbboo", "YbbbbboYYbbbbbbo", "YrbbbboYYrrrrrro",
        "YbbbbboYYbbbbbbo", "bbbbbooybbbbbboo", "bbbooyybbbbbooyy", "bbbbyyybbbbbbyyy",
        "bbbbyybbbbbbbyyb", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);
    let belt_e_f2 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "bbbbbbyybbbbbbby", "bbbbbbyyybbbbbby",
        "ybbbbbooyybbbbbo", "yybbbbbbooybbbbb", "oYYbbbbbboYYbbbb", "oYYrrrrrroYYrrrr",
        "oYYbbbbbboYYbbbb", "yybbbbbbooybbbbb", "ybbbbbooyybbbbbo", "bbbbbbyyybbbbbby",
        "bbbbbbyybbbbbbby", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);
    let belt_e_f3 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "ybbbbbbbyybbbbbb", "yybbbbbbyyybbbbb",
        "oyybbbbbooyybbbb", "booybbbbbbooybbb", "bboYYbbbbbboYYbb", "rroYYrrrrrroYYrr",
        "bboYYbbbbbboYYbb", "booybbbbbbooybbb", "oyybbbbbooyybbbb", "yybbbbbbyyybbbbb",
        "ybbbbbbbyybbbbbb", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);

    // 9..12: Belt South
    let belt_s_f0 = parse_sprite([
        "BgyyoobbbbooyygB", "BgbyyyoooooyybgB", "BgbbbyyyyyybbbgB", "BgbbbbYYYYbbbbgB",
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgyyoobbbbooyygB",
        "BgbyyyoooooyybgB", "BgbbbyyyyyybbbgB", "BgbbbbYYYYbbbbgB", "BgbbbbbbbbbbbbgB",
        "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB",
    ], BELT_PAL);
    let belt_s_f1 = parse_sprite([
        "BgbbbbYYYYbbbbgB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB",
        "BgyyoobbbbooyygB", "BgbyyyoooooyybgB", "BgbbbyyyyyybbbgB", "BgbbbbYYYYbbbbgB",
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgbbbbbbbbbbbbgB",
        "BgbrrrrrrrrrrbgB", "BgyyoobbbbooyygB", "BgbyyyoooooyybgB", "BgbbbyyyyyybbbgB",
    ], BELT_PAL);
    let belt_s_f2 = parse_sprite([
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgyyoobbbbooyygB",
        "BgbyyyoooooyybgB", "BgbbbyyyyyybbbgB", "BgbbbbYYYYbbbbgB", "BgbbbbbbbbbbbbgB",
        "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB",
        "BgyyoobbbbooyygB", "BgbyyyoooooyybgB", "BgbbbyyyyyybbbgB", "BgbbbbYYYYbbbbgB",
    ], BELT_PAL);
    let belt_s_f3 = parse_sprite([
        "BgbbbbbbbbbbbbgB", "BgyyoobbbbooyygB", "BgbyyyoooooyybgB", "BgbbbyyyyyybbbgB",
        "BgbbbbYYYYbbbbgB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgbbbbbbbbbbbbgB",
        "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB", "BgyyoobbbbooyygB", "BgbyyyoooooyybgB",
        "BgbbbyyyyyybbbgB", "BgbbbbYYYYbbbbgB", "BgbbbbbbbbbbbbgB", "BgbrrrrrrrrrrbgB",
    ], BELT_PAL);

    // 13..16: Belt West
    let belt_w_f0 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "bbbyybbbbbbbyybb", "bbyyybbbbbbyyybb",
        "byyoobbbbbyyoobb", "yooabbbbbbyooabb", "YoobbbbbbYoobbbb", "YoorrrrrrYoorrrr",
        "YoobbbbbbYoobbbb", "yooabbbbbbyooabb", "byyoobbbbbyyoobb", "bbyyybbbbbbyyybb",
        "bbbyybbbbbbbyybb", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);
    let belt_w_f1 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "bbyybbbbbbbyybbb", "byyybbbbbbyyybbb",
        "yyoobbbbbyyoobbb", "ooabbbbbbyooabbb", "oobbbbbbYoobbbbb", "oorrrrrrYoorrrrr",
        "oobbbbbbYoobbbbb", "ooabbbbbbyooabbb", "yyoobbbbbyyoobbb", "byyybbbbbbyyybbb",
        "bbyybbbbbbbyybbb", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);
    let belt_w_f2 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "byybbbbbbbyybbbb", "yyybbbbbbyyybbbb",
        "yoobbbbbyyoobbbb", "oabbbbbbyooabbbb", "obbbbbbYoobbbbbb", "orrrrrrYoorrrrrr",
        "obbbbbbYoobbbbbb", "oabbbbbbyooabbbb", "yoobbbbbyyoobbbb", "yyybbbbbbyyybbbb",
        "byybbbbbbbyybbbb", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);
    let belt_w_f3 = parse_sprite([
        "BBBBBBBBBBBBBBBB", "gggggggggggggggg", "yybbbbbbbyybbbbb", "yybbbbbbyyybbbbb",
        "oobbbbbyyoobbbbb", "abbbbbbyooabbbbb", "bbbbbbYoobbbbbbb", "rrrrrrYoorrrrrrr",
        "bbbbbbYoobbbbbbb", "abbbbbbyooabbbbb", "oobbbbbyyoobbbbb", "yybbbbbbyyybbbbb",
        "yybbbbbbbyybbbbb", "bbbbbbbbbbbbbbbb", "gggggggggggggggg", "BBBBBBBBBBBBBBBB",
    ], BELT_PAL);

    // 17..20: Drill North
    let drill_n_f0 = parse_sprite([
        "mrmmYYYmYYYmmrmm", "rdddyyydyyydddrr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mdddddcCCddddddm", "mdddddcCCddddddm", "mddCCCwwCCwCCddm",
        "mddcccwwccwccddm", "mdddddcCCddddddm", "mdddddcCCddddddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rddddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_n_f1 = parse_sprite([
        "mrmmYYYmYYYmmrmm", "rdddyyydyyydddrr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mddCcddddddcCddm", "mdddCcddddcCdddm", "mddddCcwwcCddddm",
        "mddddcCwwCcddddm", "mdddCcddddcCdddm", "mddCcddddddcCddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rddddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_n_f2 = parse_sprite([
        "mrmmYYYmYYYmmrmm", "rdddyyydyyydddrr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mddddddcCCdddddm", "mddddddcCCdddddm", "mddCCCwwCCwCCddm",
        "mddcccwwccwccddm", "mdddddcCCddddddm", "mdddddcCCddddddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rddddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_n_f3 = parse_sprite([
        "mrmmYYYmYYYmmrmm", "rdddyyydyyydddrr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mddddcCddddCcddm", "mdddCcddddcCdddm", "mddddCcwwcCddddm",
        "mddddcCwwCcddddm", "mdddCcddddcCdddm", "mddcCddddddCcddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rddddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);

    // 21..24: Drill East
    let drill_e_f0 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rdddddddddddddrr", "mdddddddddddddyY", "mddmmmddddmmmdyY",
        "mddmrrddddrrmdmY", "mdddddcCCdddddmY", "mdddddcCCdddddmY", "mddCCCwwCCwCCdmY",
        "mddcccwwccwccdmY", "mdddddcCCdddddmY", "mdddddcCCdddddmY", "mddmrrddddrrmdmY",
        "mddmmmddddmmmdyY", "mdddddddddddddyY", "rdddddddddddddrr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_e_f1 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rdddddddddddddrr", "mdddddddddddddyY", "mddmmmddddmmmdyY",
        "mddmrrddddrrmdmY", "mddCcddddddcCdmy", "mdddCcddddcCdymy", "mddddCcwwcCdymyy",
        "mddddcCwwCcdymyy", "mdddCcddddcCdymy", "mddCcddddddcCdmy", "mddmrrddddrrmdmY",
        "mddmmmddddmmmdyY", "mdddddddddddddyY", "rdddddddddddddrr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_e_f2 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rdddddddddddddrr", "mdddddddddddddyY", "mddmmmddddmmmdyY",
        "mddmrrddddrrmdmY", "mddddddcCCddddmy", "mddddddcCCddddmy", "mddCCCwwCCwCCdmy",
        "mddcccwwccwccdmy", "mdddddcCCdddddmy", "mdddddcCCdddddmy", "mddmrrddddrrmdmY",
        "mddmmmddddmmmdyY", "mdddddddddddddyY", "rdddddddddddddrr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_e_f3 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rdddddddddddddrr", "mdddddddddddddyY", "mddmmmddddmmmdyY",
        "mddmrrddddrrmdmY", "mddddcCddddCcdmy", "mdddCcddddcCdymy", "mddddCcwwcCdymyy",
        "mddddcCwwCcdymyy", "mdddCcddddcCdymy", "mddcCddddddCcdmy", "mddmrrddddrrmdmY",
        "mddmmmddddmmmdyY", "mdddddddddddddyY", "rdddddddddddddrr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);

    // 25..28: Drill South
    let drill_s_f0 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rddddddddddddddr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mdddddcCCddddddm", "mdddddcCCddddddm", "mddCCCwwCCwCCddm",
        "mddcccwwccwccddm", "mdddddcCCddddddm", "mdddddcCCddddddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rdddyyydyyydddrr", "mrmmYYYmYYYmmrmm",
    ], DRILL_PAL);
    let drill_s_f1 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rddddddddddddddr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mddCcddddddcCddm", "mdddCcddddcCdddm", "mddddCcwwcCddddm",
        "mddddcCwwCcddddm", "mdddCcddddcCdddm", "mddCcddddddcCddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rdddyyydyyydddrr", "mrmmYYYmYYYmmrmm",
    ], DRILL_PAL);
    let drill_s_f2 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rddddddddddddddr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mddddddcCCdddddm", "mddddddcCCdddddm", "mddCCCwwCCwCCddm",
        "mddcccwwccwccddm", "mdddddcCCddddddm", "mdddddcCCddddddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rdddyyydyyydddrr", "mrmmYYYmYYYmmrmm",
    ], DRILL_PAL);
    let drill_s_f3 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rddddddddddddddr", "mddddddddddddddm", "mddmmmddddmmmddm",
        "mddmrrddddrrmddm", "mddddcCddddCcddm", "mdddCcddddcCdddm", "mddddCcwwcCddddm",
        "mddddcCwwCcddddm", "mdddCcddddcCdddm", "mddcCddddddCcddm", "mddmrrddddrrmddm",
        "mddmmmddddmmmddm", "mddddddddddddddm", "rdddyyydyyydddrr", "mrmmYYYmYYYmmrmm",
    ], DRILL_PAL);

    // 29..32: Drill West
    let drill_w_f0 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rrdddddddddddddr", "Yydddddddddddddm", "Yydmmmddddmmmddm",
        "Ymdmrrddddrrmddm", "YmddddcCCddddddm", "YmddddcCCddddddm", "YmddCCCwwCCwCCdm",
        "Ymddcccwwccwccdm", "YmddddcCCddddddm", "YmddddcCCddddddm", "Ymdmrrddddrrmddm",
        "Yydmmmddddmmmddm", "Yydddddddddddddm", "rrdddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_w_f1 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rrdddddddddddddr", "Yydddddddddddddm", "Yydmmmddddmmmddm",
        "Ymdmrrddddrrmddm", "ymdCcddddddcCddm", "ymddCcddddcCdddm", "yymddCcwwcCddddm",
        "yymddcCwwCcddddm", "ymddCcddddcCdddm", "ymdCcddddddcCddm", "Ymdmrrddddrrmddm",
        "Yydmmmddddmmmddm", "Yydddddddddddddm", "rrdddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_w_f2 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rrdddddddddddddr", "Yydddddddddddddm", "Yydmmmddddmmmddm",
        "Ymdmrrddddrrmddm", "ymdddddcCCdddddm", "ymdddddcCCdddddm", "ymddCCCwwCCwCCdm",
        "ymddcccwwccwccdm", "ymdddddcCCdddddm", "ymdddddcCCdddddm", "Ymdmrrddddrrmddm",
        "Yydmmmddddmmmddm", "Yydddddddddddddm", "rrdddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);
    let drill_w_f3 = parse_sprite([
        "mrmmmmmmmmmmmmrm", "rrdddddddddddddr", "Yydddddddddddddm", "Yydmmmddddmmmddm",
        "Ymdmrrddddrrmddm", "ymddddcCddddCcdd", "ymdddCcddddcCddd", "yymdddCcwwcCdddd",
        "yymdddcCwwCcdddd", "ymdddCcddddcCddd", "ymddcCddddddCcdd", "Ymdmrrddddrrmddm",
        "Yydmmmddddmmmddm", "Yydddddddddddddm", "rrdddddddddddddr", "mrmmmmmmmmmmmmrm",
    ], DRILL_PAL);

    // 33..36: Smelter
    let smelter_f0 = parse_sprite([
        "..sddkkkkkkdds..", ".sdddkkkkkkddds.", "sddddssssssdddds", "sddssssssssssdds",
        "sdssssssssssssds", "sdssfssssssfsdds", "sdsfffffffffssds", "sdsffrrrrrrffsds",
        "sdsffrooyorffsds", "sdsffroyyyOffsds", "sdsffroyYYOffsds", "sdsffrOYyOYrfsds",
        "sdssfroyoyOffsds", "sddssffffffssdds", "sddddssssssdddds", ".sdddddddddddds.",
    ], SMELTER_PAL);
    let smelter_f1 = parse_sprite([
        "..sddkkkkkkdds..", ".sdddkkkkkkddds.", "sddddssssssdddds", "sddssssssssssdds",
        "sdssssssssssssds", "sdssfssssssfsdds", "sdsfffffffffssds", "sdsffrrYYrrffsds",
        "sdsffrOYYoyffsds", "sdsffroyYYOffsds", "sdsffroyyyOffsds", "sdsffrOYyyYrfsds",
        "sdssfroyOyOffsds", "sddssffffffssdds", "sddddssssssdddds", ".sdddddddddddds.",
    ], SMELTER_PAL);
    let smelter_f2 = parse_sprite([
        "..sddkkkkkkdds..", ".sdddkkkkkkddds.", "sddddssssssdddds", "sddssssssssssdds",
        "sdssssssssssssds", "sdssfssssssfsdds", "sdsfffffffffssds", "sdsffrYYYYrffsds",
        "sdsffroYYYYOffsd", "sdsffroyYYyOffsd", "sdsffroYYYYOffsd", "sdsffrOYyyOYrfsd",
        "sdssfroyyyOffsds", "sddssffffffssdds", "sddddssssssdddds", ".sdddddddddddds.",
    ], SMELTER_PAL);
    let smelter_f3 = parse_sprite([
        "..sddkkkkkkdds..", ".sdddkkkkkkddds.", "sddddssssssdddds", "sddssssssssssdds",
        "sdssssssssssssds", "sdssfssssssfsdds", "sdsfffffffffssds", "sdsffrryyrrffsds",
        "sdsffroyYYOffsds", "sdsffroYYYYOffsd", "sdsffrOYYYYrffsd", "sdsffroyYYOffsds",
        "sdssfroyoyOffsds", "sddssffffffssdds", "sddddssssssdddds", ".sdddddddddddds.",
    ], SMELTER_PAL);

    // 37: Chest
    let chest_sprite = parse_sprite([
        "MmmmmmmmmmmmmmmM", "mMMblllllllbbMMm", "mbblWWWWWWWWlbbm", "mbbwWWWWWWWWwbbm",
        "mbbssssssssssbbm", "mbbllllllllllbbm", "mbbWWWWggWWWWbbm", "mbbwwwwgGwwwwbbm",
        "mbbWWWWggWWWWbbm", "mbbssssssssssbbm", "mbbllllllllllbbm", "mbbWWWWWWWWWWbbm",
        "mbbwwwwwwwwwwbbm", "mbbWWWWWWWWWWbbm", "mMMbssssssssbMMm", "MmmmmmmmmmmmmmmM",
    ], CHEST_PAL);

    // 38: Wall
    let wall_sprite = parse_sprite([
        "SSSS..SSSS..SSSS", "ssss..ssss..ssss", "sssd..sssd..sssd", "mmmmddmmmmddmmmm",
        "SSSSSSSSmSSSSSSS", "ssssssssmsssssss", "ssssssssmsssssss", "mmmmmmmmdmmmmmmm",
        "SSSdSSSSSSSSmSSS", "sssdssssssssmsss", "sssdssssssssmsss", "mmmmdmmmmmmmdmmm",
        "SSSSSSSSmSSSSSSS", "ssssssssmsssssss", "ssssssssmsssssss", "mmmmmmmmdmmmmmmm",
    ], WALL_PAL);

    // 39..42: Turret (N, E, S, W)
    let turret_n = parse_sprite([
        "......k..k......", "......c..c......", "....rcC..Cc.....", "...bBcCc.cCcB...",
        "..bB..c..c..Bb..", ".bB...c..c...Bb.", ".Br...cCwwCc..rB", "bB..cCCwwCCc..Bb",
        "bB.dcCCwwCCcd.Bb", "bB..dddddddd..Bb", ".Br.dddddddd.rB.", ".bB.dddddddd.Bb.",
        "..bB.dyyyyd.Bb..", "...bBddddddBb...", "....rbbbbbr.....", "................",
    ], TURRET_PAL);
    let turret_e = parse_sprite([
        "....rbbbbbr.....", "...bBddddddBb...", "..bB.dddddd.Bb..", ".bB..dddddd..Bb.",
        ".Br..ddcCCd...rB", "bB..ddcCCCccCCck", "bB..ddcCCCwwCC..", "bB.dyycCCCwwCC..",
        "bB.dyycCCCwwCC..", "bB..ddcCCCwwCC..", "bB..ddcCCCccCCck", ".Br..ddcCCd...rB",
        ".bB..dddddd..Bb.", "..bB.dddddd.Bb..", "...bBddddddBb...", "....rbbbbbr.....",
    ], TURRET_PAL);
    let turret_s = parse_sprite([
        "....rbbbbbr.....", "...bBddddddBb...", "..bB.dyyyyd.Bb..", ".bB.dddddddd.Bb.",
        ".Br.dddddddd.rB.", "bB..dddddddd..Bb", "bB.dcCCwwCCcd.Bb", "bB..cCCwwCCc..Bb",
        ".Br...cCwwCc..rB", ".bB...c..c...Bb.", "..bB..c..c..Bb..", "...bBcCc.cCcB...",
        "....rcC..Cc.....", "......c..c......", "......k..k......", "................",
    ], TURRET_PAL);
    let turret_w = parse_sprite([
        "....rbbbbbr.....", "...bBddddddBb...", "..bB.dddddd.Bb..", ".bB..dddddd..Bb.",
        ".Br...dCCcd..rB.", "kcCCccCCCcd..bB.", "..CCwwCCCcd..bB.", "..CCwwCCCcyydbB.",
        "..CCwwCCCcyydbB.", "..CCwwCCCcd..bB.", "kcCCccCCCcd..bB.", ".Br...dCCcd..rB.",
        ".bB..dddddd..Bb.", "..bB.dddddd.Bb..", "...bBddddddBb...", "....rbbbbbr.....",
    ], TURRET_PAL);

    // 43..46: Heart (F0..F3)
    let heart_f0 = parse_sprite([
        "..kkddddddddkk..", ".kdrrrrrrrrrrdk.", "kdrc........crdk", "kdr..........rdk",
        "dd............dd", "dd....cCCc....dd", "dd...cCCCCc...dd", "dr..cCCwwCCc..rd",
        "dr..cCCwwCCc..rd", "dd...cCCCCc...dd", "dd...cCCCCc...dd", "dd....cCCc....dd",
        "dd............dd", "kdr..........rdk", "kdrc........crdk", ".kdrrrrrrrrrrdk.",
    ], HEART_PAL);
    let heart_f1 = parse_sprite([
        "..kkddddddddkk..", ".kdrrrrrrrrrrdk.", "kdree......eerdk", "kdre........erdk",
        "dd...eCCCCe...dd", "dd..eCCeeCCe..dd", "de.eCCwwwwCCe.ed", "dr.eCCwWWwCCe.rd",
        "dr.eCCwWWwCCe.rd", "de.eCCwwwwCCe.ed", "dd..eCCeeCCe..dd", "dd...eCCCCe...dd",
        "kdre........erdk", "kdree......eerdk", ".kdrrrrrrrrrrdk.", "..kkddddddddkk..",
    ], HEART_PAL);
    let heart_f2 = parse_sprite([
        "..kkddddddddkk..", ".kdrrrrrrrrrrdk.", "kdrWeeWWWWeeWrdk", "kdrW...ee...Wrdk",
        "ddW..eCCCCe..Wdd", "dd..eCCWWCCe..dd", "dWeeCCWWWWCCeeWd", "drWeCCWWWWCCeWrd",
        "drWeCCWWWWCCeWrd", "dWeeCCWWWWCCeeWd", "dd..eCCWWCCe..dd", "ddW..eCCCCe..Wdd",
        "kdrW...ee...Wrdk", "kdrWeeWWWWeeWrdk", ".kdrrrrrrrrrrdk.", "..kkddddddddkk..",
    ], HEART_PAL);
    let heart_f3 = parse_sprite([
        "..kkddddddddkk..", ".kdrrrrrrrrrrdk.", "kdrc..eeee..crdk", "kdree......eerdk",
        "dd...eCCCCe...dd", "dd..cCCeeCCc..dd", "dd.cCCewweCCc.dd", "dr.cCCwWWwCCc.rd",
        "dr.cCCwWWwCCc.rd", "dd.cCCewweCCc.dd", "dd..cCCeeCCc..dd", "dd...eCCCCe...dd",
        "kdree......eerdk", "kdrc..eeee..crdk", ".kdrrrrrrrrrrdk.", "..kkddddddddkk..",
    ], HEART_PAL);

    // 47: Ore Deposit
    let ore_deposit = parse_sprite([
        "ddddrdddkkddddrd", "drrddkkddkrrrddd", "dddkkkdOOskkddrd", "ddkkddoOOSSwkddd",
        "dkkdrdoOOSswdkrd", "kkrdddoOOskkdddd", "kdddkkddkddrkddd", "drdkkdddkrkkddrd",
        "dddkddoOskkdrddd", "drkkdoOOSSwkddrd", "dkkdooOOSswddrdd", "kkdddoOskkkddrdd",
        "kddrkddkdrkkdddd", "drddrkkdddkkdrrd", "ddrdddkkddkkdddd", "rddddrddkkdddddr",
    ], ORE_PAL);

    // 48..51: Goblin (F0..F3)
    let goblin_f0 = parse_sprite([
        "..Hh........hH..", "..HHh......hHH..", "...HHh.dd.hHH...", "....HbbddbbH....",
        "....bbbrrbbb....", "...bbbyrrybbb...", "...dbbkkkkbbd...", "....ddbbbbdd....",
        "..rbbBBBBBBbbr..", ".rrdbBBBBBBbdrr.", ".k.ddBBBBBBdd.k.", "....dbbddbbd....",
        "....dbb..bbd....", "...kbb....bbk...", "..kbb......k....", "................",
    ], GOBLIN_PAL);
    let goblin_f1 = parse_sprite([
        "..Hh........hH..", "..HHh......hHH..", "...HHh.dd.hHH...", "....HbbddbbH....",
        "....bbbrrbbb....", "...bbbyrrybbb...", "...dbbkkkkbbd...", "....ddbbbbdd....",
        "..rbbBBBBBBbbr..", ".rrdbBBBBBBbdrr.", ".k.ddBBBBBBdd.k.", "....dbbddbbd....",
        "....dbb..bbd....", "...kbb....bbk...", "...kb......bk...", "................",
    ], GOBLIN_PAL);
    let goblin_f2 = parse_sprite([
        "..Hh........hH..", "..HHh......hHH..", "...HHh.dd.hHH...", "....HbbddbbH....",
        "....bbbrrbbb....", "...bbbyrrybbb...", "...dbbkkkkbbd...", "....ddbbbbdd....",
        "..rbbBBBBBBbbr..", ".rrdbBBBBBBbdrr.", ".k.ddBBBBBBdd.k.", "....dbbddbbd....",
        "....dbb..bbd....", "...kbb....bbk...", "....k......bbk..", "................",
    ], GOBLIN_PAL);
    let goblin_f3 = parse_sprite([
        "..Hh........hH..", "..HHh......hHH..", "...HHh.dd.hHH...", "....HbbddbbH....",
        "....bbbrrbbb....", "...bbbyrrybbb...", "...dbbkkkkbbd...", "....ddbbbbdd....",
        "..rbbBBBBBBbbr..", ".rrdbBBBBBBbdrr.", ".k.ddBBBBBBdd.k.", "....dbbddbbd....",
        "....dbb..bbd....", "...kbb....bbk...", "...kb......bk...", "................",
    ], GOBLIN_PAL);

    // 52..55: Inserter (N, E, S, W)
    let inserter_n = parse_sprite([
        "......C..C......", "......c..c......", ".......cc.......", ".......aa.......",
        ".......aa.......", ".......aa.......", ".......aa.......", "......pppp......",
        ".....pVVVVp.....", "....vVVVVVVv....", "...vvVVVVVVvv...", "...vvvVVVVvvv...",
        "....vvvvvvvv....", "................", "................", "................",
    ], INSERTER_PAL);
    let inserter_e = parse_sprite([
        "................", "....vvvv........", "...vvVVvv.......", "..vvVVVVvv......",
        ".vVVVVVVVVp.....", ".vVVVVpppppaaaaa", ".vVVVVpppppaacCC", ".vVVVVpppppaacCC",
        ".vVVVVpppppaaaaa", ".vVVVVVVVVp.....", "..vvVVVVvv......", "...vvVVvv.......",
        "....vvvv........", "................", "................", "................",
    ], INSERTER_PAL);
    let inserter_s = parse_sprite([
        "................", "................", "................", "....vvvvvvvv....",
        "...vvvVVVVvvv...", "...vvVVVVVVvv...", "....vVVVVVVv....", ".....pVVVVp.....",
        "......pppp......", ".......aa.......", ".......aa.......", ".......aa.......",
        ".......aa.......", ".......cc.......", "......c..c......", "......C..C......",
    ], INSERTER_PAL);
    let inserter_w = parse_sprite([
        "................", "........vvvv....", ".......vvVVvv...", "......vvVVVVvv..",
        ".....pVVVVVVVVv.", "aaaaapppppVVVVv.", "CCcaapppppVVVVv.", "CCcaapppppVVVVv.",
        "aaaaapppppVVVVv.", ".....pVVVVVVVVv.", "......vvVVVVvv..", ".......vvVVvv...",
        "........vvvv....", "................", "................", "................",
    ], INSERTER_PAL);

    let all_sprites: [[u32; 256]; 56] = [
        empty_sprite,   // 0
        belt_n_f0, belt_n_f1, belt_n_f2, belt_n_f3, // 1..4
        belt_e_f0, belt_e_f1, belt_e_f2, belt_e_f3, // 5..8
        belt_s_f0, belt_s_f1, belt_s_f2, belt_s_f3, // 9..12
        belt_w_f0, belt_w_f1, belt_w_f2, belt_w_f3, // 13..16
        drill_n_f0, drill_n_f1, drill_n_f2, drill_n_f3, // 17..20
        drill_e_f0, drill_e_f1, drill_e_f2, drill_e_f3, // 21..24
        drill_s_f0, drill_s_f1, drill_s_f2, drill_s_f3, // 25..28
        drill_w_f0, drill_w_f1, drill_w_f2, drill_w_f3, // 29..32
        smelter_f0, smelter_f1, smelter_f2, smelter_f3, // 33..36
        chest_sprite,  // 37
        wall_sprite,   // 38
        turret_n, turret_e, turret_s, turret_w, // 39..42
        heart_f0, heart_f1, heart_f2, heart_f3, // 43..46
        ore_deposit,   // 47
        goblin_f0, goblin_f1, goblin_f2, goblin_f3, // 48..51
        inserter_n, inserter_e, inserter_s, inserter_w, // 52..55
    ];

    // Serialize pixels into contiguous byte buffer (56 * 256 * 4 = 57,344 bytes)
    let mut pixel_bytes = Vec::with_capacity(56 * 1024);
    for sprite in &all_sprites {
        for &word in sprite {
            pixel_bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    assert_eq!(pixel_bytes.len(), 56 * 1024);

    let checksum = fnv1a_64(&pixel_bytes);

    // Build 64-byte AtlasHeader
    let mut header_bytes = [0u8; 64];
    let magic: u64 = 0x41544C415358414D; // "AXMATLAS"
    let version: u32 = 1;
    let sprite_count: u32 = 56;
    let sprite_size: u32 = 16;
    let bytes_per_sprite: u32 = 1024;

    header_bytes[0..8].copy_from_slice(&magic.to_le_bytes());
    header_bytes[8..12].copy_from_slice(&version.to_le_bytes());
    header_bytes[12..16].copy_from_slice(&sprite_count.to_le_bytes());
    header_bytes[16..20].copy_from_slice(&sprite_size.to_le_bytes());
    header_bytes[20..24].copy_from_slice(&bytes_per_sprite.to_le_bytes());
    header_bytes[24..32].copy_from_slice(&checksum.to_le_bytes());
    // 32..64 remains zero-padded

    // Write full binary slab (64 + 57344 = 57408 bytes)
    let mut file = File::create(&dest_path).expect("Failed to create sprites.atlas");
    file.write_all(&header_bytes).expect("Failed to write atlas header");
    file.write_all(&pixel_bytes).expect("Failed to write atlas pixels");

    println!("cargo:rerun-if-changed=build.rs");
}
