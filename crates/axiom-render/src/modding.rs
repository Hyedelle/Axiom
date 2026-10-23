//! Steam Workshop & User Modding Dynamic Ingestion Pipeline.
//!
//! Allows external mecha part modifications (GLB meshes, custom texture skins,
//! stat overrides) to be ingested dynamically into the game engine at runtime.

use crate::mesh3d::{parse_glb, MechaModel3D, PartMesh};
use crate::texture::TextureMap;
use serde::{Deserialize, Serialize};

/// Error types occurring during mod package ingestion or publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModError {
    InvalidManifest(String),
    GlbParseFailed(String),
    PartNotFound(String),
    InvalidTextureDimensions,
    ValidationFailed(String),
    PackagingFailed(String),
    IoError(String),
}

impl std::fmt::Display for ModError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(msg) => write!(f, "Invalid mod manifest: {}", msg),
            Self::GlbParseFailed(msg) => write!(f, "Failed to parse mod GLB: {}", msg),
            Self::PartNotFound(msg) => write!(f, "Target part not found: {}", msg),
            Self::InvalidTextureDimensions => write!(f, "Texture dimensions must be power of two"),
            Self::ValidationFailed(msg) => write!(f, "Mod validation failed: {}", msg),
            Self::PackagingFailed(msg) => write!(f, "Mod packaging failed: {}", msg),
            Self::IoError(msg) => write!(f, "I/O error during mod processing: {}", msg),
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

// ---------------------------------------------------------------------------
// Steam Workshop UGC Cloud Publisher & Standalone .axmod Packager
// ---------------------------------------------------------------------------

pub const AXMOD_MAGIC: [u8; 4] = *b"AXMD";
pub const AXMOD_VERSION: u32 = 1;

/// Steam Workshop UGC upload visibility setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UgcVisibility {
    Public = 0,
    FriendsOnly = 1,
    Private = 2,
    Unlisted = 3,
}

impl UgcVisibility {
    pub const fn as_vdf_val(&self) -> &'static str {
        match self {
            Self::Public => "0",
            Self::FriendsOnly => "1",
            Self::Private => "2",
            Self::Unlisted => "3",
        }
    }
}

/// Configuration for Steam Workshop UGC publication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SteamUgcConfig {
    pub app_id: u32,
    pub published_file_id: u64, // 0 for brand new item, existing ID for update
    pub title: String,
    pub description: String,
    pub changelog: String,
    pub visibility: UgcVisibility,
    pub tags: Vec<String>,
}

impl Default for SteamUgcConfig {
    fn default() -> Self {
        Self {
            app_id: 3245670, // Mimic The Show / Axiom Steam AppId
            published_file_id: 0,
            title: "Custom Workshop Mecha Mod".to_string(),
            description: "High-performance custom mecha boxing modification for Mimic The Show."
                .to_string(),
            changelog: "Initial release via Axiom Workshop Publisher.".to_string(),
            visibility: UgcVisibility::Public,
            tags: vec!["Mecha".to_string(), "Custom Part".to_string()],
        }
    }
}

/// Detailed validation report for a mod directory before publishing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModValidationReport {
    pub mod_id: String,
    pub title: String,
    pub version: String,
    pub target_slot: String,
    pub is_valid: bool,
    pub has_manifest: bool,
    pub has_mesh: bool,
    pub has_texture: bool,
    pub has_preview: bool,
    pub total_asset_files: usize,
    pub total_bytes: u64,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Result of an .axmod packaging operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AxmodPackageInfo {
    pub mod_id: String,
    pub file_count: usize,
    pub uncompressed_size: u64,
    pub package_size: u64,
    pub checksum: u64,
}

/// Result of a Workshop publication dry-run or submission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModPublishResult {
    pub mod_id: String,
    pub app_id: u32,
    pub published_file_id: u64,
    pub vdf_path: PathBuf,
    pub axmod_path: Option<PathBuf>,
    pub ready_for_steam: bool,
    pub summary: String,
}

/// Steam Workshop Cloud UGC Publisher & Standalone Container Engine.
pub struct WorkshopPublisher;

impl WorkshopPublisher {
    /// Validates a local mod directory against Steam Workshop & Axiom Engine specifications.
    pub fn validate_mod(mod_dir: &Path) -> Result<ModValidationReport, ModError> {
        if !mod_dir.exists() || !mod_dir.is_dir() {
            return Err(ModError::ValidationFailed(format!(
                "Directory does not exist: {:?}",
                mod_dir
            )));
        }

        let manifest_path = if mod_dir.join("mod.json").exists() {
            mod_dir.join("mod.json")
        } else if mod_dir.join("manifest.json").exists() {
            mod_dir.join("manifest.json")
        } else {
            return Ok(ModValidationReport {
                mod_id: "unknown".to_string(),
                title: "unknown".to_string(),
                version: "0.0.0".to_string(),
                target_slot: "none".to_string(),
                is_valid: false,
                has_manifest: false,
                has_mesh: false,
                has_texture: false,
                has_preview: false,
                total_asset_files: 0,
                total_bytes: 0,
                errors: vec!["Missing mod.json or manifest.json in root directory".to_string()],
                warnings: Vec::new(),
            });
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        let manifest_data = fs::read_to_string(&manifest_path)
            .map_err(|e| ModError::IoError(format!("Failed to read manifest: {}", e)))?;

        let manifest: WorkshopModManifest = match serde_json::from_str(&manifest_data) {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("Corrupted JSON manifest: {}", e));
                return Ok(ModValidationReport {
                    mod_id: "corrupted".to_string(),
                    title: "corrupted".to_string(),
                    version: "0.0.0".to_string(),
                    target_slot: "none".to_string(),
                    is_valid: false,
                    has_manifest: true,
                    has_mesh: false,
                    has_texture: false,
                    has_preview: false,
                    total_asset_files: 0,
                    total_bytes: 0,
                    errors,
                    warnings,
                });
            }
        };

        // Semantic validation of manifest fields
        if manifest.mod_id.trim().is_empty() {
            errors.push("mod_id cannot be empty".to_string());
        }
        if manifest.title.trim().is_empty() {
            errors.push("title cannot be empty".to_string());
        }
        if manifest.author.trim().is_empty() {
            warnings.push("author is empty, default will be used".to_string());
        }
        let valid_slots = ["Head", "Core", "Arm", "Leg"];
        if !valid_slots.contains(&manifest.target_slot.as_str()) {
            errors.push(format!(
                "Invalid target_slot '{}', must be one of: Head, Core, Arm, Leg",
                manifest.target_slot
            ));
        }

        // Scan asset files
        let mut total_asset_files = 0;
        let mut total_bytes: u64 = 0;
        let mut has_mesh = false;
        let mut has_texture = false;
        let mut has_preview = false;

        if let Ok(entries) = fs::read_dir(mod_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    total_asset_files += 1;
                    if let Ok(meta) = entry.metadata() {
                        total_bytes += meta.len();
                    }

                    let file_name = p.file_name().unwrap_or_default().to_string_lossy();
                    let ext = p
                        .extension()
                        .map(|s| s.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();

                    if ext == "glb" {
                        has_mesh = true;
                        // Check glTF binary header magic
                        if let Ok(bytes) = fs::read(&p) {
                            if bytes.len() >= 12 {
                                let magic = &bytes[0..4];
                                if magic != b"glTF" {
                                    errors.push(format!(
                                        "Mesh file '{}' is not a valid binary glTF (invalid magic)",
                                        file_name
                                    ));
                                }
                            } else {
                                errors.push(format!(
                                    "Mesh file '{}' is truncated (<12 bytes)",
                                    file_name
                                ));
                            }
                        }
                    } else if ext == "raw" || ext == "png" {
                        if file_name.starts_with("preview") {
                            has_preview = true;
                        } else {
                            has_texture = true;
                        }
                    }
                }
            }
        }

        if !has_mesh && !has_texture {
            warnings.push("No .glb mesh or .raw/.png texture found (mod contains manifest only)".to_string());
        }

        if !has_preview {
            warnings.push("No preview.png or preview.raw found; Steam Workshop will display default icon".to_string());
        }

        if total_bytes > 50 * 1024 * 1024 {
            errors.push(format!(
                "Total mod package size ({} bytes) exceeds 50MB limit",
                total_bytes
            ));
        }

        let is_valid = errors.is_empty();

        Ok(ModValidationReport {
            mod_id: manifest.mod_id,
            title: manifest.title,
            version: manifest.version,
            target_slot: manifest.target_slot,
            is_valid,
            has_manifest: true,
            has_mesh,
            has_texture,
            has_preview,
            total_asset_files,
            total_bytes,
            errors,
            warnings,
        })
    }

    /// Generates Valve VDF configuration file for SteamCMD UGC publishing.
    pub fn generate_steam_vdf(
        mod_dir: &Path,
        config: &SteamUgcConfig,
    ) -> Result<String, ModError> {
        let abs_mod_dir = fs::canonicalize(mod_dir)
            .unwrap_or_else(|_| mod_dir.to_path_buf())
            .to_string_lossy()
            .replace('/', "\\");

        let preview_path = if mod_dir.join("preview.png").exists() {
            fs::canonicalize(mod_dir.join("preview.png"))
                .unwrap_or_else(|_| mod_dir.join("preview.png"))
                .to_string_lossy()
                .replace('/', "\\")
        } else if mod_dir.join("preview.raw").exists() {
            fs::canonicalize(mod_dir.join("preview.raw"))
                .unwrap_or_else(|_| mod_dir.join("preview.raw"))
                .to_string_lossy()
                .replace('/', "\\")
        } else {
            String::new()
        };

        let mut vdf = String::with_capacity(512);
        vdf.push_str("\"workshopitem\"\n{\n");
        vdf.push_str(&format!("  \"appid\" \"{}\"\n", config.app_id));
        vdf.push_str(&format!(
            "  \"publishedfileid\" \"{}\"\n",
            config.published_file_id
        ));
        vdf.push_str(&format!("  \"contentfolder\" \"{}\"\n", abs_mod_dir));
        if !preview_path.is_empty() {
            vdf.push_str(&format!("  \"previewfile\" \"{}\"\n", preview_path));
        }
        vdf.push_str(&format!(
            "  \"visibility\" \"{}\"\n",
            config.visibility.as_vdf_val()
        ));
        vdf.push_str(&format!("  \"title\" \"{}\"\n", config.title));
        vdf.push_str(&format!("  \"description\" \"{}\"\n", config.description));
        vdf.push_str(&format!("  \"changenote\" \"{}\"\n", config.changelog));
        vdf.push_str("}\n");

        Ok(vdf)
    }

    /// Packs the entire mod directory into an ultra-compact standalone `.axmod` binary archive.
    pub fn pack_axmod(mod_dir: &Path, out_file: &Path) -> Result<AxmodPackageInfo, ModError> {
        let report = Self::validate_mod(mod_dir)?;
        if !report.is_valid {
            return Err(ModError::PackagingFailed(format!(
                "Mod validation failed with {} errors: {:?}",
                report.errors.len(),
                report.errors
            )));
        }

        let manifest_path = if mod_dir.join("mod.json").exists() {
            mod_dir.join("mod.json")
        } else {
            mod_dir.join("manifest.json")
        };

        let manifest_str = fs::read_to_string(&manifest_path)
            .map_err(|e| ModError::IoError(e.to_string()))?;

        // Collect all files in mod_dir
        let mut files_data = Vec::new();
        let mut uncompressed_size: u64 = 0;

        if let Ok(entries) = fs::read_dir(mod_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    let rel_name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let bytes = fs::read(&p).map_err(|e| ModError::IoError(e.to_string()))?;
                    uncompressed_size += bytes.len() as u64;
                    files_data.push((rel_name, bytes));
                }
            }
        }

        // Build binary buffer
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&AXMOD_MAGIC);
        buffer.extend_from_slice(&AXMOD_VERSION.to_le_bytes());
        buffer.extend_from_slice(&(files_data.len() as u32).to_le_bytes());

        // Manifest JSON
        let manifest_bytes = manifest_str.as_bytes();
        buffer.extend_from_slice(&(manifest_bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(manifest_bytes);

        // Files table
        for (name, bytes) in &files_data {
            let name_bytes = name.as_bytes();
            buffer.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            buffer.extend_from_slice(name_bytes);
            buffer.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buffer.extend_from_slice(bytes);

            // 64-bit FNV-1a checksum of the file
            let mut hash: u64 = 0xCBF29CE484222325;
            for &b in bytes {
                hash ^= b as u64;
                hash = hash.wrapping_mul(0x100000001B3);
            }
            buffer.extend_from_slice(&hash.to_le_bytes());
        }

        // Overall checksum
        let mut total_checksum: u64 = 0xCBF29CE484222325;
        for &b in &buffer {
            total_checksum ^= b as u64;
            total_checksum = total_checksum.wrapping_mul(0x100000001B3);
        }

        if let Some(parent) = out_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(out_file, &buffer)
            .map_err(|e| ModError::IoError(format!("Failed to write axmod file: {}", e)))?;

        Ok(AxmodPackageInfo {
            mod_id: report.mod_id,
            file_count: files_data.len(),
            uncompressed_size,
            package_size: buffer.len() as u64,
            checksum: total_checksum,
        })
    }

    /// Unpacks an `.axmod` archive into a destination directory.
    pub fn unpack_axmod(axmod_file: &Path, target_dir: &Path) -> Result<usize, ModError> {
        let bytes = fs::read(axmod_file)
            .map_err(|e| ModError::IoError(format!("Failed to read axmod: {}", e)))?;

        if bytes.len() < 16 {
            return Err(ModError::PackagingFailed("Axmod file is truncated (<16 bytes)".to_string()));
        }

        if &bytes[0..4] != &AXMOD_MAGIC {
            return Err(ModError::PackagingFailed("Invalid axmod magic header".to_string()));
        }

        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != AXMOD_VERSION {
            return Err(ModError::PackagingFailed(format!(
                "Unsupported axmod version: {}",
                version
            )));
        }

        let file_count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let manifest_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;

        let mut offset = 16;
        if bytes.len() < offset + manifest_len {
            return Err(ModError::PackagingFailed("Truncated manifest in axmod".to_string()));
        }

        let manifest_bytes = &bytes[offset..offset + manifest_len];
        offset += manifest_len;

        fs::create_dir_all(target_dir)
            .map_err(|e| ModError::IoError(e.to_string()))?;

        // Write mod.json
        fs::write(target_dir.join("mod.json"), manifest_bytes)
            .map_err(|e| ModError::IoError(e.to_string()))?;

        let mut extracted_files = 1;

        for _ in 0..file_count {
            if offset + 2 > bytes.len() {
                break;
            }
            let name_len = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as usize;
            offset += 2;

            if offset + name_len + 4 > bytes.len() {
                return Err(ModError::PackagingFailed("Truncated file header in axmod".to_string()));
            }

            let name_str = String::from_utf8_lossy(&bytes[offset..offset + name_len]).to_string();
            offset += name_len;

            let file_size = u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + file_size + 8 > bytes.len() {
                return Err(ModError::PackagingFailed("Truncated file payload in axmod".to_string()));
            }

            let file_bytes = &bytes[offset..offset + file_size];
            offset += file_size;

            let _expected_hash = u64::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]);
            offset += 8;

            let dest_path = target_dir.join(&name_str);
            if let Some(parent) = dest_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&dest_path, file_bytes)
                .map_err(|e| ModError::IoError(e.to_string()))?;
            extracted_files += 1;
        }

        Ok(extracted_files)
    }

    /// Performs complete dry-run validation and prepares Steam VDF configuration.
    pub fn publish_dry_run(
        mod_dir: &Path,
        config: &SteamUgcConfig,
    ) -> Result<ModPublishResult, ModError> {
        let report = Self::validate_mod(mod_dir)?;
        if !report.is_valid {
            return Err(ModError::ValidationFailed(format!(
                "Dry-run aborted: {} errors encountered: {:?}",
                report.errors.len(),
                report.errors
            )));
        }

        let vdf_content = Self::generate_steam_vdf(mod_dir, config)?;
        let vdf_path = mod_dir.join("steam_workshop.vdf");
        fs::write(&vdf_path, &vdf_content)
            .map_err(|e| ModError::IoError(format!("Failed to write steam_workshop.vdf: {}", e)))?;

        // Optional .axmod packaging into parent or mod dir
        let axmod_name = format!("{}.axmod", report.mod_id);
        let axmod_path = mod_dir.join(&axmod_name);
        let _pkg_info = Self::pack_axmod(mod_dir, &axmod_path)?;

        let summary = format!(
            "Mod '{}' validated successfully. Ready for Steam UGC publish (AppId: {}, VDF: {:?}, Assets: {} files, Total: {} bytes)",
            report.mod_id, config.app_id, vdf_path, report.total_asset_files, report.total_bytes
        );

        Ok(ModPublishResult {
            mod_id: report.mod_id,
            app_id: config.app_id,
            published_file_id: config.published_file_id,
            vdf_path,
            axmod_path: Some(axmod_path),
            ready_for_steam: true,
            summary,
        })
    }
}
