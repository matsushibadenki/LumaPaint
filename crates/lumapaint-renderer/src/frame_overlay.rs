//! Transient text-frame guides. Never enter document SVG or its raster cache.
use super::Viewport;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy)]
pub struct FrameOverlay {
    pub corners: [[f32; 2]; 4],
    pub handles: bool,
    pub baseline: Option<[[f32; 2]; 2]>,
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
    styled_vertices(overlay, viewport, false)
}
fn styled_vertices(overlay: FrameOverlay, viewport: Viewport, highlighted: bool) -> Vec<Vertex> {
    let size = [
        viewport.width as f32 / viewport.scale,
        viewport.height as f32 / viewport.scale,
    ];
    let scale = ((size[0] - 48.0) / viewport.document_width)
        .min((size[1] - 48.0) / viewport.document_height)
        .max(0.01)
        * viewport.zoom;
    let blue = if highlighted {
        [1., 0.35, 0.65, 1.]
    } else {
        [60.0 / 255.0, 160.0 / 255.0, 1.0, 1.0]
    };
    let mut vertices = Vec::with_capacity(120);
    for (index, [a, b]) in (0..4)
        .map(|i| [overlay.corners[i], overlay.corners[(i + 1) % 4]])
        .chain(overlay.baseline)
        .enumerate()
    {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = dx.hypot(dy).max(0.001);
        let half_width = if highlighted { 1. } else { 0.5 };
        let normal = [
            -dy / length * half_width / scale,
            dx / length * half_width / scale,
        ];
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
        if overlay.handles && index < 4 {
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

#[cfg(test)]
pub(crate) fn buffer(
    device: &wgpu::Device,
    overlay: FrameOverlay,
    viewport: Viewport,
) -> (wgpu::Buffer, u32) {
    buffer_many(device, &[overlay], viewport)
}
#[cfg(test)]
pub(crate) fn buffer_many(
    device: &wgpu::Device,
    overlays: &[FrameOverlay],
    viewport: Viewport,
) -> (wgpu::Buffer, u32) {
    buffer_highlighted(device, overlays, &[], viewport)
}
pub(crate) fn buffer_highlighted(
    device: &wgpu::Device,
    overlays: &[FrameOverlay],
    highlighted: &[FrameOverlay],
    viewport: Viewport,
) -> (wgpu::Buffer, u32) {
    let vertices: Vec<Vertex> = overlays
        .iter()
        .flat_map(|o| vertices(*o, viewport))
        .chain(
            highlighted
                .iter()
                .flat_map(|o| styled_vertices(*o, viewport, true)),
        )
        .collect();
    (
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Frame guide vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        vertices.len() as u32,
    )
}
pub(crate) fn graphics_frames(document: &lumapaint_core::document::Document) -> Vec<FrameOverlay> {
    let mut overlays = Vec::new();
    for object in document
        .svg_layers()
        .filter(|l| l.visible)
        .flat_map(|l| l.vector_objects.iter())
        .filter(|o| o.visible && o.image_frame.is_some())
    {
        let Some([x, y, r, b]) = lumapaint_core::stroke::path_bounds(&object.path.data) else {
            continue;
        };
        let [a, bb, c, d, e, f] = object.transform;
        let world = |p: [f32; 2]| [a * p[0] + c * p[1] + e, bb * p[0] + d * p[1] + f];
        let ellipse = object.kind == lumapaint_core::vector::VectorObjectKind::Ellipse;
        if ellipse {
            for i in 0..32 {
                let point = |j| {
                    let t = j as f32 * std::f32::consts::TAU / 32.;
                    world([
                        (x + r) * 0.5 + (r - x) * 0.5 * t.cos(),
                        (y + b) * 0.5 + (b - y) * 0.5 * t.sin(),
                    ])
                };
                let p = point(i);
                let q = point(i + 1);
                overlays.push(FrameOverlay {
                    corners: [p, q, q, p],
                    handles: false,
                    baseline: None,
                });
            }
        } else {
            overlays.push(FrameOverlay {
                corners: [[x, y], [r, y], [r, b], [x, b]].map(world),
                handles: false,
                baseline: None,
            });
        }
        if object.image_frame.as_ref().unwrap().image.is_none() {
            let inset = if ellipse {
                (1. - std::f32::consts::FRAC_1_SQRT_2) * 0.5
            } else {
                0.
            };
            let dx = (r - x) * inset;
            let dy = (b - y) * inset;
            let p = world([x + dx, y + dy]);
            let q = world([r - dx, b - dy]);
            let p2 = world([r - dx, y + dy]);
            let q2 = world([x + dx, b - dy]);
            overlays.push(FrameOverlay {
                corners: [p, q, q, p],
                handles: false,
                baseline: Some([p2, q2]),
            });
        }
    }
    overlays
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

pub(crate) fn document_guides(
    document: &lumapaint_core::document::Document,
    viewport: Viewport,
) -> Vec<FrameOverlay> {
    guide_overlays(document, viewport, false)
}
pub(crate) fn selected_guides(
    document: &lumapaint_core::document::Document,
    viewport: Viewport,
) -> Vec<FrameOverlay> {
    guide_overlays(document, viewport, true)
}
fn guide_overlays(
    document: &lumapaint_core::document::Document,
    viewport: Viewport,
    selected: bool,
) -> Vec<FrameOverlay> {
    if !document.guides().visible {
        return vec![];
    }
    let start = viewport.document_point(0., 0.);
    let end = viewport.document_point(
        viewport.width as f32 / viewport.scale,
        viewport.height as f32 / viewport.scale,
    );
    document
        .guides()
        .items
        .iter()
        .filter(|g| {
            !selected || (!document.guides().locked && document.guides().selected.contains(&g.id))
        })
        .flat_map(|g| {
            let edges = match g.axis.as_deref() {
                Some("horizontal") => vec![[[start.x, g.position], [end.x, g.position]]],
                Some("vertical") => vec![[[g.position, start.y], [g.position, end.y]]],
                _ => g
                    .objects
                    .iter()
                    .flat_map(|o| lumapaint_core::stroke::path_edges(&o.path.data, o.transform))
                    .collect(),
            };
            edges.into_iter().map(|[p, q]| FrameOverlay {
                corners: [p, q, q, p],
                handles: false,
                baseline: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_use_constant_screen_size_and_small_geometry() {
        let overlay = FrameOverlay {
            corners: [[20.0, 20.0], [220.0, 20.0], [220.0, 120.0], [20.0, 120.0]],
            handles: true,
            baseline: None,
        };
        let viewport = Viewport {
            pasteboard_color: None,
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
                pasteboard_color: None,
                zoom: 2.0,
                ..viewport
            },
        );
        assert_eq!(
            vertices(
                FrameOverlay {
                    baseline: Some([[20., 80.], [220., 80.]]),
                    ..overlay
                },
                viewport
            )
            .len(),
            126
        );
        assert_eq!(normal.len(), 120); // Four lines and eight bordered handles.
        assert_eq!(
            vertices(
                FrameOverlay {
                    handles: false,
                    baseline: None,
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

#[cfg(test)]
mod guide_tests {
    use super::*;
    use lumapaint_core::document::{Document, GuideEdit};
    #[test]
    fn selected_guides_are_highlighted_only_when_visible_and_unlocked() {
        let mut d = Document::default();
        let edit = |action: &str| GuideEdit {
            action: action.into(),
            id: None,
            axis: Some("vertical".into()),
            position: Some(100.),
            delta: None,
        };
        d.edit_guides(edit("add")).unwrap();
        d.edit_guides(edit("lock")).unwrap();
        let id = d.guides().items[0].id.clone();
        d.select_guide(Some(id));
        let v = Viewport::new(1200., 800., 2., 1., true).unwrap();
        let selected = selected_guides(&d, v);
        assert_eq!(selected.len(), 1);
        let normal = styled_vertices(selected[0], v, false);
        let highlight = styled_vertices(selected[0], v, true);
        assert_ne!(normal[0].color, highlight[0].color);
        let width = |points: &Vec<Vertex>| (points[0].position[0] - points[2].position[0]).abs();
        assert!((width(&highlight) - width(&normal) * 2.).abs() < 0.0001);
        d.edit_guides(edit("lock")).unwrap();
        assert!(selected_guides(&d, v).is_empty());
        d.edit_guides(edit("visibility")).unwrap();
        assert!(document_guides(&d, v).is_empty());
    }
    #[test]
    fn guides_cover_visible_pasteboard_and_hide_without_changing_artwork() {
        let mut d = Document::default();
        d.edit_guides(GuideEdit {
            action: "add".into(),
            id: None,
            axis: Some("horizontal".into()),
            position: Some(123.),
            delta: None,
        })
        .unwrap();
        let viewport = Viewport::new(1200., 800., 2., 1., true).unwrap();
        let overlays = document_guides(&d, viewport);
        assert_eq!(overlays.len(), 1);
        let p = viewport.document_point(0., 0.);
        let q = viewport.document_point(
            viewport.width as f32 / viewport.scale,
            viewport.height as f32 / viewport.scale,
        );
        assert_eq!(overlays[0].corners[0], [p.x, 123.]);
        assert_eq!(overlays[0].corners[1], [q.x, 123.]);
        assert_eq!(d.visible_svg_layers().count(), 0);
        d.edit_guides(GuideEdit {
            action: "visibility".into(),
            id: None,
            axis: None,
            position: None,
            delta: None,
        })
        .unwrap();
        assert!(document_guides(&d, viewport).is_empty());
        assert_eq!(d.guides().items.len(), 1);
    }
}
