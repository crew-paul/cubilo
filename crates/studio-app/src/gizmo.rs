//! Screen-sized axis handles; edits stay in parameters and rigid transforms.
use eframe::egui;
use glam::{DMat4, DQuat, DVec3, EulerRot};
use studio_core::{Node, NodeKind, Scene};
use studio_render::Camera;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Orbit,
    Move,
    Rotate,
    Resize,
}
pub struct Drag {
    original: Node,
    axis: usize,
    start: egui::Pos2,
    screen_direction: egui::Vec2,
    pixels_per_mm: f32,
    screen_origin: egui::Pos2,
    parent_inverse: DMat4,
    world_origin: DVec3,
    rotation_sign: f64,
}
fn project(camera: &Camera, rect: egui::Rect, p: DVec3) -> Option<egui::Pos2> {
    let p = camera.matrix(rect.width() / rect.height()) * p.as_vec3().extend(1.0);
    if p.w <= 0.0 {
        return None;
    }
    let ndc = p.truncate() / p.w;
    Some(egui::pos2(
        rect.left() + (ndc.x + 1.0) * rect.width() / 2.0,
        rect.top() + (1.0 - ndc.y) * rect.height() / 2.0,
    ))
}
pub(crate) fn pointer_start(ui: &egui::Ui, response: &egui::Response) -> Option<egui::Pos2> {
    ui.input(|input| {
        input
            .events
            .iter()
            .find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(*pos),
                _ => None,
            })
            .or_else(|| input.pointer.press_origin())
    })
    .or_else(|| response.interact_pointer_pos())
}
pub(crate) fn drag_start(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) -> Option<egui::Pos2> {
    let pressed = ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } if rect.contains(*pos) && ui.clip_rect().contains(*pos) => Some(*pos),
            _ => None,
        })
    });
    if let Some(pos) = pressed {
        if ui
            .ctx()
            .layer_id_at(pos)
            .is_none_or(|layer| layer == ui.layer_id())
        {
            return Some(pos);
        }
    }
    response
        .drag_started_by(egui::PointerButton::Primary)
        .then(|| pointer_start(ui, response))
        .flatten()
}
/// Updates contain a full candidate node. Caller validates atomically via Command.
pub struct Frame<'a> {
    pub rect: egui::Rect,
    pub camera: &'a Camera,
    pub scene: &'a Scene,
    pub id: u64,
    pub tool: Tool,
    pub snap: Option<f64>,
}
pub fn show(ui: &mut egui::Ui, frame: Frame<'_>, drag: &mut Option<Drag>) -> Option<Node> {
    let Frame {
        rect,
        camera,
        scene,
        id,
        tool,
        snap,
    } = frame;
    if tool == Tool::Orbit {
        return None;
    }
    let node = scene.nodes.get(&id)?;
    if drag.as_ref().is_some_and(|d| d.original.id != id) {
        *drag = None;
    }
    if tool == Tool::Resize && !matches!(node.kind, NodeKind::Primitive(_)) {
        return None;
    }
    let world = scene.world_matrix(id)?;
    let origin = world.transform_point3(DVec3::ZERO);
    let screen_origin = project(camera, rect, origin)?;
    let parent = world * node.transform.matrix().inverse();
    let size = camera.distance as f64 * 0.15;
    let colors = [
        egui::Color32::from_rgb(245, 90, 85),
        egui::Color32::from_rgb(100, 220, 130),
        egui::Color32::from_rgb(100, 160, 255),
    ];
    for axis in 0..3 {
        let local_axis = DVec3::AXES[axis];
        let direction = if tool == Tool::Move {
            local_axis
        } else {
            world.transform_vector3(local_axis).normalize()
        };
        let u = world
            .transform_vector3(DVec3::AXES[(axis + 1) % 3])
            .normalize();
        let v = direction.cross(u).normalize();
        let angle = 0.4 + axis as f64 * 0.8;
        let handle_direction = if tool == Tool::Rotate {
            u * angle.cos() + v * angle.sin()
        } else {
            direction
        };
        let end = project(camera, rect, origin + handle_direction * size)?;
        let screen_axis = end - screen_origin;
        if screen_axis.length() < 14.0 {
            continue;
        }
        let painter = ui.painter();
        let mut started = None;
        if tool == Tool::Rotate {
            let ring: Vec<_> = (0..=48)
                .filter_map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 48.0;
                    project(camera, rect, origin + (u * a.cos() + v * a.sin()) * size)
                })
                .collect();
            painter.add(egui::Shape::line(
                ring.clone(),
                egui::Stroke::new(2.5, colors[axis]),
            ));
            for (segment, points) in ring.windows(2).enumerate() {
                let hit = ui
                    .interact(
                        egui::Rect::from_two_pos(points[0], points[1]).expand(9.0),
                        egui::Id::unique(("rotation-ring", axis, segment)),
                        egui::Sense::drag(),
                    )
                    .on_hover_cursor(egui::CursorIcon::Grab);
                if let Some(pos) = drag_start(
                    ui,
                    &hit,
                    egui::Rect::from_two_pos(points[0], points[1]).expand(9.0),
                ) {
                    started = Some(pos);
                }
            }
        } else {
            painter.line_segment([screen_origin, end], egui::Stroke::new(3.0, colors[axis]));
        }
        if tool == Tool::Resize {
            painter.rect_filled(
                egui::Rect::from_center_size(end, egui::vec2(13.0, 13.0)),
                2.0,
                colors[axis],
            );
        } else {
            painter.circle_filled(end, 9.0, colors[axis]);
        }
        painter.text(
            end + egui::vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            if tool == Tool::Rotate {
                ["앞뒤", "좌우", "빙글"][axis]
            } else {
                ["가로", "세로", "위"][axis]
            },
            egui::FontId::proportional(16.0),
            colors[axis],
        );
        let response = ui
            .interact(
                egui::Rect::from_center_size(end, egui::vec2(38.0, 38.0)),
                egui::Id::unique(("axis-handle", axis)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Grab);
        if let Some(pos) = drag_start(
            ui,
            &response,
            egui::Rect::from_center_size(end, egui::vec2(38.0, 38.0)),
        ) {
            started = Some(pos);
        }
        if let Some(start) = started {
            *drag = Some(Drag {
                original: node.clone(),
                axis,
                start,
                screen_direction: screen_axis.normalized(),
                pixels_per_mm: screen_axis.length() / size as f32,
                screen_origin,
                parent_inverse: parent.inverse(),
                world_origin: origin,
                rotation_sign: {
                    let a = project(camera, rect, origin + u * size)? - screen_origin;
                    let b = project(camera, rect, origin + v * size)? - screen_origin;
                    (a.x * b.y - a.y * b.x).signum() as f64
                },
            });
        }
    }
    let active = drag.as_ref()?;
    let finished = !ui.input(|i| i.pointer.primary_down());
    let pointer = ui.input(|i| i.pointer.interact_pos())?;
    let mut candidate = active.original.clone();
    let mut amount =
        ((pointer - active.start).dot(active.screen_direction) / active.pixels_per_mm) as f64;
    if let Some(step) = snap {
        amount = if tool == Tool::Move {
            ((active.world_origin[active.axis] + amount) / step).round() * step
                - active.world_origin[active.axis]
        } else {
            (amount / step).round() * step
        };
    }
    match tool {
        Tool::Move => {
            let delta = active
                .parent_inverse
                .transform_vector3(DVec3::AXES[active.axis] * amount);
            for axis in 0..3 {
                candidate.transform.position[axis] += delta[axis];
            }
        }
        Tool::Resize => {
            if let NodeKind::Primitive(p) = &mut candidate.kind {
                if p.resize_axis(active.axis, amount).is_err() {
                    return None;
                }
            }
        }
        Tool::Rotate => {
            let a = active.start - active.screen_origin;
            let b = pointer - active.screen_origin;
            let mut angle = (b.y.atan2(b.x) - a.y.atan2(a.x)) as f64;
            if snap.is_some() {
                let step = 15.0_f64.to_radians();
                angle = (angle / step).round() * step;
            }
            let [x, y, z] = candidate.transform.rotation.map(f64::to_radians);
            let q = DQuat::from_euler(EulerRot::XYZ, x, y, z)
                * DQuat::from_axis_angle(DVec3::AXES[active.axis], angle * active.rotation_sign);
            let (x, y, z) = q.to_euler(EulerRot::XYZ);
            candidate.transform.rotation = [x, y, z].map(f64::to_degrees);
        }
        Tool::Orbit => return None,
    }
    if finished {
        *drag = None;
    }
    Some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{Command, Editor, Primitive};
    fn simulate(tool: Tool, coalesced: bool) -> Node {
        let ctx = egui::Context::default();
        let mut editor = Editor::default();
        let id = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        let camera = Camera::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let origin = project(&camera, rect, DVec3::ZERO).unwrap();
        let end = if tool == Tool::Rotate {
            project(
                &camera,
                rect,
                (DVec3::X * 2.0_f64.cos() + DVec3::Y * 2.0_f64.sin())
                    * (camera.distance as f64 * 0.15),
            )
            .unwrap()
        } else {
            project(&camera, rect, DVec3::X * (camera.distance as f64 * 0.15)).unwrap()
        };
        let destination = if tool == Tool::Rotate {
            let d = end - origin;
            let a = 30.0_f32.to_radians();
            origin + egui::vec2(d.x * a.cos() - d.y * a.sin(), d.x * a.sin() + d.y * a.cos())
        } else {
            end + (end - origin).normalized() * 60.0
        };
        let button = |pressed| egui::Event::PointerButton {
            pos: if pressed { end } else { destination },
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let frames = if coalesced {
            vec![
                vec![],
                vec![egui::Event::PointerMoved(end)],
                vec![
                    button(true),
                    egui::Event::PointerMoved(destination),
                    button(false),
                ],
            ]
        } else {
            vec![
                vec![],
                vec![egui::Event::PointerMoved(end)],
                vec![button(true)],
                vec![egui::Event::PointerMoved(destination)],
                vec![button(false)],
            ]
        };
        let mut drag = None;
        for (i, events) in frames.into_iter().enumerate() {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    focused: true,
                    time: Some(i as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.allocate_exact_size(rect.size(), egui::Sense::click_and_drag());
                    if let Some(node) = show(
                        ui,
                        Frame {
                            rect,
                            camera: &camera,
                            scene: &editor.scene,
                            id,
                            tool,
                            snap: None,
                        },
                        &mut drag,
                    ) {
                        editor.apply(Command::SetNode(node)).unwrap();
                    }
                },
            );
            output.textures_delta.clear();
        }
        editor.scene.nodes[&id].clone()
    }
    #[test]
    fn pointer_drag_reaches_move_and_parameter_resize() {
        let moved = simulate(Tool::Move, false);
        assert!(
            moved.transform.position[0] > 1.0,
            "move did not receive drag"
        );
        assert_eq!(moved.transform.position[1], 0.0);
        let resized = simulate(Tool::Resize, false);
        let NodeKind::Primitive(Primitive::Cylinder(p)) = resized.kind else {
            unreachable!()
        };
        assert!(p.diameter > 60.0, "resize did not receive drag");
        assert_eq!(p.hollow.wall, 2.0);
    }
    #[test]
    fn pointer_rotation_changes_only_selected_local_axis() {
        let node = simulate(Tool::Rotate, false);
        assert!(node.transform.rotation[0].abs() < 1e-6);
        assert!(node.transform.rotation[1].abs() < 1e-6);
        assert!((node.transform.rotation[2].abs() - 30.0).abs() < 0.001);
    }
    #[test]
    fn fast_rotation_drag_in_one_input_frame_is_not_lost() {
        let node = simulate(Tool::Rotate, true);
        assert!((node.transform.rotation[2].abs() - 30.0).abs() < 0.001);
    }
}
