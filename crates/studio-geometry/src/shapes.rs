use super::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rounded {
    pub enabled: bool,
    pub radius: f64,
}
impl Default for Rounded {
    fn default() -> Self {
        Self {
            enabled: false,
            radius: 5.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Taper {
    pub enabled: bool,
    pub top_diameter: f64,
}
impl Default for Taper {
    fn default() -> Self {
        Self {
            enabled: false,
            top_diameter: 40.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphereParams {
    pub diameter: f64,
    pub segments: usize,
    pub rings: usize,
}
impl Default for SphereParams {
    fn default() -> Self {
        Self {
            diameter: 60.0,
            segments: 64,
            rings: 32,
        }
    }
}
impl SphereParams {
    pub fn validate(&self) -> GeometryResult<()> {
        dimension(self.diameter)?;
        subdivisions(self.segments, self.rings)
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        let r = self.diameter / 2.0;
        let mut profile = vec![[0.0, 0.0]];
        for i in 1..self.rings {
            let angle = -FRAC_PI_2 + PI * i as f64 / self.rings as f64;
            profile.push([r * angle.cos(), r + r * angle.sin()]);
        }
        profile.push([0.0, self.diameter]);
        revolve(&profile, self.segments)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HemisphereParams {
    pub diameter: f64,
    pub segments: usize,
    pub rings: usize,
}
impl Default for HemisphereParams {
    fn default() -> Self {
        Self {
            diameter: 60.0,
            segments: 64,
            rings: 16,
        }
    }
}
impl HemisphereParams {
    pub fn validate(&self) -> GeometryResult<()> {
        dimension(self.diameter)?;
        subdivisions(self.segments, self.rings)
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        let r = self.diameter / 2.0;
        let mut profile = vec![[0.0, 0.0]];
        for i in 0..self.rings {
            let angle = FRAC_PI_2 * i as f64 / self.rings as f64;
            profile.push([r * angle.cos(), r * angle.sin()]);
        }
        profile.push([0.0, r]);
        revolve(&profile, self.segments)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConeParams {
    pub bottom_diameter: f64,
    pub top_diameter: f64,
    pub height: f64,
    pub segments: usize,
}
impl Default for ConeParams {
    fn default() -> Self {
        Self {
            bottom_diameter: 60.0,
            top_diameter: 0.0,
            height: 80.0,
            segments: 64,
        }
    }
}
impl ConeParams {
    pub fn validate(&self) -> GeometryResult<()> {
        dimension(self.bottom_diameter)?;
        dimension(self.height)?;
        if self.top_diameter != 0.0 {
            dimension(self.top_diameter)?;
        }
        if !(3..=4096).contains(&self.segments) {
            return Err("원뿔 분할 수 범위 초과");
        }
        Ok(())
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        let mut profile = vec![[0.0, 0.0], [self.bottom_diameter / 2.0, 0.0]];
        if self.top_diameter > 0.0 {
            profile.push([self.top_diameter / 2.0, self.height]);
        }
        profile.push([0.0, self.height]);
        revolve(&profile, self.segments)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorusParams {
    pub major_diameter: f64,
    pub tube_diameter: f64,
    pub segments: usize,
    pub tube_segments: usize,
}
impl Default for TorusParams {
    fn default() -> Self {
        Self {
            major_diameter: 60.0,
            tube_diameter: 12.0,
            segments: 64,
            tube_segments: 24,
        }
    }
}
impl TorusParams {
    pub fn validate(&self) -> GeometryResult<()> {
        dimension(self.major_diameter)?;
        dimension(self.tube_diameter)?;
        if self.tube_diameter >= self.major_diameter {
            return Err("링 굵기는 중심 지름보다 작아야 합니다");
        }
        subdivisions(self.segments, self.tube_segments)
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        let n = self.segments;
        let m = self.tube_segments;
        let r = self.tube_diameter / 2.0;
        let major = self.major_diameter / 2.0;
        let mut vertices = Vec::with_capacity(n * m);
        let mut triangles = Vec::with_capacity(n * m * 2);
        for i in 0..n {
            let u = TAU * i as f64 / n as f64;
            for j in 0..m {
                let v = TAU * j as f64 / m as f64;
                let radius = major + r * v.cos();
                vertices.push([radius * u.cos(), radius * u.sin(), r + r * v.sin()]);
            }
        }
        for i in 0..n {
            for j in 0..m {
                let a = i * m + j;
                let b = ((i + 1) % n) * m + j;
                let c = ((i + 1) % n) * m + (j + 1) % m;
                let d = i * m + (j + 1) % m;
                triangles.extend([[a, b, c], [a, c, d]]);
            }
        }
        Mesh::from_parts(vertices, triangles)
    }
}
fn subdivisions(segments: usize, rings: usize) -> GeometryResult<()> {
    if !(8..=256).contains(&segments) || !(4..=128).contains(&rings) {
        return Err("분할 수 범위 초과");
    }
    Ok(())
}
/// Revolve a convex radial profile from bottom axis to top axis; poles share one vertex.
pub(super) fn revolve(profile: &[[f64; 2]], segments: usize) -> GeometryResult<Mesh> {
    let mut mesh = Mesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    let mut rings = Vec::new();
    for &[radius, z] in profile {
        let start = mesh.vertices.len();
        if radius == 0.0 {
            mesh.vertices.push([0.0, 0.0, z]);
            rings.push((start, 1));
        } else {
            mesh.vertices.extend(
                circle(radius, segments)
                    .into_iter()
                    .map(|p| [p[0], p[1], z]),
            );
            rings.push((start, segments));
        }
    }
    for pair in rings.windows(2) {
        let [(a, an), (b, bn)] = [pair[0], pair[1]];
        for i in 0..segments {
            let j = (i + 1) % segments;
            if an == 1 && bn > 1 {
                mesh.triangles.push([a, b + j, b + i]);
            } else if bn == 1 && an > 1 {
                mesh.triangles.push([a + i, a + j, b]);
            } else if an > 1 && bn > 1 {
                quad(&mut mesh, [a + i, a + j, b + j, b + i]);
            }
        }
    }
    mesh.validate()?;
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn volume(mesh: &Mesh, expected: f64, tolerance: f64) {
        mesh.validate().unwrap();
        assert!(
            (mesh.signed_volume() / expected - 1.0).abs() < tolerance,
            "volume {} expected {}",
            mesh.signed_volume(),
            expected
        );
    }
    #[test]
    fn analytic_volumes_of_sphere_hemisphere_torus_and_cone() {
        let sphere = SphereParams::default();
        volume(
            &sphere.mesh().unwrap(),
            4.0 / 3.0 * PI * 30.0_f64.powi(3),
            0.005,
        );
        volume(
            &HemisphereParams::default().mesh().unwrap(),
            2.0 / 3.0 * PI * 30.0_f64.powi(3),
            0.005,
        );
        volume(
            &TorusParams::default().mesh().unwrap(),
            2.0 * PI * PI * 30.0 * 36.0,
            0.02,
        );
        for top in [0.0, 20.0, 60.0, 100.0] {
            let cone = ConeParams {
                top_diameter: top,
                ..ConeParams::default()
            };
            volume(
                &cone.mesh().unwrap(),
                PI * cone.height / 3.0 * (900.0 + 30.0 * top / 2.0 + (top / 2.0).powi(2)),
                0.002,
            );
        }
    }
    #[test]
    fn rounded_box_hollow_inner_radius_and_volume() {
        for radius in [1.0, 2.0, 5.0, 20.0] {
            for enabled in [false, true] {
                for open_top in [false, true] {
                    let p = BoxParams {
                        width: 60.0,
                        depth: 50.0,
                        height: 80.0,
                        rounded: Rounded {
                            enabled: true,
                            radius,
                        },
                        hollow: Hollow {
                            enabled,
                            open_top,
                            ..Hollow::default()
                        },
                    };
                    let area = |w: f64, d: f64, r: f64| w * d - (4.0 - PI) * r * r;
                    let expected = area(60.0, 50.0, radius) * 80.0
                        - if enabled {
                            area(56.0, 46.0, (radius - 2.0).max(0.0))
                                * if open_top { 77.0 } else { 74.0 }
                        } else {
                            0.0
                        };
                    volume(&p.mesh().unwrap(), expected, 0.005);
                }
            }
        }
    }
    #[test]
    fn taper_perpendicular_thickness_and_closed_top() {
        for top_diameter in [30.0, 60.0, 100.0] {
            for open_top in [false, true] {
                let p = CylinderParams {
                    diameter: 60.0,
                    height: 80.0,
                    segments: 96,
                    rounded: Rounded::default(),
                    taper: Taper {
                        enabled: true,
                        top_diameter,
                    },
                    hollow: Hollow {
                        enabled: true,
                        open_top,
                        ..Hollow::default()
                    },
                };
                let mesh = p.mesh().unwrap();
                let slope = (top_diameter / 2.0 - 30.0) / 80.0;
                let i = 2 * p.segments;
                let first = mesh.vertices()[i];
                let normal_distance = (30.0 + slope * first[2] - first[0].hypot(first[1]))
                    / (1.0 + slope * slope).sqrt();
                assert!((normal_distance - 2.0).abs() < 1e-10);
                let z0 = 3.0;
                let z1 = if open_top { 80.0 } else { 77.0 };
                let offset = 2.0 * (1.0 + slope * slope).sqrt();
                let r0 = 30.0 + slope * z0 - offset;
                let r1 = 30.0 + slope * z1 - offset;
                let frustum = |a: f64, b: f64, h: f64| PI * h / 3.0 * (a * a + a * b + b * b);
                volume(
                    &mesh,
                    frustum(30.0, top_diameter / 2.0, 80.0) - frustum(r0, r1, z1 - z0),
                    0.001,
                );
            }
        }
    }
    #[test]
    fn rounded_cylinder_and_invalid_combinations() {
        let p = CylinderParams {
            diameter: 60.0,
            height: 80.0,
            segments: 96,
            rounded: Rounded {
                enabled: true,
                radius: 5.0,
            },
            taper: Taper::default(),
            hollow: Hollow::default(),
        };
        let mesh = p.mesh().unwrap();
        let volume = mesh.signed_volume();
        assert!(volume > PI * 25.0 * 25.0 * 80.0 && volume < PI * 30.0 * 30.0 * 80.0);
        assert!(CylinderParams {
            hollow: Hollow {
                enabled: true,
                ..p.hollow
            },
            ..p
        }
        .mesh()
        .is_err());
        assert!(CylinderParams {
            taper: Taper {
                enabled: true,
                ..p.taper
            },
            ..p
        }
        .mesh()
        .is_err());
        for value in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(SphereParams {
                diameter: value,
                ..SphereParams::default()
            }
            .mesh()
            .is_err());
            assert!(HemisphereParams {
                diameter: value,
                ..HemisphereParams::default()
            }
            .mesh()
            .is_err());
            assert!(TorusParams {
                tube_diameter: value,
                ..TorusParams::default()
            }
            .mesh()
            .is_err());
            assert!(ConeParams {
                height: value,
                ..ConeParams::default()
            }
            .mesh()
            .is_err());
        }
        assert!(TorusParams {
            tube_diameter: 60.0,
            ..TorusParams::default()
        }
        .mesh()
        .is_err());
        assert!(SphereParams {
            segments: usize::MAX,
            ..SphereParams::default()
        }
        .mesh()
        .is_err());
    }
}
