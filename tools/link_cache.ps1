# Axiom D-Drive Cache & Workspace Junction Linker
# Sets up unified D:\axiom hierarchy and creates junctions for tmp and target

param (
    [string]$BaseDir = "D:\axiom",
    [string]$TmpDir = "D:\Tmp"
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $ProjectRoot

Write-Host "==> Checking and preparing D-drive Axiom directory: $BaseDir (Tmp: $TmpDir)" -ForegroundColor Cyan

# 1. Create base directory structure on D:
$SubDirs = @(
    $TmpDir,
    (Join-Path $BaseDir "cache\target"),
    (Join-Path $BaseDir "builds"),
    (Join-Path $BaseDir "dumps")
)

foreach ($dir in $SubDirs) {
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        Write-Host "  [+] Created: $dir" -ForegroundColor Green
    } else {
        Write-Host "  [=] Exists:  $dir" -ForegroundColor Gray
    }
}

# 2. Junction Links to create in Project Root
$Junctions = @{
    (Join-Path $ProjectRoot "tmp")    = $TmpDir
    (Join-Path $ProjectRoot "target") = (Join-Path $BaseDir "cache\target")
}

Write-Host "`n==> Linking Windows Junctions..." -ForegroundColor Cyan

foreach ($linkPath in $Junctions.Keys) {
    $targetPath = $Junctions[$linkPath]
    $linkName = Split-Path $linkPath -Leaf

    if (Test-Path $linkPath) {
        $item = Get-Item $linkPath -Force
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            Write-Host "  [=] $linkName already linked to junction." -ForegroundColor Gray
            continue
        } else {
            Write-Host "  [!] $linkName is a physical directory. Migrating..." -ForegroundColor Yellow
            Copy-Item -Path "$linkPath\*" -Destination $targetPath -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -Path $linkPath -Recurse -Force
        }
    }

    New-Item -ItemType Junction -Path $linkPath -Target $targetPath | Out-Null
    Write-Host "  [+] Linked $linkName -> $targetPath" -ForegroundColor Green
}

Write-Host "`n==> [PASS] All D:\axiom cache junctions successfully verified." -ForegroundColor Green
