//! Axiom CLI 3D Voxel Terrain Studio & Custom Destruction Map Exporter.
//!
//! Provides CLI subcommands to inspect, generate, and export `.axvm` maps.

use std::fs;
use std::path::Path;
use axiom_core::Fixed;
use axiom_core::voxel_collapse::DynamicCollapseSystem;
use axiom_core::voxel_destruct::VOXEL_DIM;
use axiom_core::voxel_shard::VoxelShardPool;
use axiom_render::framebuffer::FrameBuffer;
use axiom_render::math3d::Mat4Fixed;
use axiom_render::voxel_collapse_render::render_dynamic_collapse_chunks;
use axiom_render::voxel_shard_render::render_voxel_shards;
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
        "bench" | "collapse-bench" => {
            let mut preset = VoxelPreset::MonolithRuins;
            let mut iterations = 50usize;

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
                            _ => VoxelPreset::MonolithRuins,
                        };
                        i += 2;
                    }
                    "--ticks" | "--iterations" if i + 1 < args.len() => {
                        iterations = args[i + 1].parse().unwrap_or(50);
                        i += 2;
                    }
                    _ => i += 1,
                }
            }

            println!("⚡ [Axiom Voxel] Running Dynamic Collapse & Shard Ballistic Benchmark...");
            println!("  Preset:      {} (Seed: 42)", preset.name());
            println!("  Iterations:  {} steps", iterations);

            let mut volume = generate_voxel_preset(preset, 42);
            let mut collapse = DynamicCollapseSystem::<16>::new(42);
            let mut shards = VoxelShardPool::<128>::new(42);
            let mut fb = FrameBuffer::new(320, 240);
            let mut depth_buffer = vec![i32::MIN; 320 * 240];
            let view_mat = Mat4Fixed::IDENTITY;
            let cam_dist = Fixed::from_raw(65536);
            let focal = 65536;

            let start = std::time::Instant::now();
            let mut total_collapsed = 0usize;
            let mut total_shattered = 0u32;
            let mut rendered_chunks = 0usize;
            let mut rendered_shards = 0usize;

            for step_idx in 0..iterations {
                if step_idx % 5 == 0 {
                    let cx = ((step_idx * 3) % 10 + 3) as i16;
                    let cz = ((step_idx * 7) % 10 + 3) as i16;
                    volume.carve_crater_with_shards(cx, cz, 2, 3, &mut shards, [0, -45000, 0], 2048);
                    total_collapsed += collapse.detect_and_collapse(&mut volume, 0, [0, -45000, 0], 2048);
                }
                let (_b, sh) = collapse.step(Some(&mut shards));
                total_shattered += sh;
                shards.step();

                depth_buffer.fill(i32::MIN);
                rendered_chunks += render_dynamic_collapse_chunks(&mut fb, &mut depth_buffer, &collapse, &view_mat, cam_dist, focal);
                rendered_shards += render_voxel_shards(&mut fb, &mut depth_buffer, &shards, &view_mat, cam_dist, focal);
            }

            let elapsed = start.elapsed();
            let us_per_tick = elapsed.as_micros() as f64 / iterations as f64;
            let tps = (iterations as f64 / elapsed.as_secs_f64()) as u64;

            println!("🏁 Benchmark Completed:");
            println!("  Total Time:          {:?}", elapsed);
            println!("  Throughput:          {} ticks/sec ({:.2} µs/step)", tps, us_per_tick);
            println!("  Chunks Collapsed:    {}", total_collapsed);
            println!("  Chunks Shattered:    {}", total_shattered);
            println!("  Active Shards:       {}", shards.active_count);
            println!("  Rendered Chunks:     {}", rendered_chunks);
            println!("  Rendered Shards:     {}", rendered_shards);
            println!("  Consensus Hash:      0x{:016X}", collapse.compute_consensus_hash() ^ shards.compute_consensus_hash());
        }
        "bench-extreme" => {
            let mut steps = 10_000usize;
            let mut seed = 987654321u64;

            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--steps" | "--ticks" if i + 1 < args.len() => {
                        steps = args[i + 1].parse().unwrap_or(10_000);
                        i += 2;
                    }
                    "--seed" if i + 1 < args.len() => {
                        seed = args[i + 1].parse().unwrap_or(987654321);
                        i += 2;
                    }
                    _ => i += 1,
                }
            }

            println!("🔥 [Axiom Voxel] Running Extreme Multi-Material Thermal-Collapse Benchmark...");
            println!("  Steps:       {} integration ticks", steps);
            println!("  Seed:        {}", seed);

            let mut field = axiom_core::thermal_stress::ThermalStressField16::new();
            let mut volume = axiom_core::voxel_destruct::VoxelVolume16::new_solid(axiom_core::voxel_destruct::VOXEL_STONE);
            let mut collapse = DynamicCollapseSystem::<16>::new(seed);
            let mut shards = VoxelShardPool::<128>::new(seed);

            let start = std::time::Instant::now();
            let mut total_phase_transitions = 0u32;
            let mut total_shattered = 0u32;

            for step in 0..steps {
                if step % 50 == 0 {
                    let cx = (step * 7) % VOXEL_DIM;
                    let cz = (step * 11) % VOXEL_DIM;
                    let peak_t = 800 + ((step % 200) as u16);
                    let peak_s = 750 + ((step % 250) as u16);
                    field.inject_thermal_pulse(cx, 4, cz, 2, peak_t, peak_s);
                }

                let res = field.step_thermal_and_collapse_advanced(
                    &mut volume,
                    &mut collapse,
                    15,
                    1,
                    [-16384, 0, -16384],
                    2048,
                );
                total_phase_transitions += res.phase_transitions;

                let (_c, sh) = collapse.step(Some(&mut shards));
                total_shattered += sh;
                shards.step();
            }

            let elapsed = start.elapsed();
            let us_per_tick = elapsed.as_micros() as f64 / steps as f64;
            let tps = (steps as f64 / elapsed.as_secs_f64()) as u64;

            let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
            hash ^= field.compute_consensus_hash();
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= collapse.compute_consensus_hash();
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= shards.compute_consensus_hash();
            hash = hash.wrapping_mul(0x100_0000_01B3);

            println!("🏁 Extreme Physics Benchmark Completed:");
            println!("  Total Time:          {:?}", elapsed);
            println!("  Throughput:          {} TPS ({:.2} µs/step)", tps, us_per_tick);
            println!("  Phase Transitions:   {}", total_phase_transitions);
            println!("  Chunks Collapsed:    {}", collapse.total_collapsed);
            println!("  Chunks Shattered:    {}", total_shattered);
            println!("  Active Shards:       {}", shards.active_count);
            println!("  Consensus Hash:      0x{:016X}", hash);
        }
        "bench-fluid" => {
            let mut steps = 5000usize;
            let mut seed = 0x160_FEED_CAFEu64;
            let mut use_simd = false;

            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--steps" if i + 1 < args.len() => {
                        steps = args[i + 1].parse().unwrap_or(5000);
                        i += 2;
                    }
                    "--seed" if i + 1 < args.len() => {
                        seed = args[i + 1].parse().unwrap_or(0x160_FEED_CAFE);
                        i += 2;
                    }
                    "--simd" => {
                        use_simd = true;
                        i += 1;
                    }
                    _ => i += 1,
                }
            }

            println!("💧 [Axiom Voxel] Running 3D Fluid Dynamics & Pore Stress Relaxation Benchmark...");
            println!("  Engine Mode: {}", if use_simd { "SIMD / 3D Red-Black Checkerboard" } else { "Sequential Iterative" });
            println!("  Steps:       {} integration ticks", steps);
            println!("  Seed:        0x{:016X}", seed);

            let mut simd = axiom_core::voxel_fluid_simd::SimdFluidEngine::new();
            let mut fluid = axiom_core::voxel_fluid::VoxelFluidField16::new();
            let mut thermal = axiom_core::thermal_stress::ThermalStressField16::new();
            let mut volume = axiom_core::voxel_destruct::VoxelVolume16::new_solid(axiom_core::voxel_destruct::VOXEL_STONE);
            let mut collapse = DynamicCollapseSystem::<16>::new(seed);
            let mut shards = VoxelShardPool::<128>::new(seed);

            let start = std::time::Instant::now();
            let mut total_quenched = 0u32;
            let mut total_relaxed = 0u32;
            let mut total_phase_transitions = 0u32;

            for step in 0..steps {
                if step % 50 == 0 {
                    let cx = (step * 7) % VOXEL_DIM;
                    let cz = (step * 11) % VOXEL_DIM;
                    let peak_t = 800 + ((step % 200) as u16);
                    let peak_s = 700 + ((step % 250) as u16);
                    thermal.inject_thermal_pulse(cx, 2, cz, 2, peak_t, peak_s);
                }

                if step % 25 == 0 {
                    let fx = (step * 7) % VOXEL_DIM;
                    let fz = (step * 11) % VOXEL_DIM;
                    fluid.inject_fluid(&mut volume, fx, 2, fz, 220);
                }

                let (q_temp, r_stress) = fluid.relax_thermal_and_stress(&mut thermal);
                total_quenched += q_temp;
                total_relaxed += r_stress;

                if use_simd {
                    let _ = simd.step_fluid_simd(&mut fluid, &mut volume, Some(&mut thermal), 25);
                } else {
                    let _ = fluid.step_fluid(&mut volume, Some(&mut thermal), 25);
                }

                let thermal_res = thermal.step_thermal_and_collapse_advanced(
                    &mut volume,
                    &mut collapse,
                    15,
                    1,
                    [-16384, 0, -16384],
                    2048,
                );
                total_phase_transitions += thermal_res.phase_transitions;

                let (_c, _sh) = collapse.step(Some(&mut shards));
                shards.step();
            }

            let elapsed = start.elapsed();
            let us_per_tick = elapsed.as_micros() as f64 / steps as f64;
            let tps = (steps as f64 / elapsed.as_secs_f64()) as u64;

            let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
            if use_simd {
                hash ^= simd.compute_consensus_hash();
                hash = hash.wrapping_mul(0x100_0000_01B3);
            }
            hash ^= fluid.compute_consensus_hash();
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= thermal.compute_consensus_hash();
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= collapse.compute_consensus_hash();
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= shards.compute_consensus_hash();
            hash = hash.wrapping_mul(0x100_0000_01B3);

            println!("🏁 Fluid Physics Benchmark Completed:");
            println!("  Total Time:          {:?}", elapsed);
            println!("  Throughput:          {} TPS ({:.2} µs/step)", tps, us_per_tick);
            if use_simd {
                println!("  Sleep Slices Saved:  {} slice cycles", simd.sleep_cycles_saved);
            }
            println!("  Total Infiltrated:   {} fluid units", fluid.total_infiltrated);
            println!("  Total Evaporated:    {} fluid units", fluid.total_evaporated);
            println!("  Heat Quenched:       {} temp units", total_quenched);
            println!("  Stress Relaxed:      {} stress units", total_relaxed);
            println!("  Phase Transitions:   {}", total_phase_transitions);
            println!("  Consensus Hash:      0x{:016X}", hash);
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
    println!("  axiom voxel bench [--preset <preset>] [--ticks <n>]");
    println!("  axiom voxel bench-extreme [--steps <10000>] [--seed <num>]");
    println!("  axiom voxel bench-fluid [--steps <5000>] [--seed <num>] [--simd]");
}
