//! 3D Mesh and glTF 2.0 Binary (GLB) Ingestion Pipeline.
//!
//! Direct ingestion for Blender-exported mecha assets (such as `hero_a.glb`),
//! splitting rigid parts into modular nodes without engine bloat or memory overhead.

use crate::math3d::Vec3Fixed;
use serde::Deserialize;

/// 3D Vertex with fixed-point position and normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vertex3D {
    pub pos: Vec3Fixed,
    pub norm: Vec3Fixed,
    pub u: i32, // Fixed-point texture coordinate (0..65536)
    pub v: i32,
}

/// A rigid mecha part mesh (e.g., `hero_torso`, `hero_glove_R`).
#[derive(Debug, Clone)]
pub struct PartMesh {
    pub name: String,
    pub vertices: Vec<Vertex3D>,
    pub indices: Vec<u32>,
}

impl PartMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// A complete 3D mecha model composed of discrete rigid parts.
#[derive(Debug, Clone, Default)]
pub struct MechaModel3D {
    pub parts: Vec<PartMesh>,
}

impl MechaModel3D {
    pub fn find_part(&self, name: &str) -> Option<&PartMesh> {
        self.parts.iter().find(|p| p.name == name)
    }

    pub fn total_triangles(&self) -> usize {
        self.parts.iter().map(|p| p.triangle_count()).sum()
    }

    pub fn total_vertices(&self) -> usize {
        self.parts.iter().map(|p| p.vertices.len()).sum()
    }
}

// Minimal glTF 2.0 JSON schema deserializers
#[derive(Deserialize)]
struct GltfRoot {
    #[serde(default)]
    nodes: Vec<GltfNode>,
    #[serde(default)]
    meshes: Vec<GltfMesh>,
    #[serde(default, rename = "bufferViews")]
    buffer_views: Vec<GltfBufferView>,
    #[serde(default)]
    accessors: Vec<GltfAccessor>,
}

#[derive(Deserialize)]
struct GltfNode {
    #[serde(default)]
    name: Option<String>,
    mesh: Option<usize>,
    #[serde(default)]
    translation: Option<[f32; 3]>,
    #[serde(default)]
    children: Option<Vec<usize>>,
}

#[derive(Deserialize)]
struct GltfMesh {
    #[serde(default)]
    name: Option<String>,
    primitives: Vec<GltfPrimitive>,
}

#[derive(Deserialize)]
struct GltfPrimitive {
    attributes: std::collections::HashMap<String, usize>,
    indices: Option<usize>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GltfBufferView {
    #[serde(default, rename = "byteOffset")]
    byte_offset: usize,
    #[serde(default, rename = "byteLength")]
    byte_length: usize,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GltfAccessor {
    #[serde(rename = "bufferView")]
    buffer_view: Option<usize>,
    #[serde(default, rename = "byteOffset")]
    byte_offset: usize,
    #[serde(rename = "componentType")]
    component_type: u32,
    count: usize,
    #[serde(rename = "type")]
    type_str: String,
}

/// Parses a GLB (glTF 2.0 Binary) byte slice into a `MechaModel3D`.
pub fn parse_glb(bytes: &[u8]) -> Result<MechaModel3D, String> {
    if bytes.len() < 12 {
        return Err("GLB file too small".into());
    }

    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != 0x46546C67 {
        return Err(format!("Invalid GLB magic: 0x{:08X}", magic));
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != 2 {
        return Err(format!("Unsupported GLB version: {}", version));
    }

    // Chunk 0: JSON
    let mut offset = 12;
    if offset + 8 > bytes.len() {
        return Err("Unexpected EOF reading Chunk 0 header".into());
    }
    let chunk0_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let chunk0_type = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
    offset += 8;
    if chunk0_type != 0x4E4F534A {
        return Err("Chunk 0 is not JSON".into());
    }
    if offset + chunk0_len > bytes.len() {
        return Err("Chunk 0 data out of bounds".into());
    }
    let json_bytes = &bytes[offset..offset + chunk0_len];
    offset += chunk0_len;

    // Chunk 1: BIN
    if offset + 8 > bytes.len() {
        return Err("Unexpected EOF reading Chunk 1 header".into());
    }
    let chunk1_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let chunk1_type = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
    offset += 8;
    if chunk1_type != 0x004E4942 {
        return Err("Chunk 1 is not BIN".into());
    }
    if offset + chunk1_len > bytes.len() {
        return Err("Chunk 1 data out of bounds".into());
    }
    let bin_bytes = &bytes[offset..offset + chunk1_len];

    // Deserialize glTF JSON
    let root: GltfRoot = serde_json::from_slice(json_bytes)
        .map_err(|e| format!("Failed to parse glTF JSON: {}", e))?;

    let mut model = MechaModel3D::default();

    // Build parent-child map to accumulate rest translations down the node hierarchy
    let mut parent_map = vec![None; root.nodes.len()];
    for (i, node) in root.nodes.iter().enumerate() {
        if let Some(children) = &node.children {
            for &child in children {
                if child < root.nodes.len() {
                    parent_map[child] = Some(i);
                }
            }
        }
    }

    let mut node_translations: Vec<[f32; 3]> = Vec::with_capacity(root.nodes.len());
    for i in 0..root.nodes.len() {
        let mut t = root.nodes[i].translation.unwrap_or([0.0, 0.0, 0.0]);
        let mut curr = i;
        while let Some(parent) = parent_map[curr] {
            let pt = root.nodes[parent].translation.unwrap_or([0.0, 0.0, 0.0]);
            t[0] += pt[0];
            t[1] += pt[1];
            t[2] += pt[2];
            curr = parent;
        }
        node_translations.push(t);
    }

    // Iterate through nodes to preserve Blender part names and rest positions
    for (node_idx, node) in root.nodes.iter().enumerate() {
        let mesh_idx = match node.mesh {
            Some(idx) if idx < root.meshes.len() => idx,
            _ => continue,
        };
        let node_t = node_translations[node_idx];

        let mesh = &root.meshes[mesh_idx];
        let part_name = node
            .name
            .clone()
            .or_else(|| mesh.name.clone())
            .unwrap_or_else(|| format!("part_{}", mesh_idx));

        for prim in &mesh.primitives {
            let pos_accessor_idx = match prim.attributes.get("POSITION") {
                Some(&idx) if idx < root.accessors.len() => idx,
                _ => continue,
            };

            let pos_acc = &root.accessors[pos_accessor_idx];
            let pos_bv_idx = match pos_acc.buffer_view {
                Some(idx) if idx < root.buffer_views.len() => idx,
                _ => continue,
            };
            let pos_bv = &root.buffer_views[pos_bv_idx];
            let pos_offset = pos_bv.byte_offset + pos_acc.byte_offset;

            // Optional Normals
            let norm_data = prim.attributes.get("NORMAL").and_then(|&idx| {
                if idx < root.accessors.len() {
                    let acc = &root.accessors[idx];
                    acc.buffer_view.and_then(|bv_idx| {
                        if bv_idx < root.buffer_views.len() {
                            let bv = &root.buffer_views[bv_idx];
                            Some((bv.byte_offset + acc.byte_offset, acc.count))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            });

            // Optional UVs
            let uv_data = prim.attributes.get("TEXCOORD_0").and_then(|&idx| {
                if idx < root.accessors.len() {
                    let acc = &root.accessors[idx];
                    acc.buffer_view.and_then(|bv_idx| {
                        if bv_idx < root.buffer_views.len() {
                            let bv = &root.buffer_views[bv_idx];
                            Some((bv.byte_offset + acc.byte_offset, acc.count))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            });

            let mut vertices = Vec::with_capacity(pos_acc.count);
            for i in 0..pos_acc.count {
                // Read float position [x, y, z]
                let p_off = pos_offset + i * 12;
                if p_off + 12 > bin_bytes.len() {
                    break;
                }
                let px = f32::from_le_bytes(bin_bytes[p_off..p_off + 4].try_into().unwrap()) + node_t[0];
                let py = f32::from_le_bytes(bin_bytes[p_off + 4..p_off + 8].try_into().unwrap()) + node_t[1];
                let pz = f32::from_le_bytes(bin_bytes[p_off + 8..p_off + 12].try_into().unwrap()) + node_t[2];

                // Read normal if available
                let norm = if let Some((n_off_base, count)) = norm_data {
                    if i < count && n_off_base + (i + 1) * 12 <= bin_bytes.len() {
                        let off = n_off_base + i * 12;
                        let nx = f32::from_le_bytes(bin_bytes[off..off + 4].try_into().unwrap());
                        let ny = f32::from_le_bytes(bin_bytes[off + 4..off + 8].try_into().unwrap());
                        let nz = f32::from_le_bytes(bin_bytes[off + 8..off + 12].try_into().unwrap());
                        Vec3Fixed::from_f32(nx, ny, nz)
                    } else {
                        Vec3Fixed::UP
                    }
                } else {
                    Vec3Fixed::UP
                };

                // Read UV if available
                let (u, v) = if let Some((uv_off_base, count)) = uv_data {
                    if i < count && uv_off_base + (i + 1) * 8 <= bin_bytes.len() {
                        let off = uv_off_base + i * 8;
                        let tu = f32::from_le_bytes(bin_bytes[off..off + 4].try_into().unwrap());
                        let tv = f32::from_le_bytes(bin_bytes[off + 4..off + 8].try_into().unwrap());
                        ((tu * 65536.0) as i32, (tv * 65536.0) as i32)
                    } else {
                        (0, 0)
                    }
                } else {
                    (0, 0)
                };

                vertices.push(Vertex3D {
                    pos: Vec3Fixed::from_f32(px, py, pz),
                    norm,
                    u,
                    v,
                });
            }

            // Read Indices
            let mut indices = Vec::new();
            if let Some(idx_accessor_idx) = prim.indices {
                if idx_accessor_idx < root.accessors.len() {
                    let idx_acc = &root.accessors[idx_accessor_idx];
                    if let Some(idx_bv_idx) = idx_acc.buffer_view {
                        if idx_bv_idx < root.buffer_views.len() {
                            let idx_bv = &root.buffer_views[idx_bv_idx];
                            let off_base = idx_bv.byte_offset + idx_acc.byte_offset;

                            indices.reserve(idx_acc.count);
                            match idx_acc.component_type {
                                5123 => {
                                    // UNSIGNED_SHORT (u16)
                                    for i in 0..idx_acc.count {
                                        let off = off_base + i * 2;
                                        if off + 2 <= bin_bytes.len() {
                                            let val = u16::from_le_bytes(
                                                bin_bytes[off..off + 2].try_into().unwrap(),
                                            );
                                            indices.push(val as u32);
                                        }
                                    }
                                }
                                5125 => {
                                    // UNSIGNED_INT (u32)
                                    for i in 0..idx_acc.count {
                                        let off = off_base + i * 4;
                                        if off + 4 <= bin_bytes.len() {
                                            let val = u32::from_le_bytes(
                                                bin_bytes[off..off + 4].try_into().unwrap(),
                                            );
                                            indices.push(val);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            } else {
                // Non-indexed primitives: sequential
                indices = (0..vertices.len() as u32).collect();
            }

            model.parts.push(PartMesh {
                name: part_name.clone(),
                vertices,
                indices,
            });
        }
    }

    Ok(model)
}
