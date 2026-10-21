# Axiom Guard - Mechanical Constitution Enforcer
# Usage: powershell -ExecutionPolicy Bypass -File tools/guard.ps1

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $ScriptDir
Set-Location $Root

$TmpDir = Join-Path $Root "tmp"
if (-not (Test-Path $TmpDir)) { New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null }
$VerdictFile = Join-Path $TmpDir "verdict.txt"
$LogFile = Join-Path $TmpDir "guard.log"

Write-Host "[Axiom Guard] Running mechanical constitutional checks..." -ForegroundColor Cyan

# Check 1: Determinism check (No f32 / f64 in crates/axiom-core/src)
$CoreSrc = Join-Path $Root "crates\axiom-core\src"
if (Test-Path $CoreSrc) {
    $floatMatches = Get-ChildItem -Path $CoreSrc -Filter "*.rs" -Recurse | Select-String -Pattern "\b(f32|f64)\b"
    if ($floatMatches) {
        Write-Host "[VIOLATION] Floating-point math (f32/f64) detected in axiom-core!" -ForegroundColor Red
        $floatMatches | ForEach-Object { Write-Host "   $($_.Path):$($_.LineNumber) -> $($_.Line.Trim())" -ForegroundColor Yellow }
        "GUARD_FAIL: Float in axiom-core" | Out-File $VerdictFile -Encoding utf8
        exit 1
    }
}

# Check 2: Secret leak protection
$secretMatches = Get-ChildItem -Path $Root -Include "*.rs", "*.toml" -Recurse | 
    Where-Object { $_.FullName -notmatch "\\target\\" -and $_.FullName -notmatch "\\tmp\\" } |
    Select-String -Pattern "(\.secrets|api[_-]?key\s*[:=]\s*['`"][^'`"]+)"
if ($secretMatches) {
    Write-Host "[VIOLATION] Secret or .secrets reference detected in code!" -ForegroundColor Red
    $secretMatches | ForEach-Object { Write-Host "   $($_.Path):$($_.LineNumber)" -ForegroundColor Yellow }
    "GUARD_FAIL: Secrets detected" | Out-File $VerdictFile -Encoding utf8
    exit 1
}

# Check 3: Rust test verification via cmd to avoid PowerShell stderr trap
Write-Host "[Axiom Guard] Executing cargo test --workspace..." -ForegroundColor Cyan
cmd.exe /c "cargo test --workspace > tmp\guard.log 2>&1"
$TestCode = $LASTEXITCODE

if ($TestCode -ne 0) {
    Write-Host "[FAIL] cargo test failed!" -ForegroundColor Red
    Get-Content $LogFile -Tail 15 | ForEach-Object { Write-Host "   $_" -ForegroundColor Yellow }
    "GUARD_FAIL: Tests failed (Exit $TestCode)" | Out-File $VerdictFile -Encoding utf8
    exit $TestCode
}

# All Passed
Write-Host "[PASS] All constitutional checks cleared (Determinism / Security / Tests 100% OK)" -ForegroundColor Green
"GUARD_PASS" | Out-File $VerdictFile -Encoding utf8
exit 0
