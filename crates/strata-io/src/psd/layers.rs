use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayerType {
    Pixel,
    Folder,
    Divider,
    Invert,
    Exposure(f64),
}

pub struct Mask<'a> {
    pub bounds: CoverageRect,
    pub background: u8,
    pub enabled: bool,
    channel: &'a [u8],
}
impl Mask<'_> {
    pub fn samples(&self, format: PixelFormat) -> Result<Vec<u8>> {
        channels::decode(self.channel, self.bounds.width, self.bounds.height, format)
    }
}

pub struct LayerRecord<'a> {
    pub bounds: CoverageRect,
    pub name: String,
    pub kind: LayerType,
    pub blend: [u8; 4],
    pub opacity: u8,
    pub fill_opacity: u8,
    pub clipped: bool,
    pub visible: bool,
    pub protection: u32,
    pub label: u16,
    pub mask: Option<Mask<'a>>,
    channels: Vec<(i16, usize, &'a [u8])>,
}

impl LayerRecord<'_> {
    pub fn pixels(&self, format: PixelFormat) -> Result<Option<PixelBuffer>> {
        if self.kind != LayerType::Pixel || self.bounds.width == 0 || self.bounds.height == 0 {
            return Ok(None);
        }
        let mut pixels = channels::opaque(self.bounds.width, self.bounds.height, format)?;
        for &(id, _, data) in &self.channels {
            if id == -2 {
                continue;
            }
            let channel = channels::decode(data, self.bounds.width, self.bounds.height, format)?;
            channels::insert(&mut pixels, if id < 0 { 3 } else { id as usize }, &channel);
        }
        Ok(Some(pixels))
    }
}

pub(super) fn read<'a>(
    mut section: Reader<'a>,
    format: PixelFormat,
    limit: usize,
    names: &mut Option<Vec<String>>,
) -> Result<(Vec<LayerRecord<'a>>, bool)> {
    if section.0.is_empty() {
        return Ok((Vec::new(), false));
    }
    let mut info = section.section()?;
    if !section.0.is_empty() {
        section.section()?;
    } // Global quick-mask overlay/display settings.
    while section.0.len() >= 12 {
        let sig = section.array::<4>()?;
        if &sig != b"8BIM" && &sig != b"8B64" {
            return Err(Invalid("global block signature"));
        }
        let tag = section.array::<4>()?;
        let block = section.section()?;
        section.take(block.0.len().next_multiple_of(4) - block.0.len())?;
        match &tag {
            b"pths" => {
                if names.is_some() {
                    return Err(Invalid("duplicate Unicode path names"));
                }
                *names = Some(super::paths::read_names(block)?);
            }
            b"Lr16" if format == PixelFormat::Rgba16 && info.0.is_empty() => info = block,
            b"Layr" if format == PixelFormat::Rgba8 && info.0.is_empty() => info = block,
            b"Lr16" | b"Layr" | b"Lr32" => {
                return Err(Unsupported("multiple or incompatible layer tables".into()));
            }
            _ => {}
        }
    }
    section.padding(3)?;
    if info.0.is_empty() {
        return Ok((Vec::new(), false));
    }
    let signed = info.short()? as i16;
    let count = usize::from(signed.unsigned_abs());
    if count > limit.saturating_mul(2) {
        return Err(Limit);
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        records.push(record(&mut info)?);
    }
    if records
        .iter()
        .filter(|r| r.kind != LayerType::Divider)
        .count()
        > limit
    {
        return Err(Limit);
    }
    for layer in &mut records {
        for (id, len, data) in &mut layer.channels {
            *data = info.take(*len)?;
            if *id == -2 {
                layer
                    .mask
                    .as_mut()
                    .ok_or(Invalid("mask channel without bounds"))?
                    .channel = data;
            }
        }
        if layer.mask.as_ref().is_some_and(|m| m.channel.is_empty()) {
            return Err(Invalid("missing mask channel"));
        }
    }
    info.padding(3)?;
    Ok((records, signed < 0))
}
fn record<'a>(src: &mut Reader<'a>) -> Result<LayerRecord<'a>> {
    let bounds = src.rect()?;
    let count = src.short()?;
    if count > 5 {
        return Err(Unsupported("additional layer channels".into()));
    }
    let mut channels = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let id = src.short()? as i16;
        let length = src.word()? as usize;
        if !(-2..=2).contains(&id) {
            return Err(Unsupported(format!("layer channel {id}")));
        }
        if channels.iter().any(|&(seen, _, _)| seen == id) || length < 2 {
            return Err(Invalid("channel directory"));
        }
        channels.push((id, length, &[][..]));
    }
    src.signature(b"8BIM")?;
    let blend = src.array()?;
    let opacity = src.byte()?;
    let clip = src.byte()?;
    let flags = src.byte()?;
    if clip > 1 || src.byte()? != 0 {
        return Err(Invalid("layer flags"));
    }
    let mut extra = src.section()?;
    let mut mask = extra.section()?;
    let mask = if mask.0.is_empty() {
        None
    } else {
        let rect = mask.rect()?;
        let background = mask.byte()?;
        let flags = mask.byte()?;
        if !matches!(background, 0 | 255) || flags & !2 != 0 || mask.0.len() != 2 {
            return Err(Unsupported("advanced raster mask options".into()));
        }
        mask.padding(2)?;
        Some(Mask {
            bounds: rect,
            background,
            enabled: flags & 2 == 0,
            channel: &[],
        })
    };
    let ranges = extra.section()?;
    if !ranges.0.len().is_multiple_of(8) || ranges.0.chunks_exact(4).any(|r| r != [0, 0, 255, 255])
    {
        return Err(Unsupported("Blend If ranges".into()));
    }
    let legacy_name = extra.pascal(4)?;
    let mut layer = LayerRecord {
        bounds,
        name: String::new(),
        kind: LayerType::Pixel,
        blend,
        opacity,
        fill_opacity: 255,
        clipped: clip != 0,
        visible: flags & 2 == 0,
        protection: u32::from(flags & 1),
        label: 0,
        mask,
        channels,
    };
    let mut unicode = None;
    while extra.0.len() >= 12 {
        extra.signature(b"8BIM")?;
        let tag = extra.array::<4>()?;
        let mut block = extra.section()?;
        // PSD writers either include the even pad in the length, or append it.
        extra.take(block.0.len() % 2)?;
        match &tag {
            b"luni" => {
                let count = block.word()? as usize;
                if count > 1024 {
                    return Err(Limit);
                }
                let units = (0..count)
                    .map(|_| block.short())
                    .collect::<Result<Vec<_>>>()?;
                let name = String::from_utf16(&units).map_err(|_| Invalid("UTF-16 layer name"))?;
                unicode = Some(name.trim_end_matches('\0').to_string());
            }
            b"lsct" | b"lsdk" => {
                layer.kind = match block.word()? {
                    0 => LayerType::Pixel,
                    1 | 2 => LayerType::Folder,
                    3 => LayerType::Divider,
                    _ => return Err(Invalid("section divider type")),
                };
                if !block.0.is_empty() {
                    block.signature(b"8BIM")?;
                    layer.blend = block.array()?;
                }
                if !block.0.is_empty() && block.word()? != 0 {
                    return Err(Unsupported("scene group".into()));
                }
            }
            b"nvrt" => layer.kind = LayerType::Invert,
            b"expA" => {
                if block.short()? != 1 {
                    return Err(Unsupported("exposure version".into()));
                }
                let stops = f32::from_bits(block.word()?);
                let offset = f32::from_bits(block.word()?);
                let gamma = f32::from_bits(block.word()?);
                if !(-20.0..=20.0).contains(&stops) || offset != 0.0 || gamma != 1.0 {
                    return Err(Unsupported("exposure offset, gamma or range".into()));
                }
                layer.kind = LayerType::Exposure(stops.into());
            }
            b"iOpa" => layer.fill_opacity = block.byte()?,
            b"lspf" => {
                let bits = block.word()?;
                // Bit 3 only controls automatic nesting into artboards. Artboard
                // layers are rejected as unsupported by the additional-info parser.
                if bits & !0x8000_000f != 0 {
                    return Err(Unsupported("layer protection flags".into()));
                }
                layer.protection |= bits;
            }
            b"lclr" => {
                layer.label = block.short()?;
                if layer.label > 7 {
                    return Err(Unsupported("layer color label".into()));
                }
            }
            b"clbl" if block.byte()? == 1 => {}
            b"infx" | b"knko" if block.byte()? == 0 => {}
            b"lyid" | b"lnsr" | b"fxrp" | b"shmd" | b"lyvr" => {}
            _ => {
                return Err(Unsupported(format!(
                    "layer block {}",
                    String::from_utf8_lossy(&tag)
                )));
            }
        }
    }
    extra.padding(3)?;
    layer.name = match unicode {
        Some(name) => name,
        None => std::str::from_utf8(legacy_name)
            .map_err(|_| Unsupported("legacy layer-name encoding".into()))?
            .to_string(),
    };
    if layer.name.len() > 1024 {
        return Err(Limit);
    }
    if layer.kind == LayerType::Pixel && bounds.width > 0 && bounds.height > 0 {
        if flags & 24 == 24 {
            return Err(Unsupported("non-pixel layer".into()));
        }
        if !(0..3).all(|rgb| layer.channels.iter().any(|&(id, _, _)| id == rgb)) {
            return Err(Invalid("missing RGB layer channel"));
        }
    }
    Ok(layer)
}
