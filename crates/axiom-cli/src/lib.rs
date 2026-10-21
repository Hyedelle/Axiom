//! # Axiom CLI (`axiom-cli`)
//!
//! Command-line tools for headless replay verification, state dumping, and benchmarks.

pub mod balancer;
pub mod replayer;
pub mod rpg_demo;
pub mod session;
pub mod time_travel;

pub use balancer::{MatchStats, MonteCarloBalancer};
pub use replayer::{ReplayReport, Replayer};
pub use rpg_demo::{OpenWorldRpgSession, RpgReport};
pub use session::{HeadlessSession, SessionReport};
pub use time_travel::{AsciiView, TimeTravelDemo};
