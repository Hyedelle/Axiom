//! # Axiom Render (`axiom-render`)
//!
//! Ultra-lightweight, headless-first rendering layer for the Axiom Engine.
//! Decoupled from `axiom-core` to keep the simulation 100% deterministic and pure.

pub mod camera3d;
pub mod cutin;
pub mod framebuffer;
pub mod kinematics3d;
pub mod math3d;
pub mod mesh3d;
pub mod modding;
pub mod newspaper;
pub mod particles3d;
pub mod raster3d;
pub mod sprites;
pub mod texture;

pub use camera3d::{Camera3D, CameraShot};
pub use cutin::{render_cutin_overlay, CutinKind};
pub use framebuffer::{upscale_nearest_2x, FrameBuffer};
pub use kinematics3d::{
    evaluate_forward_kinematics, get_part_matrix, MechaPose3D, PartTransformTable,
};
pub use math3d::{cos_bams, sin_bams, Mat4Fixed, Vec3Fixed};
pub use mesh3d::{parse_glb, MechaModel3D, PartMesh, Vertex3D};
pub use modding::{
    AxmodPackageInfo, ModError, ModPublishResult, ModValidationReport, SteamUgcConfig,
    UgcVisibility, WorkshopManager, WorkshopModManifest, WorkshopModPackage, WorkshopPublisher,
    AXMOD_MAGIC, AXMOD_VERSION,
};
pub use newspaper::{
    apply_newsprint_filter, draw_newspaper_photo_frame, NewspaperGravureConfig, PhotoFilter,
};
pub use particles3d::{Particle3D, ParticleSystem3D};
pub use raster3d::{CelShadingPalette, Rasterizer3D};
pub use sprites::*;
pub use texture::{blend_decal, modulate_color, TextureMap, TextureWrapMode};
