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

// ---------------------------------------------------------------------------
// Steam Workshop & Local Mod Directory Manager
// ---------------------------------------------------------------------------

use std::fs;
use std::path::{Path, PathBuf};

/// High-level Steam Workshop and Local Mod Ingestion Manager.
#[derive(Debug, Clone)]
pub struct WorkshopManager {
    pub mods_dir: PathBuf,
    pub installed_mods: Vec<WorkshopModPackage>,
    pub active_mod_ids: Vec<String>,
}

impl WorkshopManager {
    /// Creates a new WorkshopManager pointing to a mods directory.
    pub fn new<P: Into<PathBuf>>(mods_dir: P) -> Self {
        let dir = mods_dir.into();
        Self {
            mods_dir: dir,
            installed_mods: Vec::new(),
            active_mod_ids: Vec::new(),
        }
    }

    /// Scans the mods directory, discovering all valid subdirectories with `mod.json` or `manifest.json`.
    pub fn scan_mods(&mut self) -> Result<usize, ModError> {
        if !self.mods_dir.exists() {
            let _ = fs::create_dir_all(&self.mods_dir);
            return Ok(0);
        }

        let entries = fs::read_dir(&self.mods_dir)
            .map_err(|e| ModError::InvalidManifest(format!("Failed to read mods dir: {}", e)))?;

        let mut discovered = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = if path.join("mod.json").exists() {
                    path.join("mod.json")
                } else if path.join("manifest.json").exists() {
                    path.join("manifest.json")
                } else {
                    continue;
                };

                let manifest_str = match fs::read_to_string(&manifest_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let manifest: WorkshopModManifest = match serde_json::from_str(&manifest_str) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                // Look for accompanying .glb file in the same mod directory
                let mut glb_mesh = None;
                if let Ok(files) = fs::read_dir(&path) {
                    for f in files.flatten() {
                        let fp = f.path();
                        if fp.extension().map_or(false, |ext| ext == "glb") {
                            if let Ok(bytes) = fs::read(&fp) {
                                if let Ok(model) = parse_glb(&bytes) {
                                    if let Some(part) = model.parts.into_iter().find(|p| p.name == manifest.target_mesh_name) {
                                        glb_mesh = Some(part);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // If no GLB was found or matched, generate a procedural reinforced part mesh
                let part_mesh = glb_mesh.unwrap_or_else(|| {
                    PartMesh::create_box(
                        &manifest.target_mesh_name,
                        crate::math3d::Vec3Fixed::from_f32(-0.2, -0.2, -0.2),
                        crate::math3d::Vec3Fixed::from_f32(0.2, 0.2, 0.2),
                    )
                });

                let texture = Some(TextureMap::procedural_hazard_stripes(64));

                let pkg = WorkshopModPackage::from_mesh(manifest.clone(), part_mesh, texture);
                discovered.push(pkg);
            }
        }

        self.installed_mods = discovered;
        if self.active_mod_ids.is_empty() {
            self.active_mod_ids = self.installed_mods.iter().map(|m| m.manifest.mod_id.clone()).collect();
        }

        Ok(self.installed_mods.len())
    }

    /// Toggles activation state for a specific mod.
    pub fn toggle_mod(&mut self, mod_id: &str) -> bool {
        if let Some(idx) = self.active_mod_ids.iter().position(|id| id == mod_id) {
            self.active_mod_ids.remove(idx);
            false
        } else {
            self.active_mod_ids.push(mod_id.to_string());
            true
        }
    }

    /// Applies all active mods to the given mecha model.
    pub fn apply_active_mods(&self, model: &mut MechaModel3D) -> usize {
        let mut count = 0;
        for pkg in &self.installed_mods {
            if self.active_mod_ids.contains(&pkg.manifest.mod_id) {
                if pkg.inject_into_model(model) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Hot-swaps all active mods into the target model in real-time.
    pub fn hot_swap_reload(&mut self, model: &mut MechaModel3D) -> Result<usize, ModError> {
        self.scan_mods()?;
        Ok(self.apply_active_mods(model))
    }

    /// Creates a template mod package on disk for creators to inspect and edit.
    pub fn create_template_mod(target_dir: &Path, manifest: &WorkshopModManifest) -> Result<PathBuf, std::io::Error> {
        let mod_dir = target_dir.join(&manifest.mod_id);
        fs::create_dir_all(&mod_dir)?;

        let json_path = mod_dir.join("mod.json");
        let json_data = serde_json::to_string_pretty(manifest).unwrap_or_default();
        fs::write(&json_path, json_data)?;

        let tex = TextureMap::procedural_hazard_stripes(64);
        let raw_tex_path = mod_dir.join("hazard_plate.raw");
        let mut raw_bytes = Vec::with_capacity(tex.pixels.len() * 4);
        for &px in &tex.pixels {
            raw_bytes.extend_from_slice(&px.to_le_bytes());
        }
        fs::write(raw_tex_path, raw_bytes)?;

        Ok(mod_dir)
    }
}
