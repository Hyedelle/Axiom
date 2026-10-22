//! T-074 integration tests: pure-integer power grid settlement (brownout / accumulator cycle)
//! and allocation-free pipe fluid transport (mass conservation, pumps, check valves), plus
//! bit-level replay determinism of both simulators coupled together.

use axiom_core::{
    FluidNetwork, FluidType, PipeKind, PipeNode, PowerGridManager, PowerNode, PowerRole,
    DIR_EAST, DIR_NORTH, DIR_SOUTH, DIR_WEST, NO_NET,
};

type Grid = PowerGridManager<16, 256>;
type Pipes = FluidNetwork<512>;

#[test]
fn test_power_grid_brownout_and_accumulator() {
    let mut g = Grid::new();
    let pole = g.add_node(PowerNode::pole(0, 0, 10, 10)).unwrap();
    let gen = g.add_node(PowerNode::generator(1, 0, 100)).unwrap();
    let c0 = g.add_node(PowerNode::consumer(2, 0, 40)).unwrap();
    let c1 = g.add_node(PowerNode::consumer(3, 0, 40)).unwrap();
    let acc = g.add_node(PowerNode::accumulator(5, 0, 500, 30)).unwrap();

    // Phase A: 100 produced vs 80 demanded -> 20 surplus charges the accumulator every tick.
    for _ in 0..10 {
        g.tick();
        let net = g.network(g.net_of(pole)).unwrap();
        assert_eq!(net.satisfaction_pct, 100);
        assert_eq!(net.delivered, 80);
        assert_eq!(net.charged, 20);
        assert_eq!(net.produced, 100);
        assert_eq!(net.wasted, 0);
        assert_eq!(net.deficit, 0);
        assert_eq!(g.node(c0).unwrap().flow_rate, 40);
        assert_eq!(g.node(gen).unwrap().flow_rate, 100);
    }
    assert_eq!(g.node(acc).unwrap().energy_stored, 200);
    assert_eq!(g.efficiency_pct(c1), 100);

    // Phase B: third consumer -> 120 demand, 20 deficit covered by the accumulator for 10 ticks.
    let c2 = g.add_node(PowerNode::consumer(4, 0, 40)).unwrap();
    for t in 0..10 {
        g.tick();
        let net = g.network(g.net_of(pole)).unwrap();
        assert_eq!(net.demand, 120);
        assert_eq!(net.discharged, 20, "tick {t}");
        assert_eq!(net.satisfaction_pct, 100);
        assert_eq!(net.delivered, 120);
        assert_eq!(net.brownout_ticks, 0);
        assert_eq!(g.node(c2).unwrap().flow_rate, 40);
    }
    assert_eq!(g.node(acc).unwrap().energy_stored, 0);

    // Phase C: accumulator empty -> brownout at floor(100 * 100 / 120) = 83 %.
    g.tick();
    {
        let net = g.network(g.net_of(pole)).unwrap();
        assert_eq!(net.satisfaction_pct, 83);
        assert_eq!(net.delivered, 100);
        assert_eq!(net.deficit, 20);
        assert_eq!(net.discharged, 0);
        assert_eq!(net.brownout_ticks, 1);
        let sum: u32 = [c0, c1, c2].iter().map(|&c| g.node(c).unwrap().flow_rate as u32).sum();
        assert_eq!(sum, 100, "every delivered unit is attributed to exactly one consumer");
        // 40 * 100 / 120 = 33 each, remainder 1 goes to the first consumer in slot order.
        assert_eq!(g.node(c0).unwrap().flow_rate, 34);
        assert_eq!(g.node(c1).unwrap().flow_rate, 33);
        assert_eq!(g.node(c2).unwrap().flow_rate, 33);
        assert_eq!(g.efficiency_pct(c2), 83);
    }

    // Phase D: generator removed -> total blackout, consumers idle.
    let removed = g.remove_node(gen).unwrap();
    assert_eq!(removed.role, PowerRole::Generator);
    g.tick();
    {
        let net = g.network(g.net_of(pole)).unwrap();
        assert_eq!(net.generation_capacity, 0);
        assert_eq!(net.satisfaction_pct, 0);
        assert_eq!(net.delivered, 0);
        assert_eq!(net.deficit, 120);
    }
    let c0 = g.find_node(2, 0).unwrap();
    assert_eq!(g.efficiency_pct(c0), 0);
    assert_eq!(g.node(c0).unwrap().flow_rate, 0);

    // Phase E: oversized generator -> accumulator charges at its rate limit, rest is wasted.
    g.add_node(PowerNode::generator(1, 0, 200)).unwrap();
    g.tick();
    {
        let net = g.network(g.net_of(pole)).unwrap();
        assert_eq!(net.satisfaction_pct, 100);
        assert_eq!(net.charged, 30);
        assert_eq!(net.wasted, 50);
        assert_eq!(net.produced, 150);
    }
    let acc = g.find_node(5, 0).unwrap();
    assert_eq!(g.node(acc).unwrap().energy_stored, 30);

    // An island without a pole is never powered.
    let island = g.add_node(PowerNode::consumer(500, 500, 10)).unwrap();
    g.tick();
    assert_eq!(g.net_of(island), NO_NET);
    assert_eq!(g.efficiency_pct(island), 0);
}

fn build_u_tube(net: &mut Pipes) -> (u16, u16) {
    // Left leg x = 0 (y 0..=5), bottom (1, 5), right leg x = 2 (y 5..=0): 13 segments of 100.
    let left_top = net.add_pipe(PipeNode::pipe(0, 0, 100)).unwrap();
    for y in 1..=5i16 {
        net.add_pipe(PipeNode::pipe(0, y, 100)).unwrap();
    }
    net.add_pipe(PipeNode::pipe(1, 5, 100)).unwrap();
    for y in (1..=5i16).rev() {
        net.add_pipe(PipeNode::pipe(2, y, 100)).unwrap();
    }
    let right_top = net.add_pipe(PipeNode::pipe(2, 0, 100)).unwrap();
    (left_top, right_top)
}

#[test]
fn test_fluid_mass_conservation_and_flow() {
    // --- U-tube: water poured into one leg ends level in both legs, nothing lost. ---
    let mut net = Pipes::new();
    let (left_top, right_top) = build_u_tube(&mut net);
    let mut injected = 0u32;
    let mut pour_ticks = 0u32;
    while injected < 600 && pour_ticks < 60 {
        injected += net.inject(left_top, FluidType::Water, 100, 20) as u32;
        net.tick();
        pour_ticks += 1;
        assert_eq!(net.total_volume(), injected);
    }
    assert!(injected >= 600, "injected {injected} in {pour_ticks} ticks");
    for _ in 0..400 {
        net.tick();
        assert_eq!(net.total_volume(), injected, "mass must be conserved every tick");
    }
    // Integer dead zone: `delta / 2` truncates to 0 when neighbours differ by exactly 1 unit,
    // so every adjacent pair settles within 1 unit (a 13-segment chain spans at most 12).
    // Segments were added in chain order, so slot order == chain order here.
    let vols: Vec<u16> = net.pipes().iter().map(|p| p.volume).collect();
    for w in vols.windows(2) {
        assert!(w[0].abs_diff(w[1]) <= 1, "adjacent segments must settle within 1 unit: {vols:?}");
    }
    let (lo, hi) = (*vols.iter().min().unwrap(), *vols.iter().max().unwrap());
    assert!(hi - lo <= 12, "U-tube must level out (min {lo}, max {hi})");
    assert!(lo >= 40, "fluid must reach the far leg (min {lo})");
    assert_eq!(net.pipe(right_top).unwrap().fluid_type, FluidType::Water);
    assert!(net.pipes().iter().all(|p| p.temperature == 20));
    assert_eq!(net.stats().drained_total, 0);
    assert_eq!(net.stats().injected_total as u32, injected);

    // --- Draining the far leg keeps the ledger balanced. ---
    let (kind, drained) = net.drain(right_top, 30);
    assert_eq!(kind, FluidType::Water);
    assert_eq!(drained, 30);
    for _ in 0..50 {
        net.tick();
    }
    let s = net.stats();
    assert_eq!(s.total_volume as u64 + s.drained_total, s.injected_total);

    // --- Ring with a pump: perpetual circulation, still conserved. ---
    let mut ring = Pipes::new();
    let a = ring.add_pipe(PipeNode::pipe(0, 0, 100)).unwrap();
    let pump = ring.add_pipe(PipeNode::pump(1, 0, DIR_EAST, 25)).unwrap();
    ring.add_pipe(PipeNode::pipe(2, 0, 100)).unwrap();
    ring.add_pipe(PipeNode::pipe(2, 1, 100)).unwrap();
    ring.add_pipe(PipeNode::pipe(2, 2, 100)).unwrap();
    ring.add_pipe(PipeNode::pipe(1, 2, 100)).unwrap();
    let fill = ring.add_pipe(PipeNode::pipe(0, 2, 100)).unwrap();
    ring.add_pipe(PipeNode::pipe(0, 1, 100)).unwrap();
    assert_eq!(ring.inject(pump, FluidType::CrudeOil, 10, 30), 0, "pumps hold no fluid");
    let mut total = 0u32;
    let mut pour_ticks = 0u32;
    while total < 300 && pour_ticks < 60 {
        total += ring.inject(fill, FluidType::CrudeOil, 100, 30) as u32;
        ring.tick();
        pour_ticks += 1;
        assert_eq!(ring.total_volume(), total);
    }
    assert!(total >= 300, "poured {total} in {pour_ticks} ticks");
    let mut pumped = 0u64;
    for _ in 0..200 {
        ring.tick();
        assert_eq!(ring.total_volume(), total);
        pumped += ring.stats().pumped_last_tick as u64;
        assert_eq!(ring.pipe(pump).unwrap().volume, 0);
    }
    assert!(pumped > 200 * 10, "pump must keep the ring circulating (pumped {pumped})");
    // The pump drains its back segment and overfills its front segment.
    assert!(ring.pipe(a).unwrap().volume < ring.pipe(ring.find_pipe(2, 0).unwrap()).unwrap().volume);
    assert_eq!(ring.pipe(pump).unwrap().kind, PipeKind::Pump);

    // Brownout coupling: at 0 % efficiency the pump stalls and the ring levels out.
    assert!(ring.set_pump_efficiency(pump, 0));
    for _ in 0..200 {
        ring.tick();
        assert_eq!(ring.total_volume(), total);
    }
    assert_eq!(ring.stats().pumped_last_tick, 0);
    // The pump splits the ring into an open chain; passive flow still levels it.
    let vols: Vec<u16> = ring.pipes().iter().filter(|p| p.kind != PipeKind::Pump).map(|p| p.volume).collect();
    assert!(vols.iter().max().unwrap() - vols.iter().min().unwrap() <= 6, "7-segment chain: {vols:?}");

    // --- Check valve: forward flow only. ---
    let mut valve_net = Pipes::new();
    let src = valve_net.add_pipe(PipeNode::tank(0, 0, 1000)).unwrap();
    let valve = valve_net.add_pipe(PipeNode::check_valve(1, 0, DIR_EAST, 100)).unwrap();
    let dst = valve_net.add_pipe(PipeNode::tank(2, 0, 1000)).unwrap();
    assert_eq!(valve_net.inject(dst, FluidType::Water, 500, 50), 500);
    for _ in 0..100 {
        valve_net.tick();
        assert_eq!(valve_net.total_volume(), 500);
    }
    assert_eq!(valve_net.pipe(src).unwrap().volume, 0, "no backflow through the valve");
    assert_eq!(valve_net.pipe(valve).unwrap().volume, 0);
    assert_eq!(valve_net.pipe(dst).unwrap().volume, 500);
    assert_eq!(valve_net.inject(src, FluidType::Water, 900, 10), 900);
    for _ in 0..300 {
        valve_net.tick();
        assert_eq!(valve_net.total_volume(), 1400);
    }
    let (sv, dv) = (valve_net.pipe(src).unwrap().volume, valve_net.pipe(dst).unwrap().volume);
    assert!(dv > 500 && sv < 900, "forward flow must reach the destination (src {sv}, dst {dv})");
    // Integer volume-weighted temperature mixing: 500 @ 50 + inflow @ 10 lands strictly between.
    let t = valve_net.pipe(dst).unwrap().temperature;
    assert!(t > 10 && t < 50, "mixed temperature {t}");

    // Vertical valve orientation uses north / south neighbours.
    let mut v2 = Pipes::new();
    let top = v2.add_pipe(PipeNode::tank(0, 0, 1000)).unwrap();
    v2.add_pipe(PipeNode::check_valve(0, 1, DIR_NORTH, 100)).unwrap();
    let bottom = v2.add_pipe(PipeNode::tank(0, 2, 1000)).unwrap();
    v2.inject(top, FluidType::Steam, 400, 165);
    for _ in 0..100 {
        v2.tick();
    }
    assert_eq!(v2.pipe(bottom).unwrap().volume, 0, "south-bound flow blocked by a north valve");
    assert_eq!(v2.total_volume(), 400);
    let _ = (DIR_SOUTH, DIR_WEST);
}

/// Coupled scenario: pumps run at the power network's satisfaction percentage.
fn run_coupled(ticks: usize, oil_per_tick: u16) -> (Vec<u64>, Vec<u64>, Grid, Pipes) {
    let mut grid = Grid::new();
    let mut pipes = Pipes::new();

    grid.add_node(PowerNode::pole(0, 0, 12, 12)).unwrap();
    let gen = grid.add_node(PowerNode::generator(1, 0, 90)).unwrap();
    for i in 0..4i32 {
        grid.add_node(PowerNode::consumer(2 + i, 1, 30)).unwrap();
    }
    let acc = grid.add_node(PowerNode::accumulator(6, 0, 400, 20)).unwrap();
    // Second, independent network far away.
    grid.add_node(PowerNode::pole(1000, 1000, 5, 5)).unwrap();
    grid.add_node(PowerNode::generator(1001, 1000, 10)).unwrap();
    grid.add_node(PowerNode::consumer(1002, 1000, 25)).unwrap();

    let well = pipes.add_pipe(PipeNode::tank(0, 0, 2000)).unwrap();
    let pump = pipes.add_pipe(PipeNode::pump(1, 0, DIR_EAST, 40)).unwrap();
    for x in 2..12i16 {
        pipes.add_pipe(PipeNode::pipe(x, 0, 100)).unwrap();
    }
    pipes.add_pipe(PipeNode::check_valve(12, 0, DIR_EAST, 100)).unwrap();
    let refinery = pipes.add_pipe(PipeNode::tank(13, 0, 1500)).unwrap();
    // A water branch hanging off the main line never mixes with the oil.
    let water = pipes.add_pipe(PipeNode::pipe(5, 1, 100)).unwrap();
    pipes.inject(water, FluidType::Water, 60, 15);

    let mut grid_hashes = Vec::with_capacity(ticks);
    let mut pipe_hashes = Vec::with_capacity(ticks);
    for t in 0..ticks {
        // Scripted events.
        if t == 100 {
            grid.set_capacity(gen, 60);
        }
        if t == 200 {
            grid.add_node(PowerNode::consumer(7, 1, 30)).unwrap();
        }
        if t == 260 {
            let idx = grid.find_node(2, 1).unwrap();
            grid.remove_node(idx);
        }
        grid.tick();
        let eff = grid.efficiency_pct(acc);
        pipes.set_pump_efficiency(pump, eff);
        pipes.inject(well, FluidType::CrudeOil, oil_per_tick, 25);
        if t % 7 == 0 {
            pipes.drain(refinery, 30);
        }
        pipes.tick();
        grid_hashes.push(grid.compute_hash());
        pipe_hashes.push(pipes.compute_hash());
    }
    (grid_hashes, pipe_hashes, grid, pipes)
}

#[test]
fn test_power_fluid_determinism() {
    const TICKS: usize = 400;
    let (g1, p1, grid_a, pipes_a) = run_coupled(TICKS, 35);
    let (g2, p2, grid_b, pipes_b) = run_coupled(TICKS, 35);
    assert_eq!(g1, g2, "power grid replay must be bit-identical");
    assert_eq!(p1, p2, "fluid network replay must be bit-identical");
    assert_eq!(grid_a.compute_hash(), grid_b.compute_hash());
    assert_eq!(pipes_a.compute_hash(), pipes_b.compute_hash());
    assert_eq!(grid_a.nodes(), grid_b.nodes());
    assert_eq!(pipes_a.pipes(), pipes_b.pipes());

    // The scenario actually exercised brownout and the pump coupling.
    assert!(grid_a.network_count() == 2);
    let main_net = grid_a.networks()[0];
    assert!(main_net.brownout_ticks > 0, "generator cut at t=100 must cause a brownout");
    assert!(main_net.satisfaction_pct < 100);
    let s = pipes_a.stats();
    assert_eq!(s.total_volume as u64 + s.drained_total, s.injected_total + 0, "ledger (initial 0)");
    assert_eq!(pipes_a.volume_of(FluidType::Water), 60, "water branch untouched by oil");
    assert!(pipes_a.volume_of(FluidType::CrudeOil) > 1000);
    assert!(s.blocked_last_tick > 0, "oil keeps knocking on the water branch");

    // Cloned mid-state continues identically (pure state transition).
    let (_, _, mut grid_c, mut pipes_c) = run_coupled(150, 35);
    let mut grid_d = grid_c.clone();
    let mut pipes_d = pipes_c.clone();
    for _ in 0..50 {
        grid_c.tick();
        grid_d.tick();
        pipes_c.tick();
        pipes_d.tick();
    }
    assert_eq!(grid_c.compute_hash(), grid_d.compute_hash());
    assert_eq!(pipes_c.compute_hash(), pipes_d.compute_hash());

    // A single input difference diverges the fluid hash but not the (independent) grid hash.
    let (g3, p3, _, _) = run_coupled(TICKS, 36);
    assert_eq!(g1, g3);
    assert_ne!(p1, p3);
}
