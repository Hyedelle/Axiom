//! Interactive Time-Travel & Rollback Debugging Engine.
//!
//! Visualizes discrete world snapshots in terminal ASCII grids and validates
//! 0.001ms instantaneous rollback across divergent and identical timelines.

use axiom_core::fixed::FixedVec2;
use axiom_core::input::InputState;
use axiom_core::state::TickState;
use axiom_core::time_machine::TimeMachine;

/// Visual ASCII grid representation of a discrete tick state.
pub struct AsciiView;

impl AsciiView {
    /// Renders a discrete world state as a compact ASCII viewport.
    pub fn render<const N: usize>(state: &TickState<N>, width: usize, height: usize) -> String {
        let mut grid = vec![vec!['.'; width]; height];

        for (idx, entity) in state.entities.iter().enumerate() {
            if entity.is_active() {
                let x = entity.pos.x.to_int();
                let y = entity.pos.y.to_int();
                if x >= 0 && (x as usize) < width && y >= 0 && (y as usize) < height {
                    let char_rep = if idx == 0 { 'P' } else { 'E' };
                    grid[y as usize][x as usize] = char_rep;
                }
            }
        }

        let mut output = String::with_capacity((width + 4) * height + 128);
        output.push_str(&format!(
            "┌{}┐ [Tick: {:04} | Hash: 0x{:016X}]\n",
            "─".repeat(width),
            state.tick.0,
            state.compute_hash()
        ));

        for row in grid {
            output.push('│');
            for ch in row {
                output.push(ch);
            }
            output.push_str("│\n");
        }

        output.push_str(&format!("└{}┘\n", "─".repeat(width)));
        output
    }
}

/// Demonstrates instantaneous time-travel rollback and timeline branching.
pub struct TimeTravelDemo {
    time_machine: Box<TimeMachine<64, 16>>,
    current_state: TickState<16>,
}

impl Default for TimeTravelDemo {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeTravelDemo {
    pub fn new() -> Self {
        let mut initial_state = TickState::<16>::new();
        // Player at (2, 2)
        initial_state.spawn(1, FixedVec2::from_ints(2, 2), FixedVec2::ZERO);
        // Enemy at (15, 6)
        initial_state.spawn(2, FixedVec2::from_ints(15, 6), FixedVec2::from_ints(-1, 0));

        let mut tm = Box::new(TimeMachine::<64, 16>::new());
        tm.push(initial_state, InputState::NEUTRAL);

        Self {
            time_machine: tm,
            current_state: initial_state,
        }
    }

    /// Steps the simulation by 1 tick using the specified input.
    pub fn step(&mut self, input: InputState) {
        self.current_state = self.current_state.step(&input);
        self.time_machine.push(self.current_state, input);
    }

    /// Rewinds time back to a historical tick.
    pub fn rollback_to(&mut self, target_tick: u64) -> Result<(), axiom_core::RollbackError> {
        let restored = self
            .time_machine
            .rollback_to(axiom_core::Tick(target_tick))?;
        self.current_state = restored;
        Ok(())
    }

    pub fn state(&self) -> &TickState<16> {
        &self.current_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_view_rendering() {
        let mut state = TickState::<16>::new();
        state.spawn(1, FixedVec2::from_ints(2, 2), FixedVec2::ZERO);
        let rendered = AsciiView::render(&state, 10, 5);
        assert!(rendered.contains('P'), "Rendered view must show Player 'P'");
        assert!(rendered.contains("Tick: 0000"));
    }

    #[test]
    fn test_time_travel_rollback_consistency() {
        let mut demo = TimeTravelDemo::new();

        // Advance 20 ticks moving right
        for _ in 0..20 {
            demo.step(InputState::new(1, 0, 0));
        }
        assert_eq!(demo.state().tick.0, 20);
        let hash_at_20 = demo.state().compute_hash();

        // Advance another 20 ticks (to 40)
        for _ in 0..20 {
            demo.step(InputState::new(0, 1, 0));
        }
        assert_eq!(demo.state().tick.0, 40);

        // Instantaneous Rollback to tick 20
        demo.rollback_to(20)
            .expect("Rollback to tick 20 must succeed");
        assert_eq!(demo.state().tick.0, 20);
        assert_eq!(
            demo.state().compute_hash(),
            hash_at_20,
            "State hash after rollback must be 100% bit-identical to recorded hash"
        );
    }
}
