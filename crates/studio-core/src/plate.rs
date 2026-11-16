//! Keep visible root solids above Z=0, preserving local Boolean cutters.
use crate::*;
#[derive(Default)]
pub(crate) struct PlateCache {
    meshes: BTreeMap<NodeId, (Vec<Node>, Arc<Mesh>)>,
}
impl PlateCache {
    pub fn remember(&mut self, scene: &Scene, items: &[Evaluated]) {
        for item in items {
            self.meshes
                .insert(item.id, (subtree_key(scene, item.id), item.mesh.clone()));
        }
    }
    pub fn enforce(&mut self, scene: &mut Scene) -> CoreResult<()> {
        self.meshes.retain(|id, _| scene.nodes.contains_key(id));
        for id in scene.roots.clone() {
            let bottom = self.minimum(scene, id, DMat4::IDENTITY)?;
            if bottom < -1e-9 {
                scene.nodes.get_mut(&id).unwrap().transform.position[2] -= bottom;
            }
        }
        Ok(())
    }
    fn minimum(&mut self, scene: &Scene, id: NodeId, parent: DMat4) -> CoreResult<f64> {
        let node = &scene.nodes[&id];
        let world = parent * node.transform.matrix();
        let key = subtree_key(scene, id);
        if self.meshes.get(&id).is_none_or(|(old, _)| *old != key) {
            if let NodeKind::Primitive(p) = &node.kind {
                self.meshes.insert(id, (key.clone(), Arc::new(p.mesh()?)));
            }
        }
        if let Some((old, mesh)) = self.meshes.get(&id) {
            if *old == key {
                return Ok(mesh
                    .vertices()
                    .iter()
                    .map(|&p| world.transform_point3(DVec3::from_array(p)).z)
                    .fold(f64::INFINITY, f64::min));
            }
        }
        // Before a new CSG result is available, use a conservative bound. Subtractors
        // may cross below the plate; only the visible base constrains a difference.
        let (children, intersection) = match &node.kind {
            NodeKind::Primitive(_) => unreachable!(),
            NodeKind::Group { children } => (children.as_slice(), false),
            NodeKind::Boolean {
                operation: BooleanOp::Difference,
                children,
            } => (&children[..1], false),
            NodeKind::Boolean {
                operation,
                children,
            } => (children.as_slice(), *operation == BooleanOp::Intersection),
        };
        let mut bound = if intersection {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        for &child in children {
            let z = self.minimum(scene, child, world)?;
            bound = if intersection {
                bound.max(z)
            } else {
                bound.min(z)
            };
        }
        Ok(bound)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn bottom(scene: &Scene) -> f64 {
        scene
            .evaluate()
            .unwrap()
            .iter()
            .flat_map(|n| {
                n.mesh
                    .vertices()
                    .iter()
                    .map(|&p| n.world.transform_point3(DVec3::from_array(p)).z)
            })
            .fold(f64::INFINITY, f64::min)
    }
    #[test]
    fn move_rotate_resize_and_load_stay_above_plate() {
        let mut editor = Editor::default();
        let id = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let mut node = editor.scene.nodes[&id].clone();
        node.transform.position[2] = -50.0;
        editor.apply(Command::SetNode(node)).unwrap();
        assert!(bottom(&editor.scene).abs() < 1e-8);
        let mut node = editor.scene.nodes[&id].clone();
        node.transform.rotation = [35.0, 50.0, 20.0];
        editor.apply(Command::SetNode(node)).unwrap();
        assert!(bottom(&editor.scene) >= -1e-8);
        let mut node = editor.scene.nodes[&id].clone();
        let NodeKind::Primitive(p) = &mut node.kind else {
            unreachable!()
        };
        p.resize_axis(0, 40.0).unwrap();
        editor.apply(Command::SetNode(node)).unwrap();
        assert!(bottom(&editor.scene) >= -1e-8);
        let mut loaded = editor.scene.clone();
        loaded.nodes.get_mut(&id).unwrap().transform.position[2] = -100.0;
        editor.replace_scene(loaded).unwrap();
        assert!(bottom(&editor.scene) >= -1e-8);
    }
    #[test]
    fn hidden_difference_cutter_can_cross_plate_without_lifting_base() {
        let mut editor = Editor::default();
        let a = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let b = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        editor
            .apply(Command::Boolean {
                operation: BooleanOp::Difference,
                children: vec![a, b],
            })
            .unwrap();
        let mut cutter = editor.scene.nodes[&b].clone();
        cutter.transform.position[2] = -10.0;
        editor.apply(Command::SetNode(cutter)).unwrap();
        assert_eq!(editor.scene.nodes[&b].transform.position[2], -10.0);
        assert!(bottom(&editor.scene) >= -1e-8);
    }
}
