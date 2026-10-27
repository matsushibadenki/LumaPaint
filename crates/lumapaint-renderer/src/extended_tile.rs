//! Floating-point tile uploads in premultiplied extended linear sRGB.
//! This texture is a building block; it must be presented through a wide-gamut
//! output transform and must not be sampled by the encoded RGBA8 tile shader.
use half::f16;

pub struct LinearTileUpload {
    pub origin: [u32; 2],
    pub extent: [u32; 2],
    /// Number of RGBA pixels per source row.
    pub row_pixels: u32,
    pub pixels: Vec<f32>,
}

pub struct ExtendedTileTexture {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

impl ExtendedTileTexture {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || width > device.limits().max_texture_dimension_2d
            || height > device.limits().max_texture_dimension_2d
        {
            return Err("Invalid extended tile texture dimensions".into());
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Extended linear tile composite"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear extended tile composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        queue.submit([encoder.finish()]);
        Ok(Self {
            texture,
            width,
            height,
        })
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Validate every tile before any write, and retain out-of-sRGB values.
    pub fn upload(&self, queue: &wgpu::Queue, uploads: &[LinearTileUpload]) -> Result<(), String> {
        let mut prepared = Vec::with_capacity(uploads.len());
        for tile in uploads {
            let [width, height] = tile.extent;
            if width == 0
                || height == 0
                || tile.row_pixels < width
                || tile.origin[0]
                    .checked_add(width)
                    .is_none_or(|x| x > self.width)
                || tile.origin[1]
                    .checked_add(height)
                    .is_none_or(|y| y > self.height)
            {
                return Err("Invalid extended tile bounds".into());
            }
            let count = (tile.row_pixels as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4))
                .ok_or("Extended tile size overflow")?;
            if tile.pixels.len() != count {
                return Err("Invalid extended tile payload".into());
            }
            let bytes_per_row = width
                .checked_mul(8)
                .and_then(|n| n.checked_add(255))
                .ok_or("Extended tile row overflow")?
                / 256
                * 256;
            let mut data = vec![0u8; bytes_per_row as usize * height as usize];
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let src = (y * tile.row_pixels as usize + x) * 4;
                    let rgba = &tile.pixels[src..src + 4];
                    if rgba.iter().any(|v| !v.is_finite() || v.abs() > 65504.0)
                        || !(0.0..=1.0).contains(&rgba[3])
                    {
                        return Err("Invalid extended tile color".into());
                    }
                    for (c, value) in rgba.iter().enumerate() {
                        let dst = y * bytes_per_row as usize + x * 8 + c * 2;
                        data[dst..dst + 2]
                            .copy_from_slice(&f16::from_f32(*value).to_bits().to_le_bytes());
                    }
                }
            }
            prepared.push((tile, bytes_per_row, data));
        }
        for (tile, stride, data) in prepared {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: tile.origin[0],
                        y: tile.origin[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(tile.extent[1]),
                },
                wgpu::Extent3d {
                    width: tile.extent[0],
                    height: tile.extent[1],
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(())
    }
}
