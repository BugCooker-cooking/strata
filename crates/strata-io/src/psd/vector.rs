//! Standard PSD v3 vector-mask settings and RGB solid-color descriptors.
use super::{
    Invalid, Reader, Result, Unsupported,
    paths::{self, SavedPath},
};

pub struct VectorMaskRecord {
    pub path: SavedPath,
    pub enabled: bool,
    pub inverted: bool,
    pub linked: bool,
}
pub(super) fn read_mask(mut block: Reader<'_>) -> Result<VectorMaskRecord> {
    if block.word()? != 3 {
        return Err(Unsupported("vector mask version".into()));
    }
    let flags = block.word()?;
    if flags & !7 != 0 {
        return Err(Unsupported("vector mask flags".into()));
    }
    // Photoshop may include 4-byte alignment in the declared tagged-block length.
    let tail = block.0.len() % 26;
    if tail > 3 {
        return Err(Invalid("vector mask record length"));
    }
    let records = block.take(block.0.len() - tail)?;
    block.padding(3)?;
    let path = SavedPath::read_contours(b"", records, &mut 0, true)?;
    Ok(VectorMaskRecord {
        path,
        inverted: flags & 1 != 0,
        linked: flags & 2 == 0,
        enabled: flags & 4 == 0,
    })
}
pub(super) fn read_solid(mut block: Reader<'_>) -> Result<[f64; 3]> {
    if block.word()? != 16 {
        return Err(Unsupported("solid color descriptor version".into()));
    }
    paths::descriptor(&mut block, b"null", b"Clr ", b"Objc")?;
    paths::unicode(&mut block)?;
    if paths::key(&mut block)? != b"RGBC" || block.word()? != 3 {
        return Err(Unsupported("non-RGB solid color".into()));
    }
    let mut result = [None; 3];
    for _ in 0..3 {
        let key = paths::key(&mut block)?;
        let i = match key {
            b"Rd  " => 0,
            b"Grn " => 1,
            b"Bl  " => 2,
            _ => return Err(Unsupported("RGB solid color field".into())),
        };
        block.signature(b"doub")?;
        let sample = f64::from_be_bytes(block.array()?);
        if !(0.0..=255.0).contains(&sample) || result[i].replace(sample).is_some() {
            return Err(Invalid("solid color value"));
        }
    }
    block.padding(3)?;
    Ok(result.map(Option::unwrap))
}
pub fn encode_solid(color: [f64; 3]) -> Vec<u8> {
    let mut out = Vec::from(16u32.to_be_bytes());
    paths::write_descriptor(&mut out, b"null", b"Clr ", b"Objc");
    paths::write_text(&mut out, "");
    paths::write_key(&mut out, b"RGBC");
    out.extend(3u32.to_be_bytes());
    for (key, sample) in [b"Rd  ", b"Grn ", b"Bl  "].into_iter().zip(color) {
        paths::write_key(&mut out, key);
        out.extend(b"doub");
        out.extend(sample.to_be_bytes());
    }
    out
}

// Standard shmd container. Other readers use the full standard vmsk geometry.
// A verified Strata hint preserves sharing between a saved path and its layer
// masks across re-opening; it is ignored when another editor changes a curve.
pub struct MaskSourceHint {
    pub resource: u16,
    pub shape_data: Vec<u8>,
    pub matrix: [f64; 9],
}
impl MaskSourceHint {
    pub fn encode(&self) -> Vec<u8> {
        let mut data = Vec::from(*b"StrataVM");
        data.extend((if self.shape_data.is_empty() { 1u16 } else { 2 }).to_be_bytes());
        data.extend(self.resource.to_be_bytes());
        for value in self.matrix {
            data.extend(value.to_be_bytes());
        }
        data.extend(&self.shape_data);
        let mut out = Vec::from(1u32.to_be_bytes());
        out.extend(b"8BIMsVML");
        out.extend([0; 4]);
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        out
    }
}
pub(super) fn read_metadata(mut block: Reader<'_>) -> Result<Option<MaskSourceHint>> {
    let count = block.word()?;
    if count as usize > block.0.len() / 16 {
        return Err(Invalid("layer metadata count"));
    }
    let mut hint = None;
    for _ in 0..count {
        let sig = block.array::<4>()?;
        if &sig != b"8BIM" && &sig != b"8ELE" {
            return Err(Invalid("layer metadata signature"));
        }
        let key = block.array::<4>()?;
        block.take(4)?;
        let mut data = block.section()?;
        if &key != b"sVML" || !data.0.starts_with(b"StrataVM") {
            continue;
        }
        data.take(8)?;
        let version = data.short()?;
        if !matches!(version, 1 | 2) {
            continue;
        }
        let resource = data.short()?;
        let mut matrix = [0.; 9];
        for v in &mut matrix {
            *v = f64::from_be_bytes(data.array()?);
        }
        if matrix.iter().all(|v| v.is_finite())
            && (version == 1 && data.0.is_empty() || version == 2 && data.0.len() <= 4096)
        {
            hint = Some(MaskSourceHint {
                resource,
                matrix,
                shape_data: data.0.to_vec(),
            });
        }
    }
    block.padding(3)?;
    Ok(hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psd::paths::{SavedContour, SavedKnot};
    #[test]
    fn photoshop_tag_padding_and_mask_flags_decode_with_knots_intact() {
        let square = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
        let path = SavedPath {
            name: "".into(),
            contours: vec![SavedContour {
                open: false,
                knots: square
                    .map(|p| SavedKnot {
                        points: [p; 3],
                        linked: false,
                    })
                    .into_iter()
                    .collect(),
            }],
        };
        let mut tag = Vec::from(3u32.to_be_bytes());
        tag.extend(7u32.to_be_bytes());
        tag.extend(path.encode().unwrap());
        tag.resize(tag.len().next_multiple_of(4), 0);
        let mask = read_mask(Reader(&tag)).unwrap();
        assert!(mask.inverted);
        assert!(!mask.enabled);
        assert!(!mask.linked);
        assert_eq!(mask.path.contours[0].knots.len(), 4);
        *tag.last_mut().unwrap() = 7;
        assert!(read_mask(Reader(&tag)).is_err());
    }
    #[test]
    fn mixed_metadata_entries_preserve_the_mask_hint_and_skip_unrelated_payloads() {
        let matrix = [1., 0., 7., 0., 1., -8., 0., 0., 1.];
        let hint = MaskSourceHint {
            resource: 2003,
            shape_data: Vec::new(),
            matrix,
        }
        .encode();
        let mut data = Vec::from(2u32.to_be_bytes());
        data.extend(b"8ELEtest");
        data.extend([0; 4]);
        data.extend(3u32.to_be_bytes());
        data.extend(b"abc");
        data.extend(&hint[4..]);
        let read = read_metadata(Reader(&data)).unwrap().unwrap();
        assert_eq!(read.resource, 2003);
        assert_eq!(read.matrix, matrix);
        assert!(read_metadata(Reader(&data[..data.len() - 2])).is_err());
        let clr = [254.9, 127.5, 26.0];
        assert_eq!(read_solid(Reader(&encode_solid(clr))).unwrap(), clr);
    }
}
