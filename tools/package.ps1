# Axiom Steam / Standalone Release Packaging Script
# Purpose: Build optimized, LTO-stripped single-binary artifact with asset slab, saves, and guides.
# Usage: powershell -ExecutionPolicy Bypass -File tools/package.ps1 [-Version "1.5.0"] [-Clean]

param (
    [string]$Version = "1.5.0",
    [switch]$Clean
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $ProjectRoot

$TmpDir = Join-Path $ProjectRoot "tmp"
if (-not (Test-Path $TmpDir)) {
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
}

$DistRoot = Join-Path $ProjectRoot "dist"
$BundleName = "axiom-v$Version-windows-x64"
$BundleDir = Join-Path $DistRoot $BundleName
$ZipFile = Join-Path $DistRoot "$BundleName.zip"
$BuildLog = Join-Path $TmpDir "cargo_package.log"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host " 📦 Axiom Steam / Standalone Single-Binary Packager v$Version" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

if ($Clean -and (Test-Path $DistRoot)) {
    Write-Host "--> Cleaning previous dist directory..." -ForegroundColor Yellow
    Remove-Item -Path $DistRoot -Recurse -Force | Out-Null
}

# 1. Compile Release Binary
Write-Host "--> Compiling optimized release binary (LTO + Single Codegen Unit + Stripped)..." -ForegroundColor Cyan
$Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

cmd.exe /c "cargo build --release -p axiom-cli > tmp\cargo_package.log 2>&1"
$BuildExitCode = $LASTEXITCODE

$Stopwatch.Stop()
$BuildElapsedMs = $Stopwatch.ElapsedMilliseconds

if ($BuildExitCode -ne 0) {
    Write-Host "[FAIL] Release compilation failed! (ExitCode: $BuildExitCode)" -ForegroundColor Red
    Write-Host "--- Last 10 lines of cargo_package.log ---" -ForegroundColor Yellow
    Get-Content $BuildLog -Tail 10 | ForEach-Object { Write-Host "  $_" -ForegroundColor Yellow }
    exit $BuildExitCode
}

Write-Host "[OK] Release binary compiled in ${BuildElapsedMs}ms." -ForegroundColor Green

# 2. Source Binary Verification
$SourceExe = Join-Path $ProjectRoot "target\release\axiom-cli.exe"
if (-not (Test-Path $SourceExe)) {
    Write-Host "[FAIL] Output binary not found at $SourceExe!" -ForegroundColor Red
    exit 1
}

# 3. Prepare Staging Bundle Directory
if (Test-Path $BundleDir) {
    Remove-Item -Path $BundleDir -Recurse -Force | Out-Null
}
New-Item -ItemType Directory -Path $BundleDir -Force | Out-Null

$SavesDir = Join-Path $BundleDir "saves"
New-Item -ItemType Directory -Path $SavesDir -Force | Out-Null
New-Item -ItemType File -Path (Join-Path $SavesDir ".gitkeep") -Force | Out-Null

# 4. Copy and Rename Executable
$TargetExe = Join-Path $BundleDir "axiom.exe"
Copy-Item -Path $SourceExe -Destination $TargetExe -Force

$ExeSize = (Get-Item $TargetExe).Length
$ExeSizeKb = [math]::Round($ExeSize / 1KB, 2)
$ExeSizeMb = [math]::Round($ExeSize / 1MB, 2)
Write-Host "[OK] Single executable staged: axiom.exe ($ExeSizeMb MB / $ExeSizeKb KB)" -ForegroundColor Green

# 5. Generate Comprehensive Player Guide (README.txt)
$ReadmePath = Join-Path $BundleDir "README.txt"
$ReadmeContent = @"
================================================================================
          AXIOM ENGINE v$Version - NATIVE FACTORY & BASE DEFENSE STUDIO
================================================================================
Copyright (c) 2026 Hyedelle <hyedelle@dreamhub.engine>. All rights reserved.
Engine Architecture: Pure Integer Determinism (0 Floats) | 64B L1 Cache Aligned

[1. SYSTEM REQUIREMENTS]
- OS: Windows 10 / 11 (64-bit)
- CPU: Any x86_64 CPU (Optimized for L1/L2 data cache alignment)
- Memory: 64 MB RAM
- Graphics: Any graphics card supporting software 2D framebuffer blit
- Storage: Under 15 MB free disk space
- Audio: Windows native audio (Zero external DLLs required)

[2. QUICK START]
1. Run 'axiom.exe'.
2. The game boots instantly in a standalone 704x544 window.
3. Left-click anywhere on the grid to build structures.
4. Right-click or press Delete to demolish structures.

[3. CONTROLS & HOTKEYS]
- Camera / Cursor:
  * Mouse Cursor: Highlights grid cell (32x32 tiles)
  * Left Click:   Place selected structure
  * Right Click:  Demolish structure under cursor
  * R:            Rotate conveyor belt / inserter direction (East -> South -> West -> North)
  * Space / Enter: Place at cursor
  * Delete / Backspace: Clear tile at cursor

- Build Tools:
  * Key 1: Conveyor Belt     (Standard logistics, speed 50)
  * Key 2: Mining Drill      (Extracts ore from resource deposit)
  * Key 3: Smelter Furnace   (Smelts raw ore into metal plates)
  * Key 4: Storage Chest     (Stores up to 100 items)
  * Key 5: Defense Wall      (200 HP fortification barrier)
  * Key 6: Gun Turret        (Fires physical ammo: Iron Ore / Iron Plate / Copper Plate)
  * Key 7: Standard Inserter (Picks up items behind, drops in front)
  * Key 8: Demolish Tool     (Clears structures)
  * Key 9: Fast Belt         (High-speed logistics, speed 100, 2x throughput)
  * Key 0: Filter Inserter   (Smart inserter: only moves configured item type)
  * Key L: Laser Turret      (High-tech beam projector, self-charging energy capacitor)

- System & Display:
  * F3:  Toggle Real-time Profiler & 60-Frame Sparkline (µs sim/render benchmarks)
  * F5:  Quicksave base and defense state to 'saves/quick.axiom'
  * F9:  Quickload base and defense state from 'saves/quick.axiom'
  * F10: Toggle 1x (704x544) / 2x (1408x1088) Nearest-Neighbor Integer Scaling
  * F11: Toggle Fullscreen (Aspect-ratio preserved letterbox)
  * M:   Toggle Sound Mute
  * ESC: Clean shutdown

[4. AUTOMATED PRODUCTION GUIDE]
To build an automated defense and supply line:
1. Place a Mining Drill on an Iron Ore deposit pointing towards a Conveyor Belt.
2. Direct the Conveyor Belt into a Smelter Furnace using an Inserter.
3. Use another Inserter to extract Iron Plates from the Smelter onto a Belt.
4. Route the Iron Plates directly adjacent to a Gun Turret or into a Storage Chest.
5. Deploy Walls and Laser Turrets along the outer perimeter to hold off monster waves!

[5. SAVES & PERSISTENCE]
Saved games are stored in the 'saves/' directory with a 64-byte aligned binary
format ('SAVAXIOM' magic header) featuring zero-loss bit-identical reproducibility.
================================================================================
"@
Set-Content -Path $ReadmePath -Value $ReadmeContent -Encoding UTF8
Write-Host "[OK] Player guide generated: README.txt" -ForegroundColor Green

# 6. Generate Packaging Manifest (manifest.json)
$ManifestPath = Join-Path $BundleDir "manifest.json"
$Sha256Hash = (Get-FileHash -Path $TargetExe -Algorithm SHA256).Hash
$BuildDate = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz")

$ManifestObj = [PSCustomObject]@{
    product = "Axiom Engine - Native Studio & Factory Defense"
    version = $Version
    target_platform = "windows-x64"
    executable = "axiom.exe"
    executable_size_bytes = $ExeSize
    executable_sha256 = $Sha256Hash
    build_timestamp = $BuildDate
    architecture = [PSCustomObject]@{
        determinism = "pure_integer_fixed_point_0_floats"
        l1_cache_alignment = "64_bytes"
        render_pipeline = "compile_time_baked_binary_atlas_88_slabs"
        audio = "windows_native_pcm_synthesis_22050hz_16bit"
    }
}
$ManifestJson = $ManifestObj | ConvertTo-Json -Depth 5
Set-Content -Path $ManifestPath -Value $ManifestJson -Encoding UTF8
Write-Host "[OK] Distribution manifest generated: manifest.json" -ForegroundColor Green

# 7. Create Distribution ZIP Archive
if (Test-Path $ZipFile) {
    Remove-Item -Path $ZipFile -Force | Out-Null
}
Write-Host "--> Compressing distribution archive ($BundleName.zip)..." -ForegroundColor Cyan
Compress-Archive -Path (Join-Path $BundleDir "*") -DestinationPath $ZipFile -CompressionLevel Optimal

$ZipSize = (Get-Item $ZipFile).Length
$ZipSizeKb = [math]::Round($ZipSize / 1KB, 2)
$ZipSizeMb = [math]::Round($ZipSize / 1MB, 2)
$ZipHash = (Get-FileHash -Path $ZipFile -Algorithm SHA256).Hash

Write-Host "============================================================" -ForegroundColor Green
Write-Host " 🎉 Packaging Completed Successfully!" -ForegroundColor Green
Write-Host " - Target Directory: $BundleDir" -ForegroundColor White
Write-Host " - Standalone Exe:   $TargetExe ($ExeSizeMb MB / $ExeSizeKb KB)" -ForegroundColor White
Write-Host " - Exe SHA-256:      $Sha256Hash" -ForegroundColor Gray
Write-Host " - Zip Archive:      $ZipFile ($ZipSizeMb MB / $ZipSizeKb KB)" -ForegroundColor White
Write-Host " - Zip SHA-256:      $ZipHash" -ForegroundColor Gray
Write-Host "============================================================" -ForegroundColor Green

exit 0
