//! Bounded simple XY outlines extruded into watertight solids.
use super::*;
pub const MAX_OUTLINE_POINTS: usize = 256;
pub const MAX_STROKE_POINTS: usize = 2048;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SketchParams {
    pub points: Vec<[f64; 2]>,
    pub thickness: f64,
    /// Original pen input, retained independently from the editable simplified outline.
    #[serde(default)]
    pub stroke: Vec<[f64; 2]>,
}
fn turn(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn on(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> bool {
    turn(a, b, p).abs() <= 1e-9
        && (0..2).all(|i| p[i] >= a[i].min(b[i]) - 1e-9 && p[i] <= a[i].max(b[i]) + 1e-9)
}
fn intersects(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let [u, v, w, x] = [turn(a, b, c), turn(a, b, d), turn(c, d, a), turn(c, d, b)];
    (u.signum() != v.signum()
        && w.signum() != x.signum()
        && u.abs() > 1e-9
        && v.abs() > 1e-9
        && w.abs() > 1e-9
        && x.abs() > 1e-9)
        || on(a, b, c)
        || on(a, b, d)
        || on(c, d, a)
        || on(c, d, b)
}
/// Indices of intersecting edges, for feedback on the drawing canvas.
pub fn crossing_edges(points: &[[f64; 2]]) -> Vec<usize> {
    if points.len() > MAX_OUTLINE_POINTS {
        return Vec::new();
    }
    let n = points.len();
    let mut edges = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            if (i + 1) % n == j || (j + 1) % n == i {
                continue;
            }
            if intersects(
                points[i],
                points[(i + 1) % n],
                points[j],
                points[(j + 1) % n],
            ) {
                edges.extend([i, j]);
            }
        }
    }
    edges.sort_unstable();
    edges.dedup();
    edges
}
impl SketchParams {
    pub fn validate(&self) -> GeometryResult<()> {
        dimension(self.thickness)?;
        if !(3..=MAX_OUTLINE_POINTS).contains(&self.points.len()) {
            return Err("점을 3~256개로 이어 모양을 만들어 주세요");
        }
        if self.stroke.len() > MAX_STROKE_POINTS
            || self
                .points
                .iter()
                .chain(&self.stroke)
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        {
            return Err("그림이 너무 크거나 복잡해요");
        }
        let n = self.points.len();
        let mut area = 0.0;
        for i in 0..n {
            let a = self.points[i];
            let b = self.points[(i + 1) % n];
            let c = self.points[(i + 2) % n];
            if (a[0] - b[0]).hypot(a[1] - b[1]) < 0.001 {
                return Err("겹쳐 있는 점을 옮기거나 지워 주세요");
            }
            if turn(a, b, c).abs() < 1e-9
                && (a[0] - b[0]) * (c[0] - b[0]) + (a[1] - b[1]) * (c[1] - b[1]) > 0.0
            {
                return Err("선이 되돌아 겹쳐 있어요. 점을 옮겨 주세요");
            }
            area += a[0] * b[1] - a[1] * b[0];
        }
        if !crossing_edges(&self.points).is_empty() {
            return Err("빨간 선이 서로 만나고 있어요. 점을 옮겨 풀어 주세요");
        }
        if area.abs() < 0.01 {
            return Err("안쪽이 생기도록 모양을 넓혀 주세요");
        }
        Ok(())
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        let mut points = self.points.clone();
        let area: f64 = (0..points.len())
            .map(|i| {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                a[0] * b[1] - a[1] * b[0]
            })
            .sum();
        if area < 0.0 {
            points.reverse();
        }
        // Remove redundant straight vertices before earcut; source outline is unchanged.
        loop {
            let n = points.len();
            let redundant = (0..n).find(|&i| {
                turn(points[(i + n - 1) % n], points[i], points[(i + 1) % n]).abs() < 1e-9
            });
            if let Some(i) = redundant {
                if n <= 3 {
                    break;
                }
                points.remove(i);
            } else {
                break;
            }
        }
        let n = points.len();
        let coords: Vec<_> = points.iter().flatten().copied().collect();
        let indices = earcutr::earcut(&coords, &[], 2).map_err(|_| "모양 안쪽을 만들 수 없어요")?;
        let mut mesh = Mesh {
            vertices: points
                .iter()
                .map(|p| [p[0], p[1], 0.0])
                .chain(points.iter().map(|p| [p[0], p[1], self.thickness]))
                .collect(),
            triangles: Vec::new(),
        };
        connect(&mut mesh, 0, n, n, false);
        for t in indices.as_chunks::<3>().0 {
            let mut t = *t;
            if turn(points[t[0]], points[t[1]], points[t[2]]) < 0.0 {
                t.swap(1, 2);
            }
            mesh.triangles.push([t[2], t[1], t[0]]);
            mesh.triangles.push(t.map(|i| i + n));
        }
        mesh.validate()?;
        Ok(mesh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concave_both_directions_and_straight_points_are_closed() {
        let mut p = SketchParams {
            points: vec![
                [0., 0.],
                [10., 0.],
                [20., 0.],
                [20., 10.],
                [10., 10.],
                [10., 20.],
                [0., 20.],
            ],
            thickness: 3.,
            stroke: vec![],
        };
        for _ in 0..2 {
            let mesh = p.mesh().unwrap();
            mesh.validate().unwrap();
            assert!((mesh.signed_volume() - 900.).abs() < 1e-9);
            p.points.reverse();
        }
    }
    #[test]
    fn rejects_crossing_touching_retraced_and_invalid_inputs() {
        for points in [
            vec![[0., 0.], [10., 10.], [0., 10.], [10., 0.]],
            vec![[0., 0.], [10., 0.], [5., 0.], [0., 10.]],
            vec![[0., 0.], [0., 0.], [0., 10.]],
            vec![[0., 0.], [f64::NAN, 2.], [2., 4.]],
            vec![[0., 0.]; 257],
            vec![[0., 0.], [10., 0.], [10., 10.], [5., 0.], [0., 10.]],
        ] {
            assert!(SketchParams {
                points,
                thickness: 3.,
                stroke: vec![]
            }
            .mesh()
            .is_err());
        }
    }
}
