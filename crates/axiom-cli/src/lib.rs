//! # Axiom CLI (`axiom-cli`)
//!
//! Command-line tools for headless replay verification, state dumping, and benchmarks.

pub mod balancer;
pub mod coop;
pub mod desktop;
pub mod replayer;
pub mod rpg_demo;
pub mod session;
pub mod telemetry;
pub mod time_travel;

pub use balancer::{MatchStats, MonteCarloBalancer};
pub use coop::{MultiplayerRaidPeer, RaidCommand, BOSS_ID, HERO_1_ID, HERO_2_ID};
pub use desktop::run_desktop_window;
pub use replayer::{ReplayReport, Replayer};
pub use rpg_demo::{OpenWorldRpgSession, RpgReport};
pub use session::{HeadlessSession, SessionReport};
pub use telemetry::{EngineTelemetry, SubsystemHealth};
pub use time_travel::{AsciiView, TimeTravelDemo};
