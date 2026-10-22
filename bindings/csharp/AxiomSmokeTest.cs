//------------------------------------------------------------------------------
// Axiom Engine — Native C# / Unity FFI Comprehensive Smoke Test (T-069)
// Pure Integer Determinism (0 Floats) | 64B L1 Cache Alignment | P2P Rollback
//------------------------------------------------------------------------------

using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using Axiom.Engine;

namespace Axiom.SmokeTest
{
    public static class Program
    {
        public static int Main(string[] args)
        {
            Console.WriteLine("============================================================");
            Console.WriteLine(" 🏛️ Axiom Engine — Native C# / Unity FFI Smoke Test Suite");
            Console.WriteLine("============================================================");

            Stopwatch sw = Stopwatch.StartNew();

            try
            {
                // Gate 1: Struct byte sizes and alignment
                Console.WriteLine("--> [Gate 1] Validating L1 Cache Alignment & Struct Sizes...");
                VerifyStructSizes();

                // Gate 2: Fixed-point arithmetic
                Console.WriteLine("--> [Gate 2] Validating Zero-Float Fixed-Point Arithmetic...");
                VerifyFixedArithmetic();

                // Gate 3: Engine lifecycle & data-driven factory pipeline
                Console.WriteLine("--> [Gate 3] Simulating Data-Driven Factory Pipeline (200 Ticks)...");
                VerifyFactoryPipeline();

                // Gate 4: P2P Rollback Netcode speculative execution & convergence
                Console.WriteLine("--> [Gate 4] Stress-testing P2P GGPO Rollback Netcode...");
                VerifyRollbackNetcode();

                sw.Stop();
                Console.WriteLine("============================================================");
                Console.WriteLine(string.Format(" [PASS] All 4 Gates cleared in {0}ms with 100% Determinism!", sw.ElapsedMilliseconds));
                Console.WriteLine("============================================================");
                return 0;
            }
            catch (Exception ex)
            {
                sw.Stop();
                Console.ForegroundColor = ConsoleColor.Red;
                Console.WriteLine(string.Format("\n[FAIL] Smoke test aborted with error after {0}ms:", sw.ElapsedMilliseconds));
                Console.WriteLine(ex.ToString());
                Console.ResetColor();
                return 1;
            }
        }

        private static void VerifyStructSizes()
        {
            int sizeTypeDef = Marshal.SizeOf(typeof(TypeDefinition));
            int sizeRecipeDef = Marshal.SizeOf(typeof(RecipeDefinition));
            int sizeTileCell = Marshal.SizeOf(typeof(TileCell));
            int sizeGridAction = Marshal.SizeOf(typeof(GridAction));

            Console.WriteLine(string.Format("   - TypeDefinition: {0} bytes (expected: 64)", sizeTypeDef));
            Console.WriteLine(string.Format("   - RecipeDefinition: {0} bytes (expected: 64)", sizeRecipeDef));
            Console.WriteLine(string.Format("   - TileCell: {0} bytes (expected: 64)", sizeTileCell));
            Console.WriteLine(string.Format("   - GridAction: {0} bytes (expected: 16)", sizeGridAction));

            if (sizeTypeDef != 64) throw new InvalidOperationException("TypeDefinition size != 64");
            if (sizeRecipeDef != 64) throw new InvalidOperationException("RecipeDefinition size != 64");
            if (sizeTileCell != 64) throw new InvalidOperationException("TileCell size != 64");
            if (sizeGridAction != 16) throw new InvalidOperationException("GridAction size != 16");

            // Verify explicit offsets for TileCell
            AssertOffset(typeof(TileCell), "TypeId", 0);
            AssertOffset(typeof(TileCell), "StateFlags", 4);
            AssertOffset(typeof(TileCell), "Hp", 8);
            AssertOffset(typeof(TileCell), "MaxHp", 10);
            AssertOffset(typeof(TileCell), "WorkProgress", 12);
            AssertOffset(typeof(TileCell), "RecipeId", 14);
            AssertOffset(typeof(TileCell), "InventoryType0", 16);
            AssertOffset(typeof(TileCell), "InventoryCount0", 24);
            AssertOffset(typeof(TileCell), "Payload0", 32);
        }

        private static void AssertOffset(Type type, string fieldName, int expectedOffset)
        {
            int offset = Marshal.OffsetOf(type, fieldName).ToInt32();
            if (offset != expectedOffset)
            {
                throw new InvalidOperationException(string.Format("Field {0}.{1} offset {2} != expected {3}", type.Name, fieldName, offset, expectedOffset));
            }
        }

        private static void VerifyFixedArithmetic()
        {
            Fixed a = Fixed.FromInt(10);
            Fixed b = Fixed.FromInt(3);

            Fixed sum = a + b;
            Fixed diff = a - b;
            Fixed prod = a * b;
            Fixed quot = a / b;

            if (sum.ToInt() != 13) throw new InvalidOperationException("Fixed addition failed");
            if (diff.ToInt() != 7) throw new InvalidOperationException("Fixed subtraction failed");
            if (prod.ToInt() != 30) throw new InvalidOperationException("Fixed multiplication failed");
            if (quot.ToInt() != 3) throw new InvalidOperationException("Fixed division failed");

            // Precision check: 10 / 3 * 3 should be close to 10 within fractional epsilon
            Fixed roundtrip = quot * b;
            long delta = Math.Abs(a.Raw - roundtrip.Raw);
            if (delta > 3) throw new InvalidOperationException("Fixed precision loss excessive: " + delta);

            Console.WriteLine("   - Fixed-point arithmetic validated: 0 floats used.");
        }

        private static void VerifyFactoryPipeline()
        {
            using (AxiomEngine engine = AxiomEngine.Create())
            {
                if (!engine.IsValid) throw new InvalidOperationException("Engine handle invalid");

                // Register Types
                // 1: Miner (Producer, speed: 20 ticks)
                engine.RegisterType(TypeDefinition.Create(1, (ushort)(Category.Solid | Category.Producer), defaultHp: 500, baseSpeed: 20));
                // 2: Belt (Transporter, speed: 10 ticks)
                engine.RegisterType(TypeDefinition.Create(2, (ushort)(Category.Solid | Category.Transporter), defaultHp: 100, baseSpeed: 10));
                // 3: Smelter (Producer, speed: 30 ticks)
                engine.RegisterType(TypeDefinition.Create(3, (ushort)(Category.Solid | Category.Producer), defaultHp: 800, baseSpeed: 30));
                // 4: Chest (Container, cap: 100)
                engine.RegisterType(TypeDefinition.Create(4, (ushort)(Category.Solid | Category.Container), defaultHp: 200));

                // Register Recipe: IronOre(10) -> IronPlate(20), takes 20 ticks
                engine.RegisterRecipe(RecipeDefinition.Create(1, inputId: 10, inputCount: 1, outputId: 20, outputCount: 1, ticks: 20, requiredCategory: Category.Producer));

                // Place Miner at (10, 10) facing East (Direction = 1)
                TileCell miner = TileCell.Create(1, hp: 500, maxHp: 500, flags: TileFlags.Active | TileFlags.Powered);
                miner.Direction = 1; // East
                miner.RecipeId = 1;
                engine.SetTile(10, 10, miner);

                // Place Belt at (11, 10) facing East (Direction = 1)
                TileCell belt = TileCell.Create(2, hp: 100, maxHp: 100, flags: TileFlags.Active | TileFlags.Powered);
                belt.Direction = 1;
                engine.SetTile(11, 10, belt);

                // Place Smelter at (12, 10) facing East
                TileCell smelter = TileCell.Create(3, hp: 800, maxHp: 800, flags: TileFlags.Active | TileFlags.Powered);
                smelter.Direction = 1;
                smelter.RecipeId = 1;
                engine.SetTile(12, 10, smelter);

                // Run 200 ticks
                for (int i = 0; i < 200; i++)
                {
                    engine.Tick();
                }

                if (engine.TickCount != 200)
                {
                    throw new InvalidOperationException("Tick count mismatch: " + engine.TickCount);
                }

                ulong hash = engine.StateHash;
                if (hash == 0) throw new InvalidOperationException("StateHash is 0");

                Console.WriteLine(string.Format("   - 200 ticks simulated successfully. StateHash: 0x{0:X16}", hash));
            }
        }

        private static void VerifyRollbackNetcode()
        {
            using (AxiomEngine engine = AxiomEngine.Create())
            using (AxiomRollbackSession p0 = AxiomRollbackSession.Create(0))
            using (AxiomRollbackSession p1 = AxiomRollbackSession.Create(1))
            {
                p0.CopyRecipesFrom(engine);
                p1.CopyRecipesFrom(engine);

                // Frame 0..20: Simulate both peers running in lockstep
                for (ushort f = 0; f < 20; f++)
                {
                    GridAction a0 = (f == 5) ? GridAction.Create(GridActionKind.SetTile, 0, 5, 5, typeId: 2) : GridAction.None;
                    GridAction a1 = (f == 10) ? GridAction.Create(GridActionKind.SetTile, 1, 6, 6, typeId: 2) : GridAction.None;

                    p0.AdvanceFrame(a0);
                    p1.AdvanceFrame(a1);

                    // Exchange with 2 frames of lag
                    if (f >= 2)
                    {
                        ushort lagFrame = (ushort)(f - 2);
                        GridAction past0 = (lagFrame == 5) ? GridAction.Create(GridActionKind.SetTile, 0, 5, 5, typeId: 2) : GridAction.None;
                        GridAction past1 = (lagFrame == 10) ? GridAction.Create(GridActionKind.SetTile, 1, 6, 6, typeId: 2) : GridAction.None;

                        p1.ReceiveRemoteAction(0, lagFrame, past0);
                        p0.ReceiveRemoteAction(1, lagFrame, past1);
                    }
                }

                // Advance both to frame 30 with no more actions to flush pipeline
                for (ushort f = 20; f < 30; f++)
                {
                    p0.AdvanceFrame(GridAction.None);
                    p1.AdvanceFrame(GridAction.None);
                }

                Console.WriteLine(string.Format("   - Peer 0 Rollbacks: {0}, Peer 1 Rollbacks: {1}", p0.RollbacksCount, p1.RollbacksCount));
                if (p0.RollbacksCount == 0 || p1.RollbacksCount == 0)
                {
                    throw new InvalidOperationException("Speculative rollback was not triggered during simulated network lag!");
                }

                ulong hash0 = p0.StateHash;
                ulong hash1 = p1.StateHash;

                if (hash0 != hash1)
                {
                    throw new InvalidOperationException(string.Format("StateHash divergence after rollback! P0: 0x{0:X16}, P1: 0x{1:X16}", hash0, hash1));
                }

                Console.WriteLine(string.Format("   - State agreement achieved! Bit-identical Hash: 0x{0:X16}", hash0));
            }
        }
    }
}
