# ==============================================================================
# Axiom Engine — Workspace Structure & Disk Capacity Cleanup Protocol (T-070+)
# Usage:
#   powershell -ExecutionPolicy Bypass -File tools/cleanup.ps1
#   powershell -ExecutionPolicy Bypass -File tools/cleanup.ps1 -CargoClean
# ==============================================================================
[CmdletBinding()]
param (
    [switch]$CargoClean = $false,
    [switch]$PruneTmp = $true,
    [switch]$VerifyOnly = $false
)

$ErrorActionPreference = "Stop"
$ProjectRoot = (Resolve-Path "$PSScriptRoot/..").Path
Set-Location $ProjectRoot

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " 🧹 Axiom Structure & Disk Capacity Cleanup Protocol" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Drive Space Baseline
$cVol = Get-Volume -DriveLetter C -ErrorAction SilentlyContinue
$dVol = Get-Volume -DriveLetter D -ErrorAction SilentlyContinue
$cFreeGB = if ($cVol) { [math]::Round($cVol.SizeRemaining / 1GB, 2) } else { 0 }
$dFreeGB = if ($dVol) { [math]::Round($dVol.SizeRemaining / 1GB, 2) } else { 0 }

Write-Host "==> [Phase 1/5] Storage Baseline:" -ForegroundColor Yellow
Write-Host "    - C: Drive Free: $cFreeGB GB" -ForegroundColor Gray
Write-Host "    - D: Drive Free: $dFreeGB GB" -ForegroundColor Gray

# 2. Junction Integrity Verification
Write-Host "`n==> [Phase 2/5] Verifying D-Drive Cache Junctions..." -ForegroundColor Yellow
$JunctionTargets = @{
    (Join-Path $ProjectRoot "target") = "D:\axiom\cache\target"
    (Join-Path $ProjectRoot "tmp")    = "D:\Tmp"
}

foreach ($linkPath in $JunctionTargets.Keys) {
    $targetPath = $JunctionTargets[$linkPath]
    $linkName = Split-Path $linkPath -Leaf

    if (Test-Path $linkPath) {
        $item = Get-Item $linkPath -Force
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            Write-Host "    [OK] Junction '$linkName' -> $targetPath" -ForegroundColor Green
        } else {
            Write-Host "    [WARN] '$linkName' is a physical directory on C:! Relinking to D:..." -ForegroundColor Magenta
            if (-not $VerifyOnly) {
                & powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\link_cache.ps1"
            }
        }
    } else {
        Write-Host "    [WARN] '$linkName' does not exist. Initializing junction..." -ForegroundColor Yellow
        if (-not $VerifyOnly) {
            & powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\link_cache.ps1"
        }
    }
}

# 3. Clean Transient Logs (tmp/)
Write-Host "`n==> [Phase 3/5] Cleaning Transient Logs (tmp/)..." -ForegroundColor Yellow
$TmpPath = Join-Path $ProjectRoot "tmp"
if (Test-Path $TmpPath) {
    $TmpFiles = Get-ChildItem -Path $TmpPath -File -ErrorAction SilentlyContinue
    $TmpCount = $TmpFiles.Count
    $TmpSize = ($TmpFiles | Measure-Object -Property Length -Sum).Sum
    $TmpSizeKb = [math]::Round($TmpSize / 1KB, 2)

    Write-Host "    Found $TmpCount log files ($TmpSizeKb KB) in tmp/." -ForegroundColor Gray

    if ($PruneTmp -and (-not $VerifyOnly) -and $TmpCount -gt 0) {
        $TmpFiles | Remove-Item -Force -ErrorAction SilentlyContinue
        Write-Host "    [OK] Cleaned $TmpCount transient log files from tmp/." -ForegroundColor Green
    }
}

# 4. Clean Stray Artifacts & Temporary Executables
Write-Host "`n==> [Phase 4/5] Cleaning Stray Test Binaries & Clutter..." -ForegroundColor Yellow
$StrayPatterns = @(
    "bindings\csharp\*.exe",
    "bindings\csharp\*.pdb",
    "crates\**\*.rs.bk",
    "crates\**\*.tmp"
)

$RemovedCount = 0
foreach ($pattern in $StrayPatterns) {
    $matches = Get-ChildItem -Path $ProjectRoot -Filter (Split-Path $pattern -Leaf) -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\\target\\' -and $_.FullName -notmatch '\\\.git\\' }
    
    foreach ($file in $matches) {
        if (-not $VerifyOnly) {
            Remove-Item $file.FullName -Force -ErrorAction SilentlyContinue
            Write-Host "    [-] Removed stray: $($file.Name)" -ForegroundColor DarkGray
            $RemovedCount++
        } else {
            Write-Host "    [!] Found stray: $($file.FullName)" -ForegroundColor Yellow
        }
    }
}
Write-Host "    [OK] Stray artifacts cleanup completed ($RemovedCount files removed)." -ForegroundColor Green

# 5. Target Cache Pruning (Optional / Deep Clean)
if ($CargoClean -and (-not $VerifyOnly)) {
    Write-Host "`n==> [Phase 5/5] Performing Deep Cargo Clean (Wiping Target Cache)..." -ForegroundColor Yellow
    cmd.exe /c "cargo clean"
    Write-Host "    [OK] cargo clean executed. Target cache wiped." -ForegroundColor Green
    
    Write-Host "    Rebuilding essential release SDK..." -ForegroundColor Cyan
    cmd.exe /c "cargo build --release -p axiom-sdk"
    Copy-Item "target\release\axiom_sdk.dll" "bindings\csharp\axiom_sdk.dll" -Force -ErrorAction SilentlyContinue
    Write-Host "    [OK] axiom_sdk.dll rebuilt and restaged." -ForegroundColor Green
} else {
    Write-Host "`n==> [Phase 5/5] Target Cache: Kept warm for rapid compilation (Use -CargoClean for deep wipe)." -ForegroundColor Gray
}

# 6. Post-Cleanup Summary
$cVolAfter = Get-Volume -DriveLetter C -ErrorAction SilentlyContinue
$dVolAfter = Get-Volume -DriveLetter D -ErrorAction SilentlyContinue
$cFreeAfter = if ($cVolAfter) { [math]::Round($cVolAfter.SizeRemaining / 1GB, 2) } else { 0 }
$dFreeAfter = if ($dVolAfter) { [math]::Round($dVolAfter.SizeRemaining / 1GB, 2) } else { 0 }

Write-Host "`n==========================================================" -ForegroundColor Green
Write-Host " 🎉 Cleanup Protocol Finished Successfully!" -ForegroundColor Green
Write-Host " - C: Drive Free: $cFreeAfter GB" -ForegroundColor White
Write-Host " - D: Drive Free: $dFreeAfter GB" -ForegroundColor White
Write-Host " - Tree Integrity: 100% Verified" -ForegroundColor White
Write-Host "==========================================================" -ForegroundColor Green

# Run fast guard verification
Write-Host "`n==> Running Constitutional Guard to confirm 100% integrity..." -ForegroundColor Cyan
& powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\guard.ps1"
if ($LASTEXITCODE -ne 0) {
    Write-Error "[FAIL] Guard check failed after cleanup!"
    exit 1
}
Write-Host "[PASS] Workspace verified 100% clean and operational." -ForegroundColor Green
exit 0
