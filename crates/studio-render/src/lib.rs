//! Offscreen wgpu viewport, independent of egui widgets.
use glam::{Mat4, Vec3, Vec4};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use studio_core::{Evaluated, NodeId};
use studio_geometry::{cross, sub, Mesh};
use wgpu::util::DeviceExt;

pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: -0.9,
            pitch: 0.55,
            distance: 220.0,
            target: Vec3::new(0.0, 0.0, 35.0),
        }
    }
}
impl Camera {
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        let offset = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
        ) * self.distance;
        Mat4::perspective_rh(45.0_f32.to_radians(), aspect, 0.1, 10000.0)
            * Mat4::look_at_rh(self.target + offset, self.target, Vec3::Z)
    }
    pub fn ray(&self, normalized: [f32; 2], aspect: f32) -> (glam::DVec3, glam::DVec3) {
        let inverse = self.matrix(aspect).inverse();
        let unproject = |z| {
            let p = inverse * Vec4::new(normalized[0], normalized[1], z, 1.0);
            (p.truncate() / p.w).as_dvec3()
        };
        let near = unproject(0.0);
        (near, (unproject(1.0) - near).normalize())
    }
    pub fn orbit(&mut self, x: f32, y: f32) {
        self.yaw -= x * 0.01;
        self.pitch = (self.pitch + y * 0.01).clamp(-1.5, 1.5);
    }
    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(2.0, 5000.0);
    }
    pub fn pan(&mut self, x: f32, y: f32) {
        let right = Vec3::new(-self.yaw.sin(), self.yaw.cos(), 0.0);
        let offset = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
        );
        let up = offset.cross(right);
        self.target += (right * x + up * y) * self.distance * 0.001;
    }
}

struct GpuObject {
    mesh: Arc<Mesh>,
    vertex: wgpu::Buffer,
    count: u32,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    world: Mat4,
    color: [f32; 3],
}
pub struct Viewport {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    grid_pipeline: wgpu::RenderPipeline,
    grid_vertex: wgpu::Buffer,
    grid_uniform: wgpu::Buffer,
    grid_bind: wgpu::BindGroup,
    grid_count: u32,
    objects: BTreeMap<NodeId, GpuObject>,
    pub color: wgpu::TextureView,
    depth: wgpu::TextureView,
    size: [u32; 2],
}
fn texture(device: &wgpu::Device, size: [u32; 2], depth: bool) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("cubilo viewport"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if depth {
                wgpu::TextureFormat::Depth32Float
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | if depth {
                    wgpu::TextureUsages::empty()
                } else {
                    wgpu::TextureUsages::TEXTURE_BINDING
                },
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl Viewport {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cubilo mesh"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = pipeline(
            device,
            &module,
            &pipeline_layout,
            wgpu::PrimitiveTopology::TriangleList,
        );
        let grid_pipeline = pipeline_for_grid(device, &module, &pipeline_layout);
        let mut grid_data = Vec::new();
        for i in -50..=50 {
            let coordinate = i as f32 * 10.0;
            for point in [
                [coordinate, -500.0, 0.0],
                [coordinate, 500.0, 0.0],
                [-500.0, coordinate, 0.0],
                [500.0, coordinate, 0.0],
            ] {
                for value in point.into_iter().chain([0.0, 0.0, 1.0]) {
                    grid_data.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        let grid_vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("10mm grid"),
            contents: &grid_data,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grid_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grid transform"),
            size: 144,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grid_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: grid_uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            layout,
            grid_pipeline,
            grid_vertex,
            grid_uniform,
            grid_bind,
            grid_count: 404,
            objects: BTreeMap::new(),
            color: texture(device, [1, 1], false),
            depth: texture(device, [1, 1], true),
            size: [1, 1],
        }
    }
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) -> bool {
        let max = device.limits().max_texture_dimension_2d;
        let size = size.map(|v| v.clamp(1, max));
        if self.size == size {
            return false;
        }
        self.size = size;
        self.color = texture(device, size, false);
        self.depth = texture(device, size, true);
        true
    }
    pub fn set_scene(&mut self, device: &wgpu::Device, scene: &[Evaluated]) {
        self.objects
            .retain(|id, _| scene.iter().any(|n| n.id == *id));
        for item in scene {
            if self
                .objects
                .get(&item.id)
                .is_none_or(|old| !Arc::ptr_eq(&old.mesh, &item.mesh))
            {
                let mesh = &item.mesh;
                let mut data = Vec::with_capacity(mesh.triangles().len() * 72);
                for &[a, b, c] in mesh.triangles() {
                    let [a, b, c] = [a, b, c].map(|i| mesh.vertices()[i]);
                    let n = cross(sub(b, a), sub(c, a));
                    let len = n.iter().map(|v| v * v).sum::<f64>().sqrt();
                    for p in [a, b, c] {
                        for v in p.into_iter().chain(n.map(|v| v / len)) {
                            data.extend_from_slice(&(v as f32).to_le_bytes());
                        }
                    }
                }
                let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh"),
                    contents: &data,
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("object"),
                    size: 144,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
                self.objects.insert(
                    item.id,
                    GpuObject {
                        mesh: item.mesh.clone(),
                        vertex,
                        count: (mesh.triangles().len() * 3) as u32,
                        uniform,
                        bind_group,
                        world: item.world.as_mat4(),
                        color: item.color,
                    },
                );
            }
            let object = self.objects.get_mut(&item.id).unwrap();
            object.world = item.world.as_mat4();
            object.color = item.color;
        }
    }
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        selected: &BTreeSet<NodeId>,
    ) {
        let camera_matrix = camera.matrix(self.size[0] as f32 / self.size[1] as f32);
        let grid_bytes: Vec<u8> = camera_matrix
            .to_cols_array()
            .into_iter()
            .chain(Mat4::IDENTITY.to_cols_array())
            .chain([0.86, 0.89, 0.94, 2.0])
            .flat_map(f32::to_le_bytes)
            .collect();
        queue.write_buffer(&self.grid_uniform, 0, &grid_bytes);
        for (&id, object) in &self.objects {
            let color = if selected.contains(&id) {
                [
                    object.color[0] * 0.9 + 0.1,
                    object.color[1] * 0.9 + 0.1,
                    object.color[2] * 0.9 + 0.1,
                    1.0,
                ]
            } else {
                [object.color[0], object.color[1], object.color[2], 1.0]
            };
            let bytes: Vec<u8> = (camera_matrix * object.world)
                .to_cols_array()
                .into_iter()
                .chain(object.world.to_cols_array())
                .chain(color)
                .flat_map(f32::to_le_bytes)
                .collect();
            queue.write_buffer(&object.uniform, 0, &bytes);
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cubilo viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.952,
                            g: 0.967,
                            b: 0.99,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.grid_pipeline);
            pass.set_bind_group(0, &self.grid_bind, &[]);
            pass.set_vertex_buffer(0, self.grid_vertex.slice(..));
            pass.draw(0..self.grid_count, 0..1);
            pass.set_pipeline(&self.pipeline);
            for object in self.objects.values() {
                pass.set_bind_group(0, &object.bind_group, &[]);
                pass.set_vertex_buffer(0, object.vertex.slice(..));
                pass.draw(0..object.count, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    topology: wgpu::PrimitiveTopology,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cubilo mesh"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}
fn pipeline_for_grid(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    pipeline(device, module, layout, wgpu::PrimitiveTopology::LineList)
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{Command, Editor, Evaluator, Primitive};
    #[test]
    #[ignore = "requires a real GPU; run explicitly on validation hardware"]
    fn offscreen_gpu_scene_and_selection_smoke() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
        eprintln!("GPU smoke: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).expect("GPU device");
        let mut viewport = Viewport::new(&device);
        viewport.resize(&device, [320, 240]);
        let mut editor = Editor::default();
        let id = editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap()[0];
        let mut evaluator = Evaluator::default();
        let scene = evaluator.evaluate(&editor.scene).unwrap();
        viewport.set_scene(&device, &scene);
        viewport.render(&device, &queue, &Camera::default(), &BTreeSet::from([id]));
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        editor.apply(Command::Delete(vec![id])).unwrap();
        viewport.set_scene(&device, &evaluator.evaluate(&editor.scene).unwrap());
        viewport.render(&device, &queue, &Camera::default(), &BTreeSet::new());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
}
