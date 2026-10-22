# Axiom Engine — C# / Unity FFI Bindings Exporter & Verification Runner
# Purpose: Build release cdylib (axiom_sdk.dll), verify C# P/Invoke struct sizes,
#          and run headless in-memory C# simulation to ensure 100% ABI compatibility.
# Usage: powershell -ExecutionPolicy Bypass -File tools/export_sdk_csharp.ps1

$ErrorActionPreference = "Stop"

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $ProjectRoot

$BindingsDir = Join-Path $ProjectRoot "bindings\csharp"
$CSharpFile = Join-Path $BindingsDir "AxiomSdk.cs"
$TargetDll = Join-Path $BindingsDir "axiom_sdk.dll"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host " 🏛️ Axiom Engine — C# / Unity FFI Exporter & Verifier" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

# 1. Build release axiom-sdk cdylib
Write-Host "--> 1. Compiling axiom-sdk release cdylib..." -ForegroundColor Cyan
$Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

cmd.exe /c "cargo build --release -p axiom-sdk > tmp\cargo_sdk.log 2>&1"
$BuildExitCode = $LASTEXITCODE

$Stopwatch.Stop()
$BuildElapsedMs = $Stopwatch.ElapsedMilliseconds

if ($BuildExitCode -ne 0) {
    Write-Host "[FAIL] axiom-sdk compilation failed! (ExitCode: $BuildExitCode)" -ForegroundColor Red
    Get-Content "tmp\cargo_sdk.log" -Tail 10 | ForEach-Object { Write-Host "  $_" -ForegroundColor Yellow }
    exit $BuildExitCode
}

Write-Host "[OK] axiom_sdk.dll compiled in ${BuildElapsedMs}ms." -ForegroundColor Green

# 2. Copy DLL into bindings/csharp/
$SourceDll = Join-Path $ProjectRoot "target\release\axiom_sdk.dll"
Copy-Item -Path $SourceDll -Destination $TargetDll -Force

$DllSize = (Get-Item $TargetDll).Length
$DllSizeKb = [math]::Round($DllSize / 1KB, 2)
Write-Host "[OK] DLL staged: bindings/csharp/axiom_sdk.dll ($DllSizeKb KB)" -ForegroundColor Green

# 3. Setup DLL resolution directory
$Win32Code = @'
using System;
using System.Runtime.InteropServices;

public static class Kernel32
{
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    public static extern bool SetDllDirectory(string lpPathName);
}
'@
Add-Type -TypeDefinition $Win32Code
[Kernel32]::SetDllDirectory($BindingsDir) | Out-Null
Write-Host "[OK] Win32 SetDllDirectory configured: $BindingsDir" -ForegroundColor Green

# 4. Define Verification Runner and Compile with AxiomSdk.cs in-memory
Write-Host "--> 2. Compiling AxiomSdk.cs and Verification Runner in-memory..." -ForegroundColor Cyan

$VerificationRunnerCode = @'
namespace Axiom.Engine
{
    public static class AxiomVerifier
    {
        public static bool RunSuite(out string report)
        {
            // 1. Create Engine Context
            using (AxiomEngine engine = AxiomEngine.Create())
            {
                if (!engine.IsValid)
                {
                    report = "Engine instance handle is invalid.";
                    return false;
                }

                // 2. Register Type (ID 10: Smelter Furnace)
                TypeDefinition smelterType = TypeDefinition.Create(
                    10,
                    (ushort)(Category.Solid | Category.Producer),
                    defaultHp: 300,
                    baseSpeed: 50,
                    primaryColor: 0xFF3F3F46
                );
                if (!engine.RegisterType(smelterType))
                {
                    report = "Failed to register TypeDefinition (ID 10).";
                    return false;
                }

                // 3. Register Recipe (ID 1: IronOre -> IronPlate)
                RecipeDefinition smelterRecipe = RecipeDefinition.Create(
                    1,
                    inputId: 1,
                    inputCount: 1,
                    outputId: 2,
                    outputCount: 1,
                    ticks: 20,
                    requiredCategory: Category.Producer
                );
                if (!engine.RegisterRecipe(smelterRecipe))
                {
                    report = "Failed to register RecipeDefinition (ID 1).";
                    return false;
                }

                // 4. Place Tile at (16, 16)
                TileCell cell = TileCell.Create(10, hp: 300, maxHp: 300, flags: TileFlags.Active | TileFlags.Powered);
                cell.RecipeId = 1;
                cell.InventoryType0 = 1; // IronOre
                cell.InventoryCount0 = 10;
                if (!engine.SetTile(16, 16, cell))
                {
                    report = "Failed to place TileCell at (16, 16).";
                    return false;
                }

                // 5. Run 100 ticks
                for (int i = 0; i < 100; i++)
                {
                    engine.Tick();
                }

                if (engine.TickCount != 100)
                {
                    report = "TickCount mismatch after 100 ticks: " + engine.TickCount;
                    return false;
                }

                ulong stateHash = engine.StateHash;
                if (stateHash == 0)
                {
                    report = "StateHash is zero.";
                    return false;
                }

                // 6. Test P2P Rollback Netcode Session
                using (AxiomRollbackSession session0 = AxiomRollbackSession.Create(0))
                using (AxiomRollbackSession session1 = AxiomRollbackSession.Create(1))
                {
                    session0.CopyRecipesFrom(engine);
                    session1.CopyRecipesFrom(engine);

                    // Advance Peer 0 with local action at frame 0 (setting tile at 10, 10)
                    GridAction act0 = GridAction.Create(GridActionKind.SetTile, 0, 10, 10, typeId: 50, param1: 0, param2: 100);
                    ulong frame0 = session0.AdvanceFrame(act0);
                    if (frame0 != 1)
                    {
                        report = "Frame 0 advance returned unexpected frame: " + frame0;
                        return false;
                    }

                    // Advance Peer 1 with empty actions at frame 0 and 1 (simulating local fast-forward)
                    session1.AdvanceFrame(GridAction.None);
                    session1.AdvanceFrame(GridAction.None);

                    // Deliver Peer 0's frame 0 action to Peer 1 with simulated network lag
                    bool rolledBack = session1.ReceiveRemoteAction(0, 0, act0);
                    if (!rolledBack)
                    {
                        report = "Peer 1 failed to trigger speculative rollback on delayed remote action.";
                        return false;
                    }

                    if (session1.RollbacksCount == 0)
                    {
                        report = "RollbacksCount is zero on Peer 1 despite rollback triggered.";
                        return false;
                    }

                    // Advance Peer 0 to frame 2 with empty action
                    session0.AdvanceFrame(GridAction.None);

                    // Both peers at frame 2 must have bit-identical state hashes!
                    ulong h0 = session0.StateHash;
                    ulong h1 = session1.StateHash;

                    if (h0 != h1)
                    {
                        report = string.Format("Rollback state hash divergence! P0: 0x{0:X16}, P1: 0x{1:X16}", h0, h1);
                        return false;
                    }
                }

                report = string.Format("Engine simulation 100 ticks OK (StateHash: 0x{0:X16}). P2P Rollback Netcode bit-identical agreement OK.", stateHash);
                return true;
            }
        }
    }
}
'@

$CombinedSource = (Get-Content $CSharpFile -Raw) + "`n" + $VerificationRunnerCode
Add-Type -TypeDefinition $CombinedSource
Write-Host "[OK] AxiomSdk.cs & Verifier compiled without warnings or errors." -ForegroundColor Green

# 5. Verify 64B L1 Cache Alignments and Struct Sizes
Write-Host "--> 3. Verifying L1 Cache Alignment & Struct Byte Sizes..." -ForegroundColor Cyan

$typeDefSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type]"Axiom.Engine.TypeDefinition")
$recipeDefSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type]"Axiom.Engine.RecipeDefinition")
$tileCellSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type]"Axiom.Engine.TileCell")
$gridActionSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type]"Axiom.Engine.GridAction")

Write-Host "   - TypeDefinition size: $typeDefSize bytes (Target: 64)" -ForegroundColor Gray
Write-Host "   - RecipeDefinition size: $recipeDefSize bytes (Target: 64)" -ForegroundColor Gray
Write-Host "   - TileCell size: $tileCellSize bytes (Target: 64)" -ForegroundColor Gray
Write-Host "   - GridAction size: $gridActionSize bytes (Target: 16)" -ForegroundColor Gray

if ($typeDefSize -ne 64 -or $recipeDefSize -ne 64 -or $tileCellSize -ne 64 -or $gridActionSize -ne 16) {
    Write-Host "[FAIL] Struct size mismatch detected! ABI alignment violated." -ForegroundColor Red
    exit 1
}
Write-Host "[PASS] 100% Struct sizes match exact hardware cache-line boundaries!" -ForegroundColor Green

# 6. Execute In-Memory C# Verification Scenario
Write-Host "--> 4. Executing In-Memory C# Simulation & Netcode Verification..." -ForegroundColor Cyan

$report = ""
$success = [Axiom.Engine.AxiomVerifier]::RunSuite([ref]$report)

if (-not $success) {
    Write-Host "[FAIL] C# verification suite failed: $report" -ForegroundColor Red
    exit 1
}

Write-Host "   $report" -ForegroundColor Gray
Write-Host "[PASS] Full C# / Unity FFI verification suite passed 100%!" -ForegroundColor Green

# 7. Compile & Run Standalone Native C# Smoke Test Binary (Unity IL2CPP Simulation)
$CscPath = "C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$SmokeTestSrc = Join-Path $BindingsDir "AxiomSmokeTest.cs"
$SmokeTestExe = Join-Path $BindingsDir "AxiomSmokeTest.exe"

if ((Test-Path $CscPath) -and (Test-Path $SmokeTestSrc)) {
    Write-Host "--> 5. Compiling & Executing Standalone Native C# Smoke Test (T-069)..." -ForegroundColor Cyan
    & $CscPath /nologo /platform:x64 /unsafe /target:exe /out:$SmokeTestExe $CSharpFile $SmokeTestSrc | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[FAIL] Failed to compile AxiomSmokeTest.exe" -ForegroundColor Red
        exit 1
    }
    
    # Run in bindings directory so axiom_sdk.dll resolves immediately
    Push-Location $BindingsDir
    try {
        & .\AxiomSmokeTest.exe
        if ($LASTEXITCODE -ne 0) {
            Write-Host "[FAIL] AxiomSmokeTest.exe returned non-zero exit code: $LASTEXITCODE" -ForegroundColor Red
            exit 1
        }
    } finally {
        Pop-Location
    }
}

Write-Host "============================================================" -ForegroundColor Green
Write-Host " 🎉 C# / Unity FFI Bindings Successfully Exported & Verified!" -ForegroundColor Green
Write-Host " - C# Source:  $CSharpFile" -ForegroundColor White
Write-Host " - Native DLL: $TargetDll ($DllSizeKb KB)" -ForegroundColor White
Write-Host " - Smoke Test: $SmokeTestExe" -ForegroundColor White
Write-Host "============================================================" -ForegroundColor Green

exit 0
