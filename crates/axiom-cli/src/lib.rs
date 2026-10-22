//! # Axiom CLI (`axiom-cli`)
//!
//! Command-line tools for headless replay verification, state dumping, and benchmarks.

pub mod balancer;
pub mod boxing_view;
pub mod coop;
pub mod desktop;
pub mod replayer;
pub mod rpg_demo;
pub mod session;
pub mod sound;
pub mod telemetry;
pub mod time_travel;
pub mod world_view;

pub use balancer::{MatchStats, MonteCarloBalancer};
pub use boxing_view::BoxingView;
pub use coop::{MultiplayerRaidPeer, RaidCommand, BOSS_ID, HERO_1_ID, HERO_2_ID};
pub use desktop::{
    run_desktop_window, DesktopEngine, ProfilerMetrics, SaveFileHeader, ViewMode, DT_FIXED,
    MAX_ACCUMULATOR, SAVE_MAGIC, SAVE_VERSION, TICK_RATE_HZ,
};
pub use world_view::{
    archetype_sprite_style, deposit_color, sample_world_cell, terrain_color, OpenWorldView,
    WorldCamera, WorldCell, WorldChunkManager, CAMERA_SPEED, CAMERA_SPRINT_MULT, VIEW_H,
    VIEW_TILES_H, VIEW_TILES_W, VIEW_W, VIEW_X, VIEW_Y, WORLD_MONSTER_CAP, WORLD_VIEW_RADIUS,
};
pub use replayer::{ReplayReport, Replayer};
pub use rpg_demo::{OpenWorldRpgSession, RpgReport};
pub use session::{HeadlessSession, SessionReport};
pub use sound::*;
pub use telemetry::{EngineTelemetry, SubsystemHealth};
pub use time_travel::{AsciiView, TimeTravelDemo};
