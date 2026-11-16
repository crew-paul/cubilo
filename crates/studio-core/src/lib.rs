//! Serializable editable scene, commands and bounded parameter-only undo history.
mod edit;
mod plate;
pub use edit::{aligned_nodes, AlignMode};
use glam::{DMat4, DQuat, DVec3, EulerRot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use studio_geometry::{
    boolean::{boolean, BooleanOp, CancelToken},
    TextParams,
};
use studio_geometry::{
    BoxParams, ConeParams, CylinderParams, HemisphereParams, Hollow, Mesh, SphereParams,
    TorusParams,
};

pub type NodeId = u64;
pub type CoreResult<T> = Result<T, String>;
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_NODES: usize = 5000;
pub const MAX_TRIANGLES: usize = 2_000_000;
const MAX_HISTORY: usize = 100;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub position: [f64; 3],
    /// XYZ Euler angles in degrees; no scale (primitive dimensions own size).
    pub rotation: [f64; 3],
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
        }
    }
}
impl Transform {
    pub fn matrix(&self) -> DMat4 {
        let [x, y, z] = self.rotation.map(f64::to_radians);
        DMat4::from_rotation_translation(
            DQuat::from_euler(EulerRot::XYZ, x, y, z),
            DVec3::from_array(self.position),
        )
    }
    fn from_matrix(matrix: DMat4) -> Self {
        let (_, q, t) = matrix.to_scale_rotation_translation();
        let (x, y, z) = q.to_euler(EulerRot::XYZ);
        Self {
            position: t.to_array(),
            rotation: [x, y, z].map(f64::to_degrees),
        }
    }
    fn validate(&self) -> CoreResult<()> {
        if self
            .position
            .iter()
            .chain(&self.rotation)
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        {
            return Err("위치·회전 값이 허용 범위를 벗어났습니다".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
pub enum Primitive {
    Box(BoxParams),
    Cylinder(CylinderParams),
    Sphere(SphereParams),
    Cone(ConeParams),
    Torus(TorusParams),
    Hemisphere(HemisphereParams),
    Text(TextParams),
    Sketch(studio_geometry::SketchParams),
}
impl Primitive {
    pub fn box_default() -> Self {
        Self::Box(BoxParams {
            rounded: Default::default(),
            width: 60.0,
            depth: 50.0,
            height: 80.0,
            hollow: Hollow::default(),
        })
    }
    pub fn cylinder_default() -> Self {
        Self::Cylinder(CylinderParams {
            rounded: Default::default(),
            taper: Default::default(),
            diameter: 60.0,
            height: 80.0,
            segments: 64,
            hollow: Hollow::default(),
        })
    }
    pub fn label(&self) -> &str {
        match self {
            Self::Box(_) => "상자",
            Self::Cylinder(_) => "원기둥",
            Self::Sphere(_) => "구",
            Self::Cone(_) => "원뿔",
            Self::Torus(_) => "토러스",
            Self::Hemisphere(_) => "반구",
            Self::Text(_) => "텍스트",
            Self::Sketch(_) => "내 모양",
        }
    }
    pub fn validate(&self) -> CoreResult<()> {
        match self {
            Self::Box(p) => p.validate(),
            Self::Cylinder(p) => p.validate(),
            Self::Sphere(p) => p.validate(),
            Self::Cone(p) => p.validate(),
            Self::Torus(p) => p.validate(),
            Self::Hemisphere(p) => p.validate(),
            Self::Text(p) => p.validate(),
            Self::Sketch(p) => p.validate(),
        }
        .map_err(str::to_string)
    }
    pub fn estimated_triangles(&self) -> usize {
        match self {
            Self::Box(_) => 416,
            Self::Cylinder(p) => p
                .segments
                .saturating_mul(if p.rounded.enabled { 52 } else { 8 }),
            Self::Sphere(p) => p.segments.saturating_mul(p.rings).saturating_mul(2),
            Self::Hemisphere(p) => p.segments.saturating_mul(p.rings).saturating_mul(2),
            Self::Torus(p) => p.segments.saturating_mul(p.tube_segments).saturating_mul(2),
            Self::Cone(p) => p.segments.saturating_mul(4),
            Self::Text(p) => p.text.chars().count().saturating_mul(12_000),
            Self::Sketch(p) => p.points.len().saturating_mul(4),
        }
    }
    pub fn mesh(&self) -> CoreResult<Mesh> {
        match self {
            Self::Box(p) => p.mesh(),
            Self::Cylinder(p) => p.mesh(),
            Self::Sphere(p) => p.mesh(),
            Self::Cone(p) => p.mesh(),
            Self::Torus(p) => p.mesh(),
            Self::Hemisphere(p) => p.mesh(),
            Self::Text(p) => p.mesh(),
            Self::Sketch(p) => p.mesh(),
        }
        .map_err(str::to_string)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum NodeKind {
    Primitive(Primitive),
    Group {
        children: Vec<NodeId>,
    },
    Boolean {
        operation: BooleanOp,
        children: Vec<NodeId>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub transform: Transform,
    pub kind: NodeKind,
    pub color: [f32; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scene {
    pub schema_version: u32,
    pub unit: String,
    pub nodes: BTreeMap<NodeId, Node>,
    pub roots: Vec<NodeId>,
    pub next_id: NodeId,
}
impl Default for Scene {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            unit: "mm".into(),
            nodes: BTreeMap::new(),
            roots: Vec::new(),
            next_id: 1,
        }
    }
}
pub struct Evaluated {
    pub id: NodeId,
    pub mesh: Arc<Mesh>,
    pub world: DMat4,
    pub color: [f32; 3],
}
impl Evaluated {
    pub fn world_mesh(&self) -> CoreResult<Mesh> {
        Mesh::from_parts(
            self.mesh
                .vertices()
                .iter()
                .map(|&p| self.world.transform_point3(DVec3::from_array(p)).to_array())
                .collect(),
            self.mesh.triangles().to_vec(),
        )
        .map_err(str::to_string)
    }
}
impl Scene {
    fn allocate(&mut self) -> CoreResult<NodeId> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("객체 ID 한도 초과")?;
        Ok(id)
    }
    pub fn validate(&self) -> CoreResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err("지원하지 않는 프로젝트 버전입니다".into());
        }
        if self.unit != "mm" || self.nodes.len() > MAX_NODES || self.next_id == 0 {
            return Err("잘못된 프로젝트 설정입니다".into());
        }
        for (&id, node) in &self.nodes {
            if id == 0 || id != node.id || id >= self.next_id || node.name.len() > 1024 {
                return Err("잘못된 객체 ID 또는 이름입니다".into());
            }
            node.transform.validate()?;
            if node
                .color
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err("잘못된 도형 색상입니다".into());
            }
        }
        let complexity: usize = self
            .nodes
            .values()
            .map(|n| match &n.kind {
                NodeKind::Primitive(p) => p.estimated_triangles(),
                _ => 0,
            })
            .try_fold(0usize, |sum, n| sum.checked_add(n))
            .ok_or("프로젝트 메시가 너무 복잡합니다")?;
        if complexity > MAX_TRIANGLES {
            return Err("프로젝트 메시가 너무 복잡합니다".into());
        }
        let mut seen = BTreeSet::new();
        for &id in &self.roots {
            self.visit(id, 0, &mut seen)?;
        }
        if seen.len() != self.nodes.len() {
            return Err("연결되지 않은 객체가 있습니다".into());
        }
        Ok(())
    }
    fn visit(&self, id: NodeId, depth: usize, seen: &mut BTreeSet<NodeId>) -> CoreResult<()> {
        if depth > 64 || !seen.insert(id) {
            return Err("중복·순환 참조 또는 지나치게 깊은 그룹입니다".into());
        }
        let node = self.nodes.get(&id).ok_or("객체 참조가 없습니다")?;
        if let NodeKind::Boolean { children, .. } = &node.kind {
            if children.len() < 2 || children.len() > 32 || depth > 16 {
                return Err("Boolean은 2~32개 도형과 깊이 16까지 지원합니다".into());
            }
        }
        match &node.kind {
            NodeKind::Primitive(p) => {
                p.validate()?;
            }
            NodeKind::Group { children } | NodeKind::Boolean { children, .. } => {
                if children.is_empty() {
                    return Err("빈 그룹입니다".into());
                }
                for &id in children {
                    self.visit(id, depth + 1, seen)?;
                }
            }
        }
        Ok(())
    }
    pub fn evaluate(&self) -> CoreResult<Vec<Evaluated>> {
        Evaluator::default().evaluate(self)
    }
    pub fn export_mesh(&self) -> CoreResult<Mesh> {
        merge_evaluated(self.evaluate()?)
    }
    fn remove_tree(&mut self, id: NodeId) {
        if let Some(Node {
            kind: NodeKind::Group { children } | NodeKind::Boolean { children, .. },
            ..
        }) = self.nodes.remove(&id)
        {
            for child in children {
                self.remove_tree(child);
            }
        }
    }
    fn duplicate_tree(&mut self, id: NodeId) -> CoreResult<NodeId> {
        let mut node = self.nodes.get(&id).ok_or("복제할 객체가 없습니다")?.clone();
        node.id = self.allocate()?;
        if let NodeKind::Group { children } | NodeKind::Boolean { children, .. } = &mut node.kind {
            *children = children
                .clone()
                .into_iter()
                .map(|child| self.duplicate_tree(child))
                .collect::<CoreResult<_>>()?;
        }
        let new = node.id;
        self.nodes.insert(new, node);
        Ok(new)
    }
}
pub fn merge_evaluated(evaluated: Vec<Evaluated>) -> CoreResult<Mesh> {
    if evaluated.is_empty() {
        return Err("내보낼 형상이 없습니다".into());
    }
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for item in evaluated {
        let mesh = item.world_mesh()?;
        let offset = vertices.len();
        vertices.extend_from_slice(mesh.vertices());
        triangles.extend(mesh.triangles().iter().map(|t| t.map(|i| i + offset)));
    }
    Mesh::from_parts(vertices, triangles).map_err(str::to_string)
}

#[derive(Default)]
pub struct Evaluator {
    cache: BTreeMap<NodeId, (Primitive, Arc<Mesh>)>,
    booleans: BTreeMap<NodeId, (Vec<Node>, Option<Arc<Mesh>>)>,
}
impl Evaluator {
    pub fn evaluate(&mut self, scene: &Scene) -> CoreResult<Vec<Evaluated>> {
        self.evaluate_with_token(scene, &CancelToken::new())
    }
    pub fn evaluate_with_token(
        &mut self,
        scene: &Scene,
        token: &CancelToken,
    ) -> CoreResult<Vec<Evaluated>> {
        scene.validate()?;
        self.cache.retain(|id, _| scene.nodes.contains_key(id));
        self.booleans.retain(|id, _| scene.nodes.contains_key(id));
        let mut output = Vec::new();
        for &id in &scene.roots {
            self.node(scene, id, DMat4::IDENTITY, &mut output, token)?;
        }
        if output
            .iter()
            .map(|n| n.mesh.triangles().len())
            .sum::<usize>()
            > MAX_TRIANGLES
        {
            return Err("평가 결과가 너무 복잡합니다".into());
        }
        Ok(output)
    }
    fn node(
        &mut self,
        scene: &Scene,
        id: NodeId,
        parent: DMat4,
        out: &mut Vec<Evaluated>,
        token: &CancelToken,
    ) -> CoreResult<()> {
        if token.is_cancelled() {
            return Err("계산이 취소되었습니다".into());
        }
        let node = &scene.nodes[&id];
        let world = parent * node.transform.matrix();
        match &node.kind {
            NodeKind::Primitive(p) => {
                if self.cache.get(&id).is_none_or(|(old, _)| old != p) {
                    self.cache.insert(id, (p.clone(), Arc::new(p.mesh()?)));
                }
                out.push(Evaluated {
                    id,
                    world,
                    color: node.color,
                    mesh: self.cache[&id].1.clone(),
                });
            }
            NodeKind::Group { children } => {
                for &child in children {
                    self.node(scene, child, world, out, token)?;
                }
            }
            NodeKind::Boolean {
                operation,
                children,
            } => {
                let key = subtree_key(scene, id);
                if self.booleans.get(&id).is_none_or(|(old, _)| *old != key) {
                    let mut operands = children.iter();
                    let mut result = self.operand(
                        scene,
                        *operands.next().ok_or("Boolean 입력이 없습니다")?,
                        token,
                    )?;
                    for &child in operands {
                        result = combine(
                            result,
                            self.operand(scene, child, token)?,
                            *operation,
                            token,
                        )?;
                    }
                    self.booleans.insert(id, (key, result.map(Arc::new)));
                }
                if let Some(mesh) = &self.booleans[&id].1 {
                    out.push(Evaluated {
                        id,
                        world,
                        color: node.color,
                        mesh: mesh.clone(),
                    });
                }
            }
        }
        Ok(())
    }
    fn operand(
        &mut self,
        scene: &Scene,
        id: NodeId,
        token: &CancelToken,
    ) -> CoreResult<Option<Mesh>> {
        let mut items = Vec::new();
        self.node(scene, id, DMat4::IDENTITY, &mut items, token)?;
        let mut result = None;
        for item in items {
            result = combine(result, Some(item.world_mesh()?), BooleanOp::Union, token)?;
        }
        Ok(result)
    }
}
fn combine(
    a: Option<Mesh>,
    b: Option<Mesh>,
    operation: BooleanOp,
    token: &CancelToken,
) -> CoreResult<Option<Mesh>> {
    match (a, b, operation) {
        (Some(a), Some(b), op) => boolean(&a, &b, op, token),
        (Some(a), None, BooleanOp::Union | BooleanOp::Difference) => Ok(Some(a)),
        (None, Some(b), BooleanOp::Union) => Ok(Some(b)),
        _ => Ok(None),
    }
}
fn subtree_key(scene: &Scene, id: NodeId) -> Vec<Node> {
    fn collect(scene: &Scene, id: NodeId, out: &mut Vec<Node>) {
        let node = scene.nodes[&id].clone();
        let children = match &node.kind {
            NodeKind::Primitive(_) => Vec::new(),
            NodeKind::Group { children } | NodeKind::Boolean { children, .. } => children.clone(),
        };
        out.push(node);
        for child in children {
            collect(scene, child, out);
        }
    }
    let mut nodes = Vec::new();
    collect(scene, id, &mut nodes);
    nodes[0].transform = Transform::default();
    nodes
}

#[derive(Clone, Debug)]
pub enum Command {
    Add(Primitive),
    SetNode(Node),
    SetNodes(Vec<Node>),
    Delete(Vec<NodeId>),
    Duplicate(Vec<NodeId>),
    Group(Vec<NodeId>),
    Boolean {
        operation: BooleanOp,
        children: Vec<NodeId>,
    },
    Ungroup(NodeId),
}
struct History {
    before: Scene,
    after: Scene,
}
#[derive(Default)]
pub struct Editor {
    pub scene: Scene,
    pub revision: u64,
    undo: Vec<History>,
    redo: Vec<History>,
    gesture: Option<Scene>,
    plate: plate::PlateCache,
}
impl Editor {
    pub fn cache_evaluated(&mut self, items: &[Evaluated]) {
        self.plate.remember(&self.scene, items);
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn replace_scene(&mut self, mut scene: Scene) -> CoreResult<()> {
        scene.validate()?;
        self.plate.enforce(&mut scene)?;
        self.scene = scene;
        self.undo.clear();
        self.redo.clear();
        self.gesture = None;
        self.revision += 1;
        Ok(())
    }
    pub fn begin_gesture(&mut self) {
        if self.gesture.is_none() {
            self.gesture = Some(self.scene.clone());
        }
    }
    pub fn end_gesture(&mut self) {
        if let Some(before) = self.gesture.take() {
            self.record(before);
        }
    }
    fn record(&mut self, before: Scene) {
        if before != self.scene {
            self.redo.clear();
            self.undo.push(History {
                before,
                after: self.scene.clone(),
            });
            if self.undo.len() > MAX_HISTORY {
                self.undo.remove(0);
            }
        }
    }
    pub fn apply(&mut self, command: Command) -> CoreResult<Vec<NodeId>> {
        let before = self.scene.clone();
        let mut candidate = before.clone();
        let selected = match command {
            Command::Add(primitive) => {
                let id = candidate.allocate()?;
                let name = format!("{} {}", primitive.label(), id);
                candidate.nodes.insert(
                    id,
                    Node {
                        id,
                        name,
                        transform: Transform::default(),
                        kind: NodeKind::Primitive(primitive),
                        color: [0.23, 0.62, 0.86],
                    },
                );
                candidate.roots.push(id);
                vec![id]
            }
            Command::SetNode(node) => {
                if !candidate.nodes.contains_key(&node.id) {
                    return Err("객체가 없습니다".into());
                }
                let id = node.id;
                candidate.nodes.insert(id, node);
                vec![id]
            }
            Command::SetNodes(nodes) => {
                let ids: Vec<_> = nodes.iter().map(|n| n.id).collect();
                if ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
                    || ids.iter().any(|id| !candidate.nodes.contains_key(id))
                {
                    return Err("수정할 객체 ID가 잘못되었습니다".into());
                }
                for node in nodes {
                    candidate.nodes.insert(node.id, node);
                }
                ids
            }
            Command::Delete(ids) => {
                if ids.iter().any(|id| !candidate.roots.contains(id)) {
                    return Err("삭제할 최상위 객체를 선택하세요".into());
                }
                for id in ids {
                    candidate.roots.retain(|&root| root != id);
                    candidate.remove_tree(id);
                }
                vec![]
            }
            Command::Duplicate(ids) => {
                if ids.iter().any(|id| !candidate.roots.contains(id)) {
                    return Err("복제할 최상위 객체를 선택하세요".into());
                }
                let mut result = Vec::new();
                for id in ids {
                    let new = candidate.duplicate_tree(id)?;
                    let node = candidate.nodes.get_mut(&new).unwrap();
                    node.transform.position[0] += 10.0;
                    node.name.push_str(" 복사");
                    candidate.roots.push(new);
                    result.push(new);
                }
                result
            }
            Command::Group(ids) => {
                let unique: BTreeSet<_> = ids.iter().copied().collect();
                if unique.len() < 2
                    || unique.len() != ids.len()
                    || ids.iter().any(|id| !candidate.roots.contains(id))
                {
                    return Err("두 개 이상의 최상위 객체를 선택하세요".into());
                }
                let id = candidate.allocate()?;
                candidate.roots.retain(|root| !unique.contains(root));
                candidate.roots.push(id);
                candidate.nodes.insert(
                    id,
                    Node {
                        id,
                        name: format!("그룹 {id}"),
                        transform: Transform::default(),
                        kind: NodeKind::Group { children: ids },
                        color: [0.23, 0.62, 0.86],
                    },
                );
                vec![id]
            }
            Command::Boolean {
                operation,
                children: ids,
            } => {
                let unique: BTreeSet<_> = ids.iter().copied().collect();
                if unique.len() < 2
                    || unique.len() != ids.len()
                    || ids.iter().any(|id| !candidate.roots.contains(id))
                {
                    return Err("두 개 이상의 최상위 객체를 선택하세요".into());
                }
                let id = candidate.allocate()?;
                candidate.roots.retain(|root| !unique.contains(root));
                candidate.roots.push(id);
                candidate.nodes.insert(
                    id,
                    Node {
                        id,
                        name: format!("{} {id}", operation.label()),
                        transform: Transform::default(),
                        kind: NodeKind::Boolean {
                            operation,
                            children: ids,
                        },
                        color: [0.23, 0.62, 0.86],
                    },
                );
                vec![id]
            }
            Command::Ungroup(id) => {
                if !candidate.roots.contains(&id) {
                    return Err("최상위 그룹을 선택하세요".into());
                }
                let node = candidate.nodes.get(&id).ok_or("그룹이 없습니다")?.clone();
                let NodeKind::Group { children } = node.kind else {
                    return Err("그룹을 선택하세요".into());
                };
                for child in &children {
                    let n = candidate.nodes.get_mut(child).ok_or("자식이 없습니다")?;
                    n.transform =
                        Transform::from_matrix(node.transform.matrix() * n.transform.matrix());
                }
                candidate.nodes.remove(&id);
                candidate.roots.retain(|&root| root != id);
                candidate.roots.extend_from_slice(&children);
                children
            }
        };
        candidate.validate()?;
        self.plate.enforce(&mut candidate)?;
        candidate.validate()?;
        if candidate != before {
            self.scene = candidate;
            self.revision += 1;
            if self.gesture.is_none() {
                self.record(before);
            }
        }
        Ok(selected)
    }
    pub fn undo(&mut self) {
        self.end_gesture();
        if let Some(h) = self.undo.pop() {
            self.scene = h.before.clone();
            self.redo.push(h);
            self.revision += 1;
        }
    }
    pub fn redo(&mut self) {
        self.end_gesture();
        if let Some(h) = self.redo.pop() {
            self.scene = h.after.clone();
            self.undo.push(h);
            self.revision += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn add(editor: &mut Editor) -> NodeId {
        editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0]
    }
    #[test]
    fn invalid_resize_is_atomic_and_gesture_is_one_undo() {
        let mut editor = Editor::default();
        let id = add(&mut editor);
        let mut node = editor.scene.nodes[&id].clone();
        let NodeKind::Primitive(Primitive::Box(p)) = &mut node.kind else {
            unreachable!()
        };
        p.hollow.enabled = true;
        editor.apply(Command::SetNode(node)).unwrap();
        let before = editor.scene.clone();
        let revision = editor.revision;
        let mut invalid = editor.scene.nodes[&id].clone();
        let NodeKind::Primitive(Primitive::Box(p)) = &mut invalid.kind else {
            unreachable!()
        };
        p.width = 1.0;
        assert!(editor.apply(Command::SetNode(invalid)).is_err());
        assert_eq!(editor.scene, before);
        assert_eq!(editor.revision, revision);
        editor.begin_gesture();
        for x in [1.0, 2.0, 3.0] {
            let mut node = editor.scene.nodes[&id].clone();
            node.transform.position[0] = x;
            editor.apply(Command::SetNode(node)).unwrap();
        }
        editor.end_gesture();
        let after = editor.scene.clone();
        editor.undo();
        assert_eq!(editor.scene, before);
        editor.redo();
        assert_eq!(editor.scene, after);
        editor.undo();
        editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap();
        assert!(!editor.can_redo());
    }
    #[test]
    fn grouping_duplication_and_ungroup_keep_world_geometry() {
        let mut editor = Editor::default();
        let a = add(&mut editor);
        let b = add(&mut editor);
        let mut node = editor.scene.nodes[&b].clone();
        node.transform.position = [100.0, 20.0, 0.0];
        editor.apply(Command::SetNode(node)).unwrap();
        let group = editor.apply(Command::Group(vec![a, b])).unwrap()[0];
        let mut node = editor.scene.nodes[&group].clone();
        node.transform.position = [10.0, 20.0, 30.0];
        node.transform.rotation = [20.0, 30.0, 40.0];
        editor.apply(Command::SetNode(node)).unwrap();
        let world_before: Vec<_> = editor
            .scene
            .evaluate()
            .unwrap()
            .iter()
            .map(|n| n.world)
            .collect();
        editor.apply(Command::Ungroup(group)).unwrap();
        for (a, b) in world_before.iter().zip(editor.scene.evaluate().unwrap()) {
            assert!(a.abs_diff_eq(b.world, 1e-9));
        }
        let duplicate = editor.apply(Command::Duplicate(vec![a])).unwrap()[0];
        assert_ne!(duplicate, a);
        assert_eq!(
            editor.scene.nodes[&duplicate].transform.position[0],
            editor.scene.nodes[&a].transform.position[0] + 10.0
        );
        let group = editor.apply(Command::Group(vec![a, b])).unwrap()[0];
        let copy = editor.apply(Command::Duplicate(vec![group])).unwrap()[0];
        let NodeKind::Group { children } = &editor.scene.nodes[&copy].kind else {
            unreachable!()
        };
        assert!(!children.contains(&a));
        assert!(!children.contains(&b));
        editor.apply(Command::Delete(vec![group])).unwrap();
        assert!(!editor.scene.nodes.contains_key(&a));
        editor.undo();
        assert!(editor.scene.nodes.contains_key(&a));
    }
    #[test]
    fn rejects_cycles_orphans_bad_ids_and_future_versions() {
        let mut editor = Editor::default();
        let id = add(&mut editor);
        let valid = editor.scene.clone();
        editor.scene.roots.push(id);
        assert!(editor.scene.validate().is_err());
        editor.scene = valid.clone();
        editor.scene.roots.clear();
        assert!(editor.scene.validate().is_err());
        editor.scene = valid.clone();
        editor.scene.nodes.get_mut(&id).unwrap().kind = NodeKind::Group { children: vec![id] };
        assert!(editor.scene.validate().is_err());
        editor.scene = valid.clone();
        editor.scene.schema_version += 1;
        assert!(editor.scene.validate().is_err());
        editor.scene = valid;
        editor.scene.next_id = id;
        assert!(editor.scene.validate().is_err());
    }
}

/// Two-sided ray picking: works on inner cavity faces and transformed objects.
pub fn pick(items: &[Evaluated], origin: DVec3, direction: DVec3) -> Option<(NodeId, f64)> {
    let mut nearest = None;
    for item in items {
        let inverse = item.world.inverse();
        let o = inverse.transform_point3(origin);
        let d = inverse.transform_vector3(direction);
        for &[a, b, c] in item.mesh.triangles() {
            let [a, b, c] = [a, b, c].map(|i| DVec3::from_array(item.mesh.vertices()[i]));
            let e1 = b - a;
            let e2 = c - a;
            let p = d.cross(e2);
            let determinant = e1.dot(p);
            if determinant.abs() < 1e-12 {
                continue;
            }
            let inv = 1.0 / determinant;
            let t = o - a;
            let u = t.dot(p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = t.cross(e1);
            let v = d.dot(q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let distance = e2.dot(q) * inv;
            if distance >= 0.0 && nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((item.id, distance));
            }
        }
    }
    nearest
}
impl Scene {
    pub fn root_for(&self, id: NodeId) -> Option<NodeId> {
        fn contains(scene: &Scene, parent: NodeId, id: NodeId) -> bool {
            parent == id
                || match &scene.nodes[&parent].kind {
                    NodeKind::Primitive(_) => false,
                    NodeKind::Group { children } | NodeKind::Boolean { children, .. } => {
                        children.iter().any(|&child| contains(scene, child, id))
                    }
                }
        }
        self.roots
            .iter()
            .copied()
            .find(|&root| contains(self, root, id))
    }
}
#[cfg(test)]
mod evaluation_tests {
    use super::*;
    #[test]
    fn transforms_reuse_mesh_and_pick_nearest_world_face() {
        let mut editor = Editor::default();
        let a = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let b = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let mut node = editor.scene.nodes[&b].clone();
        node.transform.position = [0.0, 0.0, 100.0];
        editor.apply(Command::SetNode(node)).unwrap();
        let mut evaluator = Evaluator::default();
        let first = evaluator.evaluate(&editor.scene).unwrap();
        assert_eq!(
            pick(&first, DVec3::new(0.0, 0.0, 300.0), -DVec3::Z)
                .unwrap()
                .0,
            b
        );
        assert!(pick(&first, DVec3::new(1000.0, 0.0, 300.0), -DVec3::Z).is_none());
        let mut node = editor.scene.nodes[&a].clone();
        node.transform.position = [30.0, 0.0, 0.0];
        editor.apply(Command::SetNode(node)).unwrap();
        let second = evaluator.evaluate(&editor.scene).unwrap();
        assert!(Arc::ptr_eq(&first[0].mesh, &second[0].mesh));
        let mut node = editor.scene.nodes[&a].clone();
        let NodeKind::Primitive(Primitive::Box(p)) = &mut node.kind else {
            unreachable!()
        };
        p.width = 100.0;
        editor.apply(Command::SetNode(node)).unwrap();
        let third = evaluator.evaluate(&editor.scene).unwrap();
        assert!(!Arc::ptr_eq(&second[0].mesh, &third[0].mesh));
    }
}

#[cfg(test)]
mod csg_tests {
    use super::*;
    #[test]
    fn child_edits_undo_and_parent_moves_preserve_csg_sources() {
        let mut editor = Editor::default();
        let a = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let b = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let mut node = editor.scene.nodes[&b].clone();
        node.transform.position[0] = 30.0;
        editor.apply(Command::SetNode(node)).unwrap();
        let id = editor
            .apply(Command::Boolean {
                operation: BooleanOp::Difference,
                children: vec![a, b],
            })
            .unwrap()[0];
        let mut evaluator = Evaluator::default();
        let before = evaluator.evaluate(&editor.scene).unwrap();
        assert_eq!(before.len(), 1);
        assert!(editor.scene.nodes.contains_key(&a));
        assert!(editor.scene.nodes.contains_key(&b));
        let mut parent = editor.scene.nodes[&id].clone();
        parent.transform.position[0] = 50.0;
        editor.apply(Command::SetNode(parent)).unwrap();
        let moved = evaluator.evaluate(&editor.scene).unwrap();
        assert!(Arc::ptr_eq(&before[0].mesh, &moved[0].mesh));
        let mut child = editor.scene.nodes[&b].clone();
        child.transform.position[0] = 10.0;
        editor.apply(Command::SetNode(child)).unwrap();
        let changed = evaluator.evaluate(&editor.scene).unwrap();
        assert!(changed[0].mesh.signed_volume() < before[0].mesh.signed_volume());
        editor.undo();
        let restored = evaluator.evaluate(&editor.scene).unwrap();
        assert!((restored[0].mesh.signed_volume() - before[0].mesh.signed_volume()).abs() < 1e-6);
    }
    #[test]
    fn korean_keyring_text_union_and_through_hole() {
        let mut editor = Editor::default();
        let mut base = Primitive::box_default();
        let Primitive::Box(p) = &mut base else {
            unreachable!()
        };
        p.width = 80.0;
        p.depth = 32.0;
        p.height = 4.0;
        p.rounded.enabled = true;
        let a = editor.apply(Command::Add(base)).unwrap()[0];
        let t = editor
            .apply(Command::Add(Primitive::Text(TextParams {
                height: 16.0,
                ..TextParams::default()
            })))
            .unwrap()[0];
        let mut text = editor.scene.nodes[&t].clone();
        text.transform.position[2] = 3.5;
        editor.apply(Command::SetNode(text)).unwrap();
        let union = editor
            .apply(Command::Boolean {
                operation: BooleanOp::Union,
                children: vec![a, t],
            })
            .unwrap()[0];
        let joined = editor.scene.export_mesh().unwrap();
        joined.validate().unwrap();
        assert!(joined.signed_volume() > 80.0 * 32.0 * 4.0 - 1000.0);
        let mut hole = Primitive::cylinder_default();
        let Primitive::Cylinder(p) = &mut hole else {
            unreachable!()
        };
        p.diameter = 6.0;
        p.height = 8.0;
        let h = editor.apply(Command::Add(hole)).unwrap()[0];
        let mut node = editor.scene.nodes[&h].clone();
        node.transform.position = [-32.0, 0.0, -1.0];
        editor.apply(Command::SetNode(node)).unwrap();
        editor
            .apply(Command::Boolean {
                operation: BooleanOp::Difference,
                children: vec![union, h],
            })
            .unwrap();
        let final_mesh = editor.scene.export_mesh().unwrap();
        final_mesh.validate().unwrap();
        assert!(final_mesh.signed_volume() < joined.signed_volume());
    }
}
