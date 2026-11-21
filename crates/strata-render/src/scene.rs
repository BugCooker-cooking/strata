use crate::{RenderError, ViewTransform, Viewport};
use half::f16;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
use strata_core::{
    BlendMode, Document, Layer, LayerKind,
    composite::{Compositor, LinearTile, effective_mask_tile, layer_mask_signature},
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
    mask_cache: HashMap<(TileCoord, u32), Arc<CacheSlot<GpuMask>>>,
    white_mask: Option<Arc<GpuMask>>,
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
struct GpuMask {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    signatures: Vec<u64>,
    _reservation: Reservation,
}
struct DrawTile {
    _tile: Arc<GpuTile>,
    _mask: Arc<GpuMask>,
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
            mask_cache: old.map(|s| s.mask_cache.clone()).unwrap_or_default(),
            white_mask: old.and_then(|s| s.white_mask.clone()),
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
        self.prepare_visible_cancellable(device, queue, pipeline, sampler, view, vp, &|| false)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_visible_cancellable(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &wgpu::RenderPipeline,
        sampler: &wgpu::Sampler,
        view: ViewTransform,
        vp: Viewport,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(), RenderError> {
        if cancel() {
            return Err(RenderError::Cancelled);
        }
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
        // A trailing normal exposure is evaluated on the display GPU. Its base
        // and mask textures remain reusable while the slider changes. Other
        // layer stacks retain the full compositor path.
        let tail = self
            .document
            .layers()
            .last()
            .filter(|layer| {
                layer.info().visible
                    && layer.info().parent.is_none()
                    && !layer.info().clipped
                    && layer.info().opacity > 0.0
                    && layer.info().blend_mode == BlendMode::Normal
                    && matches!(layer.info().kind, LayerKind::Exposure { .. })
            })
            .cloned();
        let prefix = self.document.layers().len() - usize::from(tail.is_some());
        let delta = tail
            .as_ref()
            .and_then(|layer| {
                if let LayerKind::Exposure { stops } = layer.info().kind {
                    Some(
                        ((2f64.powf(stops) - 1.0)
                            * layer.info().opacity
                            * layer.info().fill_opacity) as f32,
                    )
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
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
            if cancel() {
                return Err(RenderError::Cancelled);
            }
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
                        .or_insert_with(|| Compositor::signature_prefix(&self.document, c, prefix));
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
                    .map(|c| {
                        if cancel() {
                            return Err(RenderError::Cancelled);
                        }
                        self.compositor
                            .tile_prefix_cancellable(&self.document, c, prefix, cancel)
                            .map_err(|error| {
                                if matches!(error, strata_core::DocumentError::Cancelled) {
                                    RenderError::Cancelled
                                } else {
                                    err(error)
                                }
                            })
                    })
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
            let mask = self.mask_for(device, queue, coord, lod, tail.as_ref(), cancel)?;
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Tile placement"),
                size: 48,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            update_uniform(queue, &uniform, coord, lod, delta);
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
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&mask.view),
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
                _mask: mask,
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
    fn mask_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        coord: TileCoord,
        lod: u32,
        tail: Option<&Layer>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Arc<GpuMask>, RenderError> {
        let active = tail.filter(|layer| {
            layer.info().mask.as_ref().is_some_and(|m| m.enabled)
                || layer.info().vector_mask.as_ref().is_some_and(|m| m.enabled)
        });
        let Some(layer) = active else {
            if let Some(mask) = &self.white_mask {
                return Ok(mask.clone());
            }
            let mask = make_gpu_mask(
                device,
                queue,
                1,
                &[f16::ONE],
                vec![],
                &self.document.memory_pool(),
            )?;
            self.white_mask = Some(mask.clone());
            return Ok(mask);
        };
        let mut signatures = Vec::new();
        let mut coords = Vec::new();
        for y in -1..=1 {
            for x in -1..=1 {
                let c = TileCoord::new(coord.x() + x, coord.y() + y).map_err(err)?;
                signatures.push(layer_mask_signature(&self.document, layer, c));
                coords.push(c);
            }
        }
        if let Some(mask) = self
            .mask_cache
            .get(&(coord, lod))
            .and_then(|m| m.get())
            .filter(|m| m.signatures == signatures)
        {
            return Ok(mask);
        }
        let pool = self.document.memory_pool();
        let _scratch = pool
            .reserve(MemoryKind::Transient, 9 * 256 * 256 * 2 + (258 * 258 * 2))
            .map_err(err)?;
        let neighbors = coords
            .into_iter()
            .map(|coord| {
                let [x, y] = coord.origin();
                effective_mask_tile(&self.document, layer, x, y, cancel)
                    .map_err(err)
                    .map(|tile| tile.unwrap_or_else(|| vec![65535; 256 * 256]))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (size, data) = mask_mip_with_gutter(&neighbors, lod);
        let mask = make_gpu_mask(device, queue, size, &data, signatures, &pool)?;
        if self.mask_cache.len() >= MAX_GPU_TILES {
            self.mask_cache.clear();
        }
        self.mask_cache.insert(
            (coord, lod),
            CacheSlot::with_kind(&pool, mask.clone(), MemoryKind::Gpu),
        );
        Ok(mask)
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
fn update_uniform(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    coord: TileCoord,
    lod: u32,
    delta: f32,
) {
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
        delta,
        0.0,
        0.0,
        0.0,
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
    let mut add = |x: i128, y: i128, r: i128, b: i128| -> Result<(), RenderError> {
        let x0 = (x as f64).max(f64::from(left).floor()) as i64;
        let y0 = (y as f64).max(f64::from(top).floor()) as i64;
        let x1 = (r as f64).min(f64::from(right).ceil()) as i64;
        let y1 = (b as f64).min(f64::from(bottom).ceil()) as i64;
        if x1 <= x0 || y1 <= y0 {
            return Ok(());
        }
        let (a, _) = TileCoord::locate(x0, y0);
        let (b, _) = TileCoord::locate(x1 - 1, y1 - 1);
        for tx in a.x()..=b.x() {
            for ty in a.y()..=b.y() {
                out.insert(TileCoord::new(tx, ty).map_err(err)?);
                if out.len() > MAX_GPU_TILES {
                    return Err(err("visible tile limit exceeded"));
                }
            }
        }
        Ok(())
    };
    for layer in doc.layers() {
        if !doc.effective_visibility(layer.id()) || layer.info().opacity == 0.0 {
            continue;
        }
        let pad = doc.style_outset(layer.id());
        let x = i128::from(layer.info().x);
        let y = i128::from(layer.info().y);
        if matches!(
            layer.info().kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) && let Some(b) = doc.generated_canvas_bounds(layer).map_err(err)?
        {
            add(
                i128::from(b.x) - i128::from(pad[0]),
                i128::from(b.y) - i128::from(pad[1]),
                i128::from(b.x) + i128::from(b.width) + i128::from(pad[2]),
                i128::from(b.y) + i128::from(b.height) + i128::from(pad[3]),
            )?;
        }
        for coord in layer.pixels().tile_coords() {
            let [ox, oy] = coord.origin();
            let ox = x + i128::from(ox);
            let oy = y + i128::from(oy);
            add(
                ox - i128::from(pad[0]),
                oy - i128::from(pad[1]),
                ox + 256 + i128::from(pad[2]),
                oy + 256 + i128::from(pad[3]),
            )?;
        }
    }
    Ok(out)
}
fn make_gpu_mask(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: u32,
    data: &[f16],
    signatures: Vec<u64>,
    pool: &strata_pixels::memory::MemoryPool,
) -> Result<Arc<GpuMask>, RenderError> {
    let reservation = pool
        .reserve(
            MemoryKind::Gpu,
            (u64::from(size) * u64::from(size) * 2).max(256),
        )
        .map_err(err)?;
    let allocation = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Display exposure coverage"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Float,
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
        bytemuck::cast_slice(data),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size * 2),
            rows_per_image: Some(size),
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
    if let Some(error) =
        pollster::block_on(validation.pop()).or(pollster::block_on(allocation.pop()))
    {
        return Err(err(error));
    }
    let view = texture.create_view(&Default::default());
    Ok(Arc::new(GpuMask {
        _texture: texture,
        view,
        signatures,
        _reservation: reservation,
    }))
}
fn mask_mip_with_gutter(neighbors: &[Vec<u16>], lod: u32) -> (u32, Vec<f16>) {
    let step = 1i32 << lod;
    let n = 256 / step;
    let size = n + 2;
    let scale = 1.0 / (f64::from(step * step) * 65535.0);
    let mut data = Vec::with_capacity((size * size) as usize);
    for y in -1..=n {
        for x in -1..=n {
            let mut sum = 0u64;
            for dy in 0..step {
                for dx in 0..step {
                    let px = x * step + dx;
                    let py = y * step + dy;
                    let tile = ((py.div_euclid(256) + 1) * 3 + px.div_euclid(256) + 1) as usize;
                    sum += u64::from(
                        neighbors[tile][(py.rem_euclid(256) * 256 + px.rem_euclid(256)) as usize],
                    );
                }
            }
            data.push(f16::from_f64(sum as f64 * scale));
        }
    }
    (size as u32, data)
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
