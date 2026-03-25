use std::{iter, mem::size_of, sync::mpsc, time::Duration};

use egui_wgpu::{
    wgpu,
    wgpu::{BufferUsages, Extent3d, TextureUsages},
    ScreenDescriptor, WgpuConfiguration, WgpuSetup,
};

use crate::{Error, KittyFrame, Result};

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct OffscreenRenderer {
    render_state: egui_wgpu::RenderState,
}

impl OffscreenRenderer {
    pub(crate) fn new() -> Result<Self> {
        let setup = WgpuSetup::default();
        let instance = pollster::block_on(setup.new_instance());
        let config = WgpuConfiguration {
            wgpu_setup: setup,
            ..Default::default()
        };

        let render_state = pollster::block_on(egui_wgpu::RenderState::create(
            &config,
            &instance,
            None,
            egui_wgpu::RendererOptions::PREDICTABLE,
        ))
        .map_err(|err| Error::WgpuInit(err.to_string()))?;

        Ok(Self { render_state })
    }

    pub(crate) fn render(
        &mut self,
        ctx: &egui::Context,
        output: &egui::FullOutput,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    ) -> Result<KittyFrame> {
        if width == 0 || height == 0 {
            return Err(Error::WgpuRender(
                "cannot render zero-sized frame".to_owned(),
            ));
        }

        let screen = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };

        let clipped_primitives = ctx.tessellate(output.shapes.clone(), pixels_per_point);

        let texture = self
            .render_state
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("egui-term-framebuffer"),
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.render_state.target_format,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
                view_formats: &[],
            });

        let bytes_per_pixel = size_of::<u32>() as u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let output_buffer = self
            .render_state
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("egui-term-readback"),
                size: padded_bytes_per_row as u64 * height as u64,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut renderer = self.render_state.renderer.write();
        for (id, image_delta) in &output.textures_delta.set {
            renderer.update_texture(
                &self.render_state.device,
                &self.render_state.queue,
                *id,
                image_delta,
            );
        }

        let mut encoder =
            self.render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("egui-term-encoder"),
                });

        let user_cmd_buffers = renderer.update_buffers(
            &self.render_state.device,
            &self.render_state.queue,
            &mut encoder,
            &clipped_primitives,
            &screen,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut render_pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui-term-render-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut render_pass, &clipped_primitives, &screen);
        }

        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        for id in &output.textures_delta.free {
            renderer.free_texture(id);
        }
        drop(renderer);

        let submission_index = self.render_state.queue.submit(
            user_cmd_buffers
                .into_iter()
                .chain(iter::once(encoder.finish())),
        );

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        self.render_state
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: Some(WAIT_TIMEOUT),
            })
            .map_err(|err| Error::WgpuRender(format!("poll failed: {err}")))?;

        rx.recv()
            .map_err(|_| Error::WgpuRender("readback channel closed".to_owned()))?
            .map_err(|err| Error::WgpuRender(format!("map_async failed: {err}")))?;

        let mapped = buffer_slice.get_mapped_range();
        let mut rgba = vec![0_u8; (width * height * bytes_per_pixel) as usize];

        for (row_idx, row) in mapped
            .chunks_exact(padded_bytes_per_row as usize)
            .take(height as usize)
            .enumerate()
        {
            let dst_offset = row_idx * unpadded_bytes_per_row as usize;
            rgba[dst_offset..dst_offset + unpadded_bytes_per_row as usize]
                .copy_from_slice(&row[..unpadded_bytes_per_row as usize]);
        }

        drop(mapped);
        output_buffer.unmap();

        match self.render_state.target_format {
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {}
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                for chunk in rgba.chunks_exact_mut(4) {
                    chunk.swap(0, 2);
                }
            }
            format => {
                return Err(Error::WgpuRender(format!(
                    "unsupported texture format: {format:?}"
                )));
            }
        }

        Ok(KittyFrame {
            width,
            height,
            rgba,
        })
    }
}
