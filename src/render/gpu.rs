//! GPU-side implementation of the treemap shader widget.
//!
//! One instance is a single rounded rectangle. Six vertices are generated in
//! the vertex shader; instance data is uploaded to a persistent vertex buffer
//! owned by the widget state. The fragment shader evaluates a rounded-box SDF
//! and emits premultiplied alpha for Iced's sRGB render target.

use super::{DrawList, sanitize_channel, srgb_channel_to_linear, tile_visible};
use iced::Rectangle;
use iced::wgpu;
use iced::widget::shader;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Byte size of one packed instance, matching the WGSL `Instance` struct.
pub(crate) const INSTANCE_SIZE: usize = 64;

/// GPU resources that survive between frames.
///
/// Iced stores this in the shader widget's tree state. Cloning the state
/// shares the buffer, so repainting a scene does not rebuild GPU resources.
pub struct TreemapState {
    gpu: Arc<Mutex<GpuBatch>>,
}

impl Default for TreemapState {
    fn default() -> Self {
        Self {
            gpu: Arc::new(Mutex::new(GpuBatch::default())),
        }
    }
}

impl fmt::Debug for TreemapState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreemapState")
            .field("shared", &Arc::strong_count(&self.gpu))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub(crate) struct GpuBatch {
    buffer: Option<wgpu::Buffer>,
    capacity_bytes: usize,
    count: u32,
}

/// A prepared treemap frame, ready to be recorded by Iced.
///
/// This type is an implementation detail of [`super::TreemapProgram`], but it
/// must be public because it is the program's associated `Primitive` type.
#[derive(Debug)]
pub struct TreemapPrimitive {
    draw: Arc<DrawList>,
    bounds: Rectangle,
    gpu: Arc<Mutex<GpuBatch>>,
}

impl TreemapPrimitive {
    pub(crate) fn new(draw: Arc<DrawList>, bounds: Rectangle, state: &TreemapState) -> Self {
        Self {
            draw,
            bounds,
            gpu: Arc::clone(&state.gpu),
        }
    }
}

impl shader::Primitive for TreemapPrimitive {
    type Pipeline = TreemapPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        let (data, count) = encode_instances(
            &self.draw,
            self.bounds.width,
            self.bounds.height,
            pipeline.target_is_srgb,
        );

        let mut batch = self
            .gpu
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if count == 0 || data.is_empty() {
            batch.count = 0;
            return;
        }

        let needed = data.len();
        let needs_allocation = batch
            .buffer
            .as_ref()
            .is_none_or(|_| batch.capacity_bytes < needed);

        if needs_allocation {
            // Grow geometrically so small scene changes do not reallocate.
            let capacity = needed
                .checked_next_power_of_two()
                .unwrap_or(needed)
                .max(INSTANCE_SIZE);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("space.treemap.instances"),
                size: capacity as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            batch.buffer = Some(buffer);
            batch.capacity_bytes = capacity;
        }

        if let Some(buffer) = &batch.buffer {
            queue.write_buffer(buffer, 0, &data);
            batch.count = count;
        } else {
            batch.count = 0;
        }
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let batch = self
            .gpu
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if batch.count == 0 {
            return true;
        }

        let Some(buffer) = &batch.buffer else {
            return true;
        };

        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..batch.count);

        true
    }
}

/// The render pipeline shared by every treemap widget in the application.
///
/// This type is public only because it is the associated `Pipeline` type of
/// [`TreemapPrimitive`].
#[derive(Debug)]
pub struct TreemapPipeline {
    pipeline: wgpu::RenderPipeline,
    target_is_srgb: bool,
}

impl shader::Pipeline for TreemapPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // `iced_wgpu` 0.14 renders custom primitives inside its main,
        // single-sample, depth-less render pass (`Renderer::render` in
        // iced_wgpu). `Pipeline::new` is not given a sample count in this
        // version, so 1 is the only value that matches the target.
        const MULTISAMPLE_COUNT: u32 = 1;

        const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
            0 => Float32x2, // center in NDC
            1 => Float32x2, // half size in NDC
            2 => Float32x2, // half size in logical pixels
            3 => Float32,   // corner radius in logical pixels
            4 => Float32,   // inner stroke width in logical pixels
            5 => Float32x4, // fill color, linear or target space
            6 => Float32x4, // stroke color, linear or target space
        ];

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("space.treemap.pipeline_layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("space.treemap.shader"),
            source: wgpu::ShaderSource::Wgsl(TREEMAP_WGSL.into()),
        });

        let vertex_buffers = [wgpu::VertexBufferLayout {
            array_stride: INSTANCE_SIZE as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }];

        let targets = [Some(wgpu::ColorTargetState {
            format,
            // Colors are premultiplied in the fragment shader, matching the
            // rest of iced_wgpu.
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("space.treemap.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &vertex_buffers,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &targets,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: MULTISAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            target_is_srgb: format.is_srgb(),
        }
    }
}

/// Encodes one vertex-buffer instance per visible tile.
///
/// Returns the raw little-endian bytes and the instance count. The function is
/// pure so it can be tested without a GPU.
pub(crate) fn encode_instances(
    draw: &DrawList,
    width: f32,
    height: f32,
    target_is_srgb: bool,
) -> (Vec<u8>, u32) {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return (Vec::new(), 0);
    }

    let mut bytes = Vec::new();
    let mut count = 0u32;

    for tile in &draw.tiles {
        if !tile_visible(tile, width, height) {
            continue;
        }

        let rect = tile.rect;
        let half_w = rect.w * 0.5;
        let half_h = rect.h * 0.5;
        let center_x = (rect.x + half_w) / width * 2.0 - 1.0;
        let center_y = 1.0 - (rect.y + half_h) / height * 2.0;
        let half_ndc_x = half_w / width * 2.0;
        let half_ndc_y = half_h / height * 2.0;

        let radius = non_negative(tile.radius).min(half_w.min(half_h));
        let stroke = non_negative(tile.border_width).min(half_w.min(half_h));
        let fill = upload_color(tile.color, target_is_srgb);
        let stroke_color = upload_color(tile.border, target_is_srgb);

        if !center_x.is_finite()
            || !center_y.is_finite()
            || !half_ndc_x.is_finite()
            || !half_ndc_y.is_finite()
        {
            continue;
        }

        push_vec2(&mut bytes, center_x, center_y);
        push_vec2(&mut bytes, half_ndc_x, half_ndc_y);
        push_vec2(&mut bytes, half_w, half_h);
        push_f32(&mut bytes, radius);
        push_f32(&mut bytes, stroke);
        push_vec4(&mut bytes, fill);
        push_vec4(&mut bytes, stroke_color);

        count += 1;
    }

    debug_assert_eq!(bytes.len(), count as usize * INSTANCE_SIZE);
    (bytes, count)
}

fn upload_color(color: [f32; 4], target_is_srgb: bool) -> [f32; 4] {
    if !target_is_srgb {
        return [
            sanitize_channel(color[0]),
            sanitize_channel(color[1]),
            sanitize_channel(color[2]),
            sanitize_channel(color[3]),
        ];
    }

    [
        srgb_channel_to_linear(color[0]),
        srgb_channel_to_linear(color[1]),
        srgb_channel_to_linear(color[2]),
        sanitize_channel(color[3]),
    ]
}

fn non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_vec2(bytes: &mut Vec<u8>, x: f32, y: f32) {
    push_f32(bytes, x);
    push_f32(bytes, y);
}

fn push_vec4(bytes: &mut Vec<u8>, values: [f32; 4]) {
    for value in values {
        push_f32(bytes, value);
    }
}

/// WGSL kept inline as a `&str` so the renderer has no runtime asset lookup.
///
/// The widget's render pass is already scissored to its bounds by iced_wgpu;
/// this shader draws each instance over its own rectangle and antialiases the
/// rounded corners and inner stroke with an SDF. Output is premultiplied.
const TREEMAP_WGSL: &str = r#"
struct Instance {
    @location(0) center: vec2<f32>,
    @location(1) half_ndc: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) radius: f32,
    @location(4) stroke_width: f32,
    @location(5) fill_color: vec4<f32>,
    @location(6) stroke_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) radius: f32,
    @location(3) stroke_width: f32,
    @location(4) fill_color: vec4<f32>,
    @location(5) stroke_color: vec4<f32>,
};

const CORNERS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
);

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: Instance,
) -> VertexOutput {
    let corner = CORNERS[vertex_index] * 2.0 - vec2<f32>(1.0);
    let sign = corner * instance.half_size;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(
        instance.center + corner * instance.half_ndc,
        0.0,
        1.0,
    );
    out.local = sign;
    out.half_size = instance.half_size;
    out.radius = instance.radius;
    out.stroke_width = instance.stroke_width;
    out.fill_color = instance.fill_color;
    out.stroke_color = instance.stroke_color;
    return out;
}

fn rounded_box_sdf(
    point: vec2<f32>,
    half_size: vec2<f32>,
    radius: f32,
) -> f32 {
    let q = abs(point) - half_size + vec2<f32>(radius);
    return min(max(q.x, q.y), 0.0)
        + length(max(q, vec2<f32>(0.0)))
        - radius;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let distance = rounded_box_sdf(
        input.local,
        input.half_size,
        input.radius,
    );

    // One physical pixel of antialiasing. `local` is in logical pixels, so
    // the derivative accounts for the window scale factor automatically.
    let aa = max(fwidth(distance), 0.0001);
    let coverage = clamp(0.5 - distance / aa, 0.0, 1.0);

    // The stroke is drawn on the inside of the rounded rectangle. Keep the
    // same smoothstep width as the outer edge so 1px hairlines survive.
    let stroke = clamp(
        input.stroke_width,
        0.0,
        min(input.half_size.x, input.half_size.y),
    );

    var straight = input.fill_color;
    if (stroke > 0.0) {
        let stroke_mask = clamp(
            0.5 + (distance + stroke) / aa,
            0.0,
            1.0,
        );
        straight = mix(input.fill_color, input.stroke_color, stroke_mask);
    }

    let alpha = coverage * straight.a;
    return vec4<f32>(straight.rgb * alpha, alpha);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::DrawTile;
    use crate::treemap::Rect;

    fn tile(rect: Rect, color: [f32; 4]) -> DrawTile {
        DrawTile::new(rect, color)
    }

    #[test]
    fn instance_layout_and_color_conversion() {
        let draw = DrawList::new(vec![tile(
            Rect::new(0.0, 0.0, 100.0, 50.0),
            [0.5, 0.5, 0.5, 1.0],
        )]);
        let (bytes, count) = encode_instances(&draw, 200.0, 100.0, true);

        assert_eq!(count, 1);
        assert_eq!(bytes.len(), INSTANCE_SIZE);

        let values: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect();

        assert!((values[0] + 0.5).abs() < 1e-6); // center x = 50 / 200 * 2 - 1
        assert!((values[1] - 0.5).abs() < 1e-6); // center y = 1 - 25 / 100 * 2
        assert!((values[2] - 0.5).abs() < 1e-6); // half width in NDC
        assert!((values[3] - 0.5).abs() < 1e-6); // half height in NDC
        assert!((values[4] - 50.0).abs() < 1e-6); // half width in pixels
        assert!((values[5] - 25.0).abs() < 1e-6); // half height in pixels
        assert!((values[8] - 0.214_041_14).abs() < 1e-5); // linear 0.5
    }

    #[test]
    fn off_canvas_tiles_are_culled() {
        let draw = DrawList::new(vec![
            tile(Rect::new(-100.0, 0.0, 50.0, 50.0), [1.0; 4]),
            tile(Rect::new(200.0, 0.0, 50.0, 50.0), [1.0; 4]),
            tile(Rect::new(90.0, 90.0, 50.0, 50.0), [1.0; 4]),
        ]);

        let (_, count) = encode_instances(&draw, 100.0, 100.0, true);
        assert_eq!(count, 1); // only the bottom-right tile intersects
    }
}
