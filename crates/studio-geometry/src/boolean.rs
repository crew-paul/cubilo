//! Bounded, cancellable strict Manifold adapter; empty results are valid solids.
use super::Mesh;
pub use manifold_rust::cancel::CancelToken;
use manifold_rust::{
    manifold::Manifold,
    types::{Error, MeshGLP, OpType},
};
use serde::{Deserialize, Serialize};
pub const MAX_BOOLEAN_INPUT_TRIANGLES: usize = 100_000;
pub const MAX_BOOLEAN_OUTPUT_TRIANGLES: usize = 250_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersection,
}
impl BooleanOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Union => "합치기",
            Self::Difference => "빼기",
            Self::Intersection => "교집합",
        }
    }
}
pub fn boolean(
    a: &Mesh,
    b: &Mesh,
    operation: BooleanOp,
    token: &CancelToken,
) -> Result<Option<Mesh>, String> {
    if token.is_cancelled() {
        return Err("계산이 취소되었습니다".into());
    }
    if a.triangles().len() > MAX_BOOLEAN_INPUT_TRIANGLES
        || b.triangles().len() > MAX_BOOLEAN_INPUT_TRIANGLES
    {
        return Err("Boolean 입력은 도형당 100,000 삼각형까지 지원합니다".into());
    }
    let convert = |mesh: &Mesh| -> Result<Manifold, String> {
        mesh.validate().map_err(str::to_string)?;
        let input = MeshGLP::<f64, u32> {
            num_prop: 3,
            vert_properties: mesh.vertices().iter().flatten().copied().collect(),
            tri_verts: mesh
                .triangles()
                .iter()
                .flatten()
                .map(|&i| i as u32)
                .collect(),
            ..Default::default()
        };
        let result = Manifold::from_mesh_glp(&input);
        if result.status() != Error::NoError {
            return Err(format!("Boolean 입력 오류: {}", result.status().to_str()));
        }
        Ok(result)
    };
    let a = convert(a)?;
    let b = convert(b)?;
    let result = a.boolean_with_token(
        &b,
        match operation {
            BooleanOp::Union => OpType::Add,
            BooleanOp::Difference => OpType::Subtract,
            BooleanOp::Intersection => OpType::Intersect,
        },
        Some(token),
    );
    if token.is_cancelled() || result.status() == Error::Cancelled {
        return Err("계산이 취소되었습니다".into());
    }
    if result.status() != Error::NoError {
        return Err(format!("Boolean 실패: {}", result.status().to_str()));
    }
    if result.is_empty() {
        return Ok(None);
    }
    let output = result.get_mesh_glp::<f64, u32>(-1);
    if output.tri_verts.len() / 3 > MAX_BOOLEAN_OUTPUT_TRIANGLES {
        return Err("Boolean 결과가 너무 복잡합니다".into());
    }
    let stride = output.num_prop as usize;
    let count = output.vert_properties.len() / stride;
    let mut parent: Vec<usize> = (0..count).collect();
    for (&from, &to) in output.merge_from_vert.iter().zip(&output.merge_to_vert) {
        parent[from as usize] = to as usize;
    }
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut indices = std::collections::BTreeMap::new();
    for triangle in output.tri_verts.as_chunks::<3>().0 {
        let mut ids = [0; 3];
        for (target, &index) in ids.iter_mut().zip(triangle) {
            let mut canonical = index as usize;
            while parent[canonical] != canonical {
                canonical = parent[canonical];
            }
            *target = *indices.entry(canonical).or_insert_with(|| {
                let p = &output.vert_properties[canonical * stride..canonical * stride + 3];
                vertices.push([p[0], p[1], p[2]]);
                vertices.len() - 1
            });
        }
        triangles.push(ids);
    }
    Mesh::from_parts(vertices, triangles)
        .map(Some)
        .map_err(str::to_string)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxParams, Hollow};
    fn cube(x: f64) -> Mesh {
        let mesh = BoxParams {
            width: 20.0,
            depth: 20.0,
            height: 20.0,
            hollow: Hollow::default(),
            rounded: Default::default(),
        }
        .mesh()
        .unwrap();
        Mesh::from_parts(
            mesh.vertices()
                .iter()
                .map(|&p| [p[0] + x, p[1], p[2]])
                .collect(),
            mesh.triangles().to_vec(),
        )
        .unwrap()
    }
    #[test]
    fn overlapping_touching_disjoint_and_empty_solids() {
        let a = cube(0.0);
        let b = cube(10.0);
        let token = CancelToken::new();
        for (op, expected) in [
            (BooleanOp::Union, 12000.0),
            (BooleanOp::Difference, 4000.0),
            (BooleanOp::Intersection, 4000.0),
        ] {
            let mesh = boolean(&a, &b, op, &token).unwrap().unwrap();
            mesh.validate().unwrap();
            assert!((mesh.signed_volume() - expected).abs() < 1e-6);
        }
        let touching = boolean(&a, &cube(20.0), BooleanOp::Union, &token)
            .unwrap()
            .unwrap();
        assert!((touching.signed_volume() - 16000.0).abs() < 1e-6);
        assert!(boolean(&a, &cube(50.0), BooleanOp::Intersection, &token)
            .unwrap()
            .is_none());
        assert!(boolean(&a, &a, BooleanOp::Difference, &token)
            .unwrap()
            .is_none());
    }
    #[test]
    fn cancellation_is_not_success() {
        let token = CancelToken::new();
        token.cancel();
        assert!(boolean(&cube(0.0), &cube(10.0), BooleanOp::Union, &token).is_err());
    }
}
