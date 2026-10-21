# Axiom Performance & Benchmark Runner
# Purpose: Measure tick rate, entity throughput, and time-travel rollback speed in Release mode.

param (
    [switch]$VerboseOutput
)

$ErrorActionPreference = "Continue"

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $ProjectRoot

$TmpDir = Join-Path $ProjectRoot "tmp"
if (-not (Test-Path $TmpDir)) {
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
}

$LogFile = Join-Path $TmpDir "cargo_bench.log"
$VerdictFile = Join-Path $TmpDir "verdict.txt"

Write-Host "==> Running Axiom Performance Suite (Release Mode)..." -ForegroundColor Cyan

$Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

# Run tests in release mode to measure true throughput
& cargo test --release --workspace -- --nocapture > $LogFile 2>&1
$BenchExitCode = $LASTEXITCODE

$Stopwatch.Stop()
$ElapsedMs = $Stopwatch.ElapsedMilliseconds

if ($BenchExitCode -eq 0) {
    Set-Content -Path $VerdictFile -Value "BENCH_PASS" -Encoding UTF8
    Write-Host "[BENCH PASS] Axiom release suite passed in ${ElapsedMs}ms. (ExitCode: 0)" -ForegroundColor Green
    
    # Extract test result lines
    Get-Content $LogFile | Select-String "test result:" | ForEach-Object {
        Write-Host "  $_" -ForegroundColor Gray
    }
    
    if ($VerboseOutput) {
        Get-Content $LogFile | Select-Object -Last 15
    }
    exit 0
} else {
    Set-Content -Path $VerdictFile -Value "BENCH_FAIL" -Encoding UTF8
    Write-Host "[BENCH FAIL] Axiom release suite failed in ${ElapsedMs}ms. (ExitCode: $BenchExitCode)" -ForegroundColor Red
    Write-Host "--- Last 5 lines of error log (Full log: tmp/cargo_bench.log) ---" -ForegroundColor Yellow
    Get-Content $LogFile | Select-Object -Last 5
    exit $BenchExitCode
}
