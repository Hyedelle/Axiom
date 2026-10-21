//! # Axiom CLI (`axiom-cli`)
//!
//! Command-line tools for headless replay verification, state dumping, and benchmarks.

pub mod replayer;

pub use replayer::{ReplayReport, Replayer};
