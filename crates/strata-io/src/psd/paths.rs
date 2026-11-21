//! Saved path image resources and the Unicode path-name descriptor. Coordinates
//! in a knot are normalized to document dimensions and stored y before x.
use super::{Invalid, Limit, Reader, Result, Unsupported};
const SCALE: f64 = 16777216.;
const MAX_KNOTS: usize = 8192;

#[derive(Debug)]
pub struct SavedKnot {
    pub points: [[f64; 2]; 3],
    pub linked: bool,
}
#[derive(Debug)]
pub struct SavedContour {
    pub open: bool,
    /// Preceding handle, anchor, leaving handle, each an absolute normalized [x,y].
    pub knots: Vec<SavedKnot>,
}
#[derive(Debug)]
pub struct SavedPath {
    pub name: String,
    pub contours: Vec<SavedContour>,
}
impl SavedPath {
    pub(super) fn read(name: &[u8], bytes: &[u8], used: &mut usize) -> Result<Self> {
        Self::read_contours(name, bytes, used, false)
    }
    pub(super) fn read_contours(
        name: &[u8],
        bytes: &[u8],
        used: &mut usize,
        mask: bool,
    ) -> Result<Self> {
        if !bytes.len().is_multiple_of(26) {
            return Err(Invalid("saved path record length"));
        }
        let mut data = Reader(bytes);
        let mut contours = Vec::new();
        let mut path_knots = 0;
        while !data.0.is_empty() {
            let selector = data.short()?;
            let mut rec = Reader(data.take(24)?);
            match selector {
                0 | 3 => {
                    if contours.len() >= 64 {
                        return Err(Limit);
                    }
                    let count = rec.short()? as usize;
                    if !(2..=1024).contains(&count) {
                        return Err(Unsupported("saved contour knot count".into()));
                    }
                    *used += count;
                    path_knots += count;
                    if path_knots > 2048 || *used > MAX_KNOTS {
                        return Err(Limit);
                    }
                    let operation = rec.short()? as i16;
                    if !matches!(operation, 0 | 1)
                        || (mask && operation == 1 && !contours.is_empty())
                    {
                        return Err(Unsupported("saved path boolean operator".into()));
                    }
                    let open = selector == 3;
                    let mut knots = Vec::with_capacity(count);
                    for _ in 0..count {
                        let kind = data.short()?;
                        if !matches!((open, kind), (true, 4 | 5) | (false, 1 | 2)) {
                            return Err(Invalid("saved path knot selector"));
                        }
                        let mut knot = [[0.; 2]; 3];
                        for p in &mut knot {
                            let y = data.word()? as i32;
                            let x = data.word()? as i32;
                            *p = [f64::from(x) / SCALE, f64::from(y) / SCALE];
                        }
                        knots.push(SavedKnot {
                            points: knot,
                            linked: matches!(kind, 1 | 4),
                        });
                    }
                    contours.push(SavedContour { open, knots });
                }
                6 | 7 => {} // Fill rule and optional clipboard metadata.
                8 => {
                    if rec.short()? != 0 {
                        return Err(Unsupported("inverted initial path fill".into()));
                    }
                }
                _ => return Err(Unsupported(format!("saved path selector {selector}"))),
            }
        }
        if contours.is_empty() {
            return Err(Unsupported("empty saved path".into()));
        }
        Ok(Self {
            name: decode_legacy(name),
            contours,
        })
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.contours.is_empty()
            || self.contours.len() > 64
            || self
                .contours
                .iter()
                .any(|c| !(2..=1024).contains(&c.knots.len()))
            || self.contours.iter().map(|c| c.knots.len()).sum::<usize>() > 2048
        {
            return Err(Limit);
        }
        let mut out = Vec::new();
        let mut record = |selector: u16, data: [u8; 24]| {
            out.extend(selector.to_be_bytes());
            out.extend(data);
        };
        record(6, [0; 24]);
        record(8, [0; 24]);
        for contour in &self.contours {
            let mut data = [0; 24];
            data[..2].copy_from_slice(
                &u16::try_from(contour.knots.len())
                    .map_err(|_| Limit)?
                    .to_be_bytes(),
            );
            // Saved Strata contours use even-odd (exclude overlapping areas).
            record(if contour.open { 3 } else { 0 }, data);
            for knot in &contour.knots {
                let mut data = [0; 24];
                for (slot, coord) in data
                    .chunks_exact_mut(4)
                    .zip(knot.points.iter().flat_map(|p| [p[1], p[0]]))
                {
                    let fixed = (coord * SCALE).round();
                    if !(-268435456.0..268435456.0).contains(&fixed) {
                        return Err(Unsupported(
                            "path coordinate outside PSD's normalized -16..16 range".into(),
                        ));
                    }
                    slot.copy_from_slice(&(fixed as i32).to_be_bytes());
                }
                record(
                    match (contour.open, knot.linked) {
                        (true, true) => 4,
                        (true, false) => 5,
                        (false, true) => 1,
                        (false, false) => 2,
                    },
                    data,
                );
            }
        }
        Ok(out)
    }
}
const MAC_ROMAN: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{a0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{f8ff}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";
fn decode_legacy(bytes: &[u8]) -> String {
    let high: Vec<_> = MAC_ROMAN.chars().collect();
    bytes
        .iter()
        .map(|&b| {
            if b < 128 {
                char::from(b)
            } else {
                high[usize::from(b) - 128]
            }
        })
        .collect()
}
pub fn legacy_name(name: &str) -> Vec<u8> {
    name.chars()
        .take(255)
        .map(|c| {
            if c.is_ascii() {
                c as u8
            } else {
                MAC_ROMAN
                    .chars()
                    .position(|v| v == c)
                    .map_or(b'?', |i| i as u8 + 128)
            }
        })
        .collect()
}
pub(super) fn unicode(data: &mut Reader<'_>) -> Result<String> {
    let count = data.word()? as usize;
    if count > 1024 {
        return Err(Limit);
    }
    let buf = data.take(count * 2)?;
    let units = buf
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]));
    let mut s = char::decode_utf16(units)
        .collect::<std::result::Result<String, _>>()
        .map_err(|_| Invalid("UTF-16 path name"))?;
    if s.ends_with('\0') {
        s.pop();
    }
    Ok(s)
}
pub(super) fn key<'a>(d: &mut Reader<'a>) -> Result<&'a [u8]> {
    let n = d.word()? as usize;
    d.take(if n == 0 { 4 } else { n })
}
pub(super) fn descriptor(
    d: &mut Reader<'_>,
    class: &[u8],
    field: &[u8],
    kind: &[u8; 4],
) -> Result<()> {
    unicode(d)?;
    if key(d)? != class || d.word()? != 1 || key(d)? != field {
        return Err(Unsupported("Unicode path-name descriptor".into()));
    }
    d.signature(kind)
}
pub(super) fn read_names(mut data: Reader<'_>) -> Result<Vec<String>> {
    if data.word()? != 16 {
        return Err(Unsupported("Unicode path name version".into()));
    }
    descriptor(&mut data, b"pathsDataClass", b"pathList", b"VlLs")?;
    let n = data.word()? as usize;
    if n > 64 {
        return Err(Limit);
    }
    let mut names = Vec::with_capacity(n);
    for _ in 0..n {
        data.signature(b"Objc")?;
        descriptor(&mut data, b"pathInfoClass", b"pathUnicodeName", b"TEXT")?;
        names.push(unicode(&mut data)?);
    }
    data.padding(3)?;
    Ok(names)
}
pub(super) fn write_text(out: &mut Vec<u8>, s: &str) {
    out.extend(((s.encode_utf16().count() + 1) as u32).to_be_bytes());
    for unit in s.encode_utf16().chain(std::iter::once(0)) {
        out.extend(unit.to_be_bytes());
    }
}
pub(super) fn write_key(out: &mut Vec<u8>, s: &[u8]) {
    out.extend((s.len() as u32).to_be_bytes());
    out.extend(s);
}
pub(super) fn write_descriptor(out: &mut Vec<u8>, class: &[u8], field: &[u8], kind: &[u8; 4]) {
    write_text(out, "");
    write_key(out, class);
    out.extend(1u32.to_be_bytes());
    write_key(out, field);
    out.extend(kind);
}
pub fn encode_names<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> Vec<u8> {
    let mut out = Vec::from(16u32.to_be_bytes());
    write_descriptor(&mut out, b"pathsDataClass", b"pathList", b"VlLs");
    out.extend((names.len() as u32).to_be_bytes());
    for name in names {
        out.extend(b"Objc");
        write_descriptor(&mut out, b"pathInfoClass", b"pathUnicodeName", b"TEXT");
        write_text(&mut out, name);
    }
    out.resize(out.len().next_multiple_of(4), 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_names_use_mac_roman_and_unicode_descriptors_preserve_names_in_order() {
        assert_eq!(legacy_name("Café"), b"Caf\x8e");
        assert_eq!(decode_legacy(b"Caf\x8e"), "Café");
        let names = ["中文🌱", "Second", "Café"];
        let bytes = encode_names(names.into_iter());
        assert_eq!(read_names(Reader(&bytes)).unwrap(), names);
        assert!(read_names(Reader(&bytes[..bytes.len() - 8])).is_err());
    }
    #[test]
    fn open_knots_check_record_kind_and_the_last_representable_coordinate() {
        let paths = SavedPath {
            name: "Café".into(),
            contours: vec![SavedContour {
                open: true,
                knots: vec![
                    SavedKnot {
                        points: [[-16., 0.]; 3],
                        linked: false,
                    },
                    SavedKnot {
                        points: [[16. - 1. / SCALE, 1.]; 3],
                        linked: true,
                    },
                ],
            }],
        };
        let mut bytes = paths.encode().unwrap();
        let psd = SavedPath::read(b"Caf\x8e", &bytes, &mut 0).unwrap();
        assert_eq!(psd.name, "Café");
        assert_eq!(psd.contours[0].knots[1].points[0][0], 16. - 1. / SCALE);
        assert!(psd.contours[0].knots[1].linked);
        bytes[26 * 3..26 * 3 + 2].copy_from_slice(&2u16.to_be_bytes());
        assert!(matches!(
            SavedPath::read(b"a", &bytes, &mut 0),
            Err(Invalid(_))
        ));
    }
}
