use super::*;
use std::sync::Arc;
use strata_pixels::memory::{MemoryKind, Reservation};

#[derive(Clone)]
pub struct ClipboardImage(Arc<ClipboardData>);
struct ClipboardData {
    pixels: PixelBuffer,
    width: u32,
    height: u32,
    x: i64,
    y: i64,
    icc: Vec<u8>,
    _allocation: Reservation,
}
impl ClipboardImage {
    pub fn new(
        pixels: PixelBuffer,
        width: u32,
        height: u32,
        x: i64,
        y: i64,
        icc: Vec<u8>,
        pool: &strata_pixels::memory::MemoryPool,
    ) -> Result<Self, CommandError> {
        pixels
            .validate(width, height)
            .map_err(DocumentError::from)?;
        if pixels.byte_len() as u64 > crate::MAX_FLAT_IMAGE_BYTES || icc.len() > 4 * 1024 * 1024 {
            return Err(DocumentError::ResourceLimit.into());
        }
        if strata_color::resolve_profile(Some(&icc))
            .map_err(DocumentError::from)?
            .fallback
        {
            return Err(CommandError::Invalid(
                "clipboard ICC must be valid RGB".into(),
            ));
        }
        let allocation = pool
            .reserve(MemoryKind::Transient, pixels.allocation_bytes() as u64)
            .map_err(DocumentError::from)?;
        Ok(Self(Arc::new(ClipboardData {
            pixels,
            width,
            height,
            x,
            y,
            icc,
            _allocation: allocation,
        })))
    }
    pub fn pixels(&self) -> &PixelBuffer {
        &self.0.pixels
    }
    pub fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }
    pub fn origin(&self) -> (i64, i64) {
        (self.0.x, self.0.y)
    }
    pub fn icc(&self) -> &[u8] {
        &self.0.icc
    }
}
#[derive(Clone)]
pub(super) struct ClipResource {
    image: ClipboardImage,
    source: Source,
    document: Option<DocumentId>,
    state: Option<StateId>,
    layer: Option<LayerId>,
    confirmed: bool,
    issued: Instant,
}
pub(super) struct ClipCandidate {
    pub(super) token: String,
    pub(super) resource: ClipResource,
}

impl Editor {
    pub fn grant_clipboard_image(
        &mut self,
        image: ClipboardImage,
        source: Source,
    ) -> Result<String, CommandError> {
        self.clipboards
            .retain(|_, r| r.issued.elapsed() < Duration::from_secs(3600));
        if self.clipboards.len() >= 16
            && let Some(oldest) = self
                .clipboards
                .iter()
                .min_by_key(|(_, resource)| resource.issued)
                .map(|(id, _)| id.clone())
        {
            self.clipboards.remove(&oldest);
        }
        let token = Uuid::new_v4().to_string();
        self.clipboards.insert(
            token.clone(),
            ClipResource {
                image,
                source,
                document: None,
                state: None,
                layer: None,
                confirmed: true,
                issued: Instant::now(),
            },
        );
        Ok(token)
    }
    pub fn clipboard_image(
        &self,
        token: &str,
        source: &Source,
    ) -> Result<ClipboardImage, CommandError> {
        Ok(self.clip_resource(token, source)?.image.clone())
    }
    pub fn confirm_clipboard_write(
        &mut self,
        token: &str,
        source: &Source,
    ) -> Result<(), CommandError> {
        self.clip_resource(token, source)?;
        self.clipboards.get_mut(token).unwrap().confirmed = true;
        self.executions
            .push(json!({"command":"edit.clipboard.write","status":"completed","undoable":false}));
        if self.executions.len() > 256 {
            self.executions.remove(0);
        }
        Ok(())
    }
    fn clip_resource(&self, token: &str, source: &Source) -> Result<&ClipResource, CommandError> {
        self.clipboards
            .get(token)
            .filter(|r| r.source == *source && r.issued.elapsed() < Duration::from_secs(3600))
            .ok_or(CommandError::Permission)
    }
    pub(super) fn prepare_copy(
        &self,
        request: &Request,
        id: &str,
        params: Value,
    ) -> Result<ClipCandidate, CommandError> {
        let p: PixelTarget = parse(params)?;
        let doc = self.document.as_ref().ok_or(CommandError::NoDocument)?;
        let layer = if id == "edit.copy_merged" {
            None
        } else {
            Some(
                p.layer
                    .as_ref()
                    .map(|r| doc.resolve(r))
                    .transpose()?
                    .or(self.selected)
                    .ok_or(DocumentError::LayerNotFound)?,
            )
        };
        if matches!(request.source, Source::Ai { .. })
            && layer.is_some_and(|id| !doc.effective_visibility(id))
        {
            return Err(CommandError::Permission);
        }
        let image = copy(doc, layer)?;
        Ok(ClipCandidate {
            token: Uuid::new_v4().to_string(),
            resource: ClipResource {
                image,
                source: request.source.clone(),
                document: Some(doc.id()),
                state: Some(doc.state_id()),
                layer,
                confirmed: false,
                issued: Instant::now(),
            },
        })
    }
    pub(super) fn commit_copy(
        &mut self,
        candidate: ClipCandidate,
        command: &str,
    ) -> Result<(), CommandError> {
        self.clipboards
            .retain(|_, r| r.issued.elapsed() < Duration::from_secs(3600));
        if self.clipboards.len() >= 16
            && let Some(oldest) = self
                .clipboards
                .iter()
                .min_by_key(|(_, resource)| resource.issued)
                .map(|(id, _)| id.clone())
        {
            self.clipboards.remove(&oldest);
        }
        self.clipboards.insert(candidate.token, candidate.resource);
        self.executions
            .push(json!({"command":command,"status":"prepared","undoable":false}));
        if self.executions.len() > 256 {
            self.executions.remove(0);
        }
        Ok(())
    }
    pub(super) fn clipboard_edit(
        &self,
        request: &Request,
        doc: &mut Document,
        id: &str,
        params: Value,
    ) -> Result<(String, Value), CommandError> {
        let p: ClipboardResource = parse(params)?;
        let resource = self.clip_resource(&p.resource, &request.source)?;
        if id == "edit.cut" {
            if !resource.confirmed
                || resource.document != Some(doc.id())
                || resource.state != Some(doc.state_id())
            {
                return Err(CommandError::Permission);
            }
            let target = resource.layer.ok_or_else(|| {
                CommandError::Invalid(
                    "cut requires an active-layer copy, not merged clipboard content".into(),
                )
            })?;
            clear(doc, target)?;
            return Ok((
                p.resource,
                json!({"layer_id":target,"clipboard_write":"already_completed_not_undoable"}),
            ));
        }
        paste(doc, &resource.image, id == "edit.paste_in_place").map(|result| (p.resource, result))
    }
    pub(super) fn check_clipboard(
        &self,
        request: &Request,
        token: &str,
    ) -> Result<(), CommandError> {
        self.clip_resource(token, &request.source).map(|_| ())
    }
    pub(super) fn clipboard_write_confirmed(
        &self,
        request: &Request,
        token: &str,
    ) -> Result<bool, CommandError> {
        Ok(self.clip_resource(token, &request.source)?.confirmed)
    }
}
fn copy(doc: &Document, target: Option<LayerId>) -> Result<ClipboardImage, CommandError> {
    let mut snapshot = doc.clone();
    if let Some(target) = target {
        snapshot
            .layers
            .retain(|l| l.id() == target || doc.is_descendant(l.id(), target));
        let layer = snapshot
            .layers
            .iter_mut()
            .find(|l| l.id() == target)
            .ok_or(DocumentError::LayerNotFound)?;
        if matches!(
            layer.info.kind,
            crate::LayerKind::Exposure { .. } | crate::LayerKind::HueSaturation { .. }
        ) {
            return Err(CommandError::Invalid(
                "copy an adjustment with copy_merged to include its backdrop".into(),
            ));
        }
        layer.info.parent = None;
        layer.info.clipped = false;
        layer.info.visible = true;
        snapshot.active = Some(target);
    }
    let bounds = if let Some(selection) = &doc.selection {
        selection
            .content_bounds()
            .map_err(DocumentError::from)?
            .ok_or_else(|| CommandError::Invalid("selection is empty".into()))?
    } else {
        strata_pixels::CoverageRect {
            x: doc.frame.x,
            y: doc.frame.y,
            width: doc.frame.width,
            height: doc.frame.height,
        }
    };
    snapshot.frame.x = bounds.x;
    snapshot.frame.y = bounds.y;
    snapshot.frame.width = bounds.width;
    snapshot.frame.height = bounds.height;
    snapshot.selection = None;
    let bytes = doc
        .format
        .byte_count(bounds.width, bounds.height)
        .map_err(DocumentError::from)? as u64;
    let allocation = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, bytes)
        .map_err(DocumentError::from)?;
    let mut pixels = snapshot.export_pixels()?;
    if let Some(selection) = &doc.selection {
        let _mask_scratch = doc
            .memory_pool()
            .reserve(MemoryKind::Transient, 256 * 256 * 2)
            .map_err(DocumentError::from)?;
        for ty in 0..bounds.height.div_ceil(256) {
            for tx in 0..bounds.width.div_ceil(256) {
                let x = tx * 256;
                let y = ty * 256;
                let mask = selection
                    .region_tile(bounds.x + i64::from(x), bounds.y + i64::from(y))
                    .map_err(DocumentError::from)?;
                for dy in 0..(bounds.height - y).min(256) {
                    for dx in 0..(bounds.width - x).min(256) {
                        let coverage = mask[(dy * 256 + dx) as usize];
                        let alpha = (((y + dy) * bounds.width + x + dx) * 4 + 3) as usize;
                        match &mut pixels {
                            PixelBuffer::Rgba8(v) => {
                                v[alpha] = ((u32::from(v[alpha]) * u32::from(coverage) + 32767)
                                    / 65535) as u8
                            }
                            PixelBuffer::Rgba16(v) => {
                                v[alpha] = ((u64::from(v[alpha]) * u64::from(coverage) + 32767)
                                    / 65535) as u16
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(ClipboardImage(Arc::new(ClipboardData {
        pixels,
        width: bounds.width,
        height: bounds.height,
        x: bounds.x,
        y: bounds.y,
        icc: doc.working_icc.clone(),
        _allocation: allocation,
    })))
}

fn paste(
    doc: &mut Document,
    image: &ClipboardImage,
    in_place: bool,
) -> Result<Value, CommandError> {
    if doc.insertion_parent().is_some_and(|parent| {
        doc.layers
            .iter()
            .any(|l| l.info.locks.all && (l.id() == parent || doc.is_descendant(parent, l.id())))
    }) {
        return Err(CommandError::Invalid(
            "paste target parent is locked".into(),
        ));
    }
    if doc.layers.len() >= crate::MAX_LAYERS {
        return Err(DocumentError::ResourceLimit.into());
    }
    let (w, h) = image.size();
    let bytes = doc.format.byte_count(w, h).map_err(DocumentError::from)? as u64;
    let _scratch = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, bytes * 3)
        .map_err(DocumentError::from)?;
    let promoted = match (image.pixels(), doc.format) {
        (PixelBuffer::Rgba8(v), PixelFormat::Rgba16) => {
            PixelBuffer::Rgba16(v.iter().map(|v| u16::from(*v) * 257).collect())
        }
        (pixels, _) => pixels.clone(),
    };
    let converted =
        strata_color::convert_to_profile(&promoted, w, h, image.icc(), &doc.working_icc)
            .map_err(DocumentError::from)?;
    let pixels = match (converted, doc.format) {
        (PixelBuffer::Rgba16(v), PixelFormat::Rgba8) => PixelBuffer::Rgba8(
            v.into_iter()
                .map(|v| ((u32::from(v) + 128) / 257) as u8)
                .collect(),
        ),
        (pixels, _) => pixels,
    };
    let tiled =
        TiledPixels::from_dense_with_pool(w, h, &pixels, tile_limit(doc.format), doc.memory_pool())
            .map_err(DocumentError::from)?;
    let mut layer = Layer::new("Pasted".into(), tiled);
    layer.info.parent = doc.insertion_parent();
    let (x, y) = if in_place {
        image.origin()
    } else {
        (
            i64::try_from(
                i128::from(doc.frame.x)
                    + (i128::from(doc.frame.width) - i128::from(w)).div_euclid(2),
            )
            .map_err(|_| DocumentError::ResourceLimit)?,
            i64::try_from(
                i128::from(doc.frame.y)
                    + (i128::from(doc.frame.height) - i128::from(h)).div_euclid(2),
            )
            .map_err(|_| DocumentError::ResourceLimit)?,
        )
    };
    layer.info.x = x;
    layer.info.y = y;
    let id = layer.id();
    doc.layers.push(layer);
    doc.active = Some(id);
    Ok(json!({"layer_id":id}))
}
fn clear(doc: &mut Document, target: LayerId) -> Result<(), CommandError> {
    if doc.layers.iter().any(|l| {
        doc.is_descendant(target, l.id())
            && (l.info.locks.all || l.info.locks.pixels || l.info.locks.transparency)
    }) {
        return Err(CommandError::Invalid(
            "parent group locks pixels or transparency".into(),
        ));
    }
    let selection = doc.selection.clone();
    let layer = doc
        .layers
        .iter_mut()
        .find(|l| l.id() == target)
        .ok_or(DocumentError::LayerNotFound)?;
    if !matches!(layer.info.kind, crate::LayerKind::Pixel)
        || layer.info.locks.all
        || layer.info.locks.pixels
        || layer.info.locks.transparency
    {
        return Err(CommandError::Invalid(
            "clear/cut requires an unlocked pixel layer; rasterize generated content first".into(),
        ));
    }
    let coords: Vec<_> = layer.pixels.tile_coords().copied().collect();
    for coord in coords {
        let _scratch = layer
            .pixels
            .memory_pool()
            .reserve(MemoryKind::Transient, 256 * 256 * 10)
            .map_err(DocumentError::from)?;
        let [ox, oy] = coord.origin();
        let mask = selection
            .as_ref()
            .map(|mask| mask.region_tile(ox + layer.info.x, oy + layer.info.y))
            .transpose()
            .map_err(DocumentError::from)?;
        let mut tile = (*layer
            .pixels
            .tile(coord)
            .map_err(DocumentError::from)?
            .unwrap())
        .clone();
        match &mut tile {
            PixelBuffer::Rgba8(v) => {
                for (i, pixel) in v.chunks_exact_mut(4).enumerate() {
                    pixel[3] = ((u32::from(pixel[3])
                        * u32::from(65535 - mask.as_ref().map_or(65535, |m| m[i]))
                        + 32767)
                        / 65535) as u8;
                }
            }
            PixelBuffer::Rgba16(v) => {
                for (i, pixel) in v.chunks_exact_mut(4).enumerate() {
                    pixel[3] = ((u64::from(pixel[3])
                        * u64::from(65535 - mask.as_ref().map_or(65535, |m| m[i]))
                        + 32767)
                        / 65535) as u16;
                }
            }
        }
        layer
            .pixels
            .insert_tile(coord, tile)
            .map_err(DocumentError::from)?;
    }
    Ok(())
}
pub(super) fn local_edit(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    if !matches!(
        id,
        "edit.clear" | "layer.via_copy" | "layer.via_cut" | "layer.background_to_layer"
    ) {
        return Ok(None);
    }
    let p: PixelTarget = parse(params)?;
    let target = p
        .layer
        .as_ref()
        .map(|r| doc.resolve(r))
        .transpose()?
        .or(doc.active)
        .ok_or(DocumentError::LayerNotFound)?;
    if doc
        .layers
        .iter()
        .any(|l| (l.id() == target || doc.is_descendant(target, l.id())) && l.info.locks.all)
    {
        return Err(CommandError::Invalid(
            "target or parent is fully locked".into(),
        ));
    }
    if id == "layer.background_to_layer" {
        let layer = doc.layers.iter_mut().find(|l| l.id() == target).unwrap();
        if !matches!(layer.info.kind, crate::LayerKind::Pixel) {
            return Err(CommandError::Invalid(
                "background must be a pixel layer".into(),
            ));
        }
        layer.info.name = "Layer 0".into();
        return Ok(Some(json!({"layer_id":target})));
    }
    if id == "edit.clear" {
        clear(doc, target)?;
        return Ok(Some(json!({"layer_id":target})));
    }
    let image = copy(doc, Some(target))?;
    if id == "layer.via_cut" {
        clear(doc, target)?;
    }
    paste(doc, &image, true).map(Some)
}
