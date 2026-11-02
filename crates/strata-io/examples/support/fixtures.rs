use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use std::{
    error::Error,
    fs::{self, File},
    path::Path,
};

pub const FILES: &[&str] = &[
    "p3-swatches.png",
    "p3-ramp16.png",
    "p3-alpha-edge.png",
    "expected.json",
];

pub fn generate(directory: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    // Refuse a partial overwrite too; callers can choose a new output directory.
    if FILES.iter().any(|name| directory.join(name).exists()) {
        return Err(
            "fixture destination already contains generated files; choose a new directory".into(),
        );
    }
    let mut icc = strata_color::display_p3_profile()?.icc()?;
    // ICC creation date is metadata, not a color parameter. Freeze it for reproducible fixtures.
    for (i, value) in [2026u16, 11, 2, 0, 0, 0].iter().enumerate() {
        icc[24 + i * 2..26 + i * 2].copy_from_slice(&value.to_be_bytes());
    }
    icc[84..100].fill(0); // No stored MD5 profile ID after changing its date.
    let write_png =
        |name: &str, bytes: &[u8], width, height, format| -> Result<(), Box<dyn Error>> {
            let mut encoder = PngEncoder::new(File::create_new(directory.join(name))?);
            encoder.set_icc_profile(icc.clone())?;
            encoder.write_image(bytes, width, height, format)?;
            Ok(())
        };
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
        [128, 128, 128, 255],
        [0, 0, 0, 255],
    ];
    let mut swatches = Vec::with_capacity(768 * 512 * 4);
    for y in 0..512 {
        for x in 0..768 {
            swatches.extend_from_slice(&colors[(y / 256) * 3 + x / 256]);
        }
    }
    write_png(FILES[0], &swatches, 768, 512, ExtendedColorType::Rgba8)?;
    // PngEncoder takes native-endian 16-bit input and encodes network endian itself.
    let ramp: Vec<u8> = (0..=u16::MAX)
        .flat_map(|value| [value, value, value, u16::MAX])
        .flat_map(u16::to_ne_bytes)
        .collect();
    write_png(FILES[1], &ramp, 1024, 64, ExtendedColorType::Rgba16)?;
    let mut alpha = Vec::with_capacity(512 * 256 * 4);
    for _ in 0..256 {
        for x in 0..512 {
            alpha.extend_from_slice(if x < 256 {
                &[255, 0, 0, 0]
            } else {
                &[255, 255, 255, 255]
            });
        }
    }
    write_png(FILES[2], &alpha, 512, 256, ExtendedColorType::Rgba8)?;
    use std::io::Write;
    File::create_new(directory.join(FILES[3]))?.write_all(br#"{
  "schema_version": 1,
  "license": "Apache-2.0",
  "profile": "Display P3: D65, P3 primaries, sRGB transfer curve",
  "swatches": {
    "file": "p3-swatches.png", "width": 768, "height": 512, "bit_depth": 8,
    "samples": [
      {"name":"red", "xy":[128,128], "rgba":[255,0,0,255]},
      {"name":"green", "xy":[384,128], "rgba":[0,255,0,255]},
      {"name":"blue", "xy":[640,128], "rgba":[0,0,255,255]},
      {"name":"white", "xy":[128,384], "rgba":[255,255,255,255]},
      {"name":"gray", "xy":[384,384], "rgba":[128,128,128,255]},
      {"name":"black", "xy":[640,384], "rgba":[0,0,0,255]}
    ],
    "note":"Native P3 red and green exceed sRGB gamut. These are file sample values, NOT expected monitor readouts."
  },
  "ramp": {
    "file":"p3-ramp16.png", "width":1024, "height":64, "bit_depth":16,
    "formula":"R=G=B=y*1024+x; A=65535. Includes every code 0..65535 exactly once."
  },
  "alpha_edge": {
    "file":"p3-alpha-edge.png", "width":512, "height":256, "bit_depth":8,
    "formula":"x<256: [255,0,0,0]; otherwise: [255,255,255,255]. No red fringe after scaled filtering."
  },
  "display_comparison":"Compare swatch interiors in Strata and Preview on the same display, same scale and meter mode. Target <=2/255 per output channel; record actual display/profile/settings. Screen readings do not prove 16-bit source precision."
}
"#)?;
    Ok(())
}
