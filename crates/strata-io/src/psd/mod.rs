//! Read the editable RGB subset of Adobe's PSD v1 format. Layer channels borrow
//! the encoded file so the caller can expand and tile one layer at a time.
mod channels;
mod layers;

pub use layers::{LayerRecord, LayerType, Mask};
use strata_pixels::{CoverageRect, PixelBuffer, PixelFormat};

#[derive(Debug, thiserror::Error)]
pub enum PsdError {
    #[error("invalid PSD: {0}")]
    Invalid(&'static str),
    #[error("unsupported PSD feature: {0}")]
    Unsupported(String),
    #[error("PSD exceeds the import limit")]
    Limit,
}
type Result<T> = std::result::Result<T, PsdError>;
use PsdError::{Invalid, Limit, Unsupported};

#[derive(Clone, Copy)]
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let (data, rest) = self
            .0
            .split_at_checked(len)
            .ok_or(Invalid("truncated section"))?;
        self.0 = rest;
        Ok(data)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    fn short(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn word(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn section(&mut self) -> Result<Self> {
        let len = self.word()?;
        Ok(Self(self.take(len as usize)?))
    }
    fn signature(&mut self, expected: &[u8; 4]) -> Result<()> {
        if self.take(4)? != expected {
            return Err(Invalid("section signature"));
        }
        Ok(())
    }
    fn pascal(&mut self, align: usize) -> Result<&'a [u8]> {
        let len = self.byte()? as usize;
        let value = self.take(len)?;
        self.take((len + 1).next_multiple_of(align) - (len + 1))?;
        Ok(value)
    }
    fn rect(&mut self) -> Result<CoverageRect> {
        let [top, left, bottom, right] =
            [self.word()?, self.word()?, self.word()?, self.word()?].map(|v| i64::from(v as i32));
        let side = |size: i64| {
            if (0..=30000).contains(&size) {
                Ok(size as u32)
            } else {
                Err(Invalid("layer bounds"))
            }
        };
        Ok(CoverageRect {
            x: left,
            y: top,
            width: side(right - left)?,
            height: side(bottom - top)?,
        })
    }
    fn padding(self, max: usize) -> Result<()> {
        if self.0.len() > max || self.0.iter().any(|&b| b != 0) {
            return Err(Invalid("unexpected trailing data"));
        }
        Ok(())
    }
}

pub struct Psd<'a> {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub icc: Option<&'a [u8]>,
    pub resolution_dpi: u32,
    /// PSD records are in bottom-to-top order, including section dividers.
    pub layers: Vec<LayerRecord<'a>>,
    composite: &'a [u8],
}

impl<'a> Psd<'a> {
    pub fn parse(bytes: &'a [u8], layer_limit: usize) -> Result<Self> {
        let mut src = Reader(bytes);
        src.signature(b"8BPS")?;
        if src.short()? != 1 {
            return Err(Unsupported("PSB or unknown version".into()));
        }
        if src.take(6)? != [0; 6] {
            return Err(Invalid("reserved header"));
        }
        let channel_count = src.short()?;
        let (height, width) = (src.word()?, src.word()?);
        let format = match src.short()? {
            8 => PixelFormat::Rgba8,
            16 => PixelFormat::Rgba16,
            _ => return Err(Unsupported("sample depth (requires RGB 8/16)".into())),
        };
        if src.short()? != 3 {
            return Err(Unsupported("color mode (requires RGB)".into()));
        }
        if !(1..=30000).contains(&width)
            || !(1..=30000).contains(&height)
            || format.byte_count(width, height).map_err(|_| Limit)? as u64
                > crate::DEFAULT_DECODE_LIMIT
        {
            return Err(Limit);
        }
        if !src.section()?.0.is_empty() {
            return Err(Invalid("RGB color mode data"));
        }
        let mut resources = src.section()?;
        let mut icc = None;
        let mut resolution_dpi = 72;
        let mut has_composite = true;
        while !resources.0.is_empty() {
            resources.signature(b"8BIM")?;
            let id = resources.short()?;
            resources.pascal(2)?;
            let mut block = resources.section()?;
            resources.take(block.0.len() % 2)?;
            match id {
                1057 => {
                    if block.word()? != 1 {
                        return Err(Unsupported("version information".into()));
                    }
                    has_composite = block.byte()? != 0;
                }
                1039 => {
                    if block.0.len() > 4 * 1024 * 1024 {
                        return Err(Limit);
                    }
                    if icc.replace(block.0).is_some() {
                        return Err(Invalid("duplicate ICC profile"));
                    }
                }
                1005 => {
                    let horizontal = block.word()?;
                    block.take(4)?;
                    let vertical = block.word()?;
                    block.take(4)?;
                    if horizontal != vertical {
                        return Err(Unsupported(
                            "unequal horizontal and vertical resolution".into(),
                        ));
                    }
                    resolution_dpi = ((u64::from(horizontal) + 32768) / 65536) as u32;
                    if !(1..=2400).contains(&resolution_dpi) {
                        return Err(Unsupported("resolution outside 1..2400 dpi".into()));
                    }
                }
                _ => {}
            }
        }
        let (layers, merged_alpha) = layers::read(src.section()?, format, layer_limit)?;
        if layers.is_empty() && !has_composite {
            return Err(Invalid("no layers or usable composite"));
        }
        if channel_count != 3 && !(channel_count == 4 && merged_alpha) {
            return Err(Unsupported("extra document channels".into()));
        }
        Ok(Self {
            width,
            height,
            format,
            icc,
            resolution_dpi,
            layers,
            composite: src.0,
        })
    }

    /// With no layer records the planar RGB composite is the only image source.
    pub fn flat_pixels(&self) -> Result<PixelBuffer> {
        if !self.layers.is_empty() {
            return Err(Invalid("expected a flat PSD"));
        }
        let planes = channels::decode(self.composite, self.width, self.height * 3, self.format)?;
        let plane_size = planes.len() / 3;
        let mut pixels = channels::opaque(self.width, self.height, self.format)?;
        for (id, plane) in planes.chunks_exact(plane_size).enumerate() {
            channels::insert(&mut pixels, id, plane);
        }
        Ok(pixels)
    }
}
