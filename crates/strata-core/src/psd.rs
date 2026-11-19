use crate::geometry_pixels::union;
use crate::{
    BlendMode, ColorLabel, Document, DocumentCandidate, DocumentError, Layer, LayerKind,
    LayerLocks, LayerMaskInfo, MAX_LAYERS, TonalAdjustment, commands::CommandError,
    document::tile_limit,
};
use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
};
use strata_io::{
    DEFAULT_DECODE_LIMIT, IoError,
    psd::{LayerRecord, LayerType, Mask, Psd, PsdError},
};
use strata_pixels::{
    CoverageRect, MaskLayout, PixelFormat, TileCoord, TiledMask, TiledPixels,
    memory::{MemoryKind, MemoryPool},
};

pub(crate) fn open(
    path: &Path,
    mut input: File,
    pool: &MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<Document, CommandError> {
    let map = |err: PsdError| match err {
        PsdError::Unsupported(_) => IoError::Unsupported {
            path: path.into(),
            reason: err.to_string(),
        },
        PsdError::Limit => IoError::ResourceLimit {
            path: path.into(),
            limit: DEFAULT_DECODE_LIMIT,
        },
        PsdError::Invalid(_) => IoError::Decode {
            path: path.into(),
            reason: err.to_string(),
        },
    };
    let reserve = |n| {
        pool.reserve(MemoryKind::Transient, n)
            .map_err(DocumentError::from)
    };
    let size = input.metadata().map_err(IoError::from)?.len();
    if size > DEFAULT_DECODE_LIMIT {
        return Err(map(PsdError::Limit).into());
    }
    let _encoded = reserve(size)?;
    let mut file = vec![0; size as usize];
    input.rewind().map_err(IoError::from)?;
    input.read_exact(&mut file).map_err(IoError::from)?;
    let psd = Psd::parse(&file, MAX_LAYERS).map_err(map)?;
    let frame = CoverageRect {
        x: 0,
        y: 0,
        width: psd.width,
        height: psd.height,
    };
    let format = psd.format;
    let make_tiles = |rect: CoverageRect, pixels: &strata_pixels::PixelBuffer| {
        TiledPixels::from_dense_with_pool_cancellable(
            rect.width,
            rect.height,
            pixels,
            tile_limit(format),
            pool.clone(),
            cancel,
        )
        .map_err(DocumentError::from)
    };
    let mut layers = Vec::with_capacity(psd.layers.len().max(1));
    if psd.layers.is_empty() {
        let _scratch = reserve(area(frame) * 8 * format.bytes_per_sample() as u64 + 1)?;
        let pixels = psd.flat_pixels().map_err(map)?;
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        layers.push(Layer::new(name, make_tiles(frame, &pixels)?));
    } else {
        // A group mask can cover child pixels outside the artboard.
        let extent = psd
            .layers
            .iter()
            .map(|layer| layer.bounds)
            .filter(|rect| area(*rect) > 0)
            .try_fold(frame, union)?;
        let mut parents = Vec::new();
        // The visible folder is above its children, the bounding divider below.
        for src in psd.layers.iter().rev() {
            if cancel() {
                return Err(CommandError::Expired);
            }
            if src.kind == LayerType::Divider {
                parents
                    .pop()
                    .ok_or_else(|| map(PsdError::Invalid("unmatched group divider")))?;
                continue;
            }
            let mask_area = src.mask.as_ref().map_or(0, |m| area(m.bounds));
            let bps = format.bytes_per_sample() as u64;
            let rgba = area(src.bounds) * 4 * bps;
            if rgba > DEFAULT_DECODE_LIMIT || mask_area * bps > DEFAULT_DECODE_LIMIT {
                return Err(map(PsdError::Limit).into());
            }
            let _scratch = reserve((rgba + rgba / 4 + 1).max(mask_area * bps + 1) + 256 * 256 * 2)?;
            let pixels = match src.pixels(format).map_err(map)? {
                Some(pixels) => make_tiles(src.bounds, &pixels)?,
                None => TiledPixels::with_pool(format, tile_limit(format), pool.clone()),
            };
            let mut layer = Layer::new(src.name.clone(), pixels);
            configure(&mut layer, src).map_err(map)?;
            layer.info.parent = parents.last().copied();
            if src.kind == LayerType::Folder {
                if parents.len() >= 16 {
                    return Err(map(PsdError::Limit).into());
                }
                parents.push(layer.id());
            }
            if let Some(input) = &src.mask {
                let bytes = input.samples(format).map_err(map)?;
                let mask = raster_mask(input, &bytes, format, extent, pool, cancel)?;
                layer.info.mask = Some(LayerMaskInfo {
                    enabled: input.enabled,
                    layout: mask.layout(),
                    anchor_x: layer.info.x,
                    anchor_y: layer.info.y,
                });
                layer.mask = Some(mask);
            }
            layers.push(layer);
        }
        if !parents.is_empty() || layers.is_empty() {
            return Err(map(PsdError::Invalid("unclosed group")).into());
        }
        layers.reverse();
    }
    Ok(DocumentCandidate::from_layers(
        psd.width,
        psd.height,
        format,
        psd.icc.map(<[u8]>::to_vec),
        layers,
        psd.resolution_dpi,
        pool.clone(),
    )?
    .into_document())
}
fn area(rect: CoverageRect) -> u64 {
    u64::from(rect.width) * u64::from(rect.height)
}
fn configure(layer: &mut Layer, src: &LayerRecord<'_>) -> Result<(), PsdError> {
    use BlendMode::*;
    let info = &mut layer.info;
    info.kind = match src.kind {
        LayerType::Pixel => LayerKind::Pixel,
        LayerType::Folder => LayerKind::Group,
        LayerType::Invert => LayerKind::Adjustment {
            adjustment: TonalAdjustment::Invert {},
        },
        LayerType::Exposure(stops) => LayerKind::Exposure { stops },
        LayerType::Divider => return Err(PsdError::Invalid("unexpected divider")),
    };
    if src.kind == LayerType::Pixel {
        info.x = src.bounds.x;
        info.y = src.bounds.y;
    }
    info.blend_mode = match &src.blend {
        b"pass" if src.kind == LayerType::Folder => {
            info.pass_through = true;
            Normal
        }
        b"norm" => Normal,
        b"diss" => Dissolve,
        b"dark" => Darken,
        b"mul " => Multiply,
        b"idiv" => ColorBurn,
        b"lbrn" => LinearBurn,
        b"dkCl" => DarkerColor,
        b"lite" => Lighten,
        b"scrn" => Screen,
        b"div " => ColorDodge,
        b"lddg" => LinearDodge,
        b"lgCl" => LighterColor,
        b"over" => Overlay,
        b"sLit" => SoftLight,
        b"hLit" => HardLight,
        b"vLit" => VividLight,
        b"lLit" => LinearLight,
        b"pLit" => PinLight,
        b"hMix" => HardMix,
        b"diff" => Difference,
        b"smud" => Exclusion,
        b"fsub" => Subtract,
        b"fdiv" => Divide,
        b"hue " => Hue,
        b"sat " => Saturation,
        b"colr" => Color,
        b"lum " => Luminosity,
        key => {
            return Err(PsdError::Unsupported(format!(
                "blend mode {}",
                String::from_utf8_lossy(key)
            )));
        }
    };
    info.visible = src.visible;
    info.clipped = src.clipped;
    info.opacity = f64::from(src.opacity) / 255.0;
    info.fill_opacity = f64::from(src.fill_opacity) / 255.0;
    info.locks = LayerLocks {
        transparency: src.protection & 1 != 0,
        pixels: src.protection & 2 != 0,
        position: src.protection & 4 != 0,
        all: src.protection & 0x8000_0000 != 0,
    };
    info.color_label = match src.label {
        1 => Some(ColorLabel::Red),
        2 => Some(ColorLabel::Orange),
        3 => Some(ColorLabel::Yellow),
        4 => Some(ColorLabel::Green),
        5 => Some(ColorLabel::Blue),
        6 => Some(ColorLabel::Violet),
        7 => Some(ColorLabel::Gray),
        _ => None,
    };
    Ok(())
}

fn raster_mask(
    src: &Mask<'_>,
    bytes: &[u8],
    format: PixelFormat,
    extent: CoverageRect,
    pool: &MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, DocumentError> {
    let r = src.bounds;
    let bg = u16::from(src.background) * 257;
    // PSD supplies a background value outside the stored mask rectangle.
    let bounds = match (area(r), bg) {
        (0, _) => extent,
        (_, 0) => r,
        _ => union(extent, r)?,
    };
    let mut mask = TiledMask::new(
        MaskLayout {
            bounds,
            default_coverage: bg,
            inverted: false,
        },
        tile_limit(PixelFormat::Rgba8),
        pool.clone(),
    )?;
    if area(r) == 0 {
        return Ok(mask);
    }
    let right = r.x + i64::from(r.width);
    let bottom = r.y + i64::from(r.height);
    for ty in r.y.div_euclid(256)..=(bottom - 1).div_euclid(256) {
        for tx in r.x.div_euclid(256)..=(right - 1).div_euclid(256) {
            if cancel() {
                return Err(DocumentError::Cancelled);
            }
            let coord = TileCoord::new(tx, ty)?;
            let [ox, oy] = coord.origin();
            let mut encoded = vec![0; 256 * 256];
            for y in oy.max(r.y)..(oy + 256).min(bottom) {
                for x in ox.max(r.x)..(ox + 256).min(right) {
                    let i = ((y - r.y) * i64::from(r.width) + x - r.x) as usize;
                    let value = match format {
                        PixelFormat::Rgba8 => u16::from(bytes[i]) * 257,
                        PixelFormat::Rgba16 => u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]),
                    };
                    encoded[((y - oy) * 256 + x - ox) as usize] = value ^ bg;
                }
            }
            mask.insert_encoded_tile(coord, encoded)?;
        }
    }
    Ok(mask)
}
