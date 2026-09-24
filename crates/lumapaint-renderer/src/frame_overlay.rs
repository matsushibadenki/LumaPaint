//! Transient text-frame guides. Never enter document SVG or its raster cache.
use super::Viewport;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy)]
pub struct FrameOverlay {
    pub corners: [[f32; 2]; 4],
    pub handles: bool,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

fn quad(vertices: &mut Vec<Vertex>, points: [[f32; 2]; 4], color: [f32; 4]) {
    for index in [0, 1, 2, 0, 2, 3] {
        vertices.push(Vertex {
            position: points[index],
            color,
        });
    }
}

fn vertices(overlay: FrameOverlay, viewport: Viewport) -> Vec<Vertex> {
    let size = [
        viewport.width as f32 / viewport.scale,
        viewport.height as f32 / viewport.scale,
    ];
    let scale = ((size[0] - 48.0) / viewport.document_width)
        .min((size[1] - 48.0) / viewport.document_height)
        .max(0.01)
        * viewport.zoom;
    let blue = [60.0 / 255.0, 160.0 / 255.0, 1.0, 1.0];
    let mut vertices = Vec::with_capacity(120);
    for index in 0..4 {
        let a = overlay.corners[index];
        let b = overlay.corners[(index + 1) % 4];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = dx.hypot(dy).max(0.001);
        let normal = [-dy / length * 0.5 / scale, dx / length * 0.5 / scale];
        quad(
            &mut vertices,
            [
                [a[0] + normal[0], a[1] + normal[1]],
                [b[0] + normal[0], b[1] + normal[1]],
                [b[0] - normal[0], b[1] - normal[1]],
                [a[0] - normal[0], a[1] - normal[1]],
            ],
            blue,
        );
        if overlay.handles {
            for center in [a, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]] {
                for (radius, color) in [(4.0 / scale, blue), (3.0 / scale, [1.0; 4])] {
                    quad(
                        &mut vertices,
                        [
                            [center[0] - radius, center[1] - radius],
                            [center[0] + radius, center[1] - radius],
                            [center[0] + radius, center[1] + radius],
                            [center[0] - radius, center[1] + radius],
                        ],
                        color,
                    );
                }
            }
        }
    }
    vertices
}

pub(crate) fn buffer(
    device: &wgpu::Device,
    overlay: FrameOverlay,
    viewport: Viewport,
) -> (wgpu::Buffer, u32) {
    let vertices = vertices(overlay, viewport);
    (
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text frame guide vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        vertices.len() as u32,
    )
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Text frame guides"),
        source: wgpu::ShaderSource::Wgsl(include_str!("frame_overlay.wgsl").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Text frame guide pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
            }],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_use_constant_screen_size_and_small_geometry() {
        let overlay = FrameOverlay {
            corners: [[20.0, 20.0], [220.0, 20.0], [220.0, 120.0], [20.0, 120.0]],
            handles: true,
        };
        let viewport = Viewport {
            width: 1008,
            height: 688,
            scale: 1.0,
            zoom: 1.0,
            dark: false,
            pan_x: 0.0,
            pan_y: 0.0,
            document_width: 960.0,
            document_height: 640.0,
            canvas_color: lumapaint_core::document::CanvasColor::White,
        };
        let normal = vertices(overlay, viewport);
        let zoomed = vertices(
            overlay,
            Viewport {
                zoom: 2.0,
                ..viewport
            },
        );
        assert_eq!(normal.len(), 120); // Four lines and eight bordered handles.
        assert_eq!(
            vertices(
                FrameOverlay {
                    handles: false,
                    ..overlay
                },
                viewport
            )
            .len(),
            24
        );
        let normal_width = normal[7].position[0] - normal[6].position[0];
        let zoomed_width = zoomed[7].position[0] - zoomed[6].position[0];
        assert_eq!(normal_width, 8.0);
        assert_eq!(normal_width, zoomed_width * 2.0);
    }
}
