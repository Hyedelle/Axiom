//! Steam Workshop & User Modding Dynamic Ingestion Pipeline.
//!
//! Allows external mecha part modifications (GLB meshes, custom texture skins,
//! stat overrides) to be ingested dynamically into the game engine at runtime.

use crate::mesh3d::{parse_glb, MechaModel3D, PartMesh};
use crate::texture::TextureMap;
use serde::{Deserialize, Serialize};

/// Error types occurring during mod package ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModError {
    InvalidManifest(String),
    GlbParseFailed(String),
    PartNotFound(String),
    InvalidTextureDimensions,
}

impl std::fmt::Display for ModError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(msg) => write!(f, "Invalid mod manifest: {}", msg),
            Self::GlbParseFailed(msg) => write!(f, "Failed to parse mod GLB: {}", msg),
            Self::PartNotFound(msg) => write!(f, "Target part not found: {}", msg),
            Self::InvalidTextureDimensions => write!(f, "Texture dimensions must be power of two"),
        }
    }
}

impl std::error::Error for ModError {}

/// Mod manifest metadata adhering to Steam Workshop specifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkshopModManifest {
    pub mod_id: String,
    pub title: String,
    pub author: String,
    pub version: String,
    pub target_slot: String,
    pub target_mesh_name: String,
    pub atk_bonus: i32,
    pub def_bonus: i32,
    pub heat_bonus: i32,
    pub weight_bonus: i32,
    pub tags: Vec<String>,
}

impl Default for WorkshopModManifest {
    fn default() -> Self {
        Self {
            mod_id: "mod_custom_plating".to_string(),
            title: "Reinforced Hazard Plating".to_string(),
            author: "FoundryModder".to_string(),
            version: "1.0.0".to_string(),
            target_slot: "Arm".to_string(),
            target_mesh_name: "hero_glove_R".to_string(),
            atk_bonus: 8,
            def_bonus: 4,
            heat_bonus: -2,
            weight_bonus: 5,
            tags: vec!["Armor".to_string(), "Reinforced".to_string()],
        }
    }
}

/// Fully parsed and ingested Workshop Mod Part Package.
#[derive(Debug, Clone)]
pub struct WorkshopModPackage {
    pub manifest: WorkshopModManifest,
    pub mesh: PartMesh,
    pub texture: Option<TextureMap>,
}

impl WorkshopModPackage {
    /// Creates a mod package from a JSON manifest string and binary GLB bytes.
    pub fn from_manifest_and_glb(
        manifest_json: &str,
        glb_bytes: &[u8],
        texture: Option<TextureMap>,
    ) -> Result<Self, ModError> {
        let manifest: WorkshopModManifest = serde_json::from_str(manifest_json)
            .map_err(|e| ModError::InvalidManifest(e.to_string()))?;

        let model = parse_glb(glb_bytes)
            .map_err(|e| ModError::GlbParseFailed(e.to_string()))?;

        // Find matching part in the parsed GLB model, or take the first part
        let mesh = model
            .parts
            .into_iter()
            .find(|p| p.name == manifest.target_mesh_name)
            .or_else(|| {
                // If exact name not matched, take the first available non-empty part
                // and rename it to the target mesh name.
                None
            })
            .ok_or_else(|| {
                ModError::PartNotFound(format!(
                    "GLB does not contain part '{}'",
                    manifest.target_mesh_name
                ))
            })?;

        Ok(Self {
            manifest,
            mesh,
            texture,
        })
    }

    /// Ingests a mod directly from an existing `PartMesh` and manifest.
    pub fn from_mesh(
        manifest: WorkshopModManifest,
        mesh: PartMesh,
        texture: Option<TextureMap>,
    ) -> Self {
        Self {
            manifest,
            mesh,
            texture,
        }
    }

    /// Dynamically injects this mod part into a target `MechaModel3D`,
    /// replacing the existing part with matching mesh name.
    /// Returns true if a part was substituted, false if appended as a new part.
    pub fn inject_into_model(&self, model: &mut MechaModel3D) -> bool {
        let mut replaced = false;
        for part in &mut model.parts {
            if part.name == self.manifest.target_mesh_name {
                *part = self.mesh.clone();
                replaced = true;
                break;
            }
        }

        if !replaced {
            let mut new_part = self.mesh.clone();
            new_part.name = self.manifest.target_mesh_name.clone();
            model.parts.push(new_part);
        }

        replaced
    }

    /// Removes this mod part from the target model, restoring a fallback mesh.
    pub fn restore_original_in_model(
        &self,
        model: &mut MechaModel3D,
        fallback: PartMesh,
    ) -> bool {
        for part in &mut model.parts {
            if part.name == self.manifest.target_mesh_name {
                *part = fallback;
                return true;
            }
        }
        false
    }
}
