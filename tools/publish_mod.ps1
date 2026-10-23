# Axiom Workshop UGC Publisher Automation Tool
# Usage: powershell -ExecutionPolicy Bypass -File tools/publish_mod.ps1 [-ModPath <path>] [-AppId <id>] [-Changelog <msg>] [-PackOnly]

param(
    [string]$ModPath = "mods/custom_hazard_arm",
    [int]$AppId = 3245670,
    [string]$Changelog = "Automated publishing build via Axiom Engine",
    [switch]$PackOnly
)

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $ScriptDir
Set-Location $Root

Write-Host "═════════════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host " 🚀 Axiom Workshop UGC Publisher Pipeline" -ForegroundColor Cyan
Write-Host "    Target Mod:  $ModPath" -ForegroundColor Gray
Write-Host "    Steam AppId: $AppId" -ForegroundColor Gray
Write-Host "═════════════════════════════════════════════════════════════════════" -ForegroundColor Cyan

# Step 1: Validate Mod Integrity
Write-Host "`n[Step 1/3] Validating Mod Package..." -ForegroundColor Yellow
$validateOut = cmd.exe /c "cargo run -q -p axiom-cli --bin axiom-cli -- mod validate $ModPath 2>&1"
$validateCode = $LASTEXITCODE

$validateOut | ForEach-Object { Write-Host "   $_" }

if ($validateCode -ne 0 -or ($validateOut -match "Validation FAILED")) {
    Write-Host "`n❌ Validation failed. Please correct manifest and asset errors before publishing." -ForegroundColor Red
    exit 1
}

# Step 2: Pack Standalone .axmod Container
Write-Host "`n[Step 2/3] Packing Standalone .axmod Container..." -ForegroundColor Yellow
$packOut = cmd.exe /c "cargo run -q -p axiom-cli --bin axiom-cli -- mod pack $ModPath 2>&1"
$packCode = $LASTEXITCODE

$packOut | ForEach-Object { Write-Host "   $_" }

if ($packCode -ne 0) {
    Write-Host "`n❌ Packaging failed." -ForegroundColor Red
    exit $packCode
}

if ($PackOnly) {
    Write-Host "`n✨ Pack-only mode complete. Container ready." -ForegroundColor Green
    exit 0
}

# Step 3: Generate Steam VDF and Dry-Run Publication
Write-Host "`n[Step 3/3] Generating Steam Workshop VDF & Staging..." -ForegroundColor Yellow
$publishOut = cmd.exe /c "cargo run -q -p axiom-cli --bin axiom-cli -- mod publish $ModPath --appid $AppId --changelog `"$Changelog`" 2>&1"
$publishCode = $LASTEXITCODE

$publishOut | ForEach-Object { Write-Host "   $_" }

if ($publishCode -eq 0) {
    Write-Host "`n🎉 Steam Workshop publication package successfully staged!" -ForegroundColor Green
    Write-Host "   To perform actual live upload to Steam Cloud, execute the steamcmd command printed above." -ForegroundColor Cyan
} else {
    Write-Host "`n❌ Publication staging failed." -ForegroundColor Red
    exit $publishCode
}
