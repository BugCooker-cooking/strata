//! Editable PSD v1 export. The directory and channel sizes are known before
//! allocating the output. Unsupported layers fail before any file is published.
use crate::{
    ColorLabel, Document, DocumentError, Layer, LayerId, LayerKind, TonalAdjustment,
    geometry_pixels::content_bounds,
};
use strata_pixels::{
    CoverageRect, PixelBuffer, PixelFormat,
    memory::{MemoryKind, Reservation},
};

pub(crate) struct EncodedPsd {
    pub bytes: Vec<u8>,
    _allocation: Reservation,
}

#[derive(Clone, Copy)]
enum RecordKind {
    Pixel,
    Folder,
    Divider,
    Invert,
    Exposure(f32),
}
struct Record<'a> {
    layer: &'a Layer,
    kind: RecordKind,
    /// Tight nonzero sample bounds in layer-local coordinates, extended to
    /// the layer origin when leading transparent pixels precede the content.
    local: CoverageRect,
    rect: [i32; 4],
    mask_rect: Option<[i32; 4]>,
}
const EMPTY: CoverageRect = CoverageRect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
};
fn invalid(reason: impl Into<String>) -> DocumentError {
    DocumentError::Invalid(reason.into())
}
fn check(cancel: &dyn Fn() -> bool) -> Result<(), DocumentError> {
    if cancel() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}
fn length(bytes: &[u8]) -> [u8; 4] {
    (bytes.len() as u32).to_be_bytes()
}
fn block(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend(length(bytes));
    out.extend(bytes);
}
fn tag(out: &mut Vec<u8>, key: &[u8; 4], bytes: &[u8]) {
    out.extend(b"8BIM");
    out.extend(key);
    block(out, bytes);
    if !bytes.len().is_multiple_of(2) {
        out.push(0);
    }
}
fn resource(out: &mut Vec<u8>, id: u16, name: &[u8], bytes: &[u8]) {
    out.extend(b"8BIM");
    out.extend(id.to_be_bytes());
    out.push(name.len() as u8);
    out.extend(name);
    if name.len().is_multiple_of(2) {
        out.push(0);
    }
    block(out, bytes);
    if !bytes.len().is_multiple_of(2) {
        out.push(0);
    }
}
fn unsupported(layer: &Layer) -> DocumentError {
    invalid(format!(
        "PSD export cannot represent this layer: {}",
        layer.info().name
    ))
}
fn rect(r: CoverageRect, dx: i128, dy: i128) -> Result<[i32; 4], DocumentError> {
    if r.width > 30000 || r.height > 30000 {
        return Err(invalid("PSD v1 bounds exceed 30000 pixels"));
    }
    let x = i128::from(r.x) + dx;
    let y = i128::from(r.y) + dy;
    let coord = |v: i128| i32::try_from(v).map_err(|_| DocumentError::ResourceLimit);
    Ok([
        coord(y)?,
        coord(x)?,
        coord(y + i128::from(r.height))?,
        coord(x + i128::from(r.width))?,
    ])
}
fn records<'a>(
    doc: &'a Document,
    parent: Option<LayerId>,
    out: &mut Vec<Record<'a>>,
    cancel: &dyn Fn() -> bool,
) -> Result<(), DocumentError> {
    let frame = doc.frame();
    for layer in doc.layers().iter().filter(|l| l.info().parent == parent) {
        check(cancel)?;
        let i = layer.info();
        if !i.styles.is_empty() || i.name.ends_with('\0') {
            return Err(unsupported(layer));
        }
        let kind = match &i.kind {
            LayerKind::Pixel => RecordKind::Pixel,
            LayerKind::Group => RecordKind::Folder,
            LayerKind::Exposure { stops } => RecordKind::Exposure(*stops as f32),
            LayerKind::Adjustment {
                adjustment: TonalAdjustment::Invert {},
            } => RecordKind::Invert,
            _ => return Err(unsupported(layer)),
        };
        if matches!(kind, RecordKind::Folder) {
            out.push(Record {
                layer,
                kind: RecordKind::Divider,
                local: EMPTY,
                rect: [0; 4],
                mask_rect: None,
            });
            records(doc, Some(layer.id()), out, cancel)?;
        }
        let mut local = EMPTY;
        if matches!(kind, RecordKind::Pixel)
            && let Some(b) = content_bounds(layer.pixels(), [0, 0], cancel)?
        {
            let x = b.x.min(0);
            let y = b.y.min(0);
            let span = |lo: i64, base: i64, n: u32| {
                u32::try_from(i128::from(base) + i128::from(n) - i128::from(lo))
                    .map_err(|_| DocumentError::ResourceLimit)
            };
            local = CoverageRect {
                x,
                y,
                width: span(x, b.x, b.width)?,
                height: span(y, b.y, b.height)?,
            };
        }
        let dx = i128::from(i.x) - i128::from(frame.x);
        let dy = i128::from(i.y) - i128::from(frame.y);
        let layer_rect = if matches!(kind, RecordKind::Pixel) {
            rect(local, dx, dy)?
        } else {
            [0; 4]
        };
        let mask_rect = i
            .mask
            .as_ref()
            .map(|m| {
                rect(
                    m.layout.bounds,
                    dx - i128::from(m.anchor_x),
                    dy - i128::from(m.anchor_y),
                )
            })
            .transpose()?;
        out.push(Record {
            layer,
            kind,
            local,
            rect: layer_rect,
            mask_rect,
        });
    }
    Ok(())
}
fn sample_bytes(r: CoverageRect, format: PixelFormat) -> u64 {
    u64::from(r.width) * u64::from(r.height) * format.bytes_per_sample() as u64
}
impl Record<'_> {
    fn directory(&self, out: &mut Vec<u8>, format: PixelFormat) -> Result<u64, DocumentError> {
        let info = self.layer.info();
        for value in self.rect {
            out.extend(value.to_be_bytes());
        }
        let count = 4 + u16::from(self.mask_rect.is_some());
        out.extend(count.to_be_bytes());
        let mut payload = 0;
        for id in [-1i16, 0, 1, 2, -2].into_iter().take(count as usize) {
            let bounds = if id == -2 {
                info.mask.as_ref().unwrap().layout.bounds
            } else {
                self.local
            };
            let size = u32::try_from(2 + sample_bytes(bounds, format))
                .map_err(|_| DocumentError::ResourceLimit)?;
            payload += u64::from(size);
            out.extend(id.to_be_bytes());
            out.extend(size.to_be_bytes());
        }
        let divider = matches!(self.kind, RecordKind::Divider);
        let opacity = |v: f64| (v * 255.0).round() as u8;
        out.extend(b"8BIM");
        out.extend(if divider || matches!(self.kind, RecordKind::Folder) {
            b"norm"
        } else {
            super::blend_key(info.blend_mode)
        });
        let pixel = matches!(self.kind, RecordKind::Pixel);
        let flags = 8
            | if pixel { 0 } else { 16 }
            | (u8::from(!divider && !info.visible) << 1)
            | u8::from(!divider && info.locks.transparency);
        out.extend([
            if divider { 255 } else { opacity(info.opacity) },
            u8::from(!divider && info.clipped),
            flags,
            0,
        ]);
        let mut extra = Vec::new();
        if let Some(bounds) = self.mask_rect {
            extra.extend(20u32.to_be_bytes());
            for coord in bounds {
                extra.extend(coord.to_be_bytes());
            }
            extra.extend([
                0,
                if info.mask.as_ref().unwrap().enabled {
                    0
                } else {
                    2
                },
                0,
                0,
            ]);
        } else {
            extra.extend([0; 4]);
        }
        extra.extend([0; 4]); // Default blending ranges.
        let name = if divider {
            "</Layer group>"
        } else {
            &info.name
        };
        let legacy: Vec<_> = name
            .chars()
            .take(255)
            .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
            .collect();
        extra.push(legacy.len() as u8);
        extra.extend(&legacy);
        extra.resize(extra.len() + (4 - (1 + legacy.len()) % 4) % 4, 0);
        let mut text = Vec::from((name.encode_utf16().count() as u32).to_be_bytes());
        for c in name.encode_utf16() {
            text.extend(c.to_be_bytes());
        }
        tag(&mut extra, b"luni", &text);
        match self.kind {
            RecordKind::Folder => {
                let mut folder = Vec::from(1u32.to_be_bytes());
                folder.extend(b"8BIM");
                folder.extend(if info.pass_through {
                    b"pass"
                } else {
                    super::blend_key(info.blend_mode)
                });
                tag(&mut extra, b"lsct", &folder);
            }
            RecordKind::Divider => tag(&mut extra, b"lsct", &3u32.to_be_bytes()),
            RecordKind::Invert => tag(&mut extra, b"nvrt", &[]),
            RecordKind::Exposure(stops) => {
                let mut data = Vec::from(1u16.to_be_bytes());
                data.extend(stops.to_be_bytes());
                data.extend(0f32.to_be_bytes());
                data.extend(1f32.to_be_bytes());
                tag(&mut extra, b"expA", &data);
            }
            RecordKind::Pixel => {}
        }
        if !divider {
            tag(&mut extra, b"iOpa", &[opacity(info.fill_opacity), 0, 0, 0]);
            let locks = info.locks;
            let bits = u32::from(locks.transparency)
                | (u32::from(locks.pixels) << 1)
                | (u32::from(locks.position) << 2)
                | (u32::from(locks.all) << 31);
            tag(&mut extra, b"lspf", &bits.to_be_bytes());
            let label = match info.color_label {
                None => 0,
                Some(ColorLabel::Red) => 1,
                Some(ColorLabel::Orange) => 2,
                Some(ColorLabel::Yellow) => 3,
                Some(ColorLabel::Green) => 4,
                Some(ColorLabel::Blue) => 5,
                Some(ColorLabel::Violet) => 6,
                Some(ColorLabel::Gray) => 7,
            };
            tag(&mut extra, b"lclr", &[0, label, 0, 0, 0, 0, 0, 0]);
        }
        block(out, &extra);
        Ok(payload)
    }
    fn channels(
        &self,
        out: &mut Vec<u8>,
        format: PixelFormat,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(), DocumentError> {
        let r = self.local;
        let bps = format.bytes_per_sample();
        let plane = sample_bytes(r, format) as usize;
        let start = out.len();
        out.resize(start + 4 * (plane + 2), 0); // Each plane begins with compression=raw.
        if plane > 0 {
            for (coord, tile) in self.layer.pixels().tiles() {
                check(cancel)?;
                let tile = tile?;
                let [ox, oy] = coord.origin();
                for y in oy.max(r.y)..(oy + 256).min(r.y + i64::from(r.height)) {
                    let left = ox.max(r.x);
                    let right = (ox + 256).min(r.x + i64::from(r.width));
                    if right <= left {
                        continue;
                    }
                    let src = ((y - oy) * 256 + left - ox) as usize;
                    let dest = ((y - r.y) * i64::from(r.width) + left - r.x) as usize;
                    for (p, c) in [3, 0, 1, 2].into_iter().enumerate() {
                        let offset = start + p * (plane + 2) + 2 + dest * bps;
                        let len = (right - left) as usize;
                        let samples = &mut out[offset..offset + len * bps];
                        match &*tile {
                            PixelBuffer::Rgba8(v) => {
                                for (i, b) in samples.iter_mut().enumerate() {
                                    *b = v[4 * (src + i) + c];
                                }
                            }
                            PixelBuffer::Rgba16(v) => {
                                for (i, b) in samples.chunks_exact_mut(2).enumerate() {
                                    b.copy_from_slice(&v[4 * (src + i) + c].to_be_bytes());
                                }
                            }
                        }
                    }
                }
            }
        }
        if self.mask_rect.is_some() {
            let mask = self.layer.mask().unwrap();
            let r = mask.layout().bounds;
            out.extend([0; 2]);
            let start = out.len();
            out.resize(start + sample_bytes(r, format) as usize, 0);
            for y in (0..r.height).step_by(256) {
                for x in (0..r.width).step_by(256) {
                    check(cancel)?;
                    let tile = mask.region_tile(r.x + i64::from(x), r.y + i64::from(y))?;
                    for dy in 0..256.min(r.height - y) {
                        let dest =
                            ((y + dy) as usize * r.width as usize + x as usize) * bps + start;
                        for dx in 0..256.min(r.width - x) as usize {
                            let sample = tile[dy as usize * 256 + dx];
                            if bps == 1 {
                                if !sample.is_multiple_of(257) {
                                    return Err(invalid(
                                        "PSD export needs a 16-bit document to preserve this layer mask",
                                    ));
                                }
                                out[dest + dx] = (sample / 257) as u8;
                            } else {
                                out[dest + dx * 2..dest + dx * 2 + 2]
                                    .copy_from_slice(&sample.to_be_bytes());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn encode(
    doc: &Document,
    cancel: &dyn Fn() -> bool,
) -> Result<EncodedPsd, DocumentError> {
    let f = doc.frame();
    if f.width > 30000 || f.height > 30000 || doc.layers().is_empty() {
        return Err(invalid(
            "PSD v1 requires at least one layer and dimensions within 30000 pixels",
        ));
    }
    let format = doc.format();
    let mut entries = Vec::new();
    records(doc, None, &mut entries, cancel)?;
    let mut directory = Vec::new();
    let mut payload_size = 0u64;
    for entry in &entries {
        payload_size += entry.directory(&mut directory, format)?;
    }
    let mut resources = Vec::new();
    let mut dpi = Vec::new();
    for _ in 0..2 {
        dpi.extend((doc.resolution_dpi() << 16).to_be_bytes());
        dpi.extend([0, 1, 0, 1]);
    }
    resource(&mut resources, 1005, &[], &dpi);
    resource(&mut resources, 1039, &[], doc.working_icc());
    let mut unicode_names = Vec::new();
    if !doc.paths().is_empty() {
        use strata_io::psd::paths::{
            SavedContour, SavedKnot, SavedPath, encode_names, legacy_name,
        };
        let normalize = |p: [f64; 2]| {
            [
                (p[0] - f.x as f64) / f64::from(f.width),
                (p[1] - f.y as f64) / f64::from(f.height),
            ]
        };
        for (i, path) in doc.paths().iter().enumerate() {
            check(cancel)?;
            let contours = path
                .contours()
                .enumerate()
                .map(|(part, anchors)| {
                    let knots = anchors
                        .iter()
                        .map(|a| {
                            let abs = |v: [f64; 2]| {
                                normalize([a.position[0] + v[0], a.position[1] + v[1]])
                            };
                            SavedKnot {
                                linked: a.linked,
                                points: [abs(a.incoming), normalize(a.position), abs(a.outgoing)],
                            }
                        })
                        .collect();
                    SavedContour {
                        open: path.open_contours.contains(&part),
                        knots,
                    }
                })
                .collect();
            let saved = SavedPath {
                name: path.name.clone(),
                contours,
            };
            let encoded = saved
                .encode()
                .map_err(|e| invalid(format!("PSD path {}: {e}", saved.name)))?;
            resource(
                &mut resources,
                2000 + i as u16,
                &legacy_name(&saved.name),
                &encoded,
            );
        }
        tag(
            &mut unicode_names,
            b"pths",
            &encode_names(doc.paths().iter().map(|p| p.name.as_str())),
        );
    }
    let info_size = (2 + directory.len() as u64 + payload_size).next_multiple_of(2);
    // Photoshop stores 16-bit records in the Lr16 additional-information block.
    let wide = format == PixelFormat::Rgba16;
    let layer_size = if wide {
        8 + 12 + info_size.next_multiple_of(4)
    } else {
        4 + info_size + 4
    };
    let merged_size = f.width as u64 * f.height as u64 * 4 * format.bytes_per_sample() as u64;
    // Reserve the larger RGBA header. Opaque composites only need three planes.
    let layer_size = layer_size + unicode_names.len() as u64;
    let max_size = 26 + 4 + 4 + resources.len() as u64 + 4 + layer_size + 12 + 2 + merged_size;
    if max_size > strata_io::DEFAULT_DECODE_LIMIT {
        return Err(DocumentError::ResourceLimit);
    }
    let allocation = doc.memory_pool().reserve(MemoryKind::Transient, max_size)?;
    let _scratch = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, merged_size * 2 + 1024 * 1024)?;
    let merged = doc.export_pixels_cancellable(cancel)?;
    let transparent = match &merged {
        PixelBuffer::Rgba8(v) => v.chunks_exact(4).any(|p| p[3] != 255),
        PixelBuffer::Rgba16(v) => v.chunks_exact(4).any(|p| p[3] != 65535),
    };
    let channels = if transparent { 4u16 } else { 3u16 };
    let mut out = Vec::with_capacity(max_size as usize);
    out.extend(b"8BPS");
    out.extend(1u16.to_be_bytes());
    out.extend([0; 6]);
    out.extend(channels.to_be_bytes());
    out.extend(f.height.to_be_bytes());
    out.extend(f.width.to_be_bytes());
    out.extend(u16::from(doc.bit_depth()).to_be_bytes());
    out.extend(3u16.to_be_bytes());
    out.extend([0; 4]);
    block(&mut out, &resources);
    let layer_size = layer_size + if transparent { 12 } else { 0 };
    out.extend((layer_size as u32).to_be_bytes());
    if wide {
        out.extend([0; 8]);
        out.extend(b"8BIMLr16");
    }
    out.extend((info_size as u32).to_be_bytes());
    let info_start = out.len();
    let n = entries.len() as i16;
    out.extend((if transparent { -n } else { n }).to_be_bytes());
    out.extend(&directory);
    for entry in &entries {
        check(cancel)?;
        entry.channels(&mut out, format, cancel)?;
    }
    let written = out.len() - info_start;
    if written.next_multiple_of(2) != info_size as usize {
        return Err(invalid("PSD layer directory length mismatch"));
    }
    out.resize(
        info_start
            + if wide {
                (info_size as usize).next_multiple_of(4)
            } else {
                info_size as usize
            },
        0,
    );
    if !wide {
        out.extend([0; 4]);
    }
    out.extend(unicode_names);
    if transparent {
        tag(&mut out, if wide { b"Mt16" } else { b"Mtrn" }, &[]);
    }
    out.extend([0; 2]);
    for c in 0..usize::from(channels) {
        match &merged {
            PixelBuffer::Rgba8(v) => {
                for row in v.chunks_exact(f.width as usize * 4) {
                    check(cancel)?;
                    out.extend(
                        row.chunks_exact(4).map(|p| {
                            composite_code(u32::from(p[c]), u32::from(p[3]), 255, c) as u8
                        }),
                    );
                }
            }
            PixelBuffer::Rgba16(v) => {
                for row in v.chunks_exact(f.width as usize * 4) {
                    check(cancel)?;
                    for p in row.chunks_exact(4) {
                        out.extend(
                            (composite_code(u32::from(p[c]), u32::from(p[3]), 65535, c) as u16)
                                .to_be_bytes(),
                        );
                    }
                }
            }
        }
    }
    let expected_size = if transparent {
        max_size
    } else {
        max_size - 12 - merged_size / 4
    };
    if out.len() as u64 != expected_size {
        return Err(invalid("PSD composite length mismatch"));
    }
    Ok(EncodedPsd {
        bytes: out,
        _allocation: allocation,
    })
}

// PSD's merged RGB planes are stored over white when the fourth plane is
// merged transparency. Individual layer channels always carry straight RGB.
fn composite_code(color: u32, alpha: u32, max: u32, channel: usize) -> u32 {
    if channel == 3 {
        alpha
    } else {
        max - alpha + (color * alpha + max / 2) / max
    }
}
