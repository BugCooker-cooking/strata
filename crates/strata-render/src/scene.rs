use crate::{RenderError, ViewTransform, Viewport};
use half::f16;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
use strata_core::{
    Document,
    composite::{Compositor, LinearTile},
};
use strata_pixels::TileCoord;
use strata_pixels::memory::{CacheSlot, MemoryKind, Reservation};

const MAX_GPU_TILES: usize = 1024;
pub struct PreparedScene {
    pub(crate) owner: u64,
    pub(crate) scene: Scene,
    pub(crate) view: ViewTransform,
}
pub(crate) struct Scene {
    pub document: Document,
    compositor: Compositor,
    cache: HashMap<(TileCoord, u32), Arc<CacheSlot<GpuTile>>>,
    fingerprints: HashMap<TileCoord, u64>,
    visible: Vec<DrawTile>,
    last: Option<(ViewTransform, Viewport)>,
    camera: Option<(wgpu::Buffer, wgpu::BindGroup, Reservation)>,
}
struct GpuTile {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    signatures: Vec<u64>,
    _reservation: Reservation,
}
struct DrawTile {
    _tile: Arc<GpuTile>,
    _uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    coord: TileCoord,
    lod: u32,
    _reservation: Reservation,
}

pub(crate) fn pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Strata tiled canvas"),
        source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Tiled linear P3"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
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
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
fn err(e: impl std::fmt::Display) -> RenderError {
    RenderError::Gpu(e.to_string())
}
impl Scene {
    pub fn new(document: Document, old: Option<&Scene>) -> Self {
        Self {
            document,
            compositor: old
                .map(|s| s.compositor.clone())
                .unwrap_or_else(|| Compositor::new(128)),
            cache: old.map(|s| s.cache.clone()).unwrap_or_default(),
            fingerprints: HashMap::new(),
            visible: Vec::new(),
            last: None,
            camera: None,
        }
    }
    pub fn prepare_visible(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &wgpu::RenderPipeline,
        sampler: &wgpu::Sampler,
        view: ViewTransform,
        vp: Viewport,
    ) -> Result<(), RenderError> {
        if self.last == Some((view, vp)) {
            return Ok(());
        }
        if self.camera.is_none() {
            let reservation = self
                .document
                .memory_pool()
                .reserve(MemoryKind::Gpu, 256)
                .map_err(err)?;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shared canvas camera"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Canvas camera"),
                layout: &pipeline.get_bind_group_layout(1),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            self.camera = Some((buffer, bind, reservation));
        }
        queue.write_buffer(
            &self.camera.as_ref().unwrap().0,
            0,
            bytemuck::cast_slice(&[
                view.center_x,
                view.center_y,
                vp.width.max(1) as f32,
                vp.height.max(1) as f32,
                view.zoom,
                0.0,
                0.0,
                0.0,
            ]),
        );
        let lod = (-view.zoom.log2()).floor().clamp(0.0, 8.0) as u32;
        let coords = visible_coords(&self.document, view, vp)?;
        let current: Vec<_> = self.visible.iter().map(|t| (t.coord, t.lod)).collect();
        let next: Vec<_> = coords.iter().map(|c| (*c, lod)).collect();
        if self.last.is_some() && current == next {
            self.last = Some((view, vp));
            return Ok(());
        }
        let mut visible = Vec::new();
        if self.fingerprints.len() >= 4096 {
            self.fingerprints.clear();
        }
        for coord in coords {
            let mut signatures = Vec::new();
            let mut neighbor_coords = Vec::new();
            for y in -1..=1 {
                for x in -1..=1 {
                    let c = TileCoord::new(coord.x() + x, coord.y() + y).map_err(err)?;
                    // A Scene owns an immutable document snapshot. Rehash only
                    // when a new scene is created, not on every camera change.
                    let signature = *self
                        .fingerprints
                        .entry(c)
                        .or_insert_with(|| Compositor::signature(&self.document, c));
                    signatures.push(signature);
                    neighbor_coords.push(c);
                }
            }
            let cached = self
                .cache
                .get(&(coord, lod))
                .and_then(|t| t.get())
                .filter(|t| t.signatures == signatures);
            let tile = if let Some(tile) = cached {
                tile
            } else {
                let neighbors = neighbor_coords
                    .into_iter()
                    .map(|c| self.compositor.tile(&self.document, c).map_err(err))
                    .collect::<Result<Vec<_>, _>>()?;
                // Prewarm the next sharper mip when one ordinary 1.25x zoom
                // step can cross its threshold. Uploading full-resolution
                // textures far below that threshold delays photo opening.
                let near_sharper_level = lod > 0 && view.zoom * 1.25 >= 2.0f32.powi(1 - lod as i32);
                let levels = if near_sharper_level && self.last.is_none() {
                    vec![lod - 1, lod]
                } else {
                    vec![lod]
                };
                let mut selected = None;
                for level in levels {
                    let (size, data) = mip_with_gutter(&neighbors, level);
                    let reservation = self
                        .document
                        .memory_pool()
                        .reserve(MemoryKind::Gpu, u64::from(size) * u64::from(size) * 8)
                        .map_err(err)?;
                    let allocation = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
                    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
                    let texture = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("Visible source tile mip with gutter"),
                        size: wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba16Float,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        bytemuck::cast_slice(&data),
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(size * 8),
                            rows_per_image: Some(size),
                        },
                        wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                    );
                    let texture_view = texture.create_view(&Default::default());
                    if let Some(error) = pollster::block_on(validation.pop())
                        .or(pollster::block_on(allocation.pop()))
                    {
                        return Err(err(error));
                    }
                    let tile = Arc::new(GpuTile {
                        _texture: texture,
                        view: texture_view,
                        signatures: signatures.clone(),
                        _reservation: reservation,
                    });
                    if self.cache.len() >= MAX_GPU_TILES {
                        self.cache.clear();
                    }
                    // The cache owns an evictable reference; the draw list pins
                    // only the textures used by the current frame.
                    let slot = CacheSlot::with_kind(
                        &self.document.memory_pool(),
                        tile.clone(),
                        MemoryKind::Gpu,
                    );
                    self.cache.insert((coord, level), slot);
                    if level == lod {
                        selected = Some(tile);
                    }
                }
                selected.unwrap()
            };
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Tile placement"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            update_uniform(queue, &uniform, coord, lod);
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Tile display"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&tile.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
            visible.push(DrawTile {
                _reservation: self
                    .document
                    .memory_pool()
                    .reserve(MemoryKind::Gpu, 256)
                    .map_err(err)?,
                _tile: tile,
                _uniform: uniform,
                bind,
                coord,
                lod,
            });
        }
        self.visible = visible;
        self.last = Some((view, vp));
        Ok(())
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, pipeline: &'a wgpu::RenderPipeline) {
        pass.set_pipeline(pipeline);
        if let Some((_, bind, _)) = &self.camera {
            pass.set_bind_group(1, bind, &[]);
        }
        for tile in &self.visible {
            pass.set_bind_group(0, &tile.bind, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}
fn update_uniform(queue: &wgpu::Queue, buffer: &wgpu::Buffer, coord: TileCoord, lod: u32) {
    let [x, y] = coord.origin();
    let n = (256 >> lod) as f32;
    let size = n + 2.0;
    let data = [
        x as f32,
        y as f32,
        256.0,
        0.0,
        1.0 / size,
        1.0 / size,
        n / size,
        n / size,
    ];
    queue.write_buffer(buffer, 0, bytemuck::cast_slice(&data));
}
fn visible_coords(
    doc: &Document,
    view: ViewTransform,
    vp: Viewport,
) -> Result<BTreeSet<TileCoord>, RenderError> {
    let (left, top) = view.screen_to_image(vp, 0.0, 0.0);
    let (right, bottom) = view.screen_to_image(vp, vp.width as f32, vp.height as f32);
    let mut out = BTreeSet::new();
    for layer in doc.layers() {
        if !layer.info().visible || layer.info().opacity == 0.0 {
            continue;
        }
        for coord in layer.pixels().tile_coords() {
            let [x, y] = coord.origin();
            let x = x
                .checked_add(layer.info().x)
                .ok_or(RenderError::InvalidView)?;
            let y = y
                .checked_add(layer.info().y)
                .ok_or(RenderError::InvalidView)?;
            if x as f64 > right as f64
                || y as f64 > bottom as f64
                || x as f64 + 256.0 < (left as f64)
                || y as f64 + 256.0 < (top as f64)
            {
                continue;
            }
            let (a, _) = TileCoord::locate(x, y);
            let (b, _) = TileCoord::locate(x + 255, y + 255);
            for tx in a.x()..=b.x() {
                for ty in a.y()..=b.y() {
                    out.insert(TileCoord::new(tx, ty).map_err(err)?);
                }
            }
            if out.len() > MAX_GPU_TILES {
                return Err(err("visible tile limit exceeded"));
            }
        }
    }
    Ok(out)
}
fn mip_with_gutter(neighbors: &[LinearTile], lod: u32) -> (u32, Vec<f16>) {
    let step = 1i32 << lod;
    let n = 256 / step;
    let size = n + 2;
    // step is a power of two: multiplying by its squared reciprocal is exact
    // and avoids four dynamic divisions per output texel.
    let scale = 1.0 / f64::from(step * step);
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in -1..=n {
        for x in -1..=n {
            let mut sum = [0.0f64; 4];
            for dy in 0..step {
                for dx in 0..step {
                    let px = x * step + dx;
                    let py = y * step + dy;
                    let tile = ((py.div_euclid(256) + 1) * 3 + px.div_euclid(256) + 1) as usize;
                    let p =
                        neighbors[tile][(py.rem_euclid(256) * 256 + px.rem_euclid(256)) as usize];
                    for i in 0..3 {
                        sum[i] += f64::from(p[i]) * f64::from(p[3]);
                    }
                    sum[3] += f64::from(p[3]);
                }
            }
            for value in sum {
                data.push(f16::from_f64(value * scale));
            }
        }
    }
    (size as u32, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mip_filters_premultiplied_and_gutters_match_neighbors() {
        let transparent = Arc::new(
            strata_core::composite::LinearPixels::new(
                vec![[1.0, 0.0, 0.0, 0.0]; 256 * 256],
                &strata_pixels::memory::MemoryPool::global(),
            )
            .unwrap(),
        );
        let white = Arc::new(
            strata_core::composite::LinearPixels::new(
                vec![[1.0; 4]; 256 * 256],
                &strata_pixels::memory::MemoryPool::global(),
            )
            .unwrap(),
        );
        let mut tiles = vec![transparent.clone(); 9];
        tiles[4] = white.clone();
        tiles[5] = white;
        for lod in [0, 1, 4, 8] {
            let (size, data) = mip_with_gutter(&tiles, lod);
            let center = (size as usize + 1) * 4;
            assert_eq!(data[center].to_f32(), 1.0);
            assert_eq!(data[size as usize * 4].to_f32(), 0.0);
            let right = (size as usize * 2 - 1) * 4;
            assert_eq!(data[right].to_f32(), 1.0);
        }
    }
}
