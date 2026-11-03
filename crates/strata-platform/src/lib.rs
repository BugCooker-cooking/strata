//! Platform codec adapters, isolated from the portable engine.
use std::{io::Read, path::Path};
use strata_io::{DecodedImage, IoError, PlatformDecoder};

#[cfg(target_os = "macos")]
mod imageio;
mod metrics;
pub use metrics::{ProcessMetrics, process_metrics};

#[derive(Debug)]
pub struct NativeDecoder;
fn is_heif(path: &Path) -> Result<bool, IoError> {
    let mut signature = [0u8; 64];
    let mut file = std::fs::File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            IoError::NotFound(path.to_owned())
        } else {
            IoError::Io(e)
        }
    })?;
    let n = file.read(&mut signature)?;
    let heif = n >= 12
        && &signature[4..8] == b"ftyp"
        && signature[8..n].chunks_exact(4).any(|brand| {
            matches!(
                brand,
                b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
            )
        });
    Ok(heif)
}
impl PlatformDecoder for NativeDecoder {
    fn decoded_bytes(&self, path: &Path, limit: u64) -> Result<u64, IoError> {
        if is_heif(path)? {
            #[cfg(target_os = "macos")]
            {
                return imageio::decoded_bytes(path, limit);
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err(IoError::Unsupported {
                    path: path.to_owned(),
                    reason: "HEIC decoding is available on macOS".into(),
                });
            }
        }
        strata_io::decoded_bytes(path, limit)
    }

    fn decode(&self, path: &Path, limit: u64) -> Result<DecodedImage, IoError> {
        if is_heif(path)? {
            #[cfg(target_os = "macos")]
            {
                return imageio::decode(path, limit);
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err(IoError::Unsupported {
                    path: path.to_owned(),
                    reason: "HEIC requires a platform decoder; available on macOS".into(),
                });
            }
        }
        strata_io::decode_image_with_limit(path, limit)
    }
}
