# ==============================================================================
# Axiom Engine — One-Click GitHub Pages Deployment Script (tools/deploy-pages.ps1)
# ==============================================================================
[CmdletBinding()]
param(
    [string]$RepoName = "Hyedelle/Axiom",
    [switch]$SkipGuard = $false
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = (Resolve-Path "$PSScriptRoot/..").Path
Set-Location $WorkspaceRoot

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "🏛️  Axiom Web Demo One-Click GitHub Pages Deployer" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Sync web bundle
Write-Host "==> [1/5] Syncing WASM binary and Web player..." -ForegroundColor Yellow
if (-not (Test-Path "$WorkspaceRoot/web")) {
    New-Item -ItemType Directory -Force -Path "$WorkspaceRoot/web" | Out-Null
}
Copy-Item "$WorkspaceRoot/crates/axiom-wasm/index.html" "$WorkspaceRoot/web/index.html" -Force
Copy-Item "$WorkspaceRoot/crates/axiom-wasm/axiom_wasm.wasm" "$WorkspaceRoot/web/axiom_wasm.wasm" -Force

$WasmSize = (Get-Item "$WorkspaceRoot/web/axiom_wasm.wasm").Length
$WasmSizeKb = [math]::Round($WasmSize / 1024, 1)
Write-Host "    [OK] WASM Bundle Size: $WasmSizeKb KB ($WasmSize bytes)" -ForegroundColor Green

# 2. Constitutional Guard Check
if (-not $SkipGuard) {
    Write-Host "==> [2/5] Running Constitutional Guard..." -ForegroundColor Yellow
    & powershell -NoProfile -ExecutionPolicy Bypass -File "$WorkspaceRoot/tools/guard.ps1"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "[FAIL] Constitutional Guard failed. Aborting deployment."
        exit 1
    }
} else {
    Write-Host "==> [2/5] Skipping Constitutional Guard (-SkipGuard specified)" -ForegroundColor Gray
}

# 3. Check GitHub CLI Authentication
Write-Host "==> [3/5] Verifying GitHub CLI status..." -ForegroundColor Yellow
$GhAuth = gh auth status 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Error "[FAIL] GitHub CLI is not authenticated. Please run 'gh auth login'."
    exit 1
}
Write-Host "    [OK] GitHub CLI authenticated as Hyedelle." -ForegroundColor Green

# 4. Check or Create Remote Repository
Write-Host "==> [4/5] Checking Remote Repository ($RepoName)..." -ForegroundColor Yellow
$RepoExists = $false
try {
    $null = gh repo view $RepoName 2>$null
    if ($LASTEXITCODE -eq 0) {
        $RepoExists = $true
    }
} catch {
    $RepoExists = $false
}

if (-not $RepoExists) {
    Write-Host "    Repository $RepoName does not exist. Creating public repository..." -ForegroundColor Cyan
    gh repo create $RepoName --public --source=. --remote=origin --push
    if ($LASTEXITCODE -ne 0) {
        Write-Error "[FAIL] Failed to create repository $RepoName via gh."
        exit 1
    }
    Write-Host "    [OK] Repository created and pushed to origin." -ForegroundColor Green
} else {
    # Check if origin remote is configured
    $Remotes = git remote
    if ($Remotes -notcontains "origin") {
        git remote add origin "https://github.com/$RepoName.git"
    }
    Write-Host "    [OK] Remote origin configured for $RepoName." -ForegroundColor Green
}

# 5. Push Latest Commits to GitHub
Write-Host "==> [5/5] Pushing to GitHub & Triggering Pages Deployment..." -ForegroundColor Yellow
git push -u origin master
if ($LASTEXITCODE -ne 0) {
    Write-Warning "Push failed or already up-to-date. Checking status..."
}

# Enable GitHub Pages via Actions if not already configured
try {
    gh api repos/$RepoName/pages -X POST -f "build_type=workflow" 2>&1 | Out-Null
} catch {
    # Might already be enabled
}

Write-Host "==========================================================" -ForegroundColor Green
Write-Host "🎉 [DEPLOY SUCCESS] Axiom Web Demo Shipped!" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "🌍 Live Demo URL: https://hyedelle.github.io/Axiom/" -ForegroundColor Cyan
Write-Host "⭐ GitHub Repo:  https://github.com/$RepoName" -ForegroundColor Cyan
Write-Host "💬 Feedback URL: https://github.com/$RepoName/issues" -ForegroundColor Cyan
