# Axiom Isolated External Test Runner
# Purpose: Run cargo tests, redirect logs to tmp/, output only exit code and last 5 lines on error.

param (
    [switch]$VerboseOutput
)

$ErrorActionPreference = "Continue"

# Project root
$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $ProjectRoot

# Ensure tmp/ directory exists
$TmpDir = Join-Path $ProjectRoot "tmp"
if (-not (Test-Path $TmpDir)) {
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
}

$LogFile = Join-Path $TmpDir "cargo_test.log"
$VerdictFile = Join-Path $TmpDir "verdict.txt"

# Measure execution time
$Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

# Run cargo test and redirect all output to log file (skip doc-tests for rapid turnaround)
& cargo test --workspace --bins --lib --tests > $LogFile 2>&1
$TestExitCode = $LASTEXITCODE

$Stopwatch.Stop()
$ElapsedMs = $Stopwatch.ElapsedMilliseconds

if ($TestExitCode -eq 0) {
    Set-Content -Path $VerdictFile -Value "PASS" -Encoding UTF8
    Write-Host "[PASS] Axiom tests passed in ${ElapsedMs}ms. (Verdict: PASS, ExitCode: 0)" -ForegroundColor Green
    if ($VerboseOutput) {
        Get-Content $LogFile | Select-Object -Last 10
    }
    exit 0
} else {
    Set-Content -Path $VerdictFile -Value "FAIL" -Encoding UTF8
    Write-Host "[FAIL] Axiom test failed in ${ElapsedMs}ms. (Verdict: FAIL, ExitCode: $TestExitCode)" -ForegroundColor Red
    Write-Host "--- Last 5 lines of error log (Full log: tmp/cargo_test.log) ---" -ForegroundColor Yellow
    Get-Content $LogFile | Select-Object -Last 5
    exit $TestExitCode
}
