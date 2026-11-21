pub(crate) mod export;
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

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ShapeExtras {
    definition: Option<crate::ShapeDefinition>,
    depth: u8,
    alpha: u16,
    fill_opacity: f64,
}

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
    let mut paths: Vec<_> = psd
        .paths
        .iter()
        .map(|saved| std::sync::Arc::new(work_path(saved, &saved.name, psd.width, psd.height)))
        .collect();
    let saved_count = paths.len();
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
            configure(&mut layer, src, format).map_err(map)?;
            if let Some(v) = &src.vector_mask {
                let fresh = work_path(
                    &v.path,
                    &format!("Vector mask {}", paths.len() + 1),
                    psd.width,
                    psd.height,
                );
                let matched = src.mask_source.as_ref().and_then(|hint| {
                    let idx = psd.path_resource_ids.binary_search(&hint.resource).ok()?;
                    let source = paths.get(..saved_count)?.get(idx)?;
                    let m = crate::geometry::Homography::new(hint.matrix).ok()?;
                    if m.values()[6] != 0. || m.values()[7] != 0. {
                        return None;
                    }
                    let stored =
                        path_for_psd(source, [0, 0], [psd.width, psd.height], Some(&m)).ok()?;
                    if !same_knots(&stored, &v.path) {
                        return None;
                    }
                    Some((idx, m.values()))
                });
                let mut mask = if let Some((idx, matrix)) = matched {
                    let id = paths[idx].id;
                    if matches!(layer.info.kind, LayerKind::Solid { .. })
                        && let Some(extra) = src
                            .mask_source
                            .as_ref()
                            .and_then(|h| serde_json::from_slice::<ShapeExtras>(&h.shape_data).ok())
                    {
                        let maximum = if format == PixelFormat::Rgba8 {
                            255u16
                        } else {
                            65535
                        };
                        if extra.depth == format.bit_depth()
                            && extra.alpha <= maximum
                            && (0.0..=1.).contains(&extra.fill_opacity)
                            && (extra.fill_opacity * f64::from(extra.alpha) / f64::from(maximum)
                                * 255.)
                                .round() as u8
                                == src.fill_opacity
                        {
                            if let LayerKind::Solid { color } = &mut layer.info.kind {
                                color[3] = extra.alpha;
                            }
                            layer.info.fill_opacity = extra.fill_opacity;
                        }
                        if let Some(def) = extra.definition
                            && let Ok(anchors) = def.anchors()
                        {
                            let mut candidate = (*paths[idx]).clone();
                            candidate.anchors = anchors;
                            candidate.shape = Some(def);
                            if candidate.validate().is_ok()
                                && let Ok(expected) =
                                    path_for_psd(&candidate, [0, 0], [psd.width, psd.height], None)
                                && expected
                                    .encode()
                                    .is_ok_and(|a| psd.paths[idx].encode().is_ok_and(|b| a == b))
                            {
                                paths[idx] = std::sync::Arc::new(candidate);
                            }
                        }
                    }
                    let mut mask = crate::VectorMask::new(id, &layer.info, v.linked);
                    mask.matrix = matrix;
                    mask
                } else {
                    if paths.len() >= crate::paths::MAX_PATHS {
                        return Err(map(PsdError::Limit).into());
                    }
                    let mask = crate::VectorMask::new(fresh.id, &layer.info, v.linked);
                    paths.push(std::sync::Arc::new(fresh));
                    mask
                };
                mask.enabled = v.enabled;
                mask.inverted = v.inverted;
                let mask_id = mask.path;
                layer.info.vector_mask = Some(mask);
                if matches!(layer.info.kind, LayerKind::Solid { .. }) {
                    layer.info.shape_paint = Some(crate::ShapePaint {
                        owned_path: Some(mask_id),
                        ..Default::default()
                    });
                }
            }
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
    let doc = DocumentCandidate::from_layers_with_paths(
        psd.width,
        psd.height,
        format,
        psd.icc.map(<[u8]>::to_vec),
        layers,
        psd.resolution_dpi,
        pool.clone(),
        paths,
    )?
    .into_document();
    Ok(doc)
}
fn work_path(
    resource: &strata_io::psd::paths::SavedPath,
    name: &str,
    width: u32,
    height: u32,
) -> crate::WorkPath {
    let mut open_contours = std::collections::BTreeSet::new();
    let canvas = |p: [f64; 2]| [p[0] * f64::from(width), p[1] * f64::from(height)];
    let mut contours = resource.contours.iter().enumerate().map(|(i, contour)| {
        if contour.open {
            open_contours.insert(i);
        }
        contour
            .knots
            .iter()
            .map(|knot| {
                let [before, position, after] = knot.points.map(canvas);
                crate::PathAnchor {
                    position,
                    linked: knot.linked,
                    incoming: std::array::from_fn(|k| before[k] - position[k]),
                    outgoing: std::array::from_fn(|k| after[k] - position[k]),
                }
            })
            .collect()
    });
    crate::WorkPath {
        id: crate::PathId::fresh(),
        shape: None,
        name: name.into(),
        anchors: contours.next().expect("validated PSD path"),
        subpaths: contours.collect(),
        open_contours,
    }
}
fn path_for_psd(
    path: &crate::WorkPath,
    origin: [i64; 2],
    size: [u32; 2],
    placement: Option<&crate::geometry::Homography>,
) -> Result<strata_io::psd::paths::SavedPath, DocumentError> {
    use strata_io::psd::paths::{SavedContour, SavedKnot, SavedPath};
    let normalize = |p: [f64; 2]| -> Result<_, DocumentError> {
        let p = if let Some(m) = placement {
            m.map(p)
                .map_err(|e| DocumentError::Invalid(e.to_string()))?
        } else {
            p
        };
        Ok(std::array::from_fn(|k| {
            (p[k] - origin[k] as f64) / f64::from(size[k])
        }))
    };
    let mut contours = Vec::new();
    for (i, anchors) in path.contours().enumerate() {
        let mut knots = Vec::new();
        for a in anchors {
            let abs = |v: [f64; 2]| normalize([a.position[0] + v[0], a.position[1] + v[1]]);
            knots.push(SavedKnot {
                linked: a.linked,
                points: [abs(a.incoming)?, normalize(a.position)?, abs(a.outgoing)?],
            });
        }
        contours.push(SavedContour {
            open: path.open_contours.contains(&i),
            knots,
        });
    }
    Ok(SavedPath {
        name: path.name.clone(),
        contours,
    })
}
fn same_knots(a: &strata_io::psd::paths::SavedPath, b: &strata_io::psd::paths::SavedPath) -> bool {
    a.contours.len() == b.contours.len()
        && a.contours.iter().zip(&b.contours).all(|(a, b)| {
            a.open == b.open
                && a.knots.len() == b.knots.len()
                && a.knots.iter().zip(&b.knots).all(|(a, b)| {
                    a.linked == b.linked
                        && a.points
                            .iter()
                            .flatten()
                            .zip(b.points.iter().flatten())
                            .all(|(x, y)| (x - y).abs() <= 2. / 16777216.)
                })
        })
}
fn area(rect: CoverageRect) -> u64 {
    u64::from(rect.width) * u64::from(rect.height)
}
fn configure(
    layer: &mut Layer,
    src: &LayerRecord<'_>,
    format: PixelFormat,
) -> Result<(), PsdError> {
    let info = &mut layer.info;
    info.kind = match src.kind {
        LayerType::Solid(rgb) => {
            let max = if format == PixelFormat::Rgba8 {
                255
            } else {
                65535
            };
            let [r, g, b] = rgb.map(|v| (v / 255.0 * f64::from(max)).round() as u16);
            LayerKind::Solid {
                color: [r, g, b, max],
            }
        }
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
    info.pass_through = src.kind == LayerType::Folder && &src.blend == b"pass";
    info.blend_mode = if info.pass_through {
        BlendMode::Normal
    } else {
        BlendMode::ALL
            .into_iter()
            .find(|&mode| blend_key(mode) == &src.blend)
            .ok_or_else(|| {
                PsdError::Unsupported(format!(
                    "blend mode {}",
                    String::from_utf8_lossy(&src.blend)
                ))
            })?
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

pub(super) fn blend_key(mode: BlendMode) -> &'static [u8; 4] {
    match mode {
        BlendMode::Normal => b"norm",
        BlendMode::Dissolve => b"diss",
        BlendMode::Darken => b"dark",
        BlendMode::Multiply => b"mul ",
        BlendMode::ColorBurn => b"idiv",
        BlendMode::LinearBurn => b"lbrn",
        BlendMode::DarkerColor => b"dkCl",
        BlendMode::Lighten => b"lite",
        BlendMode::Screen => b"scrn",
        BlendMode::ColorDodge => b"div ",
        BlendMode::LinearDodge => b"lddg",
        BlendMode::LighterColor => b"lgCl",
        BlendMode::Overlay => b"over",
        BlendMode::SoftLight => b"sLit",
        BlendMode::HardLight => b"hLit",
        BlendMode::VividLight => b"vLit",
        BlendMode::LinearLight => b"lLit",
        BlendMode::PinLight => b"pLit",
        BlendMode::HardMix => b"hMix",
        BlendMode::Difference => b"diff",
        BlendMode::Exclusion => b"smud",
        BlendMode::Subtract => b"fsub",
        BlendMode::Divide => b"fdiv",
        BlendMode::Hue => b"hue ",
        BlendMode::Saturation => b"sat ",
        BlendMode::Color => b"colr",
        BlendMode::Luminosity => b"lum ",
    }
}
