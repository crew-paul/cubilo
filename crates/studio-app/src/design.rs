//! Cubilo's visual language: light workbench, illustrated shape cards, quiet chrome.
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke, Vec2};
use studio_core::Primitive;
pub const BLUE: Color32 = Color32::from_rgb(65, 109, 235);
pub const INK: Color32 = Color32::from_rgb(39, 54, 79);
pub const MUTED: Color32 = Color32::from_rgb(115, 130, 154);
pub const PALE: Color32 = Color32::from_rgb(242, 246, 253);
pub const LINE: Color32 = Color32::from_rgb(222, 230, 242);
pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = Color32::WHITE;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.extreme_bg_color = PALE;
    style.visuals.faint_bg_color = PALE;
    style.visuals.selection.bg_fill = Color32::from_rgb(222, 233, 255);
    style.visuals.selection.stroke = Stroke::new(1.0, BLUE);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widget.corner_radius = egui::CornerRadius::same(10);
    }
    style.visuals.widgets.inactive.weak_bg_fill = PALE;
    style.visuals.widgets.inactive.bg_fill = PALE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(229, 237, 255);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BLUE);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(215, 229, 255);
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(13.0, 9.0);
    style.spacing.interact_size = Vec2::new(40.0, 36.0);
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(23.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    ctx.set_style_of(egui::Theme::Light, style);
    ctx.set_theme(egui::Theme::Light);
}
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).strong().size(16.0));
    ui.add_space(2.0);
}
fn polygon(p: &egui::Painter, points: Vec<Pos2>, fill: Color32) {
    p.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
}
pub fn cube(p: &egui::Painter, c: Pos2, s: f32, color: Color32) {
    let a = c + Vec2::new(0.0, -s);
    let b = c + Vec2::new(s, -s * 0.45);
    let d = c + Vec2::new(-s, -s * 0.45);
    let m = c + Vec2::new(0.0, s * 0.1);
    let low = c + Vec2::new(0.0, s * 1.05);
    polygon(p, vec![a, b, m, d], color.gamma_multiply(1.15));
    polygon(p, vec![d, m, low, c + Vec2::new(-s, s * 0.5)], color);
    polygon(
        p,
        vec![m, b, c + Vec2::new(s, s * 0.5), low],
        color.gamma_multiply(0.78),
    );
}
pub fn shape_card(ui: &mut egui::Ui, shape: &Primitive) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(82.0, 88.0), egui::Sense::click());
    let (color, kind) = match shape {
        Primitive::Box(_) => (BLUE, 0),
        Primitive::Cylinder(_) => (Color32::from_rgb(56, 177, 158), 1),
        Primitive::Sphere(_) => (Color32::from_rgb(237, 156, 74), 2),
        Primitive::Cone(_) => (Color32::from_rgb(161, 118, 223), 3),
        Primitive::Torus(_) => (Color32::from_rgb(234, 126, 159), 4),
        Primitive::Hemisphere(_) => (Color32::from_rgb(88, 172, 219), 5),
        Primitive::Text(_) | Primitive::Sketch(_) => (BLUE, 6),
    };
    let p = ui.painter();
    p.rect_filled(
        rect,
        12.0,
        if response.hovered() {
            Color32::from_rgb(230, 238, 255)
        } else {
            PALE
        },
    );
    p.rect_stroke(
        rect,
        12.0,
        Stroke::new(1.0, if response.hovered() { BLUE } else { LINE }),
        egui::StrokeKind::Inside,
    );
    let c = rect.center_top() + Vec2::new(0.0, 32.0);
    match kind {
        0 => cube(p, c, 15.0, color),
        1 => {
            p.rect_filled(
                egui::Rect::from_center_size(c + Vec2::new(0.0, 2.0), Vec2::new(29.0, 24.0)),
                0.0,
                color,
            );
            p.add(egui::Shape::ellipse_filled(
                c + Vec2::new(0.0, 14.0),
                Vec2::new(14.5, 6.0),
                color,
            ));
            p.add(egui::Shape::ellipse_filled(
                c - Vec2::new(0.0, 10.0),
                Vec2::new(14.5, 6.0),
                color.gamma_multiply(1.12),
            ));
        }
        2 => {
            p.circle_filled(c, 17.0, color);
            p.circle_filled(
                c + Vec2::new(-5.0, -6.0),
                5.0,
                Color32::from_white_alpha(100),
            );
        }
        3 => {
            polygon(
                p,
                vec![
                    c + Vec2::new(0.0, -19.0),
                    c + Vec2::new(17.0, 14.0),
                    c + Vec2::new(-17.0, 14.0),
                ],
                color,
            );
            p.add(egui::Shape::ellipse_filled(
                c + Vec2::new(0.0, 14.0),
                Vec2::new(17.0, 5.0),
                color.gamma_multiply(0.9),
            ));
        }
        4 => {
            p.circle_stroke(c, 12.0, Stroke::new(10.0, color));
        }
        5 => {
            let points: Vec<_> = (0..=24)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 * std::f32::consts::PI / 24.0;
                    c + Vec2::new(a.cos() * 18.0, a.sin() * 18.0 + 8.0)
                })
                .collect();
            polygon(p, points, color);
        }
        _ => {
            p.text(
                c,
                Align2::CENTER_CENTER,
                "가",
                FontId::proportional(29.0),
                color,
            );
        }
    }
    p.text(
        rect.center_bottom() - Vec2::new(0.0, 15.0),
        Align2::CENTER_CENTER,
        shape.label(),
        FontId::proportional(14.0),
        INK,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} 추가", shape.label()))
    });
    response.on_hover_text("눌러서 작품에 넣어요")
}

/// Context actions float above the canvas and never reserve layout space.
pub fn selection_bar(
    ctx: &egui::Context,
    canvas: egui::Rect,
    visible: bool,
    contents: impl FnOnce(&mut egui::Ui),
) {
    if !visible {
        return;
    }
    egui::Area::new(egui::Id::unique("selection-actions"))
        .order(egui::Order::Foreground)
        .fixed_pos(canvas.left_top() + Vec2::new(12.0, 12.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(12)
                .inner_margin(7)
                .show(ui, |ui| {
                    ui.horizontal(contents);
                });
        });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_actions_never_resize_or_shift_canvas() {
        let ctx = egui::Context::default();
        let mut rectangles = Vec::new();
        for selected in [false, true, false, true] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(1000.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::Panel::top("fixed-tools").show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("고르기·크기");
                            ui.label("돌리기");
                        });
                    });
                    egui::CentralPanel::default().show(ui, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                        rectangles.push(rect);
                        selection_bar(ui.ctx(), rect, selected, |ui| {
                            let _ = ui.button("복사하기");
                            let _ = ui.button("지우기");
                        });
                    });
                },
            );
            output.textures_delta.clear();
        }
        assert!(rectangles.windows(2).all(|r| r[0] == r[1]));
    }
}
