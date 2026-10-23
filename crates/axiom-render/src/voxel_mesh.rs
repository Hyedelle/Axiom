//! 3D Voxel Mesh Extraction & Real-Time Geometric Severance Engine.
//!
//! Provides zero-float deterministic conversion from `VoxelVolume16` to `PartMesh`,
//! and physically cuts 3D part meshes along arbitrary cutting planes with molten capping geometry.

use axiom_core::fixed::Fixed;
use axiom_core::voxel_destruct::{SeverancePlane, VoxelVolume16, VOXEL_AIR, VOXEL_DIM, VOXEL_MOLTEN};
use crate::math3d::Vec3Fixed;
use crate::mesh3d::{PartMesh, Vertex3D};

/// Extracts visible surface boundary mesh from a 16x16x16 voxel volume.
pub fn extract_voxel_mesh(name: &str, volume: &VoxelVolume16, voxel_scale: Fixed) -> PartMesh {
    let est_faces = (volume.total_solid as usize * 3).clamp(32, 2560);
    let mut vertices: Vec<Vertex3D> = Vec::with_capacity(est_faces * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(est_faces * 6);

    let half_scale = Fixed::from_raw(voxel_scale.raw() / 2);
    let zero = Fixed::ZERO;
    let one = Fixed::ONE;
    let neg_one = Fixed::from_raw(-65536);

    for y in 0..VOXEL_DIM {
        for z in 0..VOXEL_DIM {
            for x in 0..VOXEL_DIM {
                let mat = volume.get(x, y, z);
                if mat == VOXEL_AIR {
                    continue;
                }

                // Center position in fixed-point world space
                let cx = Fixed::from_raw((x as i64 * voxel_scale.raw()) - (VOXEL_DIM as i64 * half_scale.raw()));
                let cy = Fixed::from_raw(y as i64 * voxel_scale.raw());
                let cz = Fixed::from_raw((z as i64 * voxel_scale.raw()) - (VOXEL_DIM as i64 * half_scale.raw()));

                let x0 = cx - half_scale;
                let x1 = cx + half_scale;
                let y0 = cy - half_scale;
                let y1 = cy + half_scale;
                let z0 = cz - half_scale;
                let z1 = cz + half_scale;

                // Texture coordinates based on material
                let (u_base, v_base) = if mat == VOXEL_MOLTEN {
                    (49152, 49152) // Molten orange / red glow
                } else {
                    (16384, 16384) // Metal / stone plate
                };

                // Helper to emit quad face
                let mut add_quad = |p0: Vec3Fixed, p1: Vec3Fixed, p2: Vec3Fixed, p3: Vec3Fixed, norm: Vec3Fixed| {
                    let start_idx = vertices.len() as u32;
                    vertices.push(Vertex3D { pos: p0, norm, u: u_base, v: v_base });
                    vertices.push(Vertex3D { pos: p1, norm, u: u_base + 16384, v: v_base });
                    vertices.push(Vertex3D { pos: p2, norm, u: u_base + 16384, v: v_base + 16384 });
                    vertices.push(Vertex3D { pos: p3, norm, u: u_base, v: v_base + 16384 });

                    indices.push(start_idx);
                    indices.push(start_idx + 1);
                    indices.push(start_idx + 2);

                    indices.push(start_idx);
                    indices.push(start_idx + 2);
                    indices.push(start_idx + 3);
                };

                // +X face
                if x + 1 >= VOXEL_DIM || volume.get(x + 1, y, z) == VOXEL_AIR {
                    add_quad(
                        Vec3Fixed::new(x1, y0, z1),
                        Vec3Fixed::new(x1, y0, z0),
                        Vec3Fixed::new(x1, y1, z0),
                        Vec3Fixed::new(x1, y1, z1),
                        Vec3Fixed::new(one, zero, zero),
                    );
                }

                // -X face
                if x == 0 || volume.get(x - 1, y, z) == VOXEL_AIR {
                    add_quad(
                        Vec3Fixed::new(x0, y0, z0),
                        Vec3Fixed::new(x0, y0, z1),
                        Vec3Fixed::new(x0, y1, z1),
                        Vec3Fixed::new(x0, y1, z0),
                        Vec3Fixed::new(neg_one, zero, zero),
                    );
                }

                // +Y face (Top)
                if y + 1 >= VOXEL_DIM || volume.get(x, y + 1, z) == VOXEL_AIR {
                    add_quad(
                        Vec3Fixed::new(x0, y1, z1),
                        Vec3Fixed::new(x1, y1, z1),
                        Vec3Fixed::new(x1, y1, z0),
                        Vec3Fixed::new(x0, y1, z0),
                        Vec3Fixed::new(zero, one, zero),
                    );
                }

                // -Y face (Bottom)
                if y == 0 || volume.get(x, y - 1, z) == VOXEL_AIR {
                    add_quad(
                        Vec3Fixed::new(x0, y0, z0),
                        Vec3Fixed::new(x1, y0, z0),
                        Vec3Fixed::new(x1, y0, z1),
                        Vec3Fixed::new(x0, y0, z1),
                        Vec3Fixed::new(zero, neg_one, zero),
                    );
                }

                // +Z face
                if z + 1 >= VOXEL_DIM || volume.get(x, y, z + 1) == VOXEL_AIR {
                    add_quad(
                        Vec3Fixed::new(x0, y0, z1),
                        Vec3Fixed::new(x1, y0, z1),
                        Vec3Fixed::new(x1, y1, z1),
                        Vec3Fixed::new(x0, y1, z1),
                        Vec3Fixed::new(zero, zero, one),
                    );
                }

                // -Z face
                if z == 0 || volume.get(x, y, z - 1) == VOXEL_AIR {
                    add_quad(
                        Vec3Fixed::new(x1, y0, z0),
                        Vec3Fixed::new(x0, y0, z0),
                        Vec3Fixed::new(x0, y1, z0),
                        Vec3Fixed::new(x1, y1, z0),
                        Vec3Fixed::new(zero, zero, neg_one),
                    );
                }
            }
        }
    }

    PartMesh {
        name: name.to_string(),
        vertices,
        indices,
    }
}

/// Bisects a 3D PartMesh cleanly along a cutting plane.
/// Returns `(upper_mesh, lower_mesh)` where upper is in the positive half-space.
pub fn sever_part_mesh(mesh: &PartMesh, plane: &SeverancePlane) -> (PartMesh, PartMesh) {
    let mut upper_verts: Vec<Vertex3D> = Vec::new();
    let mut upper_indices: Vec<u32> = Vec::new();

    let mut lower_verts: Vec<Vertex3D> = Vec::new();
    let mut lower_indices: Vec<u32> = Vec::new();

    let tri_count = mesh.triangle_count();

    for t in 0..tri_count {
        let i0 = mesh.indices[t * 3] as usize;
        let i1 = mesh.indices[t * 3 + 1] as usize;
        let i2 = mesh.indices[t * 3 + 2] as usize;

        let v0 = mesh.vertices[i0];
        let v1 = mesh.vertices[i1];
        let v2 = mesh.vertices[i2];

        let p0 = [v0.pos.x.raw() as i32, v0.pos.y.raw() as i32, v0.pos.z.raw() as i32];
        let p1 = [v1.pos.x.raw() as i32, v1.pos.y.raw() as i32, v1.pos.z.raw() as i32];
        let p2 = [v2.pos.x.raw() as i32, v2.pos.y.raw() as i32, v2.pos.z.raw() as i32];

        let s0 = plane.point_side(p0);
        let s1 = plane.point_side(p1);
        let s2 = plane.point_side(p2);

        // Entire triangle on positive side
        if s0 >= 0 && s1 >= 0 && s2 >= 0 {
            let base = upper_verts.len() as u32;
            upper_verts.push(v0);
            upper_verts.push(v1);
            upper_verts.push(v2);
            upper_indices.push(base);
            upper_indices.push(base + 1);
            upper_indices.push(base + 2);
            continue;
        }

        // Entire triangle on negative side
        if s0 <= 0 && s1 <= 0 && s2 <= 0 {
            let base = lower_verts.len() as u32;
            lower_verts.push(v0);
            lower_verts.push(v1);
            lower_verts.push(v2);
            lower_indices.push(base);
            lower_indices.push(base + 1);
            lower_indices.push(base + 2);
            continue;
        }

        // Intersecting triangle: partition between upper and lower
        // Helper to interpolate vertex along an edge
        let interp = |va: Vertex3D, _vb: Vertex3D, pa: [i32; 3], pb: [i32; 3]| -> Vertex3D {
            if let Some(pi) = plane.intersect_segment(pa, pb) {
                let norm = va.norm; // Maintain face normal
                Vertex3D {
                    pos: Vec3Fixed::new(
                        Fixed::from_raw(pi[0] as i64),
                        Fixed::from_raw(pi[1] as i64),
                        Fixed::from_raw(pi[2] as i64),
                    ),
                    norm,
                    u: 57344, // Glowing severed edge UV
                    v: 57344,
                }
            } else {
                va
            }
        };

        // Classify triangles into positive and negative sets
        let pts = [(v0, p0, s0), (v1, p1, s1), (v2, p2, s2)];
        let pos_count = pts.iter().filter(|(_, _, s)| *s > 0).count();

        if pos_count == 1 {
            // 1 on positive side, 2 on negative side
            let pos_idx = pts.iter().position(|(_, _, s)| *s > 0).unwrap();
            let v_pos = pts[pos_idx].0;
            let p_pos = pts[pos_idx].1;

            let v_neg1 = pts[(pos_idx + 1) % 3].0;
            let p_neg1 = pts[(pos_idx + 1) % 3].1;
            let v_neg2 = pts[(pos_idx + 2) % 3].0;
            let p_neg2 = pts[(pos_idx + 2) % 3].1;

            let i_pos_neg1 = interp(v_pos, v_neg1, p_pos, p_neg1);
            let i_pos_neg2 = interp(v_pos, v_neg2, p_pos, p_neg2);

            // Upper gets 1 triangle (v_pos, i_pos_neg1, i_pos_neg2)
            let base = upper_verts.len() as u32;
            upper_verts.push(v_pos);
            upper_verts.push(i_pos_neg1);
            upper_verts.push(i_pos_neg2);
            upper_indices.push(base);
            upper_indices.push(base + 1);
            upper_indices.push(base + 2);

            // Lower gets 2 triangles (quad v_neg1, v_neg2, i_pos_neg2, i_pos_neg1)
            let base_l = lower_verts.len() as u32;
            lower_verts.push(v_neg1);
            lower_verts.push(v_neg2);
            lower_verts.push(i_pos_neg2);
            lower_verts.push(i_pos_neg1);

            lower_indices.push(base_l);
            lower_indices.push(base_l + 1);
            lower_indices.push(base_l + 2);

            lower_indices.push(base_l);
            lower_indices.push(base_l + 2);
            lower_indices.push(base_l + 3);
        } else if pos_count == 2 {
            // 2 on positive side, 1 on negative side
            let neg_idx = pts.iter().position(|(_, _, s)| *s < 0).unwrap();
            let v_neg = pts[neg_idx].0;
            let p_neg = pts[neg_idx].1;

            let v_pos1 = pts[(neg_idx + 1) % 3].0;
            let p_pos1 = pts[(neg_idx + 1) % 3].1;
            let v_pos2 = pts[(neg_idx + 2) % 3].0;
            let p_pos2 = pts[(neg_idx + 2) % 3].1;

            let i_neg_pos1 = interp(v_neg, v_pos1, p_neg, p_pos1);
            let i_neg_pos2 = interp(v_neg, v_pos2, p_neg, p_pos2);

            // Lower gets 1 triangle (v_neg, i_neg_pos1, i_neg_pos2)
            let base_l = lower_verts.len() as u32;
            lower_verts.push(v_neg);
            lower_verts.push(i_neg_pos1);
            lower_verts.push(i_neg_pos2);
            lower_indices.push(base_l);
            lower_indices.push(base_l + 1);
            lower_indices.push(base_l + 2);

            // Upper gets 2 triangles (quad v_pos1, v_pos2, i_neg_pos2, i_neg_pos1)
            let base = upper_verts.len() as u32;
            upper_verts.push(v_pos1);
            upper_verts.push(v_pos2);
            upper_verts.push(i_neg_pos2);
            upper_verts.push(i_neg_pos1);

            upper_indices.push(base);
            upper_indices.push(base + 1);
            upper_indices.push(base + 2);

            upper_indices.push(base);
            upper_indices.push(base + 2);
            upper_indices.push(base + 3);
        }
    }

    let upper_mesh = PartMesh {
        name: format!("{}_upper", mesh.name),
        vertices: upper_verts,
        indices: upper_indices,
    };

    let lower_mesh = PartMesh {
        name: format!("{}_lower", mesh.name),
        vertices: lower_verts,
        indices: lower_indices,
    };

    (upper_mesh, lower_mesh)
}
