//! Direct corner handles around the selected object; uniform parametric resize.
use eframe::egui;
use glam::DVec3;
use studio_core::{Evaluated, Node, NodeKind};
use studio_render::Camera;
pub struct Drag {
    pub id: u64,
    original: Node,
    center: egui::Pos2,
    start_distance: f32,
}
fn project(camera: &Camera, rect: egui::Rect, p: DVec3) -> Option<egui::Pos2> {
    let p = camera.matrix(rect.width() / rect.height()) * p.as_vec3().extend(1.0);
    if p.w <= 0.0 {
        return None;
    }
    let p = p.truncate() / p.w;
    Some(egui::pos2(
        rect.left() + (p.x + 1.0) * rect.width() / 2.0,
        rect.top() + (1.0 - p.y) * rect.height() / 2.0,
    ))
}
pub fn show(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    camera: &Camera,
    node: &Node,
    item: &Evaluated,
    drag: &mut Option<Drag>,
) -> Option<Node> {
    if !matches!(node.kind, NodeKind::Primitive(_)) {
        return None;
    }
    if drag.as_ref().is_some_and(|d| d.id != node.id) {
        *drag = None;
    }
    let mut bounds = egui::Rect::NOTHING;
    for &p in item.mesh.vertices() {
        if let Some(p) = project(
            camera,
            rect,
            item.world.transform_point3(DVec3::from_array(p)),
        ) {
            bounds.extend_with(p);
        }
    }
    if !bounds.is_finite() {
        return None;
    }
    let bounds = bounds.expand(9.0);
    let p = ui.painter();
    let blue = super::design::BLUE;
    p.rect_stroke(
        bounds,
        8.0,
        egui::Stroke::new(1.5, blue.gamma_multiply(0.65)),
        egui::StrokeKind::Outside,
    );
    for (corner, pos) in [
        bounds.left_top(),
        bounds.right_top(),
        bounds.right_bottom(),
        bounds.left_bottom(),
    ]
    .into_iter()
    .enumerate()
    {
        p.circle_filled(pos, 7.0, egui::Color32::WHITE);
        p.circle_stroke(pos, 7.0, egui::Stroke::new(2.5, blue));
        let response = ui
            .interact(
                egui::Rect::from_center_size(pos, egui::vec2(30.0, 30.0)),
                egui::Id::unique(("corner-resize", corner)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(if corner % 2 == 0 {
                egui::CursorIcon::ResizeNwSe
            } else {
                egui::CursorIcon::ResizeNeSw
            });
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "크기 조절점"));
        if let Some(pointer) = super::gizmo::drag_start(
            ui,
            &response,
            egui::Rect::from_center_size(pos, egui::vec2(30.0, 30.0)),
        ) {
            *drag = Some(Drag {
                id: node.id,
                original: node.clone(),
                center: bounds.center(),
                start_distance: pointer.distance(bounds.center()).max(1.0),
            });
        }
    }
    let active = drag.as_ref()?;
    let pointer = ui.input(|i| i.pointer.interact_pos())?;
    let factor = (pointer.distance(active.center) / active.start_distance).clamp(0.02, 50.0) as f64;
    let mut candidate = active.original.clone();
    let NodeKind::Primitive(primitive) = &mut candidate.kind else {
        return None;
    };
    let valid = primitive.resize_uniform(factor).is_ok();
    if !ui.input(|i| i.pointer.primary_down()) {
        *drag = None;
    }
    valid.then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{Command, Editor, Primitive};
    #[test]
    fn corner_drag_resizes_proportionally_without_scaling_walls() {
        let ctx = egui::Context::default();
        let mut editor = Editor::default();
        let id = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()[0];
        let items = editor.scene.evaluate().unwrap();
        let camera = Camera::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut bounds = egui::Rect::NOTHING;
        for &p in items[0].mesh.vertices() {
            bounds.extend_with(project(&camera, rect, DVec3::from_array(p)).unwrap());
        }
        let bounds = bounds.expand(9.0);
        let start = bounds.right_bottom();
        let destination = bounds.center() + (start - bounds.center()) * 1.4;
        let button = |pressed| egui::Event::PointerButton {
            pos: if pressed { start } else { destination },
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut drag = None;
        for (frame, events) in [
            vec![],
            vec![egui::Event::PointerMoved(start)],
            vec![button(true)],
            vec![egui::Event::PointerMoved(destination)],
            vec![button(false)],
        ]
        .into_iter()
        .enumerate()
        {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    time: Some(frame as f64 / 60.0),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.allocate_exact_size(rect.size(), egui::Sense::click_and_drag());
                    let node = editor.scene.nodes[&id].clone();
                    if let Some(update) = show(ui, rect, &camera, &node, &items[0], &mut drag) {
                        editor.apply(Command::SetNode(update)).unwrap();
                    }
                },
            );
            output.textures_delta.clear();
        }
        let NodeKind::Primitive(Primitive::Cylinder(p)) = &editor.scene.nodes[&id].kind else {
            unreachable!()
        };
        assert!(p.diameter > 70.0);
        assert!((p.diameter / p.height - 0.75).abs() < 1e-9);
        assert_eq!(p.hollow.wall, 2.0);
        assert!(drag.is_none());
    }
}
