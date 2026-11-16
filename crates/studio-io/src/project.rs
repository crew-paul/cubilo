//! Versioned JSON project files and same-directory atomic replacement.
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};
use studio_core::Scene;
const MAX_PROJECT_BYTES: u64 = 16 * 1024 * 1024;
fn invalid(e: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
pub fn save_project(path: &Path, scene: &Scene) -> io::Result<()> {
    scene.validate().map_err(invalid)?;
    let bytes = serde_json::to_vec_pretty(scene).map_err(invalid)?;
    if bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(invalid("프로젝트 파일 크기 한도 초과"));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    // Verify exactly the data that will replace the previous project.
    let check: Scene = serde_json::from_slice(&std::fs::read(temp.path())?).map_err(invalid)?;
    check.validate().map_err(invalid)?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn load_project(path: &Path) -> io::Result<Scene> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_PROJECT_BYTES {
        return Err(invalid("프로젝트 파일 크기 한도 초과"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PROJECT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(invalid("프로젝트 파일 크기 한도 초과"));
    }
    let scene: Scene = serde_json::from_slice(&bytes).map_err(invalid)?;
    scene.validate().map_err(invalid)?;
    Ok(scene)
}
#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{Command, Editor, NodeKind, Primitive};
    #[test]
    fn round_trip_disabled_settings_transforms_and_replace() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("한글 프로젝트.k3d");
        let mut editor = Editor::default();
        let id = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        let mut node = editor.scene.nodes[&id].clone();
        node.transform.position = [20.0, 30.0, 4.0];
        node.transform.rotation = [10.0, 20.0, 30.0];
        let NodeKind::Primitive(Primitive::Cylinder(p)) = &mut node.kind else {
            unreachable!()
        };
        p.hollow.wall = 7.0;
        p.hollow.bottom = 6.0;
        p.hollow.enabled = false;
        editor.apply(Command::SetNode(node)).unwrap();
        save_project(&path, &editor.scene).unwrap();
        assert_eq!(load_project(&path).unwrap(), editor.scene);
        editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap();
        save_project(&path, &editor.scene).unwrap();
        assert_eq!(load_project(&path).unwrap(), editor.scene);
    }
    #[test]
    fn failed_save_preserves_original_and_corruption_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("project.k3d");
        let mut scene = Scene::default();
        save_project(&path, &scene).unwrap();
        let before = std::fs::read(&path).unwrap();
        scene.schema_version += 1;
        assert!(save_project(&path, &scene).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(&path, b"{truncated").unwrap();
        assert!(load_project(&path).is_err());
        std::fs::write(&path, serde_json::to_vec(&scene).unwrap()).unwrap();
        assert!(load_project(&path).is_err());
    }
}

#[cfg(test)]
mod shape_compatibility_tests {
    use super::*;
    use studio_core::{Command, Editor, Primitive};
    use studio_geometry::{BoxParams, CylinderParams, Hollow, Rounded, Taper};
    #[test]
    fn six_shapes_and_property_settings_round_trip_and_export() {
        let shapes = [
            Primitive::Box(BoxParams {
                width: 60.0,
                depth: 50.0,
                height: 80.0,
                rounded: Rounded {
                    enabled: true,
                    radius: 1.0,
                },
                hollow: Hollow {
                    enabled: true,
                    ..Hollow::default()
                },
            }),
            Primitive::Cylinder(CylinderParams {
                diameter: 60.0,
                height: 80.0,
                segments: 64,
                rounded: Rounded::default(),
                taper: Taper {
                    enabled: true,
                    top_diameter: 30.0,
                },
                hollow: Hollow {
                    enabled: true,
                    ..Hollow::default()
                },
            }),
            Primitive::Sphere(Default::default()),
            Primitive::Cone(Default::default()),
            Primitive::Torus(Default::default()),
            Primitive::Hemisphere(Default::default()),
        ];
        let mut editor = Editor::default();
        for shape in shapes {
            let mesh = shape.mesh().unwrap();
            let mut bytes = Vec::new();
            crate::write_binary_stl(&mut bytes, &mesh).unwrap();
            assert_eq!(bytes.len(), 84 + 50 * mesh.triangles().len());
            editor.apply(Command::Add(shape)).unwrap();
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("shapes.k3d");
        save_project(&path, &editor.scene).unwrap();
        assert_eq!(load_project(&path).unwrap(), editor.scene);
    }
    #[test]
    fn version_one_files_without_new_optional_properties_still_load() {
        let mut editor = Editor::default();
        editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap();
        editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap();
        let mut json = serde_json::to_value(&editor.scene).unwrap();
        for node in json["nodes"].as_object_mut().unwrap().values_mut() {
            let params = node["kind"]["data"]["params"].as_object_mut().unwrap();
            params.remove("rounded");
            params.remove("taper");
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("old.k3d");
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(load_project(&path).unwrap(), editor.scene);
    }
}

#[cfg(test)]
mod boolean_project_tests {
    use super::*;
    use studio_core::{Command, Editor, NodeKind, Primitive};
    use studio_geometry::boolean::BooleanOp;
    #[test]
    fn editable_boolean_sources_and_text_survive_project_round_trip() {
        let mut editor = Editor::default();
        let a = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let b = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        let root = editor
            .apply(Command::Boolean {
                operation: BooleanOp::Difference,
                children: vec![a, b],
            })
            .unwrap()[0];
        editor
            .apply(Command::Add(Primitive::Text(Default::default())))
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("boolean.k3d");
        save_project(&path, &editor.scene).unwrap();
        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded, editor.scene);
        let NodeKind::Boolean { children, .. } = &loaded.nodes[&root].kind else {
            unreachable!()
        };
        assert_eq!(children, &[a, b]);
        loaded.export_mesh().unwrap().validate().unwrap();
    }
}

#[cfg(test)]
mod sketch_tests {
    use super::*;
    use studio_core::{Command, Editor, NodeKind, Primitive};
    use studio_geometry::{boolean::BooleanOp, SketchParams};
    #[test]
    fn drawn_outline_remains_editable_after_save_resize_boolean_and_stl() {
        let p = SketchParams {
            points: vec![
                [-20., -20.],
                [20., -20.],
                [20., 0.],
                [0., 0.],
                [0., 20.],
                [-20., 20.],
            ],
            thickness: 5.,
            stroke: vec![
                [-20., -20.],
                [20., -20.],
                [20., 0.],
                [0., 0.],
                [0., 20.],
                [-20., 20.],
            ],
        };
        let mut editor = Editor::default();
        let id = editor
            .apply(Command::Add(Primitive::Sketch(p.clone())))
            .unwrap()[0];
        let original = editor.scene.clone();
        let mut node = editor.scene.nodes[&id].clone();
        let NodeKind::Primitive(shape) = &mut node.kind else {
            unreachable!()
        };
        shape.resize_uniform(1.5).unwrap();
        shape.resize_axis(2, 2.).unwrap();
        editor.apply(Command::SetNode(node)).unwrap();
        editor.undo();
        assert_eq!(editor.scene, original);
        editor.redo();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawing.k3d");
        save_project(&path, &editor.scene).unwrap();
        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded, editor.scene);
        let NodeKind::Primitive(Primitive::Sketch(restored)) = &loaded.nodes[&id].kind else {
            unreachable!()
        };
        assert_eq!(restored.stroke.len(), p.stroke.len());
        assert_eq!(restored.thickness, 9.5);
        let mut bytes = vec![];
        crate::write_binary_stl(&mut bytes, &loaded.export_mesh().unwrap()).unwrap();
        assert!(bytes.len() > 84);
        let cutter = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        let mut node = editor.scene.nodes[&cutter].clone();
        node.transform.position[0] = -25.;
        editor.apply(Command::SetNode(node)).unwrap();
        editor
            .apply(Command::Boolean {
                operation: BooleanOp::Union,
                children: vec![id, cutter],
            })
            .unwrap();
        editor.scene.export_mesh().unwrap().validate().unwrap();
    }
}
