//! Shaped Korean/Latin glyph outlines, flattened adaptively and extruded with holes.
use super::*;
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};
use unicode_normalization::UnicodeNormalization;
pub const DEFAULT_FONT: &[u8] = include_bytes!("../../../assets/fonts/NanumGothic-Regular.ttf");
pub const FONT_ID: &str = "nanum-gothic-regular-v1";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextParams {
    pub text: String,
    pub height: f64,
    pub thickness: f64,
    pub font: String,
}
impl Default for TextParams {
    fn default() -> Self {
        Self {
            text: "큐빌로".into(),
            height: 20.0,
            thickness: 3.0,
            font: FONT_ID.into(),
        }
    }
}
impl TextParams {
    pub fn validate(&self) -> GeometryResult<()> {
        dimension(self.height)?;
        dimension(self.thickness)?;
        if self.font != FONT_ID {
            return Err("지원하지 않는 글꼴입니다");
        }
        if self.text.trim().is_empty()
            || self.text.chars().count() > 64
            || self.text.chars().any(char::is_control)
        {
            return Err("텍스트는 줄바꿈 없이 1~64자로 입력하세요");
        }
        Ok(())
    }
    pub fn mesh(&self) -> GeometryResult<Mesh> {
        self.validate()?;
        let face = rustybuzz::Face::from_slice(DEFAULT_FONT, 0).ok_or("글꼴을 읽을 수 없습니다")?;
        let normalized: String = self.text.nfc().collect();
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(&normalized);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let tolerance = (face.units_per_em() as f64 * 0.01 / self.height).clamp(0.05, 4.0);
        let mut contours = Vec::new();
        let mut x = 0.0;
        let mut y = 0.0;
        for (glyph, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            if glyph.glyph_id == 0 {
                return Err("글꼴에 없는 문자가 포함되어 있습니다");
            }
            let mut outline = Outline {
                contours: Vec::new(),
                current: Vec::new(),
                tolerance,
            };
            face.outline_glyph(GlyphId(glyph.glyph_id as u16), &mut outline);
            outline.finish();
            let offset = [x + position.x_offset as f64, y + position.y_offset as f64];
            for mut contour in outline.contours {
                for point in &mut contour {
                    point[0] += offset[0];
                    point[1] += offset[1];
                }
                contours.push(contour);
            }
            x += position.x_advance as f64;
            y += position.y_advance as f64;
        }
        if contours.is_empty() {
            return Err("입체로 만들 수 있는 글자 윤곽선이 없습니다");
        }
        if contours.iter().map(Vec::len).sum::<usize>() > 100_000 {
            return Err("텍스트 윤곽선이 너무 복잡합니다");
        }
        let min_x = contours
            .iter()
            .flatten()
            .map(|p| p[0])
            .fold(f64::INFINITY, f64::min);
        let max_x = contours
            .iter()
            .flatten()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = contours
            .iter()
            .flatten()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min);
        let max_y = contours
            .iter()
            .flatten()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max);
        let scale = self.height / (max_y - min_y);
        for contour in &mut contours {
            for p in contour {
                p[0] = (p[0] - (min_x + max_x) / 2.0) * scale;
                p[1] = (p[1] - (min_y + max_y) / 2.0) * scale;
            }
        }
        extrude(&contours, self.thickness)
    }
}
struct Outline {
    contours: Vec<Vec<[f64; 2]>>,
    current: Vec<[f64; 2]>,
    tolerance: f64,
}
fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}
fn distance_to_line(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = d[0].hypot(d[1]);
    if len < 1e-12 {
        (p[0] - a[0]).hypot(p[1] - a[1])
    } else {
        ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / len
    }
}
impl Outline {
    fn finish(&mut self) {
        self.current
            .dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-8);
        if self.current.len() > 1 {
            let a = self.current[0];
            let b = *self.current.last().unwrap();
            if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-8 {
                self.current.pop();
            }
        }
        if self.current.len() >= 3 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
    fn quad(&mut self, a: [f64; 2], b: [f64; 2], c: [f64; 2], depth: u8) {
        if depth >= 12 || distance_to_line(b, a, c) <= self.tolerance {
            self.current.push(c);
            return;
        }
        let ab = midpoint(a, b);
        let bc = midpoint(b, c);
        let mid = midpoint(ab, bc);
        self.quad(a, ab, mid, depth + 1);
        self.quad(mid, bc, c, depth + 1);
    }
    fn cubic(&mut self, a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2], depth: u8) {
        if depth >= 12 || distance_to_line(b, a, d).max(distance_to_line(c, a, d)) <= self.tolerance
        {
            self.current.push(d);
            return;
        }
        let ab = midpoint(a, b);
        let bc = midpoint(b, c);
        let cd = midpoint(c, d);
        let abc = midpoint(ab, bc);
        let bcd = midpoint(bc, cd);
        let middle = midpoint(abc, bcd);
        self.cubic(a, ab, abc, middle, depth + 1);
        self.cubic(middle, bcd, cd, d, depth + 1);
    }
}
impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.current.push([x as f64, y as f64]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push([x as f64, y as f64]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if let Some(&a) = self.current.last() {
            self.quad(a, [x1 as f64, y1 as f64], [x as f64, y as f64], 0);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if let Some(&a) = self.current.last() {
            self.cubic(
                a,
                [x1 as f64, y1 as f64],
                [x2 as f64, y2 as f64],
                [x as f64, y as f64],
                0,
            );
        }
    }
    fn close(&mut self) {
        self.finish();
    }
}
fn area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum::<f64>()
        / 2.0
}
fn inside(p: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut result = false;
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}
fn extrude(contours: &[Vec<[f64; 2]>], thickness: f64) -> GeometryResult<Mesh> {
    let parents: Vec<_> = contours
        .iter()
        .enumerate()
        .map(|(i, c)| {
            contours
                .iter()
                .enumerate()
                .filter(|(j, other)| {
                    *j != i && area(other).abs() > area(c).abs() && inside(c[0], other)
                })
                .min_by(|(_, a), (_, b)| area(a).abs().total_cmp(&area(b).abs()))
                .map(|(j, _)| j)
        })
        .collect();
    let depth = |i: usize| {
        let mut d = 0;
        let mut parent = parents[i];
        while let Some(p) = parent {
            d += 1;
            parent = parents[p];
        }
        d
    };
    let mut mesh = Mesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for i in 0..contours.len() {
        if depth(i) % 2 != 0 {
            continue;
        }
        let indices: Vec<usize> = std::iter::once(i)
            .chain((0..contours.len()).filter(|&j| parents[j] == Some(i)))
            .collect();
        let mut points = Vec::new();
        let mut holes = Vec::new();
        let mut rings = Vec::new();
        for (k, index) in indices.into_iter().enumerate() {
            let mut ring = contours[index].clone();
            if (area(&ring) > 0.0) != (k == 0) {
                ring.reverse();
            }
            let start = points.len();
            if k > 0 {
                holes.push(start);
            }
            rings.push((start, ring.len()));
            points.extend(ring);
        }
        let n = points.len();
        let base = mesh.vertices.len();
        for z in [0.0, thickness] {
            mesh.vertices.extend(points.iter().map(|p| [p[0], p[1], z]));
        }
        let coordinates: Vec<f64> = points.iter().flatten().copied().collect();
        let triangles =
            earcutr::earcut(&coordinates, &holes, 2).map_err(|_| "글자 내부 구멍 분할 실패")?;
        for t in triangles.as_chunks::<3>().0 {
            let mut t = *t;
            let [a, b, c] = t.map(|j| [points[j][0], points[j][1], 0.0]);
            if cross(sub(b, a), sub(c, a))[2] < 0.0 {
                t.swap(1, 2);
            }
            mesh.triangles.push(t.map(|j| base + n + j));
            mesh.triangles.push([base + t[2], base + t[1], base + t[0]]);
        }
        for (start, count) in rings {
            for j in 0..count {
                let a = base + start + j;
                let b = base + start + (j + 1) % count;
                quad(&mut mesh, [a, b, b + n, a + n]);
            }
        }
    }
    mesh.validate()?;
    Ok(mesh)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn korean_latin_holes_and_exact_height_survive_extrusion() {
        for text in ["ㅇ", "ㅁ", "8AO", "큐빌로", "Hello 큐빌로"] {
            let p = TextParams {
                text: text.into(),
                ..TextParams::default()
            };
            let mesh = p.mesh().unwrap();
            mesh.validate().unwrap();
            let y0 = mesh
                .vertices()
                .iter()
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min);
            let y1 = mesh
                .vertices()
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max);
            assert!((y1 - y0 - 20.0).abs() < 1e-9);
            assert!(mesh.vertices().iter().all(|p| p[2] == 0.0 || p[2] == 3.0));
            if text == "ㅇ" || text == "ㅁ" {
                // The centre of the glyph must not be covered by either cap.
                let caps = mesh
                    .triangles()
                    .iter()
                    .filter(|t| t.iter().all(|&i| mesh.vertices()[i][2] == 3.0));
                for t in caps {
                    let [a, b, c] = t.map(|i| mesh.vertices()[i]);
                    let sign = |a: Point, b: Point| a[0] * b[1] - a[1] * b[0];
                    let signs = [sign(a, b), sign(b, c), sign(c, a)];
                    assert!(!(signs.iter().all(|&v| v >= 0.0) || signs.iter().all(|&v| v <= 0.0)));
                }
            }
        }
    }
    #[test]
    fn canonical_hangul_and_bad_input() {
        let a = TextParams {
            text: "가".into(),
            ..TextParams::default()
        }
        .mesh()
        .unwrap();
        let b = TextParams {
            text: "가".into(),
            ..TextParams::default()
        }
        .mesh()
        .unwrap();
        assert_eq!(a.vertices(), b.vertices());
        assert_eq!(a.triangles(), b.triangles());
        for text in ["", "  ", "a\nb", "😀"] {
            assert!(TextParams {
                text: text.into(),
                ..TextParams::default()
            }
            .mesh()
            .is_err());
        }
    }
}
