//! Millimetre primitives. Z=0 is the base; XY is centred on the origin.
use serde::{Deserialize, Serialize};
pub mod boolean;
mod shapes;
mod sketch;
mod text;
pub use shapes::*;
pub use sketch::{crossing_edges, SketchParams, MAX_OUTLINE_POINTS, MAX_STROKE_POINTS};
use std::collections::BTreeMap;
pub use text::{TextParams, DEFAULT_FONT, FONT_ID};

pub type Point = [f64; 3];
pub type GeometryResult<T> = Result<T, &'static str>;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hollow {
    pub enabled: bool,
    pub wall: f64,
    pub bottom: f64,
    pub open_top: bool,
}

impl Default for Hollow {
    fn default() -> Self {
        Self {
            enabled: false,
            wall: 2.0,
            bottom: 3.0,
            open_top: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxParams {
    #[serde(default)]
    pub rounded: Rounded,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub hollow: Hollow,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CylinderParams {
    #[serde(default)]
    pub rounded: Rounded,
    #[serde(default)]
    pub taper: Taper,
    pub diameter: f64,
    pub height: f64,
    pub segments: usize,
    pub hollow: Hollow,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    vertices: Vec<Point>,
    triangles: Vec<[usize; 3]>,
}

pub fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Point, b: Point) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

impl Mesh {
    pub fn from_parts(vertices: Vec<Point>, triangles: Vec<[usize; 3]>) -> GeometryResult<Self> {
        let mesh = Self {
            vertices,
            triangles,
        };
        mesh.validate()?;
        Ok(mesh)
    }

    pub fn vertices(&self) -> &[Point] {
        &self.vertices
    }
    pub fn triangles(&self) -> &[[usize; 3]] {
        &self.triangles
    }
    pub fn signed_volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|&[a, b, c]| {
                dot(self.vertices[a], cross(self.vertices[b], self.vertices[c])) / 6.0
            })
            .sum()
    }
    /// Every indexed edge must occur twice, in opposite directions.
    pub fn validate(&self) -> GeometryResult<()> {
        if self.vertices.iter().flatten().any(|v| !v.is_finite()) {
            return Err("non-finite vertex");
        }
        let mut edges = BTreeMap::new();
        for &[a, b, c] in &self.triangles {
            if [a, b, c].iter().any(|&i| i >= self.vertices.len()) {
                return Err("invalid index");
            }
            let normal = cross(
                sub(self.vertices[b], self.vertices[a]),
                sub(self.vertices[c], self.vertices[a]),
            );
            let area = dot(normal, normal);
            if !area.is_finite() || area <= 0.0 {
                return Err("degenerate triangle");
            }
            for (i, j) in [(a, b), (b, c), (c, a)] {
                let entry = edges.entry((i.min(j), i.max(j))).or_insert((0usize, 0i32));
                entry.0 += 1;
                entry.1 += if i < j { 1 } else { -1 };
            }
        }
        if edges.is_empty() || edges.values().any(|&e| e != (2, 0)) {
            return Err("mesh is not an oriented closed manifold");
        }
        let volume = self.signed_volume();
        if !volume.is_finite() || volume <= 0.0 {
            return Err("non-positive volume");
        }
        Ok(())
    }
}

fn dimension(v: f64) -> GeometryResult<()> {
    if !v.is_finite() || !(0.0001..=1_000_000.0).contains(&v) {
        Err("dimension must be finite and between 0.0001 and 1000000 mm")
    } else {
        Ok(())
    }
}
fn hollow_valid(h: Hollow, width: f64, depth: f64, height: f64) -> GeometryResult<()> {
    // Stored parameters remain valid even while Hollow is disabled.
    dimension(h.wall)?;
    dimension(h.bottom)?;
    if h.enabled
        && (2.0 * h.wall >= width
            || 2.0 * h.wall >= depth
            || h.bottom * if h.open_top { 1.0 } else { 2.0 } >= height)
    {
        return Err("hollow thickness leaves no cavity");
    }
    Ok(())
}
impl BoxParams {
    pub fn validate(&self) -> GeometryResult<()> {
        for v in [self.width, self.depth, self.height] {
            dimension(v)?;
        }
        hollow_valid(self.hollow, self.width, self.depth, self.height)?;
        dimension(self.rounded.radius)?;
        if self.rounded.enabled && self.rounded.radius >= self.width.min(self.depth) / 2.0 {
            return Err("둥글기 반경은 가로·세로 절반보다 작아야 합니다");
        }
        Ok(())
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        let radius = if self.rounded.enabled {
            self.rounded.radius
        } else {
            0.0
        };
        let outer = rounded_rectangle(self.width, self.depth, radius);
        let inner = if self.hollow.enabled {
            rounded_rectangle(
                self.width - 2.0 * self.hollow.wall,
                self.depth - 2.0 * self.hollow.wall,
                (radius - self.hollow.wall).max(0.0),
            )
        } else {
            Vec::new()
        };
        build_rings(
            outer.clone(),
            outer,
            inner.clone(),
            inner,
            self.height,
            self.hollow,
        )
    }
}
impl CylinderParams {
    fn top_radius(&self) -> f64 {
        if self.taper.enabled {
            self.taper.top_diameter / 2.0
        } else {
            self.diameter / 2.0
        }
    }
    pub fn validate(&self) -> GeometryResult<()> {
        dimension(self.diameter)?;
        dimension(self.height)?;
        dimension(self.taper.top_diameter)?;
        dimension(self.rounded.radius)?;
        if !(3..=4096).contains(&self.segments) {
            return Err("segments must be between 3 and 4096");
        }
        hollow_valid(self.hollow, self.diameter, self.diameter, self.height)?;
        if self.rounded.enabled {
            if self.hollow.enabled || self.taper.enabled {
                return Err("원기둥 둥글기는 속 비우기·좁히기와 함께 사용할 수 없습니다");
            }
            if self.rounded.radius >= self.diameter.min(self.height) / 2.0 {
                return Err("원기둥 둥글기 반경이 너무 큽니다");
            }
        }
        if self.hollow.enabled {
            let slope = (self.top_radius() - self.diameter / 2.0) / self.height;
            let offset = self.hollow.wall * (1.0 + slope * slope).sqrt();
            for z in [
                self.hollow.bottom,
                if self.hollow.open_top {
                    self.height
                } else {
                    self.height - self.hollow.bottom
                },
            ] {
                if self.diameter / 2.0 + slope * z - offset <= 0.0 {
                    return Err("경사진 내부 공간에 유효한 벽 두께를 만들 수 없습니다");
                }
            }
        }
        Ok(())
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        if self.rounded.enabled {
            let r = self.diameter / 2.0;
            let b = self.rounded.radius;
            let mut profile = vec![[0.0, 0.0]];
            for i in 0..=12 {
                let angle =
                    -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::FRAC_PI_2 / 12.0;
                profile.push([r - b + b * angle.cos(), b + b * angle.sin()]);
            }
            for i in 0..=12 {
                let angle = i as f64 * std::f64::consts::FRAC_PI_2 / 12.0;
                profile.push([r - b + b * angle.cos(), self.height - b + b * angle.sin()]);
            }
            profile.push([0.0, self.height]);
            return revolve(&profile, self.segments);
        }
        let r = self.diameter / 2.0;
        let top = self.top_radius();
        let slope = (top - r) / self.height;
        let offset = self.hollow.wall * (1.0 + slope * slope).sqrt();
        let inner_bottom = if self.hollow.enabled {
            circle(r + slope * self.hollow.bottom - offset, self.segments)
        } else {
            Vec::new()
        };
        let inner_top = if self.hollow.enabled {
            circle(
                r + slope
                    * if self.hollow.open_top {
                        self.height
                    } else {
                        self.height - self.hollow.bottom
                    }
                    - offset,
                self.segments,
            )
        } else {
            Vec::new()
        };
        build_rings(
            circle(r, self.segments),
            circle(top, self.segments),
            inner_bottom,
            inner_top,
            self.height,
            self.hollow,
        )
    }
}
fn circle(radius: f64, segments: usize) -> Vec<[f64; 2]> {
    (0..segments)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / segments as f64;
            [radius * angle.cos(), radius * angle.sin()]
        })
        .collect()
}
fn rounded_rectangle(width: f64, depth: f64, radius: f64) -> Vec<[f64; 2]> {
    if radius == 0.0 {
        return vec![
            [-width / 2.0, -depth / 2.0],
            [width / 2.0, -depth / 2.0],
            [width / 2.0, depth / 2.0],
            [-width / 2.0, depth / 2.0],
        ];
    }
    let mut points = Vec::new();
    for (corner, [cx, cy]) in [
        [width / 2.0 - radius, -depth / 2.0 + radius],
        [width / 2.0 - radius, depth / 2.0 - radius],
        [-width / 2.0 + radius, depth / 2.0 - radius],
        [-width / 2.0 + radius, -depth / 2.0 + radius],
    ]
    .into_iter()
    .enumerate()
    {
        for i in 0..=12 {
            let angle = (corner as f64 - 1.0 + i as f64 / 12.0) * std::f64::consts::FRAC_PI_2;
            points.push([cx + radius * angle.cos(), cy + radius * angle.sin()]);
        }
    }
    points
}
fn quad(mesh: &mut Mesh, [a, b, c, d]: [usize; 4]) {
    mesh.triangles.extend([[a, b, c], [a, c, d]]);
}
fn cap(mesh: &mut Mesh, base: usize, n: usize, up: bool) {
    for i in 1..n - 1 {
        let mut t = [base, base + i, base + i + 1];
        if !up {
            t.swap(1, 2);
        }
        mesh.triangles.push(t);
    }
}
fn connect(mesh: &mut Mesh, lower: usize, upper: usize, n: usize, reverse: bool) {
    for i in 0..n {
        let j = (i + 1) % n;
        let mut q = [lower + i, lower + j, upper + j, upper + i];
        if reverse {
            q.reverse();
        }
        quad(mesh, q);
    }
}
fn build_rings(
    outer_bottom: Vec<[f64; 2]>,
    outer_top: Vec<[f64; 2]>,
    inner_bottom: Vec<[f64; 2]>,
    inner_top: Vec<[f64; 2]>,
    height: f64,
    h: Hollow,
) -> GeometryResult<Mesh> {
    let n = outer_bottom.len();
    let m = inner_bottom.len();
    let mut mesh = Mesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    mesh.vertices
        .extend(outer_bottom.iter().map(|p| [p[0], p[1], 0.0]));
    mesh.vertices
        .extend(outer_top.iter().map(|p| [p[0], p[1], height]));
    connect(&mut mesh, 0, n, n, false);
    cap(&mut mesh, 0, n, false);
    if h.enabled {
        let ib = 2 * n;
        let it = ib + m;
        mesh.vertices
            .extend(inner_bottom.iter().map(|p| [p[0], p[1], h.bottom]));
        mesh.vertices.extend(inner_top.iter().map(|p| {
            [
                p[0],
                p[1],
                if h.open_top {
                    height
                } else {
                    height - h.bottom
                },
            ]
        }));
        connect(&mut mesh, ib, it, m, true);
        cap(&mut mesh, ib, m, true);
        if h.open_top {
            let coordinates: Vec<f64> = outer_top
                .iter()
                .chain(&inner_top)
                .flatten()
                .copied()
                .collect();
            let triangles = earcutr::earcut(&coordinates, &[n], 2)
                .map_err(|_| "윗면 테두리 삼각형 분할 실패")?;
            for t in triangles.as_chunks::<3>().0 {
                let mut t = [t[0], t[1], t[2]].map(|i| if i < n { n + i } else { it + i - n });
                let [a, b, c] = t.map(|i| mesh.vertices[i]);
                if cross(sub(b, a), sub(c, a))[2] < 0.0 {
                    t.swap(1, 2);
                }
                mesh.triangles.push(t);
            }
        } else {
            cap(&mut mesh, n, n, true);
            cap(&mut mesh, it, m, false);
        }
    } else {
        cap(&mut mesh, n, n, true);
    }
    mesh.validate()?;
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check(mesh: &Mesh, expected: f64) {
        mesh.validate().unwrap();
        // Independently require one forward and one backward use per edge.
        let mut edges = BTreeMap::<(usize, usize), Vec<(usize, usize)>>::new();
        for &[a, b, c] in mesh.triangles() {
            for (a, b) in [(a, b), (b, c), (c, a)] {
                edges.entry((a.min(b), a.max(b))).or_default().push((a, b));
            }
        }
        for uses in edges.values() {
            assert_eq!(uses.len(), 2);
            assert_eq!(uses[0], (uses[1].1, uses[1].0));
        }
        let mut volume = 0.0;
        // Divergence theorem using triangle centroid and oriented area.
        for &[a, b, c] in mesh.triangles() {
            let [a, b, c] = [a, b, c].map(|i| mesh.vertices()[i]);
            let normal = cross(sub(b, a), sub(c, a));
            let centroid = std::array::from_fn(|i| (a[i] + b[i] + c[i]) / 3.0);
            volume += dot(centroid, normal) / 6.0;
        }
        assert!(volume > 0.0);
        assert!(
            (volume - expected).abs() < expected * 1e-10,
            "{volume} != {expected}"
        );
    }
    #[test]
    fn box_solid_open_and_closed_cavity() {
        for enabled in [false, true] {
            for open_top in [false, true] {
                let hollow = Hollow {
                    enabled,
                    open_top,
                    ..Hollow::default()
                };
                let mesh = BoxParams {
                    rounded: Default::default(),
                    width: 60.0,
                    depth: 50.0,
                    height: 80.0,
                    hollow,
                }
                .mesh()
                .unwrap();
                let cavity = if enabled {
                    56.0 * 46.0 * if open_top { 77.0 } else { 74.0 }
                } else {
                    0.0
                };
                check(&mesh, 60.0 * 50.0 * 80.0 - cavity);
                for (axis, min, max) in [(0, -30.0, 30.0), (1, -25.0, 25.0), (2, 0.0, 80.0)] {
                    assert_eq!(
                        mesh.vertices()
                            .iter()
                            .map(|p| p[axis])
                            .fold(f64::INFINITY, f64::min),
                        min
                    );
                    assert_eq!(
                        mesh.vertices()
                            .iter()
                            .map(|p| p[axis])
                            .fold(f64::NEG_INFINITY, f64::max),
                        max
                    );
                }
                if enabled {
                    assert!(mesh.vertices().contains(&[-28.0, -23.0, 3.0]));
                    assert!(mesh.vertices().contains(&[
                        -28.0,
                        -23.0,
                        if open_top { 80.0 } else { 77.0 }
                    ]));
                }
            }
        }
    }
    #[test]
    fn cylinder_volumes_and_bounds() {
        for segments in [3, 32, 96, 4096] {
            for enabled in [false, true] {
                for open_top in [false, true] {
                    let hollow = Hollow {
                        enabled,
                        open_top,
                        ..Hollow::default()
                    };
                    let mesh = CylinderParams {
                        rounded: Default::default(),
                        taper: Default::default(),
                        diameter: 60.0,
                        height: 80.0,
                        segments,
                        hollow,
                    }
                    .mesh()
                    .unwrap();
                    let factor =
                        segments as f64 / 2.0 * (std::f64::consts::TAU / segments as f64).sin();
                    let expected = factor
                        * (900.0 * 80.0
                            - if enabled {
                                784.0 * if open_top { 77.0 } else { 74.0 }
                            } else {
                                0.0
                            });
                    check(&mesh, expected);
                    for p in mesh.vertices() {
                        assert!(p[0].hypot(p[1]) <= 30.0 + 1e-12);
                        assert!((0.0..=80.0).contains(&p[2]));
                    }
                    if segments == 96 {
                        let analytic = std::f64::consts::PI
                            * (900.0 * 80.0
                                - if enabled {
                                    784.0 * if open_top { 77.0 } else { 74.0 }
                                } else {
                                    0.0
                                });
                        assert!((mesh.signed_volume() / analytic - 1.0).abs() < 0.001);
                    }
                }
            }
        }
    }
    #[test]
    fn toggle_preserves_parameters_and_regenerates_identically() {
        assert!(!Hollow::default().enabled);
        let mut p = CylinderParams {
            rounded: Default::default(),
            taper: Default::default(),
            diameter: 60.0,
            height: 80.0,
            segments: 32,
            hollow: Hollow {
                enabled: true,
                wall: 4.0,
                bottom: 5.0,
                open_top: false,
            },
        };
        let before = p.mesh().unwrap();
        let stored = p.hollow;
        p.hollow.enabled = false;
        check(
            &p.mesh().unwrap(),
            32.0 / 2.0 * (std::f64::consts::TAU / 32.0).sin() * 900.0 * 80.0,
        );
        p.hollow.enabled = true;
        assert_eq!(p.hollow, stored);
        assert_eq!(p.mesh().unwrap().vertices(), before.vertices());
        assert_eq!(p.mesh().unwrap().triangles(), before.triangles());
        let mut b = BoxParams {
            rounded: Default::default(),
            width: 60.0,
            depth: 50.0,
            height: 80.0,
            hollow: stored,
        };
        let before = b.mesh().unwrap();
        b.hollow.enabled = false;
        check(&b.mesh().unwrap(), 60.0 * 50.0 * 80.0);
        b.hollow.enabled = true;
        assert_eq!(b.hollow, stored);
        assert_eq!(b.mesh().unwrap().triangles(), before.triangles());
        assert_eq!(b.mesh().unwrap().vertices(), before.vertices());
    }
    #[test]
    fn rejects_invalid_inputs_without_panics() {
        let b = BoxParams {
            rounded: Default::default(),
            width: 60.0,
            depth: 50.0,
            height: 80.0,
            hollow: Hollow::default(),
        };
        let c = CylinderParams {
            rounded: Default::default(),
            taper: Default::default(),
            diameter: 60.0,
            height: 80.0,
            segments: 96,
            hollow: Hollow::default(),
        };
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.0,
            0.0,
            1e-100,
            1e100,
        ] {
            for p in [
                BoxParams { width: value, ..b },
                BoxParams { depth: value, ..b },
                BoxParams { height: value, ..b },
            ] {
                assert!(p.mesh().is_err());
            }
            assert!(CylinderParams {
                diameter: value,
                ..c
            }
            .mesh()
            .is_err());
            assert!(CylinderParams { height: value, ..c }.mesh().is_err());
            for enabled in [false, true] {
                for hollow in [
                    Hollow {
                        wall: value,
                        enabled,
                        ..b.hollow
                    },
                    Hollow {
                        bottom: value,
                        enabled,
                        ..b.hollow
                    },
                ] {
                    assert!(BoxParams { hollow, ..b }.mesh().is_err());
                    assert!(CylinderParams { hollow, ..c }.mesh().is_err());
                }
            }
        }
        for segments in [0, 1, 2, 4097, usize::MAX] {
            assert!(CylinderParams { segments, ..c }.mesh().is_err());
        }
        for hollow in [
            Hollow {
                enabled: true,
                wall: 30.0,
                ..b.hollow
            },
            Hollow {
                enabled: true,
                bottom: 80.0,
                ..b.hollow
            },
            Hollow {
                enabled: true,
                bottom: 40.0,
                open_top: false,
                ..b.hollow
            },
        ] {
            assert!(BoxParams { hollow, ..b }.mesh().is_err());
            assert!(CylinderParams { hollow, ..c }.mesh().is_err());
        }
    }
}
