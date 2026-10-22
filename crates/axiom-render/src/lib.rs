//! # Axiom Render (`axiom-render`)
//!
//! Ultra-lightweight, headless-first rendering layer for the Axiom Engine.
//! Decoupled from `axiom-core` to keep the simulation 100% deterministic and pure.

pub mod framebuffer;
pub mod sprites;

pub use framebuffer::{upscale_nearest_2x, FrameBuffer};
pub use sprites::*;
