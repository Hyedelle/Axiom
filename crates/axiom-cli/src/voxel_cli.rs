//! Axiom CLI 3D Voxel Terrain Studio & Custom Destruction Map Exporter.
//!
//! Provides CLI subcommands to inspect, generate, and export `.axvm` maps.

use std::fs;
use std::path::Path;
use axiom_core::voxel_destruct::VOXEL_DIM;
use axiom_render::voxel_map::{
    deserialize_axvm, generate_voxel_preset, serialize_axvm, VoxelPreset,
};

/// Executes the `axiom voxel` CLI command family.
pub fn run_voxel_suite(args: &[String]) {
    if args.len() < 3 || args[2] == "help" || args[2] == "--help" {
        print_voxel_help();
        return;
    }

    match args[2].as_str() {
        "list" => {
            println!("🗺️ [Axiom Voxel] Built-in Procedural Terrain Presets:");
            println!("  1. flat      - {}", VoxelPreset::FlatArena.name());
            println!("  2. crater    - {}", VoxelPreset::CrateredArena.name());
            println!("  3. bunker    - {}", VoxelPreset::FortifiedBunker.name());
            println!("  4. ruins     - {}", VoxelPreset::MonolithRuins.name());
            println!("  5. volcanic  - {}", VoxelPreset::VolcanicVent.name());
        }
        "export" => {
            let mut preset = VoxelPreset::CrateredArena;
            let mut out_path = "maps/crater.axvm".to_string();
            let mut seed = 42u64;

            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--preset" if i + 1 < args.len() => {
                        preset = match args[i + 1].to_lowercase().as_str() {
                            "flat" => VoxelPreset::FlatArena,
                            "crater" => VoxelPreset::CrateredArena,
                            "bunker" => VoxelPreset::FortifiedBunker,
                            "ruins" => VoxelPreset::MonolithRuins,
                            "volcanic" => VoxelPreset::VolcanicVent,
                            _ => {
                                eprintln!("Unknown preset '{}', defaulting to crater", args[i + 1]);
                                VoxelPreset::CrateredArena
                            }
                        };
                        i += 2;
                    }
                    "--out" if i + 1 < args.len() => {
                        out_path = args[i + 1].clone();
                        i += 2;
                    }
                    "--seed" if i + 1 < args.len() => {
                        seed = args[i + 1].parse().unwrap_or(42);
                        i += 2;
                    }
                    _ => i += 1,
                }
            }

            println!(
                "🔨 Generating procedural voxel map preset [{}] with seed {}...",
                preset.name(),
                seed
            );
            let volume = generate_voxel_preset(preset, seed);
            let bytes = serialize_axvm(preset.name(), preset as u16, &volume, 1729700000);

            if let Some(parent) = Path::new(&out_path).parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = fs::create_dir_all(parent);
                }
            }

            match fs::write(&out_path, &bytes) {
                Ok(_) => {
                    println!(
                        "✅ Successfully exported {} bytes to '{}' (Solid: {}, Damaged: {})",
                        bytes.len(),
                        out_path,
                        volume.total_solid,
                        volume.damaged_count
                    );
                }
                Err(e) => {
                    eprintln!("❌ Failed to write '{}': {}", out_path, e);
                    std::process::exit(1);
                }
            }
        }
        "inspect" => {
            if args.len() < 4 {
                eprintln!("Usage: axiom voxel inspect <path/to/map.axvm>");
                std::process::exit(1);
            }
            let map_path = &args[3];
            let bytes = match fs::read(map_path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("❌ Could not read file '{}': {}", map_path, e);
                    std::process::exit(1);
                }
            };

            match deserialize_axvm(&bytes) {
                Ok((header, volume)) => {
                    let name = String::from_utf8_lossy(&header.name)
                        .trim_matches('\0')
                        .to_string();
                    println!("📦 [Axiom Voxel Map Inspection]");
                    println!("  File:            {}", map_path);
                    println!("  Map Name:        {}", name);
                    println!("  Container Magic: AXVM (v{})", header.version);
                    println!(
                        "  Dimensions:      {}x{}x{} ({} voxels)",
                        header.dim_x,
                        header.dim_y,
                        header.dim_z,
                        header.dim_x as usize * header.dim_y as usize * header.dim_z as usize
                    );
                    println!("  Solid Voxels:    {}", header.solid_voxels);
                    println!("  Damaged Count:   {}", header.damaged_count);
                    println!("  Payload FNV-1a:  0x{:016X}", header.checksum);
                    println!("  Integrity:       100% Valid");

                    // Mini ASCII Layer Slice at y = 7 (mid-height)
                    println!("\n  Cross-Section Layer (Y = 7):");
                    for z in 0..VOXEL_DIM {
                        print!("  [z={:02}] ", z);
                        for x in 0..VOXEL_DIM {
                            let mat = volume.get(x, 7, z);
                            let ch = match mat {
                                0 => '.', // Air
                                1 => '#', // Stone
                                2 => '=', // Metal
                                3 => '*', // Molten
                                4 => '@', // Armor
                                _ => '?',
                            };
                            print!("{} ", ch);
                        }
                        println!();
                    }
                }
                Err(err) => {
                    eprintln!("❌ Corrupted or invalid .axvm container: {:?}", err);
                    std::process::exit(1);
                }
            }
        }
        cmd => {
            eprintln!("Unknown voxel command '{}'. Run 'axiom voxel help'.", cmd);
        }
    }
}

fn print_voxel_help() {
    println!("Axiom 3D Voxel Terrain Studio (.axvm)");
    println!("Usage:");
    println!("  axiom voxel list");
    println!("  axiom voxel export [--preset <flat|crater|bunker|ruins|volcanic>] [--out <file.axvm>] [--seed <num>]");
    println!("  axiom voxel inspect <file.axvm>");
}
