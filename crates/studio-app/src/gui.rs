use crate::gizmo::{self, Tool};
use crate::{design, resize};
use eframe::{egui, egui_wgpu::RenderState};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};
use studio_core::{Command, Editor, Evaluated, Node, NodeId, NodeKind, Primitive, Scene};
use studio_render::{Camera, Viewport};

pub fn run() -> eframe::Result {
    eframe::run_native(
        "큐빌로",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1180.0, 760.0])
                .with_min_inner_size([900.0, 600.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(Studio::new(cc)))),
    )
}
struct Studio {
    gpu: RenderState,
    viewport: Viewport,
    texture: egui::TextureId,
    camera: Camera,
    editor: Editor,
    worker: crate::worker::GeometryWorker,
    requested_revision: u64,
    completed_revision: u64,
    evaluated: Vec<Evaluated>,
    rendered_revision: u64,
    selected: BTreeSet<NodeId>,
    status: String,
    error: String,
    project_path: Option<PathBuf>,
    saved_scene: Scene,
    recovery_path: Option<PathBuf>,
    recoverable: Option<Scene>,
    autosaved_revision: u64,
    save_due: Option<Instant>,
    confirm_load: bool,
    composing: bool,
    tool: Tool,
    gizmo_drag: Option<gizmo::Drag>,
    resize_drag: Option<resize::Drag>,
    boolean_base: Option<NodeId>,
    multi_select: bool,
    library_shapes: bool,
    fit_pending: bool,
    sketch: Option<crate::sketch::SketchEditor>,
    owners: std::collections::BTreeMap<NodeId, NodeId>,
    snap: bool,
    grid: f64,
    move_anchor: Option<glam::DVec3>,
    move_original: Vec<(NodeId, [f64; 3])>,
}
impl Studio {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "korean".into(),
            egui::FontData::from_static(studio_geometry::DEFAULT_FONT).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("korean".into());
        cc.egui_ctx.set_fonts(fonts);
        design::install(&cc.egui_ctx);
        let gpu = cc
            .wgpu_render_state
            .clone()
            .expect("wgpu renderer required");
        let info = gpu.adapter.get_info();
        eprintln!(
            "Cubilo GPU: {} {:?} {:?}",
            info.name, info.backend, info.device_type
        );
        let viewport = Viewport::new(&gpu.device);
        let texture = gpu.renderer.write().register_native_texture(
            &gpu.device,
            &viewport.color,
            eframe::wgpu::FilterMode::Linear,
        );
        let recovery_path =
            directories::ProjectDirs::from("app", "Cubilo", "Cubilo").and_then(|dirs| {
                let dir = dirs.data_local_dir();
                std::fs::create_dir_all(dir).ok()?;
                Some(dir.join("recovery.k3d"))
            });
        let recoverable = recovery_path
            .as_ref()
            .filter(|p| p.exists())
            .and_then(|p| studio_io::project::load_project(p).ok());
        let mut editor = Editor::default();
        let selected = editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap()
            .into_iter()
            .collect();
        let saved_scene = Scene::default();
        Self {
            gpu,
            viewport,
            texture,
            camera: Camera::default(),
            editor,
            worker: crate::worker::GeometryWorker::new({
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            }),
            requested_revision: u64::MAX,
            completed_revision: u64::MAX,
            evaluated: Vec::new(),
            rendered_revision: u64::MAX,
            selected,
            status: "도형을 골라 나만의 작품을 만들어 보세요".into(),
            error: String::new(),
            project_path: None,
            saved_scene,
            recovery_path,
            recoverable,
            autosaved_revision: 0,
            save_due: None,
            confirm_load: false,
            composing: false,
            tool: Tool::Resize,
            gizmo_drag: None,
            resize_drag: None,
            boolean_base: None,
            multi_select: false,
            library_shapes: true,
            fit_pending: true,
            sketch: None,
            owners: Default::default(),
            snap: true,
            grid: 1.0,
            move_anchor: None,
            move_original: Vec::new(),
        }
    }
    fn command(&mut self, command: Command) {
        self.gizmo_drag = None;
        self.resize_drag = None;
        self.move_anchor = None;
        self.move_original.clear();
        self.editor.end_gesture();
        match self.editor.apply(command) {
            Ok(ids) => {
                self.selected = ids.into_iter().collect();
                self.error.clear();
            }
            Err(e) => self.error = e,
        }
    }
    fn update(&mut self, node: Node) {
        match self.editor.apply(Command::SetNode(node)) {
            Ok(_) => self.error.clear(),
            Err(e) => self.error = e,
        }
    }
    fn dirty(&self) -> bool {
        self.editor.scene != self.saved_scene
    }
    fn file_dialog(title: &str, extension: &str) -> rfd::FileDialog {
        let mut dialog = rfd::FileDialog::new()
            .set_title(title)
            .add_filter(extension, &[extension]);
        if let Some(dirs) = directories::UserDirs::new() {
            dialog = dialog.set_directory(dirs.document_dir().unwrap_or(dirs.home_dir()));
        }
        dialog
    }
    fn save_as(&mut self) {
        if let Some(path) = Self::file_dialog("프로젝트 저장", "k3d")
            .set_file_name("작품.k3d")
            .save_file()
        {
            self.save_to(path);
        }
    }
    fn save(&mut self) {
        if let Some(path) = self.project_path.clone() {
            self.save_to(path);
        } else {
            self.save_as();
        }
    }
    fn save_to(&mut self, path: PathBuf) {
        self.editor.end_gesture();
        match studio_io::project::save_project(&path, &self.editor.scene) {
            Ok(()) => {
                self.project_path = Some(path);
                self.saved_scene = self.editor.scene.clone();
                self.status = "프로젝트 저장 완료".into();
                self.error.clear();
            }
            Err(e) => self.error = format!("저장 실패: {e}"),
        }
    }
    fn load(&mut self) {
        let Some(path) = Self::file_dialog("프로젝트 열기", "k3d").pick_file() else {
            return;
        };
        match studio_io::project::load_project(&path)
            .map_err(|e| e.to_string())
            .and_then(|scene| self.editor.replace_scene(scene))
        {
            Ok(()) => {
                self.project_path = Some(path);
                self.saved_scene = self.editor.scene.clone();
                self.selected.clear();
                self.error.clear();
                self.status = "작품을 열었어요".into();
                self.fit_pending = true;
            }
            Err(e) => self.error = format!("불러오기 실패: {e}"),
        }
    }
    fn export(&mut self) {
        // Freeze geometry at the export click, before the user chooses a file.
        let snapshot = self.editor.scene.clone();
        let revision = self.editor.revision;
        let Some(path) = Self::file_dialog("STL 내보내기 (mm)", "stl")
            .set_file_name("작품.stl")
            .save_file()
        else {
            return;
        };
        if self.worker.export(revision, snapshot, path) {
            self.status = "내보내기 대기·계산 중".into();
        }
    }
    fn refresh(&mut self) {
        self.selected
            .retain(|id| self.editor.scene.nodes.contains_key(id));
        if let Some(evaluation) = self.worker.take() {
            if evaluation.revision == self.editor.revision {
                self.completed_revision = evaluation.revision;
                match evaluation.result {
                    Ok(items) => {
                        self.editor.cache_evaluated(&items);
                        self.viewport.set_scene(&self.gpu.device, &items);
                        self.evaluated = items;
                        self.rendered_revision = evaluation.revision;
                        if self.fit_pending {
                            self.fit_scene();
                            self.fit_pending = false;
                        }
                        self.error.clear();
                    }
                    Err(error) => self.error = error,
                }
            }
        }
        if self.requested_revision != self.editor.revision {
            self.owners = self.editor.scene.root_map();
            self.worker
                .submit(self.editor.revision, self.editor.scene.clone());
            self.requested_revision = self.editor.revision;
        }
        if let Some(export) = self.worker.take_export() {
            match export.result {
                Ok(()) => {
                    self.status = format!(
                        "STL 저장 완료: {} (편집 상태 {})",
                        export.path.display(),
                        export.revision
                    );
                    self.error.clear();
                }
                Err(error) => self.error = format!("STL 저장 실패: {error}"),
            }
        }
    }
    fn autosave(&mut self, ctx: &egui::Context) {
        if self.recoverable.is_some() {
            return;
        }
        if self.autosaved_revision != self.editor.revision && self.save_due.is_none() {
            self.save_due = Some(Instant::now() + Duration::from_secs(2));
        }
        if let Some(due) = self.save_due {
            if Instant::now() >= due {
                if let Some(path) = &self.recovery_path {
                    match studio_io::project::save_project(path, &self.editor.scene) {
                        Ok(()) => self.autosaved_revision = self.editor.revision,
                        Err(e) => self.error = format!("자동 복구 저장 실패: {e}"),
                    }
                }
                self.save_due = None;
            } else {
                ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
            }
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let was_composing = self.composing;
        let ime_event = ctx.input(|input| {
            let mut saw = false;
            for event in &input.events {
                if let egui::Event::Ime(event) = event {
                    saw = true;
                    match event {
                        egui::ImeEvent::Preedit { text, .. } => self.composing = !text.is_empty(),
                        egui::ImeEvent::Commit(_) => self.composing = false,
                        _ => {}
                    }
                }
            }
            saw
        });
        if was_composing || self.composing || ime_event {
            return;
        }

        let shortcuts = ctx.input(|i| {
            let command = i.modifiers.command;
            (
                command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
                command
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                command && i.key_pressed(egui::Key::S),
                !i.modifiers.any()
                    && (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)),
            )
        });
        if !ctx.egui_wants_keyboard_input() {
            if shortcuts.0 {
                self.gizmo_drag = None;
                self.resize_drag = None;
                self.move_anchor = None;
                self.editor.undo();
            }
            if shortcuts.1 {
                self.gizmo_drag = None;
                self.resize_drag = None;
                self.move_anchor = None;
                self.editor.redo();
            }
            if shortcuts.3 && !self.selected.is_empty() {
                self.command(Command::Delete(self.selected.iter().copied().collect()));
            }
        }
        if shortcuts.2 {
            self.save();
        }
    }
    fn fit_scene(&mut self) {
        if self.evaluated.is_empty() {
            self.camera = Camera::default();
            return;
        }
        let mut min = glam::DVec3::splat(f64::INFINITY);
        let mut max = glam::DVec3::splat(f64::NEG_INFINITY);
        for item in &self.evaluated {
            for &point in item.mesh.vertices() {
                let point = item.world.transform_point3(glam::DVec3::from_array(point));
                min = min.min(point);
                max = max.max(point);
            }
        }
        self.camera.target = ((min + max) / 2.0).as_vec3();
        self.camera.distance = ((max - min).length() as f32 * 1.5 + 30.0).clamp(50.0, 5000.0);
    }
    fn property_panel(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_empty() {
            design::section(ui, "여기서 모양을 바꿔요");
            ui.add_space(12.0);
            ui.label("작품 위의 도형을 눌러 보세요.");
            ui.add_space(8.0);
            ui.colored_label(design::MUTED, "크기, 색, 모양을 고칠 수 있어요.");
            return;
        }
        if self.selected.len() > 1 {
            design::section(ui, &format!("도형 {}개를 골랐어요", self.selected.len()));
            ui.label("함께 붙이거나 구멍을 만들어요.");
            ui.add_space(12.0);
            if ui
                .add_sized(
                    [ui.available_width(), 42.0],
                    egui::Button::new(
                        egui::RichText::new("하나로 붙이기").color(egui::Color32::WHITE),
                    )
                    .fill(design::BLUE),
                )
                .clicked()
            {
                self.command(Command::Boolean {
                    operation: studio_geometry::boolean::BooleanOp::Union,
                    children: self.selected.iter().copied().collect(),
                });
            }
            if self
                .boolean_base
                .is_none_or(|id| !self.selected.contains(&id))
            {
                self.boolean_base = self.selected.first().copied();
            }
            design::section(ui, "구멍 만들기");
            ui.small("남길 도형을 고르세요");
            if let Some(mut base) = self.boolean_base {
                egui::ComboBox::from_id_salt("keep-shape")
                    .selected_text(&self.editor.scene.nodes[&base].name)
                    .show_ui(ui, |ui| {
                        for &id in &self.selected {
                            ui.selectable_value(&mut base, id, &self.editor.scene.nodes[&id].name);
                        }
                    });
                self.boolean_base = Some(base);
                if ui
                    .add_sized(
                        [ui.available_width(), 38.0],
                        egui::Button::new("다른 도형 모양으로 뚫기"),
                    )
                    .clicked()
                {
                    let children = std::iter::once(base)
                        .chain(self.selected.iter().copied().filter(|id| *id != base))
                        .collect();
                    self.command(Command::Boolean {
                        operation: studio_geometry::boolean::BooleanOp::Difference,
                        children,
                    });
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("함께 묶기").clicked() {
                    self.command(Command::Group(self.selected.iter().copied().collect()));
                }
                if ui.button("겹친 부분만").clicked() {
                    self.command(Command::Boolean {
                        operation: studio_geometry::boolean::BooleanOp::Intersection,
                        children: self.selected.iter().copied().collect(),
                    });
                }
            });
        } else {
            let id = *self.selected.first().unwrap();
            if let Some(mut node) = self.editor.scene.nodes.get(&id).cloned() {
                ui.label(
                    egui::RichText::new("모양 다듬기")
                        .size(12.0)
                        .color(design::MUTED),
                );
                let mut changed = ui
                    .add(
                        egui::TextEdit::singleline(&mut node.name)
                            .font(egui::TextStyle::Heading)
                            .frame(egui::Frame::NONE),
                    )
                    .changed();
                if let NodeKind::Primitive(p) = &mut node.kind {
                    design::section(ui, "크기");
                    ui.small("모서리 점을 끌어도 바뀌어요");
                    changed |= primitive_fields(ui, p);
                    if let Primitive::Sketch(params) = p {
                        if ui.button("윤곽 다시 그리기").clicked() {
                            self.sketch = Some(crate::sketch::SketchEditor::new(
                                Some(id),
                                Some(params.clone()),
                            ));
                        }
                    }
                } else {
                    ui.add_space(10.0);
                    ui.label("아래 목록에서 원래 도형을 골라 모양을 고칠 수 있어요.");
                }
                design::section(ui, "작품 색");
                changed |= ui.color_edit_button_rgb(&mut node.color).changed();
                egui::CollapsingHeader::new("정확한 위치와 회전").show(ui, |ui| {
                    for (label, value) in ["가로 위치", "세로 위치", "높이 위치"]
                        .into_iter()
                        .zip(&mut node.transform.position)
                    {
                        changed |= number(ui, label, value, " mm");
                    }
                    for (label, value) in ["앞뒤 돌리기", "좌우 돌리기", "빙글 돌리기"]
                        .into_iter()
                        .zip(&mut node.transform.rotation)
                    {
                        changed |= number(ui, label, value, "°");
                    }
                });
                if changed {
                    self.update(node);
                }
                if ui.button("바닥에 내려놓기").clicked()
                    && self.rendered_revision == self.editor.revision
                {
                    let min =
                        self.evaluated
                            .iter()
                            .filter(|n| self.owners.get(&n.id) == Some(&id))
                            .flat_map(|n| {
                                n.mesh.vertices().iter().map(|&p| {
                                    n.world.transform_point3(glam::DVec3::from_array(p)).z
                                })
                            })
                            .fold(f64::INFINITY, f64::min);
                    if min.is_finite() {
                        let mut node = self.editor.scene.nodes[&id].clone();
                        node.transform.position[2] -= min;
                        self.command(Command::SetNode(node));
                    }
                }
                if matches!(
                    self.editor.scene.nodes.get(&id).map(|n| &n.kind),
                    Some(NodeKind::Group { .. })
                ) && ui.button("묶음 풀기").clicked()
                {
                    self.command(Command::Ungroup(id));
                }
            }
        }
        if !self.error.is_empty() {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(255, 240, 235))
                .corner_radius(12)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.label("이 모양으로는 바꿀 수 없어요.");
                    ui.small(&self.error);
                });
        }
    }
    fn palette(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.library_shapes, true, "도형");
            ui.selectable_value(
                &mut self.library_shapes,
                false,
                format!("내 작품 {}", self.editor.scene.roots.len()),
            );
        });
        ui.add_space(8.0);
        if self.library_shapes {
            design::section(ui, "무엇을 만들까요?");
            ui.colored_label(design::MUTED, "도형을 눌러 시작해요");
            ui.add_space(10.0);
            let shapes = [
                Primitive::box_default(),
                Primitive::cylinder_default(),
                Primitive::Sphere(Default::default()),
                Primitive::Cone(Default::default()),
                Primitive::Torus(Default::default()),
                Primitive::Hemisphere(Default::default()),
            ];
            for pair in shapes.chunks(2) {
                ui.horizontal(|ui| {
                    for shape in pair {
                        if design::shape_card(ui, shape).clicked() {
                            self.command(Command::Add(shape.clone()));
                        }
                    }
                });
            }
            ui.add_space(5.0);
            if ui
                .add_sized(
                    [174.0, 48.0],
                    egui::Button::new("내 모양 그리기").fill(design::PALE),
                )
                .clicked()
            {
                self.sketch = Some(crate::sketch::SketchEditor::new(None, None));
            }
            if ui
                .add_sized([174.0, 42.0], egui::Button::new("입체 글자 만들기"))
                .clicked()
            {
                self.command(Command::Add(Primitive::Text(Default::default())));
            }
            ui.add_space(16.0);
            ui.separator();
            return;
        }
        design::section(ui, "내 작품");
        if self.editor.scene.roots.len() > 1 {
            ui.checkbox(&mut self.multi_select, "여러 개 고르기");
        }
        fn tree(
            ui: &mut egui::Ui,
            scene: &Scene,
            id: NodeId,
            depth: usize,
            selected: &mut BTreeSet<NodeId>,
            multi: bool,
        ) {
            let node = &scene.nodes[&id];
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 12.0);
                let (dot, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    dot.center(),
                    3.5,
                    egui::Color32::from_rgb(
                        (node.color[0] * 255.0) as u8,
                        (node.color[1] * 255.0) as u8,
                        (node.color[2] * 255.0) as u8,
                    ),
                );
                if ui
                    .selectable_label(selected.contains(&id), &node.name)
                    .clicked()
                {
                    if multi || ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                        if !selected.remove(&id) {
                            selected.insert(id);
                        }
                    } else {
                        *selected = BTreeSet::from([id]);
                    }
                }
            });
            if let NodeKind::Group { children } | NodeKind::Boolean { children, .. } = &node.kind {
                for &child in children {
                    tree(ui, scene, child, depth + 1, selected, multi);
                }
            }
        }
        for &id in &self.editor.scene.roots {
            tree(
                ui,
                &self.editor.scene,
                id,
                0,
                &mut self.selected,
                self.multi_select,
            );
        }
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (logo, _) = ui.allocate_exact_size(egui::vec2(38.0, 42.0), egui::Sense::hover());
            design::cube(ui.painter(), logo.center(), 13.0, design::BLUE);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("큐빌로").size(25.0).strong());
                ui.label(
                    egui::RichText::new("상상이 모양이 되는 곳")
                        .size(11.0)
                        .color(design::MUTED),
                );
            });
            ui.add_space(24.0);
            if ui
                .add_enabled(self.editor.can_undo(), egui::Button::new("되돌리기"))
                .clicked()
            {
                self.resize_drag = None;
                self.gizmo_drag = None;
                self.editor.undo();
            }
            if ui
                .add_enabled(self.editor.can_redo(), egui::Button::new("다시 하기"))
                .clicked()
            {
                self.resize_drag = None;
                self.gizmo_drag = None;
                self.editor.redo();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.worker.is_exporting() {
                    ui.spinner();
                    if ui.small_button("출력 취소").clicked() {
                        self.worker.cancel_export();
                    }
                }
                if ui
                    .add_enabled(
                        !self.worker.is_exporting(),
                        egui::Button::new(
                            egui::RichText::new("출력 파일 만들기").color(egui::Color32::WHITE),
                        )
                        .fill(design::BLUE),
                    )
                    .clicked()
                {
                    self.export();
                }
                if ui.button("작품 저장").clicked() {
                    self.save();
                }
                ui.menu_button("작품", |ui| {
                    if ui.button("다른 이름으로 저장").clicked() {
                        ui.close();
                        self.save_as();
                    }
                    if ui.button("저장한 작품 열기").clicked() {
                        ui.close();
                        if self.dirty() {
                            self.confirm_load = true;
                        } else {
                            self.load();
                        }
                    }
                });
            });
        });
    }
}
fn number(ui: &mut egui::Ui, label: &str, value: &mut f64, suffix: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(0.2).suffix(suffix))
            .changed()
    })
    .inner
}
fn rounded_field(
    ui: &mut egui::Ui,
    rounded: &mut studio_geometry::Rounded,
    available: bool,
) -> bool {
    let mut changed = ui
        .add_enabled(
            available,
            egui::Checkbox::new(&mut rounded.enabled, "모서리 둥글게"),
        )
        .changed();
    if rounded.enabled {
        changed |= number(ui, "반경", &mut rounded.radius, " mm");
    }
    if !available {
        ui.small("원기둥 둥글기는 속 비우기·좁히기와 함께 사용할 수 없습니다.");
    }
    changed
}
fn primitive_fields(ui: &mut egui::Ui, p: &mut Primitive) -> bool {
    let mut changed = false;
    let hollow = match p {
        Primitive::Box(p) => {
            changed |= number(ui, "가로", &mut p.width, " mm");
            changed |= number(ui, "세로", &mut p.depth, " mm");
            changed |= number(ui, "높이", &mut p.height, " mm");
            changed |= rounded_field(ui, &mut p.rounded, true);
            Some(&mut p.hollow)
        }
        Primitive::Cylinder(p) => {
            changed |= number(ui, "지름", &mut p.diameter, " mm");
            changed |= number(ui, "높이", &mut p.height, " mm");
            changed |= rounded_field(ui, &mut p.rounded, !p.hollow.enabled && !p.taper.enabled);
            changed |= ui
                .add_enabled(
                    !p.rounded.enabled,
                    egui::Checkbox::new(&mut p.taper.enabled, "위쪽 좁히기/넓히기"),
                )
                .changed();
            if p.taper.enabled {
                changed |= number(ui, "위쪽 지름", &mut p.taper.top_diameter, " mm");
            }
            if p.rounded.enabled {
                ui.small("속 비우기를 사용하려면 둥글기를 끄세요.");
                None
            } else {
                Some(&mut p.hollow)
            }
        }
        Primitive::Sphere(p) => {
            changed |= number(ui, "지름", &mut p.diameter, " mm");
            None
        }
        Primitive::Hemisphere(p) => {
            changed |= number(ui, "지름", &mut p.diameter, " mm");
            None
        }
        Primitive::Cone(p) => {
            changed |= number(ui, "아래 지름", &mut p.bottom_diameter, " mm");
            changed |= number(ui, "위 지름", &mut p.top_diameter, " mm");
            changed |= number(ui, "높이", &mut p.height, " mm");
            None
        }
        Primitive::Sketch(p) => {
            changed |= number(ui, "두께", &mut p.thickness, " mm");
            ui.small("윤곽은 ‘윤곽 다시 그리기’에서 고쳐요.");
            None
        }
        Primitive::Text(p) => {
            changed |= ui.text_edit_singleline(&mut p.text).changed();
            changed |= number(ui, "글자 높이", &mut p.height, " mm");
            changed |= number(ui, "돌출 두께", &mut p.thickness, " mm");
            None
        }
        Primitive::Torus(p) => {
            changed |= number(ui, "중심 지름", &mut p.major_diameter, " mm");
            changed |= number(ui, "링 굵기", &mut p.tube_diameter, " mm");
            None
        }
    };
    if let Some(hollow) = hollow {
        ui.separator();
        changed |= ui.checkbox(&mut hollow.enabled, "속 비우기").changed();
        if hollow.enabled {
            changed |= number(ui, "벽 두께", &mut hollow.wall, " mm");
            changed |= number(
                ui,
                if hollow.open_top {
                    "바닥 두께"
                } else {
                    "바닥·천장"
                },
                &mut hollow.bottom,
                " mm",
            );
            changed |= ui.checkbox(&mut hollow.open_top, "윗면 열기").changed();
        }
    }
    changed
}
impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(mut sketch) = self.sketch.take() {
            let mut action = None;
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(design::PALE).inner_margin(16))
                .show(ui, |ui| {
                    action = sketch.ui(ui);
                });
            match action {
                Some(crate::sketch::Action::Cancel) => {}
                Some(crate::sketch::Action::Finish(params)) => {
                    let command = if let Some(id) = sketch.target {
                        self.editor.scene.nodes.get(&id).cloned().map(|mut node| {
                            node.kind = NodeKind::Primitive(Primitive::Sketch(params));
                            Command::SetNode(node)
                        })
                    } else {
                        Some(Command::Add(Primitive::Sketch(params)))
                    };
                    if let Some(command) = command {
                        self.command(command);
                        self.fit_pending = true;
                    }
                    if !self.error.is_empty() {
                        self.sketch = Some(sketch);
                    }
                }
                None => self.sketch = Some(sketch),
            }
            self.autosave(ui.ctx());
            return;
        }
        self.shortcuts(ui.ctx());
        if ui.input(|i| i.pointer.primary_pressed()) {
            self.editor.begin_gesture();
        }
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .inner_margin(14),
            )
            .show(ui, |ui| self.header(ui));
        if self.recoverable.is_some() {
            egui::Panel::top("recovery").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("지난 작품을 이어서 만들까요?");
                    if ui.button("이어서 만들기").clicked() {
                        let scene = self.recoverable.take().unwrap();
                        if let Err(e) = self.editor.replace_scene(scene) {
                            self.error = e;
                        }
                        self.selected.clear();
                        self.fit_pending = true;
                    }
                    if ui.button("새로 시작하기").clicked() {
                        self.recoverable = None;
                    }
                });
            });
        }
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .inner_margin(8),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        design::MUTED,
                        if self.completed_revision != self.editor.revision {
                            "모양을 만들고 있어요…"
                        } else if self.dirty() {
                            "마음껏 만들어 보세요 · 저장하지 않은 변경이 있어요"
                        } else {
                            "작품을 저장했어요"
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(&mut self.snap, "자석 맞춤");
                        ui.add(
                            egui::DragValue::new(&mut self.grid)
                                .range(0.1..=100.0)
                                .suffix(" mm"),
                        );
                    });
                });
            });
        egui::Panel::left("shapes")
            .exact_size(212.0)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .inner_margin(18),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.palette(ui));
            });
        egui::Panel::right("properties")
            .exact_size(270.0)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .inner_margin(18),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.property_panel(ui));
            });
        egui::Panel::top("work-tools")
            .frame(egui::Frame::new().fill(design::PALE).inner_margin(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tool, Tool::Resize, "고르기·크기");
                    ui.selectable_value(&mut self.tool, Tool::Move, "옮기기");
                    ui.selectable_value(&mut self.tool, Tool::Rotate, "돌리기");
                    ui.selectable_value(&mut self.tool, Tool::Orbit, "화면 보기");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("화면 맞추기").clicked() {
                            self.fit_scene();
                            self.fit_pending = self.rendered_revision != self.editor.revision;
                        }
                    });
                });
            });
        self.refresh();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(design::PALE).inner_margin(12))
            .show(ui, |ui| {
                let size = ui.available_size().max(egui::vec2(1.0, 1.0));
                let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                let image_slot = ui.painter().add(egui::Shape::Noop);
                design::selection_bar(ui.ctx(), rect, !self.selected.is_empty(), |ui| {
                    if ui.small_button("복사하기").clicked() {
                        self.command(Command::Duplicate(self.selected.iter().copied().collect()));
                    }
                    if ui.small_button("지우기").clicked() {
                        self.command(Command::Delete(self.selected.iter().copied().collect()));
                    }
                    if self.selected.len() > 1 {
                        ui.menu_button("나란히 맞추기", |ui| {
                            for (axis, name) in ["가로 가운데", "세로 가운데", "바닥 맞춤"]
                                .into_iter()
                                .enumerate()
                            {
                                if ui.button(name).clicked() {
                                    match studio_core::aligned_nodes(
                                        &self.editor.scene,
                                        &self.evaluated,
                                        &self.selected,
                                        axis,
                                        if axis == 2 {
                                            studio_core::AlignMode::Minimum
                                        } else {
                                            studio_core::AlignMode::Center
                                        },
                                    ) {
                                        Ok(nodes) => self.command(Command::SetNodes(nodes)),
                                        Err(e) => self.error = e,
                                    }
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                let hint = match self.tool {
                    Tool::Resize => "도형은 끌어서 옮기고, 모서리 점은 크기를 바꿔요",
                    Tool::Move => "색깔 손잡이를 끌어 도형을 옮겨요",
                    Tool::Rotate => "색깔 고리를 끌어 도형을 돌려요",
                    Tool::Orbit => "빈 곳을 끌어 다른 방향에서 보세요",
                };
                ui.painter().text(
                    rect.center_bottom() - egui::vec2(0.0, 18.0),
                    egui::Align2::CENTER_BOTTOM,
                    hint,
                    egui::FontId::proportional(13.0),
                    design::MUTED,
                );

                let ray_at = |camera: &Camera, pos: egui::Pos2| {
                    camera.ray(
                        [
                            2.0 * (pos.x - rect.left()) / size.x - 1.0,
                            1.0 - 2.0 * (pos.y - rect.top()) / size.y,
                        ],
                        size.x / size.y,
                    )
                };
                if self.selected.len() == 1 {
                    let id = *self.selected.first().unwrap();
                    if self.tool == Tool::Resize {
                        self.gizmo_drag = None;
                        if let (Some(node), Some(item)) = (
                            self.editor.scene.nodes.get(&id),
                            self.evaluated.iter().find(|item| item.id == id),
                        ) {
                            if let Some(node) = resize::show(
                                ui,
                                rect,
                                &self.camera,
                                node,
                                item,
                                &mut self.resize_drag,
                            ) {
                                self.update(node);
                            }
                        }
                    } else {
                        self.resize_drag = None;
                        if let Some(node) = gizmo::show(
                            ui,
                            gizmo::Frame {
                                rect,
                                camera: &self.camera,
                                scene: &self.editor.scene,
                                id,
                                tool: self.tool,
                                snap: self.snap.then_some(self.grid),
                            },
                            &mut self.gizmo_drag,
                        ) {
                            self.update(node);
                        }
                    }
                } else {
                    self.resize_drag = None;
                    self.gizmo_drag = None;
                }
                if self.gizmo_drag.is_none()
                    && self.resize_drag.is_none()
                    && self.rendered_revision == self.editor.revision
                    && (response.clicked()
                        || response.drag_started_by(egui::PointerButton::Primary))
                {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let (origin, direction) = ray_at(&self.camera, pos);
                        let hit = studio_core::pick(&self.evaluated, origin, direction)
                            .and_then(|(id, _)| self.owners.get(&id).copied());
                        let additive = self.multi_select
                            || ui.input(|i| i.modifiers.shift || i.modifiers.command);
                        if let Some(id) = hit {
                            if additive && response.clicked() {
                                if !self.selected.remove(&id) {
                                    self.selected.insert(id);
                                }
                            } else if !self.selected.contains(&id) {
                                self.selected = BTreeSet::from([id]);
                            }
                        } else if response.clicked() && !additive {
                            self.selected.clear();
                        }
                        if matches!(self.tool, Tool::Move | Tool::Resize)
                            && response.drag_started()
                            && hit.is_some()
                        {
                            self.move_anchor = ground(origin, direction);
                            self.move_original = self
                                .selected
                                .iter()
                                .map(|id| (*id, self.editor.scene.nodes[id].transform.position))
                                .collect();
                        }
                    }
                }
                if self.gizmo_drag.is_none() && self.resize_drag.is_none() && response.dragged() {
                    let delta = ui.input(|i| i.pointer.delta());
                    if response.dragged_by(egui::PointerButton::Secondary) {
                        self.camera.pan(delta.x, delta.y);
                    } else if matches!(self.tool, Tool::Move | Tool::Resize) {
                        if let (Some(anchor), Some(pos)) =
                            (self.move_anchor, response.interact_pointer_pos())
                        {
                            let (origin, direction) = ray_at(&self.camera, pos);
                            if let Some(point) = ground(origin, direction) {
                                let offset = point - anchor;
                                let mut updates = Vec::new();
                                for (id, original) in self.move_original.clone() {
                                    let mut node = self.editor.scene.nodes[&id].clone();
                                    for axis in 0..2 {
                                        let value = original[axis] + offset[axis];
                                        node.transform.position[axis] = if self.snap {
                                            (value / self.grid).round() * self.grid
                                        } else {
                                            value
                                        };
                                    }
                                    updates.push(node);
                                }
                                if let Err(e) = self.editor.apply(Command::SetNodes(updates)) {
                                    self.error = e;
                                }
                            }
                        }
                    } else if self.tool == Tool::Orbit {
                        self.camera.orbit(delta.x, delta.y);
                    }
                }
                if response.drag_stopped() {
                    self.move_anchor = None;
                    self.move_original.clear();
                }
                if response.hovered() {
                    self.camera.zoom(ui.input(|i| i.smooth_scroll_delta.y));
                }
                self.refresh();
                let scale = ui.ctx().pixels_per_point();
                if self.viewport.resize(
                    &self.gpu.device,
                    [
                        (size.x * scale).round() as u32,
                        (size.y * scale).round() as u32,
                    ],
                ) {
                    self.gpu
                        .renderer
                        .write()
                        .update_egui_texture_from_wgpu_texture(
                            &self.gpu.device,
                            &self.viewport.color,
                            eframe::wgpu::FilterMode::Linear,
                            self.texture,
                        );
                }
                let highlighted = self
                    .evaluated
                    .iter()
                    .filter(|n| {
                        self.owners
                            .get(&n.id)
                            .copied()
                            .is_some_and(|root| self.selected.contains(&root))
                            || self.selected.contains(&n.id)
                    })
                    .map(|n| n.id)
                    .collect();
                self.viewport.render(
                    &self.gpu.device,
                    &self.gpu.queue,
                    &self.camera,
                    &highlighted,
                );
                ui.painter().set(
                    image_slot,
                    egui::Shape::image(
                        self.texture,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    ),
                );
            });
        if !ui.input(|i| i.pointer.primary_down()) {
            self.editor.end_gesture();
        }
        if self.confirm_load {
            egui::Window::new("현재 작업을 바꿀까요?")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("저장하지 않은 변경이 있습니다. 불러오면 현재 작업이 바뀝니다.");
                    ui.horizontal(|ui| {
                        if ui.button("저장 후 불러오기").clicked() {
                            self.save();
                            if !self.dirty() {
                                self.load();
                                self.confirm_load = false;
                            }
                        }
                        if ui.button("저장하지 않고 불러오기").clicked() {
                            self.load();
                            self.confirm_load = false;
                        }
                        if ui.button("취소").clicked() {
                            self.confirm_load = false;
                        }
                    });
                });
        }
        self.autosave(ui.ctx());
    }
    fn on_exit(&mut self) {
        if self.recoverable.is_some() {
            return;
        }
        if let Some(path) = &self.recovery_path {
            if self.dirty() {
                let _ = studio_io::project::save_project(path, &self.editor.scene);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
fn ground(origin: glam::DVec3, direction: glam::DVec3) -> Option<glam::DVec3> {
    if direction.z.abs() < 1e-9 {
        return None;
    }
    let t = -origin.z / direction.z;
    (t >= 0.0).then_some(origin + direction * t)
}
