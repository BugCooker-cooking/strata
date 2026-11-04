use crate::MlError;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};
#[derive(Serialize)]
pub struct ModelAsset {
    pub id: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub license: &'static str,
    pub source: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub max_bytes: u64,
}
pub const SUBJECT_MODEL: ModelAsset = ModelAsset {
    id: "u2netp",
    version: "rembg-v0.0.0",
    name: "U²-Net small salient subject model",
    license: "Apache-2.0 (U²-Net); MIT (ONNX conversion)",
    source: "https://github.com/xuebinqin/U-2-Net",
    url: "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2netp.onnx",
    sha256: "309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8",
    max_bytes: 8 * 1024 * 1024,
};

pub const PATCHED_RUNTIME_ARCHIVE_SHA256: &str =
    "00fbfd6f08bac2a4e28c66723af900d58d1b4b1c73efba6290637cd3019883d5";
pub fn verify(path: &Path, expected: &str, max_bytes: u64) -> Result<(), MlError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MlError::Missing
        } else {
            e.into()
        }
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(MlError::Invalid("asset type or size".into()));
    }
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 65536];
    let mut digest = Sha256::new();
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > max_bytes {
            return Err(MlError::Invalid("asset grew beyond limit".into()));
        }
        digest.update(&buffer[..read]);
    }
    let hash = digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if hash != expected {
        return Err(MlError::Checksum);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_asset_validation_rejects_wrong_hash_size_and_missing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.onnx");
        assert!(matches!(
            verify(&path, SUBJECT_MODEL.sha256, SUBJECT_MODEL.max_bytes),
            Err(MlError::Missing)
        ));
        fs::write(&path, b"abc").unwrap();
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&path, expected, 3).is_ok());
        assert!(verify(&path, expected, 2).is_err());
        assert!(matches!(
            verify(&path, SUBJECT_MODEL.sha256, 3),
            Err(MlError::Checksum)
        ));
        assert!(verify(directory.path(), expected, 100).is_err());
    }
}
