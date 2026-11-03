use crate::{RenderError, ViewTransform, Viewport, check_texture_fits};
use half::f16;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Instant,
};
use strata_pixels::PixelBuffer;
use strata_pixels::memory::{MemoryKind, MemoryPool, Reservation};

pub const CANVAS_BG_SRGB: [u8; 3] = [0x14, 0x14, 0x15];
static NEXT_RENDERER: AtomicU64 = AtomicU64::new(1);
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStats {
    pub cpu_ms: f32,
    pub skipped: bool,
}

/// Opaque candidate, tied to the renderer that created its bind group.
pub struct PreparedImage {
    owner: u64,
    width: u32,
    height: u32,
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    _allocation: Reservation,
}

pub struct Renderer {
    id: u64,
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: Target,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    image: Option<PreparedImage>,
    viewport: Viewport,
    view: ViewTransform,
    scene: Option<super::scene::Scene>,
    scene_pipeline: wgpu::RenderPipeline,
    background: [u8; 3],
    target_allocation: Reservation,
}

enum Target {
    Offscreen(wgpu::Texture),
    Surface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
}

fn gpu_error(error: impl std::fmt::Display) -> RenderError {
    RenderError::Gpu(error.to_string())
}

fn checked_gpu<T>(device: &wgpu::Device, operation: impl FnOnce() -> T) -> Result<T, RenderError> {
    let allocation = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = operation();
    let invalid = pollster::block_on(validation.pop());
    let oom = pollster::block_on(allocation.pop());
    if let Some(e) = invalid.or(oom) {
        Err(gpu_error(e))
    } else {
        Ok(value)
    }
}

fn extent(w: u32, h: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    }
}

fn output_texture(device: &wgpu::Device, vp: Viewport) -> Result<wgpu::Texture, RenderError> {
    checked_gpu(device, || {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Strata offscreen display"),
            size: extent(vp.width.max(1), vp.height.max(1)),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    })
}

fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

impl Renderer {
    pub fn new_offscreen(vp: Viewport) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        Self::new(instance, None, vp)
    }

    /// # Safety
    /// `layer` must be a live CAMetalLayer, strongly retained by the caller until
    /// this Renderer is dropped. All surface operations must be serialized.
    #[cfg(target_os = "macos")]
    pub unsafe fn new_for_metal_layer(
        layer: *mut std::ffi::c_void,
        vp: Viewport,
    ) -> Result<Self, RenderError> {
        if layer.is_null() {
            return Err(RenderError::Surface("null layer".into()));
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        // SAFETY: the native host owns the layer through synchronous detach/drop.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(|e| RenderError::Surface(e.to_string()))?;
        Self::new(instance, Some(surface), vp)
    }

    fn new(
        instance: wgpu::Instance,
        surface: Option<wgpu::Surface<'static>>,
        vp: Viewport,
    ) -> Result<Self, RenderError> {
        if !vp.scale.is_finite() || vp.scale <= 0.0 {
            return Err(RenderError::InvalidView);
        }
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surface.as_ref(),
            ..Default::default()
        }))
        .map_err(|_| RenderError::NoAdapter)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Strata display device"),
            ..Default::default()
        }))
        .map_err(|e| RenderError::RequestDevice(e.to_string()))?;
        check_texture_fits(
            vp.width.max(1),
            vp.height.max(1),
            device.limits().max_texture_dimension_2d,
        )?;
        // Surface drawables are estimated as three RGBA8 buffers. This tracks
        // application resources; driver/device overhead is measured as RSS.
        let target_allocation = MemoryPool::global()
            .reserve(
                MemoryKind::Gpu,
                u64::from(vp.width.max(1))
                    * u64::from(vp.height.max(1))
                    * if surface.is_some() { 12 } else { 4 }
                    + 256,
            )
            .map_err(gpu_error)?;
        let (target, format) = if let Some(surface) = surface {
            let capabilities = surface.get_capabilities(&adapter);
            let format = [wgpu::TextureFormat::Bgra8UnormSrgb, OUTPUT_FORMAT]
                .into_iter()
                .find(|format| capabilities.formats.contains(format))
                .ok_or_else(|| RenderError::Surface("sRGB transfer output unavailable".into()))?;
            let mut config = surface
                .get_default_config(&adapter, vp.width.max(1), vp.height.max(1))
                .ok_or_else(|| RenderError::Surface("unsupported surface".into()))?;
            config.format = format;
            config.color_space = wgpu::SurfaceColorSpace::DisplayP3;
            config.present_mode = wgpu::PresentMode::Fifo;
            if vp.width > 0 && vp.height > 0 {
                checked_gpu(&device, || surface.configure(&device, &config))?;
            }
            (Target::Surface { surface, config }, format)
        } else {
            (
                Target::Offscreen(output_texture(&device, vp)?),
                OUTPUT_FORMAT,
            )
        };
        let pipeline = checked_gpu(&device, || {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Strata image shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Strata display pipeline"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
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
        })?;
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("View transform"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_pipeline = super::scene::pipeline(&device, format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Linear image sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            id: NEXT_RENDERER.fetch_add(1, Ordering::Relaxed),
            device,
            queue,
            target,
            pipeline,
            uniform,
            sampler,
            image: None,
            viewport: vp,
            view: ViewTransform::default(),
            scene: None,
            scene_pipeline,
            background: CANVAS_BG_SRGB,
            target_allocation,
        })
    }

    pub fn max_texture_dimension(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }
    pub fn view(&self) -> ViewTransform {
        self.view
    }
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    /// Host presentation preference only. Never changes source pixels or export.
    pub fn set_background(&mut self, color: [u8; 3]) {
        self.background = color;
    }
    pub fn set_view(&mut self, view: ViewTransform) -> Result<(), RenderError> {
        if !view.is_valid() {
            return Err(RenderError::InvalidView);
        }
        self.view = view;
        Ok(())
    }

    pub fn fit_to_screen(&mut self) {
        if let Some(scene) = &self.scene {
            let frame = scene.document.frame();
            self.view = ViewTransform::fit(frame.width, frame.height, self.viewport);
            self.view.center_x += frame.x as f32;
            self.view.center_y += frame.y as f32;
            return;
        }
        if let Some(image) = &self.image {
            self.view = ViewTransform::fit(image.width, image.height, self.viewport);
        }
    }

    pub fn prepare_image(
        &self,
        width: u32,
        height: u32,
        pixels: &PixelBuffer,
    ) -> Result<PreparedImage, RenderError> {
        check_texture_fits(width, height, self.max_texture_dimension())?;
        pixels.validate(width, height)?;
        let bytes = u64::from(width) * u64::from(height) * 8;
        let _scratch = MemoryPool::global()
            .reserve(MemoryKind::Transient, bytes)
            .map_err(gpu_error)?;
        let allocation = MemoryPool::global()
            .reserve(MemoryKind::Gpu, bytes)
            .map_err(gpu_error)?;
        let mut cache = Vec::<f16>::new();
        cache
            .try_reserve_exact(pixels.sample_len())
            .map_err(gpu_error)?;
        let mut append = |rgba: [f32; 4]| {
            cache.extend([
                f16::from_f32(linear(rgba[0]) * rgba[3]),
                f16::from_f32(linear(rgba[1]) * rgba[3]),
                f16::from_f32(linear(rgba[2]) * rgba[3]),
                f16::from_f32(rgba[3]),
            ]);
        };
        match pixels {
            PixelBuffer::Rgba8(src) => {
                for p in src.chunks_exact(4) {
                    append([
                        p[0] as f32 / 255.0,
                        p[1] as f32 / 255.0,
                        p[2] as f32 / 255.0,
                        p[3] as f32 / 255.0,
                    ]);
                }
            }
            PixelBuffer::Rgba16(src) => {
                for p in src.chunks_exact(4) {
                    append([
                        p[0] as f32 / 65535.0,
                        p[1] as f32 / 65535.0,
                        p[2] as f32 / 65535.0,
                        p[3] as f32 / 65535.0,
                    ]);
                }
            }
        }
        checked_gpu(&self.device, || {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Linear premultiplied P3 display cache"),
                size: extent(width, height),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&cache),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 8),
                    rows_per_image: Some(height),
                },
                extent(width, height),
            );
            let texture_view = texture.create_view(&Default::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Prepared image"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            PreparedImage {
                owner: self.id,
                width,
                height,
                _texture: texture,
                bind_group,
                _allocation: allocation,
            }
        })
    }

    /// No allocations or GPU work after validating renderer ownership.
    pub fn commit_image(&mut self, prepared: PreparedImage) -> Result<(), RenderError> {
        if prepared.owner != self.id {
            return Err(gpu_error("prepared image belongs to another renderer"));
        }
        self.view = ViewTransform::fit(prepared.width, prepared.height, self.viewport);
        self.image = Some(prepared);
        self.scene = None;
        Ok(())
    }

    pub fn resize(&mut self, vp: Viewport) -> Result<(), RenderError> {
        if !vp.scale.is_finite() || vp.scale <= 0.0 {
            return Err(RenderError::InvalidView);
        }
        check_texture_fits(
            vp.width.max(1),
            vp.height.max(1),
            self.max_texture_dimension(),
        )?;
        if vp.width > 0 && vp.height > 0 {
            let allocation = MemoryPool::global()
                .reserve(
                    MemoryKind::Gpu,
                    u64::from(vp.width)
                        * u64::from(vp.height)
                        * if matches!(self.target, Target::Surface { .. }) {
                            12
                        } else {
                            4
                        }
                        + 256,
                )
                .map_err(gpu_error)?;
            match &mut self.target {
                Target::Offscreen(texture) => *texture = output_texture(&self.device, vp)?,
                Target::Surface { surface, config } => {
                    let mut next = config.clone();
                    next.width = vp.width;
                    next.height = vp.height;
                    checked_gpu(&self.device, || surface.configure(&self.device, &next))?;
                    *config = next;
                }
            }
            self.target_allocation = allocation;
        }
        self.view = self.view.rescaled(self.viewport.scale, vp.scale);
        self.viewport = vp;
        Ok(())
    }

    pub fn render(&mut self) -> Result<FrameStats, RenderError> {
        let start = Instant::now();
        if self.viewport.width == 0 || self.viewport.height == 0 {
            return Ok(FrameStats {
                cpu_ms: 0.0,
                skipped: true,
            });
        }
        if let Some(scene) = &mut self.scene {
            scene.prepare_visible(
                &self.device,
                &self.queue,
                &self.scene_pipeline,
                &self.sampler,
                self.view,
                self.viewport,
            )?;
        }
        let frame = if let Target::Surface { surface, config } = &self.target {
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return Ok(FrameStats {
                        cpu_ms: 0.0,
                        skipped: true,
                    });
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    checked_gpu(&self.device, || surface.configure(&self.device, config))?;
                    return Ok(FrameStats {
                        cpu_ms: 0.0,
                        skipped: true,
                    });
                }
                other => {
                    return Err(RenderError::Surface(format!(
                        "cannot acquire drawable: {other:?}"
                    )));
                }
            }
        } else {
            None
        };
        let target_view = match (&self.target, &frame) {
            (Target::Offscreen(texture), _) => texture.create_view(&Default::default()),
            (_, Some(frame)) => frame.texture.create_view(&Default::default()),
            _ => return Err(RenderError::Surface("missing drawable".into())),
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        if let Some(image) = &self.image {
            let matrix = self
                .view
                .image_to_ndc(self.viewport, image.width, image.height);
            self.queue
                .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&matrix));
        }
        let bg = self.background.map(|v| linear(v as f32 / 255.0) as f64);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Strata canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0],
                            g: bg[1],
                            b: bg[2],
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let Some(image) = &self.image {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &image.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
            if let Some(scene) = &self.scene {
                scene.draw(&mut pass, &self.scene_pipeline);
            }
        }
        self.queue.submit([encoder.finish()]);
        if let Some(frame) = frame {
            self.queue.present(frame);
        }
        Ok(FrameStats {
            cpu_ms: start.elapsed().as_secs_f32() * 1000.0,
            skipped: false,
        })
    }

    pub fn prepare_document(
        &mut self,
        document: &strata_core::Document,
        fit: bool,
    ) -> Result<super::scene::PreparedScene, RenderError> {
        let view = if fit {
            let f = document.frame();
            let mut v = ViewTransform::fit(f.width, f.height, self.viewport);
            v.center_x += f.x as f32;
            v.center_y += f.y as f32;
            v
        } else {
            self.view
        };
        let mut scene = super::scene::Scene::new(document.clone(), self.scene.as_ref());
        scene.prepare_visible(
            &self.device,
            &self.queue,
            &self.scene_pipeline,
            &self.sampler,
            view,
            self.viewport,
        )?;
        Ok(super::scene::PreparedScene {
            owner: self.id,
            scene,
            view,
        })
    }
    pub fn commit_document(
        &mut self,
        prepared: super::scene::PreparedScene,
    ) -> Result<(), RenderError> {
        if prepared.owner != self.id {
            return Err(gpu_error("scene belongs to another renderer"));
        }
        self.scene = Some(prepared.scene);
        self.view = prepared.view;
        self.image = None;
        Ok(())
    }

    pub fn read_offscreen_rgba(&self) -> Result<Vec<u8>, RenderError> {
        let Target::Offscreen(texture) = &self.target else {
            return Err(RenderError::NotOffscreen);
        };
        let (width, height) = (self.viewport.width, self.viewport.height);
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidView);
        }
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let _gpu = MemoryPool::global()
            .reserve(MemoryKind::Gpu, u64::from(row) * u64::from(height))
            .map_err(gpu_error)?;
        let _cpu = MemoryPool::global()
            .reserve(
                MemoryKind::Transient,
                u64::from(row) * u64::from(height) * 2,
            )
            .map_err(gpu_error)?;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Display readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            extent(width, height),
        );
        self.queue.submit([encoder.finish()]);
        let (send, receive) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(gpu_error)?;
        receive.recv().map_err(gpu_error)?.map_err(gpu_error)?;
        let bytes = buffer.slice(..).get_mapped_range().map_err(gpu_error)?;
        let mut output = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            output.extend_from_slice(&bytes[(y * row) as usize..(y * row + width * 4) as usize]);
        }
        drop(bytes);
        buffer.unmap();
        Ok(output)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Synchronous teardown before the native host releases its CAMetalLayer.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}
