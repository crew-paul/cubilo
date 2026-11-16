//! Parameter-aware direct manipulation and world-space alignment.
use crate::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignMode {
    Minimum,
    Center,
    Maximum,
}
impl Primitive {
    pub fn resize_uniform(&mut self, factor: f64) -> CoreResult<()> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err("크기를 조절할 수 없습니다".into());
        }
        match self {
            Self::Box(p) => {
                p.width *= factor;
                p.depth *= factor;
                p.height *= factor;
            }
            Self::Cylinder(p) => {
                p.diameter *= factor;
                p.height *= factor;
                if p.taper.enabled {
                    p.taper.top_diameter *= factor;
                }
            }
            Self::Sphere(p) => p.diameter *= factor,
            Self::Hemisphere(p) => p.diameter *= factor,
            Self::Cone(p) => {
                p.bottom_diameter *= factor;
                p.top_diameter *= factor;
                p.height *= factor;
            }
            Self::Torus(p) => {
                p.major_diameter *= factor;
                p.tube_diameter *= factor;
            }
            Self::Sketch(p) => {
                for point in p.points.iter_mut().chain(&mut p.stroke) {
                    for v in point {
                        *v *= factor;
                    }
                }
                p.thickness *= factor;
            }
            Self::Text(p) => {
                p.height *= factor;
                p.thickness *= factor;
            }
        }
        self.validate()
    }
    /// XY primitives are centred; Z starts at the build plate. No mesh scale is stored.
    pub fn resize_axis(&mut self, axis: usize, delta: f64) -> CoreResult<()> {
        if axis > 2 || !delta.is_finite() {
            return Err("잘못된 크기 조절입니다".into());
        }
        let radial = if axis < 2 { 2.0 * delta } else { delta };
        match self {
            Self::Box(p) => match axis {
                0 => p.width += radial,
                1 => p.depth += radial,
                _ => p.height += delta,
            },
            Self::Cylinder(p) => {
                if axis < 2 {
                    p.diameter += radial
                } else {
                    p.height += delta
                }
            }
            Self::Sphere(p) => p.diameter += radial,
            Self::Hemisphere(p) => p.diameter += 2.0 * delta,
            Self::Cone(p) => {
                if axis < 2 {
                    p.bottom_diameter += radial
                } else {
                    p.height += delta
                }
            }
            Self::Torus(p) => {
                if axis < 2 {
                    p.major_diameter += radial
                } else {
                    p.tube_diameter += delta
                }
            }
            Self::Sketch(p) => {
                if axis == 2 {
                    p.thickness += delta;
                } else {
                    let min = p
                        .points
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::INFINITY, f64::min);
                    let max = p
                        .points
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::NEG_INFINITY, f64::max);
                    let factor = (max - min + radial) / (max - min);
                    if !factor.is_finite() || factor <= 0.0 {
                        return Err("크기를 조절할 수 없어요".into());
                    }
                    let center = (min + max) / 2.0;
                    for point in p.points.iter_mut().chain(&mut p.stroke) {
                        point[axis] = center + (point[axis] - center) * factor;
                    }
                }
            }
            Self::Text(p) => {
                if axis < 2 {
                    p.height += 2.0 * delta
                } else {
                    p.thickness += delta
                }
            }
        }
        self.validate()
    }
}
impl Scene {
    pub fn world_matrix(&self, id: NodeId) -> Option<DMat4> {
        fn find(scene: &Scene, current: NodeId, id: NodeId, parent: DMat4) -> Option<DMat4> {
            let node = scene.nodes.get(&current)?;
            let world = parent * node.transform.matrix();
            if current == id {
                return Some(world);
            }
            match &node.kind {
                NodeKind::Primitive(_) => None,
                NodeKind::Group { children } | NodeKind::Boolean { children, .. } => children
                    .iter()
                    .find_map(|&child| find(scene, child, id, world)),
            }
        }
        self.roots
            .iter()
            .find_map(|&root| find(self, root, id, DMat4::IDENTITY))
    }
    pub fn root_map(&self) -> BTreeMap<NodeId, NodeId> {
        fn walk(scene: &Scene, id: NodeId, root: NodeId, map: &mut BTreeMap<NodeId, NodeId>) {
            map.insert(id, root);
            match &scene.nodes[&id].kind {
                NodeKind::Primitive(_) => {}
                NodeKind::Group { children } | NodeKind::Boolean { children, .. } => {
                    for &child in children {
                        walk(scene, child, root, map);
                    }
                }
            }
        }
        let mut map = BTreeMap::new();
        for &root in &self.roots {
            walk(self, root, root, &mut map);
        }
        map
    }
}
pub fn aligned_nodes(
    scene: &Scene,
    evaluated: &[Evaluated],
    ids: &BTreeSet<NodeId>,
    axis: usize,
    mode: AlignMode,
) -> CoreResult<Vec<Node>> {
    if axis > 2 || ids.len() < 2 || ids.iter().any(|id| !scene.roots.contains(id)) {
        return Err("정렬할 최상위 도형을 두 개 이상 선택하세요".into());
    }
    let owners = scene.root_map();
    let mut bounds = BTreeMap::new();
    for item in evaluated {
        let root = owners[&item.id];
        if !ids.contains(&root) {
            continue;
        }
        let entry = bounds
            .entry(root)
            .or_insert((f64::INFINITY, f64::NEG_INFINITY));
        for &point in item.mesh.vertices() {
            let value = item.world.transform_point3(DVec3::from_array(point))[axis];
            entry.0 = entry.0.min(value);
            entry.1 = entry.1.max(value);
        }
    }
    if bounds.len() != ids.len() {
        return Err("빈 형상은 정렬할 수 없습니다".into());
    }
    let min = bounds.values().map(|b| b.0).fold(f64::INFINITY, f64::min);
    let max = bounds
        .values()
        .map(|b| b.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let position = |b: (f64, f64)| match mode {
        AlignMode::Minimum => b.0,
        AlignMode::Center => (b.0 + b.1) / 2.0,
        AlignMode::Maximum => b.1,
    };
    let target = position((min, max));
    Ok(ids
        .iter()
        .map(|id| {
            let mut node = scene.nodes[id].clone();
            node.transform.position[axis] += target - position(bounds[id]);
            node
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn align_rotated_group_world_bounds_and_undo_as_one_command() {
        let mut editor = Editor::default();
        let a = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let b = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        let mut node = editor.scene.nodes[&b].clone();
        node.transform.position = [100.0, 30.0, 20.0];
        node.transform.rotation = [0.0, 30.0, 25.0];
        editor.apply(Command::SetNode(node)).unwrap();
        let before = editor.scene.clone();
        let evaluated = before.evaluate().unwrap();
        let updates = aligned_nodes(
            &before,
            &evaluated,
            &BTreeSet::from([a, b]),
            2,
            AlignMode::Minimum,
        )
        .unwrap();
        editor.apply(Command::SetNodes(updates)).unwrap();
        let evaluated = editor.scene.evaluate().unwrap();
        let mins: Vec<_> = evaluated
            .iter()
            .map(|n| {
                n.mesh
                    .vertices()
                    .iter()
                    .map(|&p| n.world.transform_point3(DVec3::from_array(p)).z)
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        assert!((mins[0] - mins[1]).abs() < 1e-8);
        editor.undo();
        assert_eq!(editor.scene, before);
    }
    #[test]
    fn resizing_keeps_thickness_and_failed_bulk_edit_is_atomic() {
        let mut editor = Editor::default();
        let id = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        let mut node = editor.scene.nodes[&id].clone();
        let NodeKind::Primitive(p) = &mut node.kind else {
            unreachable!()
        };
        let Primitive::Cylinder(c) = p else {
            unreachable!()
        };
        c.hollow.enabled = true;
        let wall = c.hollow.wall;
        p.resize_axis(0, 10.0).unwrap();
        let Primitive::Cylinder(c) = p else {
            unreachable!()
        };
        assert_eq!(c.diameter, 80.0);
        assert_eq!(c.hollow.wall, wall);
        editor.apply(Command::SetNodes(vec![node.clone()])).unwrap();
        let before = editor.scene.clone();
        let NodeKind::Primitive(p) = &mut node.kind else {
            unreachable!()
        };
        assert!(p.resize_axis(0, -100.0).is_err());
        assert!(editor.apply(Command::SetNodes(vec![node])).is_err());
        assert_eq!(editor.scene, before);
    }
}
