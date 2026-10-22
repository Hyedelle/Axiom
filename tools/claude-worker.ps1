# ==============================================================================
# Axiom Engine — Headless Claude Code Worker Bridge (tools/claude-worker.ps1)
# ==============================================================================
[CmdletBinding()]
param(
    [string]$TaskPrompt = "",
    [switch]$SkipGuard = $false
)

$ErrorActionPreference = "Continue"
$ProjectRoot = (Resolve-Path "$PSScriptRoot/..").Path
Set-Location $ProjectRoot

# 1. Enforce Claude.ai Max Subscription (Clear any API key to prevent pay-as-you-go billing)
[System.Environment]::SetEnvironmentVariable("ANTHROPIC_API_KEY", $null, "Process")
[System.Environment]::SetEnvironmentVariable("CLAUDE_CODE_USE_API_KEY", $null, "Process")

# Verify Claude.ai OAuth credentials exist
$CredsPath = Join-Path $env:USERPROFILE ".claude\.credentials.json"
if (-not (Test-Path $CredsPath)) {
    Write-Warning "[WARN] ~/.claude/.credentials.json not found. Run 'claude' and log in with your Claude.ai account first."
}

# 2. Extract Task from docs/tasks.md if not explicitly specified
if ([string]::IsNullOrWhiteSpace($TaskPrompt)) {
    $TasksFile = Join-Path $ProjectRoot "docs\tasks.md"
    if (Test-Path $TasksFile) {
        $Lines = Get-Content $TasksFile
        $CurrentSection = $false
        foreach ($Line in $Lines) {
            if ($Line -match 'CURRENT') {
                $CurrentSection = $true
                continue
            }
            if ($CurrentSection -and $Line -match '^## ') {
                break
            }
            if ($CurrentSection -and $Line -match '-\s*\[\s*\]\s*(.*)') {
                $TaskPrompt = $matches[1].Replace('**', '').Trim()
                break
            }
        }
    }
}

if ([string]::IsNullOrWhiteSpace($TaskPrompt)) {
    Write-Host "[WARN] No active task found in docs/tasks.md or -TaskPrompt argument." -ForegroundColor Yellow
    exit 0
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "⚡ Axiom Headless Claude Code Construction Runner" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "Task: $TaskPrompt" -ForegroundColor Green

# 3. Construct System-Guided Engineering Prompt
$CleanTask = $TaskPrompt -replace '"', '\"'
$SinglePrompt = "You are Claude Code site engineer for Axiom. Follow CLAUDE.md and AGENTS.md strictly (No float in core, 64B L1 cache align, pure determinism, DO NOT touch files outside c:\Dreamhub\Engine\Axiom). Read docs/tasks.md and docs/audit-report.md for technical requirements. Implement task: $CleanTask. Verify with tools/guard.ps1."

# 4. Invoke Claude Code non-interactively
$TmpLog = Join-Path $ProjectRoot "tmp\claude_worker.log"
$TmpErr = Join-Path $ProjectRoot "tmp\claude_worker_err.log"
Write-Host "==> [1/2] Invoking Claude Code non-interactively..." -ForegroundColor Yellow

$ClaudeCmd = (Get-Command claude.cmd -ErrorAction SilentlyContinue).Source
if (-not $ClaudeCmd) {
    $ClaudeCmd = "claude.cmd"
}

& $ClaudeCmd -p $SinglePrompt --dangerously-skip-permissions > $TmpLog 2> $TmpErr
$ExitCode = $LASTEXITCODE
Write-Host "    [Claude Finished] Exit Code: $ExitCode" -ForegroundColor $(if ($ExitCode -eq 0) { "Green" } else { "Red" })

if (Test-Path $TmpLog) {
    Get-Content $TmpLog -Tail 15 | ForEach-Object { Write-Host "    $_" -ForegroundColor Gray }
}

# 5. Run Mechanical Constitutional Guard Verification
if (-not $SkipGuard) {
    Write-Host "==> [2/2] Running Constitutional Guard..." -ForegroundColor Yellow
    & powershell -ExecutionPolicy Bypass -File "$ProjectRoot\tools\guard.ps1"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "[FAIL] Constitutional Guard failed after Claude construction."
        exit 1
    }
    Write-Host "[PASS] Claude construction cleared all constitutional gates!" -ForegroundColor Green
}

Write-Host "==========================================================" -ForegroundColor Green
Write-Host "🎉 [SUCCESS] Claude Code Worker Completed Task Successfully" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
