//! # Axiom CLI (`axiom-cli`)
//!
//! Command-line tools for headless replay verification, state dumping, and benchmarks.

pub mod replayer;
pub mod time_travel;

pub use replayer::{ReplayReport, Replayer};
pub use time_travel::{AsciiView, TimeTravelDemo};
