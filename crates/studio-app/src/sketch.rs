//! A dedicated drawing sheet; scene edits are committed only on completion.
use crate::design;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use studio_geometry::{SketchParams, MAX_OUTLINE_POINTS, MAX_STROKE_POINTS};

#[derive(Clone, PartialEq)]
struct Drawing {
    points: Vec<[f64; 2]>,
    stroke: Vec<[f64; 2]>,
    closed: bool,
    thickness: f64,
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Points,
    Pen,
    Edit,
    Insert,
}
pub enum Action {
    Cancel,
    Finish(SketchParams),
}
pub struct SketchEditor {
    pub target: Option<u64>,
    drawing: Drawing,
    undo: Vec<Drawing>,
    redo: Vec<Drawing>,
    mode: Mode,
    selected: Option<usize>,
    dragging: Option<usize>,
    moving: Option<([f64; 2], Drawing)>,
    pen_down: bool,
    thickness_start: Option<(f32, f64)>,
    smoothing: f64,
    view_center: [f64; 2],
    view_span: f64,
    message: String,
}
impl SketchEditor {
    pub fn new(target: Option<u64>, params: Option<SketchParams>) -> Self {
        let drawing = match params {
            Some(p) => Drawing {
                points: p.points,
                stroke: p.stroke,
                closed: true,
                thickness: p.thickness,
            },
            None => Drawing {
                points: vec![],
                stroke: vec![],
                closed: false,
                thickness: 5.,
            },
        };
        let mut editor = Self {
            target,
            drawing,
            undo: vec![],
            redo: vec![],
            mode: Mode::Points,
            selected: None,
            dragging: None,
            moving: None,
            pen_down: false,
            thickness_start: None,
            smoothing: 0.5,
            view_center: [0.; 2],
            view_span: 120.,
            message: String::new(),
        };
        if editor.drawing.closed {
            editor.mode = Mode::Edit;
            editor.fit();
        }
        editor
    }
    fn params(&self) -> SketchParams {
        SketchParams {
            points: self.drawing.points.clone(),
            stroke: self.drawing.stroke.clone(),
            thickness: self.drawing.thickness,
        }
    }
    fn remember(&mut self) {
        if self.undo.last() != Some(&self.drawing) {
            self.undo.push(self.drawing.clone());
            if self.undo.len() > 40 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }
    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.drawing.clone());
            self.drawing = previous;
            self.selected = None;
            self.mode = if self.drawing.closed {
                Mode::Edit
            } else {
                Mode::Points
            };
        }
    }
    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.drawing.clone());
            self.drawing = next;
            self.selected = None;
            self.mode = if self.drawing.closed {
                Mode::Edit
            } else {
                Mode::Points
            };
        }
    }
    fn fit(&mut self) {
        if self.drawing.points.is_empty() {
            return;
        }
        let min: [f64; 2] = std::array::from_fn(|a| {
            self.drawing
                .points
                .iter()
                .map(|p| p[a])
                .fold(f64::INFINITY, f64::min)
        });
        let max: [f64; 2] = std::array::from_fn(|a| {
            self.drawing
                .points
                .iter()
                .map(|p| p[a])
                .fold(f64::NEG_INFINITY, f64::max)
        });
        self.view_center = std::array::from_fn(|a| (min[a] + max[a]) / 2.);
        self.view_span = ((max[0] - min[0]).max(max[1] - min[1]) * 1.35).max(20.);
    }
    fn close(&mut self) {
        if self.drawing.points.len() >= 3 {
            self.remember();
            self.drawing.closed = true;
            self.mode = Mode::Edit;
        }
    }
    fn template(&mut self) {
        self.remember();
        self.drawing.points = vec![
            [-25., -20.],
            [25., -20.],
            [25., 15.],
            [15., 30.],
            [5., 18.],
            [-5., 18.],
            [-15., 30.],
            [-25., 15.],
        ];
        self.drawing.stroke.clear();
        self.drawing.closed = true;
        self.mode = Mode::Edit;
        self.selected = None;
        self.fit();
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        // Keep the paper fixed when tools, hints or validity change.
        let layout = SheetLayout::new(ui.available_rect_before_wrap());
        let mut action = None;
        region(ui, layout.header, "drawing-header", |ui| {
            ui.horizontal_centered(|ui| {
                if ui.add(egui::Button::new("돌아가기").frame(false)).clicked() {
                    action = Some(Action::Cancel);
                }
                ui.add_space(10.);
                ui.label(egui::RichText::new("내 모양 그리기").size(21.).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let valid = self.drawing.closed && self.params().validate().is_ok();
                    if ui
                        .add_enabled(
                            valid,
                            egui::Button::new(
                                egui::RichText::new(if self.target.is_some() {
                                    "작품에 적용"
                                } else {
                                    "완성 · 작품에 넣기"
                                })
                                .color(Color32::WHITE),
                            )
                            .fill(design::BLUE)
                            .min_size(Vec2::new(156., 42.)),
                        )
                        .clicked()
                    {
                        action = Some(Action::Finish(self.params()));
                    }
                });
            });
        });
        card(ui, layout.tools, "drawing-tools", |ui| {
            ui.label(egui::RichText::new("1  모양 만들기").strong().size(14.));
            ui.add_space(12.);
            if !self.drawing.closed {
                for (mode, title, caption) in [
                    (Mode::Points, "점 잇기", "콕콕 찍어서"),
                    (Mode::Pen, "자유선", "쓱 그려서"),
                ] {
                    if tool_tile(ui, title, caption, self.mode == mode, mode == Mode::Pen).clicked()
                    {
                        self.mode = mode;
                    }
                }
                if self.drawing.points.len() >= 3
                    && ui
                        .add_sized([120., 38.], egui::Button::new("모양 닫기"))
                        .clicked()
                {
                    self.close();
                }
            } else {
                for (mode, title, caption) in [
                    (Mode::Edit, "점 옮기기", "끌어서 다듬기"),
                    (Mode::Insert, "점 추가", "선 위를 누르기"),
                ] {
                    if tool_tile(ui, title, caption, self.mode == mode, false).clicked() {
                        self.mode = mode;
                    }
                }
            }
            ui.add_space(16.);
            ui.separator();
            ui.add_space(8.);
            if ui
                .add_sized([120., 36.], egui::Button::new("고양이로 시작"))
                .clicked()
            {
                self.template();
            }
            if ui
                .add_sized([120., 36.], egui::Button::new("새로 그리기"))
                .clicked()
            {
                self.remember();
                self.drawing.points.clear();
                self.drawing.stroke.clear();
                self.drawing.closed = false;
                self.selected = None;
                self.mode = Mode::Points;
            }
        });
        ui.painter()
            .rect_filled(layout.editing, 12., Color32::WHITE);
        region(
            ui,
            layout.editing.shrink2(Vec2::new(10., 4.)),
            "drawing-edit-actions",
            |ui| {
                ui.horizontal_centered(|ui| {
                if ui.add_enabled(!self.undo.is_empty(),egui::Button::new("되돌리기")).clicked(){self.undo();}
                if ui.add_enabled(!self.redo.is_empty(),egui::Button::new("다시 하기")).clicked(){self.redo();}
                ui.separator();
                if ui.add_enabled(self.selected.is_some() && self.drawing.points.len()>3,egui::Button::new("고른 점 지우기")).clicked(){
                    self.remember();self.drawing.points.remove(self.selected.take().unwrap());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {
                    if ui.small_button("그림 전체가 보이게 화면 맞추기")
                        .on_hover_text("그림 전체가 종이 안에 들어오도록 화면의 확대·축소와 중심을 조정해요. 실제 도형 크기는 바뀌지 않아요.")
                        .clicked(){self.fit();}
                });
            });
            },
        );
        region(ui, layout.paper, "drawing-paper", |ui| {
            self.canvas(ui, layout.paper.size())
        });
        card(ui, layout.preview, "drawing-preview", |ui| {
            ui.label(egui::RichText::new("2  입체로 만들기").size(14.).strong());
            ui.add_space(6.);
            ui.colored_label(
                design::MUTED,
                egui::RichText::new(if self.drawing.closed {
                    "손잡이를 위로 끌어 보세요"
                } else {
                    "모양이 닫히면 시작할 수 있어요"
                })
                .size(12.),
            );
            ui.add_space(14.);
            self.preview(ui);
            ui.add_space(12.);
            ui.separator();
            ui.add_space(10.);
            ui.label(egui::RichText::new("두께").size(14.).strong());
            ui.spacing_mut().slider_width = 88.;
            let before = self.drawing.thickness;
            let r = ui.add(
                egui::Slider::new(&mut self.drawing.thickness, 0.5..=100.)
                    .fixed_decimals(1)
                    .suffix(" mm"),
            );
            if r.drag_started() || (r.changed() && !r.dragged()) {
                let after = self.drawing.thickness;
                self.drawing.thickness = before;
                self.remember();
                self.drawing.thickness = after;
            }
            ui.add_space(18.);
            ui.label(egui::RichText::new("선 다듬기").size(14.).strong());
            ui.spacing_mut().slider_width = 188.;
            let available = !self.drawing.stroke.is_empty() && self.drawing.closed;
            let r = ui.add_enabled(
                available,
                egui::Slider::new(&mut self.smoothing, 0.1..=3.).show_value(false),
            );
            if r.drag_started() || (r.changed() && !r.dragged()) {
                self.remember();
            }
            if r.changed() {
                self.drawing.points = simplify(&self.drawing.stroke, self.smoothing);
                self.selected = None;
            }
            ui.horizontal(|ui| {
                ui.small("원래 선 유지");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small("매끈하게");
                });
            });
            if !available {
                ui.colored_label(
                    design::MUTED,
                    egui::RichText::new("자유선으로 그린 뒤 사용할 수 있어요").size(11.),
                );
            }
        });
        region(ui, layout.hint, "drawing-hint", |ui| {
            let error = if self.drawing.closed {
                self.params().validate().err()
            } else {
                None
            };
            let hint = error.unwrap_or(if !self.message.is_empty() {
                &self.message
            } else if self.mode == Mode::Insert {
                "선 위를 누르면 점이 하나 더 생겨요."
            } else if self.drawing.closed {
                "점을 끌면 모양이 바뀌고, 면을 끌면 전체가 이동해요."
            } else if self.mode == Mode::Pen {
                "누른 채 한 바퀴 그려요. 시작점 가까이에서 놓으면 닫혀요."
            } else {
                "점을 찍고, 처음 점을 다시 누르면 모양이 닫혀요."
            });
            ui.colored_label(
                if error.is_some() {
                    Color32::from_rgb(190, 65, 65)
                } else {
                    design::MUTED
                },
                egui::RichText::new(hint).size(13.),
            );
        });
        action
    }
    fn canvas(&mut self, ui: &mut egui::Ui, size: Vec2) {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, "모양 그리는 종이")
        });
        let scale = rect.width().min(rect.height()) as f64 / self.view_span;
        let center = self.view_center;
        let project = |p: [f64; 2]| {
            rect.center()
                + Vec2::new(
                    ((p[0] - center[0]) * scale) as f32,
                    (-(p[1] - center[1]) * scale) as f32,
                )
        };
        let unproject = |p: Pos2| {
            [
                center[0] + (p.x - rect.center().x) as f64 / scale,
                center[1] - (p.y - rect.center().y) as f64 / scale,
            ]
        };
        let nearest = |points: &[[f64; 2]], pos: Pos2| {
            points
                .iter()
                .enumerate()
                .map(|(i, &p)| (i, project(p).distance(pos)))
                .filter(|(_, d)| *d <= 13.)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        // Raw events retain the complete stroke, including quick press/move/release frames.
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if rect.contains(pos) => {
                    self.message.clear();
                    match self.mode {
                        Mode::Pen if !self.drawing.closed => {
                            self.remember();
                            self.drawing.points.clear();
                            self.drawing.stroke = vec![unproject(pos)];
                            self.pen_down = true;
                        }
                        Mode::Edit => {
                            self.selected = nearest(&self.drawing.points, pos);
                            self.dragging = self.selected;
                            self.moving = None;
                            if self.dragging.is_some() {
                                self.remember();
                            } else if self.drawing.closed
                                && self.params().validate().is_ok()
                                && inside(&self.drawing.points, unproject(pos))
                            {
                                self.remember();
                                self.moving = Some((unproject(pos), self.drawing.clone()));
                            }
                        }
                        Mode::Insert if self.drawing.points.len() < MAX_OUTLINE_POINTS => {
                            let edge = (0..self.drawing.points.len())
                                .map(|i| {
                                    (
                                        i,
                                        segment_distance(
                                            pos,
                                            project(self.drawing.points[i]),
                                            project(
                                                self.drawing.points
                                                    [(i + 1) % self.drawing.points.len()],
                                            ),
                                        ),
                                    )
                                })
                                .min_by(|a, b| a.1.total_cmp(&b.1));
                            if let Some((i, d)) = edge {
                                if d < 16. {
                                    self.remember();
                                    self.drawing.points.insert(i + 1, unproject(pos));
                                    self.selected = Some(i + 1);
                                    self.mode = Mode::Edit;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some((start, original)) = &self.moving {
                        let p = unproject(rect.clamp(pos));
                        let delta = [p[0] - start[0], p[1] - start[1]];
                        self.drawing = original.clone();
                        for p in self
                            .drawing
                            .points
                            .iter_mut()
                            .chain(&mut self.drawing.stroke)
                        {
                            p[0] += delta[0];
                            p[1] += delta[1];
                        }
                    }
                    if let Some(i) = self.dragging {
                        self.drawing.points[i] = unproject(rect.clamp(pos));
                    }
                    if self.pen_down {
                        let p = unproject(rect.clamp(pos));
                        if self.drawing.stroke.len() < MAX_STROKE_POINTS
                            && self
                                .drawing
                                .stroke
                                .last()
                                .is_none_or(|q| distance(*q, p) * scale > 2.)
                        {
                            self.drawing.stroke.push(p);
                        }
                        self.drawing.points = simplify(&self.drawing.stroke, self.smoothing);
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.dragging = None;
                    self.moving = None;
                    if self.pen_down {
                        self.pen_down = false;
                        let p = unproject(rect.clamp(pos));
                        if self.drawing.stroke.len() < MAX_STROKE_POINTS
                            && self
                                .drawing
                                .stroke
                                .last()
                                .is_none_or(|q| distance(*q, p) > 0.001)
                        {
                            self.drawing.stroke.push(p);
                        }
                        let closes = self.drawing.stroke.len() > 3
                            && distance(self.drawing.stroke[0], p) * scale < 16.;
                        if closes {
                            self.drawing.stroke.pop();
                        }
                        self.drawing.points = simplify(&self.drawing.stroke, self.smoothing);
                        self.drawing.closed = closes;
                        if closes {
                            self.mode = Mode::Edit;
                        }
                        if self.drawing.stroke.len() >= MAX_STROKE_POINTS {
                            self.message =
                                "선이 길어 여기까지 그렸어요. 점을 고쳐 마무리해 주세요.".into();
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(pos) = response
            .interact_pointer_pos()
            .filter(|_| self.mode == Mode::Points && !self.drawing.closed && response.clicked())
        {
            if self.drawing.points.len() >= 3 && project(self.drawing.points[0]).distance(pos) < 14.
            {
                self.close();
            } else if self.drawing.points.len() < MAX_OUTLINE_POINTS {
                let p = unproject(pos);
                if self
                    .drawing
                    .points
                    .last()
                    .is_none_or(|q| distance(*q, p) * scale > 4.)
                {
                    self.remember();
                    self.drawing.points.push(p);
                }
            } else {
                self.message = "점이 충분히 많아요. ‘모양 닫기’를 눌러 주세요.".into();
            }
        }
        if self.mode == Mode::Edit && self.drawing.closed {
            if let Some(pos) = response.hover_pos() {
                if nearest(&self.drawing.points, pos).is_none()
                    && inside(&self.drawing.points, unproject(pos))
                {
                    ui.ctx().set_cursor_icon(if self.moving.is_some() {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    });
                }
            }
        }
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 14., Color32::WHITE);
        let extent = [
            rect.width() as f64 / scale / 2.,
            rect.height() as f64 / scale / 2.,
        ];
        let low: [i32; 2] = std::array::from_fn(|a| ((center[a] - extent[a]) / 10.).floor() as i32);
        let high: [i32; 2] = std::array::from_fn(|a| ((center[a] + extent[a]) / 10.).ceil() as i32);
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                painter.circle_filled(
                    project([x as f64 * 10., y as f64 * 10.]),
                    1.,
                    Color32::from_rgb(218, 226, 239),
                );
            }
        }
        painter.rect_stroke(
            rect,
            14.,
            Stroke::new(1., design::LINE),
            egui::StrokeKind::Inside,
        );
        if self.drawing.points.is_empty() && !self.pen_down {
            let c = rect.center() - Vec2::new(0., 42.);
            let ink = Color32::from_rgb(183, 201, 231);
            let points = [
                c + Vec2::new(-32., 13.),
                c + Vec2::new(-8., -20.),
                c + Vec2::new(29., 6.),
            ];
            painter.add(egui::Shape::line(points.to_vec(), Stroke::new(2., ink)));
            for p in points {
                painter.circle_filled(p, 5., Color32::WHITE);
                painter.circle_stroke(p, 5., Stroke::new(2., ink));
            }
            painter.text(
                c + Vec2::new(0., 60.),
                egui::Align2::CENTER_CENTER,
                "여기서 모양을 그려요",
                egui::FontId::proportional(19.),
                design::INK,
            );
            painter.text(
                c + Vec2::new(0., 87.),
                egui::Align2::CENTER_CENTER,
                "점을 찍거나, 선을 쓱 그어 보세요",
                egui::FontId::proportional(13.),
                design::MUTED,
            );
        }
        if let Ok(mesh) = if self.drawing.closed {
            self.params().mesh()
        } else {
            Err("열린 모양")
        } {
            let mut fill = egui::Mesh::default();
            for p in mesh.vertices() {
                fill.colored_vertex(project([p[0], p[1]]), Color32::from_rgb(225, 236, 255));
            }
            for &[a, b, c] in mesh.triangles() {
                if [a, b, c].iter().all(|&i| mesh.vertices()[i][2] > 0.) {
                    fill.add_triangle(a as u32, b as u32, c as u32);
                }
            }
            painter.add(egui::Shape::mesh(fill));
        }
        let points = &self.drawing.points;
        let crossings = if self.drawing.closed {
            studio_geometry::crossing_edges(points)
        } else {
            vec![]
        };
        for i in 0..points
            .len()
            .saturating_sub(usize::from(!self.drawing.closed))
        {
            painter.line_segment(
                [project(points[i]), project(points[(i + 1) % points.len()])],
                Stroke::new(
                    3.,
                    if crossings.contains(&i) {
                        Color32::RED
                    } else {
                        design::BLUE
                    },
                ),
            );
        }
        if let Some(pos) = response
            .hover_pos()
            .filter(|_| !self.drawing.closed && !points.is_empty() && !self.pen_down)
        {
            painter.line_segment(
                [project(*points.last().unwrap()), pos],
                Stroke::new(1., design::BLUE),
            );
        }
        if !self.pen_down {
            for (i, &p) in points.iter().enumerate() {
                let radius = if Some(i) == self.selected || (i == 0 && !self.drawing.closed) {
                    8.
                } else {
                    5.
                };
                painter.circle_filled(project(p), radius, Color32::WHITE);
                painter.circle_stroke(project(p), radius, Stroke::new(2., design::BLUE));
            }
        }
    }
    fn preview(&mut self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(195., 230.), Sense::hover());
        let Ok(mesh) = (if self.drawing.closed {
            self.params().mesh()
        } else {
            Err("열린 모양")
        }) else {
            design::cube(
                ui.painter(),
                rect.center() - Vec2::new(0., 28.),
                27.,
                Color32::from_rgb(216, 227, 245),
            );
            ui.painter().text(
                rect.center() + Vec2::new(0., 42.),
                egui::Align2::CENTER_CENTER,
                "모양을 닫으면\n입체가 보여요",
                egui::FontId::proportional(13.),
                design::MUTED,
            );
            return;
        };
        let min: [f64; 2] = std::array::from_fn(|a| {
            self.drawing
                .points
                .iter()
                .map(|p| p[a])
                .fold(f64::INFINITY, f64::min)
        });
        let max: [f64; 2] = std::array::from_fn(|a| {
            self.drawing
                .points
                .iter()
                .map(|p| p[a])
                .fold(f64::NEG_INFINITY, f64::max)
        });
        let span = (max[0] - min[0] + max[1] - min[1]).max(1.);
        let scale = (140. / span).min(140. / (self.drawing.thickness + span * 0.3));
        let project = |p: [f64; 3]| {
            let x = p[0] - (min[0] + max[0]) / 2.;
            let y = p[1] - (min[1] + max[1]) / 2.;
            rect.center()
                + Vec2::new(
                    ((x - y) * scale) as f32,
                    ((x + y) * 0.35 * scale + (self.drawing.thickness / 2. - p[2]) * scale) as f32,
                )
        };
        let mut triangles = mesh.triangles().to_vec();
        triangles.sort_by(|a, b| {
            let depth = |t: &[usize; 3]| {
                t.iter()
                    .map(|&i| {
                        let p = mesh.vertices()[i];
                        p[0] + p[1]
                    })
                    .sum::<f64>()
            };
            depth(a).total_cmp(&depth(b))
        });
        let painter = ui.painter().with_clip_rect(rect);
        for t in triangles {
            let points = t.map(|i| mesh.vertices()[i]);
            let normal = studio_geometry::cross(
                studio_geometry::sub(points[1], points[0]),
                studio_geometry::sub(points[2], points[0]),
            );
            if normal[0] + normal[1] + normal[2] * 2. <= 0. {
                continue;
            }
            painter.add(egui::Shape::convex_polygon(
                points.map(project).to_vec(),
                if normal[2] > 0. {
                    Color32::from_rgb(135, 185, 250)
                } else {
                    Color32::from_rgb(77, 139, 225)
                },
                Stroke::NONE,
            ));
        }
        let handle = rect.center_top() + Vec2::new(0., 16.);
        let hit = Rect::from_center_size(handle, Vec2::new(64., 38.));
        let response = ui.interact(hit, ui.scope_id().with("thickness-handle"), Sense::drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, "위로 당겨 두께 만들기")
        });
        if let Some(pos) = crate::gizmo::drag_start(ui, &response, hit) {
            self.remember();
            self.thickness_start = Some((pos.y, self.drawing.thickness));
        }
        if let Some(((y, t), p)) = self
            .thickness_start
            .zip(ui.input(|i| i.pointer.interact_pos()))
        {
            self.drawing.thickness = (t + (y - p.y) as f64 * 0.2).clamp(0.5, 100.);
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.thickness_start = None;
        }
        painter.rect_filled(hit, 12., design::BLUE);
        painter.text(
            handle,
            egui::Align2::CENTER_CENTER,
            "↑ 두께",
            egui::FontId::proportional(17.),
            Color32::WHITE,
        );
    }
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let d = b - a;
    let t = if d.length_sq() > 0. {
        ((p - a).dot(d) / d.length_sq()).clamp(0., 1.)
    } else {
        0.
    };
    p.distance(a + d * t)
}
/// Iterative distance simplification: bounded source and output, original stroke retained.
fn simplify(stroke: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if stroke.len() < 3 {
        return stroke.to_vec();
    }
    let mut points = stroke.to_vec();
    let mut epsilon = tolerance;
    loop {
        let mut keep = vec![false; stroke.len()];
        keep[0] = true;
        keep[stroke.len() - 1] = true;
        let mut stack = vec![(0, stroke.len() - 1)];
        while let Some((a, b)) = stack.pop() {
            let aa = stroke[a];
            let bb = stroke[b];
            let dx = bb[0] - aa[0];
            let dy = bb[1] - aa[1];
            let len = dx * dx + dy * dy;
            let farthest = (a + 1..b)
                .map(|i| {
                    let p = stroke[i];
                    let t = if len > 0. {
                        ((p[0] - aa[0]) * dx + (p[1] - aa[1]) * dy) / len
                    } else {
                        0.
                    }
                    .clamp(0., 1.);
                    (i, distance(p, [aa[0] + t * dx, aa[1] + t * dy]))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = farthest.filter(|(_, d)| *d > epsilon) {
                keep[i] = true;
                stack.extend([(a, i), (i, b)]);
            }
        }
        points.clear();
        points.extend(stroke.iter().zip(keep).filter_map(|(&p, k)| k.then_some(p)));
        if points.len() <= MAX_OUTLINE_POINTS {
            return points;
        }
        epsilon *= 1.5;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoothing_preserves_source_and_history_restores_outline() {
        let mut editor = SketchEditor::new(None, None);
        editor.template();
        let before = editor.params();
        editor.remember();
        editor.drawing.points[0][0] -= 4.;
        editor.undo();
        assert_eq!(editor.params(), before);
        editor.redo();
        assert_ne!(editor.params(), before);
        let stroke: Vec<_> = (0..2048)
            .map(|i| [i as f64 * 0.02, (i as f64 * 0.1).sin()])
            .collect();
        let source = stroke.clone();
        let points = simplify(&stroke, 0.1);
        assert!(points.len() <= 256);
        assert_eq!(stroke, source);
        assert_eq!(points.first(), stroke.first());
        assert_eq!(points.last(), stroke.last());
    }
}

#[cfg(test)]
mod pointer_tests {
    use super::*;
    fn frame(ctx: &egui::Context, editor: &mut SketchEditor, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 500.))),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| {
                editor.canvas(ui, Vec2::new(500., 400.));
            },
        );
        output.textures_delta.clear();
    }
    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn points_close_then_move_and_undo_with_mouse() {
        let ctx = egui::Context::default();
        let mut editor = SketchEditor::new(None, None);
        frame(&ctx, &mut editor, vec![]);
        let points = [
            Pos2::new(120., 120.),
            Pos2::new(350., 120.),
            Pos2::new(300., 300.),
        ];
        for p in points.into_iter().chain([points[0]]) {
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(p), button(p, true)],
            );
            frame(&ctx, &mut editor, vec![button(p, false)]);
        }
        assert!(editor.drawing.closed);
        editor.params().mesh().unwrap();
        let before = editor.params();
        frame(
            &ctx,
            &mut editor,
            vec![
                button(points[0], true),
                egui::Event::PointerMoved(points[0] + Vec2::new(20., 0.)),
                button(points[0] + Vec2::new(20., 0.), false),
            ],
        );
        assert_ne!(editor.params(), before);
        editor.undo();
        assert_eq!(editor.params(), before);
    }
    #[test]
    fn face_drag_moves_outline_and_original_stroke_in_one_undo() {
        let ctx = egui::Context::default();
        let mut editor = SketchEditor::new(
            None,
            Some(SketchParams {
                points: vec![[-20., -20.], [20., -20.], [20., 20.], [-20., 20.]],
                stroke: vec![[-20., -20.], [20., -20.], [20., 20.], [-20., 20.]],
                thickness: 5.,
            }),
        );
        frame(&ctx, &mut editor, vec![]);
        let before = editor.params();
        let history = editor.undo.len();
        let start = Pos2::new(250., 200.);
        let end = start + Vec2::new(30., 20.);
        frame(
            &ctx,
            &mut editor,
            vec![
                button(start, true),
                egui::Event::PointerMoved(end),
                button(end, false),
            ],
        );
        let after = editor.params();
        let delta = [
            after.points[0][0] - before.points[0][0],
            after.points[0][1] - before.points[0][1],
        ];
        assert!(delta[0] > 0. && delta[1] < 0.);
        for (a, b) in after
            .points
            .iter()
            .chain(&after.stroke)
            .zip(before.points.iter().chain(&before.stroke))
        {
            for axis in 0..2 {
                assert!((a[axis] - b[axis] - delta[axis]).abs() < 1e-10);
            }
        }
        assert_eq!(after.thickness, before.thickness);
        assert_eq!(editor.undo.len(), history + 1);
        editor.undo();
        assert_eq!(editor.params(), before);
        editor.redo();
        assert_eq!(editor.params(), after);
    }
    #[test]
    fn freehand_coalesced_stroke_closes_and_keeps_source() {
        let ctx = egui::Context::default();
        let mut editor = SketchEditor::new(None, None);
        editor.mode = Mode::Pen;
        frame(&ctx, &mut editor, vec![]);
        let start = Pos2::new(120., 120.);
        frame(
            &ctx,
            &mut editor,
            vec![
                button(start, true),
                egui::Event::PointerMoved(Pos2::new(350., 120.)),
                egui::Event::PointerMoved(Pos2::new(350., 300.)),
                egui::Event::PointerMoved(Pos2::new(120., 300.)),
                egui::Event::PointerMoved(start),
                button(start, false),
            ],
        );
        assert!(editor.drawing.closed);
        assert!(editor.drawing.stroke.len() >= 4);
        editor.params().mesh().unwrap();
        editor.undo();
        assert!(editor.drawing.points.is_empty());
    }
}

#[cfg(test)]
mod thickness_tests {
    use super::*;
    #[test]
    fn fast_thickness_drag_is_one_undo_step() {
        let ctx = egui::Context::default();
        let mut editor = SketchEditor::new(None, None);
        editor.template();
        let before = editor.params();
        let mut start = Pos2::ZERO;
        for frame in 0..2 {
            let end = start - Vec2::new(0., 40.);
            let events = if frame == 0 {
                vec![]
            } else {
                vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::PointerButton {
                        pos: start,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::PointerMoved(end),
                    egui::Event::PointerButton {
                        pos: end,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ]
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 400.))),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    start = ui.cursor().min + Vec2::new(97.5, 16.);
                    editor.preview(ui);
                },
            );
            output.textures_delta.clear();
        }
        assert!(
            (editor.drawing.thickness - 13.).abs() < 0.01,
            "{}",
            editor.drawing.thickness
        );
        editor.undo();
        assert_eq!(editor.params(), before);
    }
}

/// Layout geometry is independent of selection and drawing validity.
struct SheetLayout {
    header: Rect,
    tools: Rect,
    editing: Rect,
    paper: Rect,
    preview: Rect,
    hint: Rect,
}
impl SheetLayout {
    fn new(rect: Rect) -> Self {
        let top = rect.top() + 128.;
        let bottom = rect.bottom() - 36.;
        let paper_left = rect.left() + 166.;
        let paper_right = rect.right() - 244.;
        Self {
            header: Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 56.)),
            tools: Rect::from_min_max(
                Pos2::new(rect.left(), top),
                Pos2::new(rect.left() + 150., bottom),
            ),
            editing: Rect::from_min_max(
                Pos2::new(rect.left(), rect.top() + 68.),
                Pos2::new(rect.right(), rect.top() + 112.),
            ),
            paper: Rect::from_min_max(Pos2::new(paper_left, top), Pos2::new(paper_right, bottom)),
            preview: Rect::from_min_max(
                Pos2::new(rect.right() - 228., top),
                Pos2::new(rect.right(), bottom),
            ),
            hint: Rect::from_min_max(Pos2::new(paper_left, bottom + 12.), rect.max),
        }
    }
}
fn region(ui: &mut egui::Ui, rect: Rect, id: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            contents(ui);
        },
    );
}
fn card(ui: &mut egui::Ui, rect: Rect, id: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.painter().rect_filled(rect, 16., Color32::WHITE);
    region(ui, rect.shrink(14.), id, |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8., 8.);
        ui.spacing_mut().button_padding = Vec2::new(10., 8.);
        egui::ScrollArea::vertical().id_salt(id).show(ui, contents);
    });
}
fn tool_tile(
    ui: &mut egui::Ui,
    title: &str,
    caption: &str,
    selected: bool,
    pen: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(122., 72.), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    let p = ui.painter();
    let color = if selected {
        design::BLUE
    } else {
        design::MUTED
    };
    p.rect_filled(
        rect,
        12.,
        if selected {
            Color32::from_rgb(232, 239, 255)
        } else if response.hovered() {
            design::PALE
        } else {
            Color32::WHITE
        },
    );
    if selected {
        p.rect_stroke(
            rect,
            12.,
            Stroke::new(1., Color32::from_rgb(190, 210, 252)),
            egui::StrokeKind::Inside,
        );
    }
    let c = rect.min + Vec2::new(22., 25.);
    if pen {
        let points = (0..20)
            .map(|i| {
                let t = i as f32 / 19.;
                c + Vec2::new(-8. + 16. * t, (t * 5.).sin() * 5.)
            })
            .collect();
        p.add(egui::Shape::line(points, Stroke::new(2., color)));
    } else {
        let points = [
            c + Vec2::new(-7., 6.),
            c + Vec2::new(0., -7.),
            c + Vec2::new(8., 4.),
        ];
        p.add(egui::Shape::line(points.to_vec(), Stroke::new(1.5, color)));
        for pos in points {
            p.circle_filled(pos, 2.5, color);
        }
    }
    p.text(
        rect.min + Vec2::new(42., 25.),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(14.),
        if selected { design::BLUE } else { design::INK },
    );
    p.text(
        rect.min + Vec2::new(14., 54.),
        egui::Align2::LEFT_CENTER,
        caption,
        egui::FontId::proportional(11.),
        design::MUTED,
    );
    response
}

fn inside(points: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut result = false;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}
