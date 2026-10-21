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

$SecretKeyPath = "C:\Dreamhub\.secrets\anthropic_api_key.txt"
if (-not (Test-Path $SecretKeyPath)) {
    $SecretKeyPath = "C:\Dreamhub\Project\.secrets\anthropic_api_key.txt"
}
if (-not (Test-Path $SecretKeyPath)) {
    Write-Error "[FAIL] Anthropic API key not found in C:\Dreamhub\.secrets or C:\Dreamhub\Project\.secrets"
    exit 1
}

# 1. Setup Process Environment for Headless Claude Code
$ApiKey = (Get-Content $SecretKeyPath -Raw).Trim()
[System.Environment]::SetEnvironmentVariable("ANTHROPIC_API_KEY", $ApiKey, "Process")
[System.Environment]::SetEnvironmentVariable("CLAUDE_CODE_USE_API_KEY", "1", "Process")

# 2. Extract Task from docs/tasks.md if not explicitly specified
if ([string]::IsNullOrWhiteSpace($TaskPrompt)) {
    $TasksFile = Join-Path $ProjectRoot "docs\tasks.md"
    if (Test-Path $TasksFile) {
        $Lines = Get-Content $TasksFile
        $CurrentSection = $false
        foreach ($Line in $Lines) {
            if ($Line -match "## ▶ \[CURRENT\]") {
                $CurrentSection = $true
                continue
            }
            if ($CurrentSection -and $Line -match "## ") {
                break
            }
            if ($CurrentSection -and $Line -match "^\s*-\s*\[\s*\]\s*\*\*(.*?)\*\*") {
                $TaskPrompt = $matches[1]
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
$SinglePrompt = "You are Claude Code site engineer for Axiom. Follow CLAUDE.md strictly (No float, 64B align, pure determinism). Task: $CleanTask. Verify with tools/guard.ps1."

# 4. Invoke Claude Code non-interactively
$TmpLog = Join-Path $ProjectRoot "tmp\claude_worker.log"
$TmpErr = Join-Path $ProjectRoot "tmp\claude_worker_err.log"
Write-Host "==> [1/2] Invoking Claude Code non-interactively..." -ForegroundColor Yellow

$ClaudeCmd = (Get-Command claude.cmd -ErrorAction SilentlyContinue).Source
if (-not $ClaudeCmd) {
    $ClaudeCmd = "claude"
}

$Proc = Start-Process -FilePath $ClaudeCmd -ArgumentList @("-p", $SinglePrompt, "--dangerously-skip-permissions") -NoNewWindow -PassThru -RedirectStandardOutput $TmpLog -RedirectStandardError $TmpErr
$Proc.WaitForExit()

$ExitCode = $Proc.ExitCode
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
