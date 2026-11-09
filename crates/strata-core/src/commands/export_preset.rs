//! App settings only. File access still needs a fresh source-bound capability.
use super::*;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuickExportFormat {
    #[default]
    Png,
    Tiff,
    Jpeg,
    Webp,
}
impl QuickExportFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Tiff => "tiff",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuickExportSpace {
    Srgb,
    DisplayP3,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportPresetOptions {
    #[serde(default)]
    pub format: QuickExportFormat,
    #[serde(default)]
    pub webp_lossless: Option<bool>,
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub width: Option<u32>,
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub height: Option<u32>,
    #[serde(default)]
    pub target_space: Option<QuickExportSpace>,
    #[serde(default)]
    #[schemars(range(min = 1, max = 100))]
    pub quality: Option<u8>,
    #[serde(default)]
    pub background: Option<[u8; 3]>,
    #[serde(default)]
    pub allow_quantization: bool,
}
impl ExportPresetOptions {
    pub(super) fn export(&self, resource: String) -> ExportFile {
        ExportFile {
            resource,
            format: self.format.name().into(),
            webp_lossless: self.webp_lossless,
            width: self.width,
            height: self.height,
            target_space: self.target_space.map(|v| {
                match v {
                    QuickExportSpace::Srgb => "srgb",
                    QuickExportSpace::DisplayP3 => "display_p3",
                }
                .into()
            }),
            quality: self.quality,
            background: self.background,
            allow_quantization: self.allow_quantization,
        }
    }
    fn validated(mut self) -> Result<Self, CommandError> {
        super::export::validate_format(&self.export(String::new()))?;
        match (self.width,self.height) {
            (None,None)=>{},
            (Some(w),Some(h)) if w>0 && h>0 && (u64::from(w)*u64::from(h)).saturating_mul(4)<=crate::MAX_FLAT_IMAGE_BYTES=>{},
            _=>return Err(CommandError::Invalid("preset width/height must both be positive and fit512MiB of8-bit samples; actual document-depth budget is checked at export".into()))
        }
        if self.format == QuickExportFormat::Webp {
            if self.width.is_some_and(|v| v > 16383) || self.height.is_some_and(|v| v > 16383) {
                return Err(CommandError::Invalid(
                    "WebP dimensions must be at most16383".into(),
                ));
            }
            self.webp_lossless = Some(self.webp_lossless.unwrap_or(true));
            if self.webp_lossless == Some(false) {
                self.quality = Some(self.quality.unwrap_or(80));
            }
        }
        if self.format == QuickExportFormat::Jpeg {
            self.quality = Some(self.quality.unwrap_or(90));
        }
        Ok(self)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportPreset {
    pub version: u64,
    pub options: ExportPresetOptions,
}
#[derive(Clone, Default)]
pub struct ExportPresetStore(Arc<Mutex<ExportPresetState>>);
#[derive(Default)]
struct ExportPresetState {
    value: ExportPreset,
    publishing: usize,
}
pub(super) struct ExportPublication(ExportPresetStore);
impl Drop for ExportPublication {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.0.0.lock() {
            slot.publishing = slot.publishing.saturating_sub(1);
        }
    }
}
impl ExportPresetStore {
    pub fn get(&self) -> Result<ExportPreset, CommandError> {
        self.0
            .lock()
            .map(|v| v.value.clone())
            .map_err(|_| CommandError::Invalid("export preset lock unavailable".into()))
    }
    /// Keep the captured version valid through file publication while allowing
    /// queries from other engines to read without waiting for disk encoding.
    pub(super) fn guard(&self, expected: u64) -> Result<ExportPublication, CommandError> {
        let mut slot = self
            .0
            .lock()
            .map_err(|_| CommandError::Invalid("export preset lock unavailable".into()))?;
        if slot.value.version != expected {
            return Err(CommandError::Stale);
        }
        slot.publishing = slot
            .publishing
            .checked_add(1)
            .ok_or_else(|| CommandError::Invalid("too many simultaneous quick exports".into()))?;
        Ok(ExportPublication(self.clone()))
    }
    pub(super) fn prepare(&self, p: SetExportPreset) -> Result<(u64, ExportPreset), CommandError> {
        let old = self.get()?;
        if p.expected_version.is_some_and(|v| v != old.version) {
            return Err(CommandError::Stale);
        }
        let options = p.options.validated()?;
        let version = if options == old.options {
            old.version
        } else {
            old.version
                .checked_add(1)
                .ok_or_else(|| CommandError::Invalid("export preset version exhausted".into()))?
        };
        Ok((old.version, ExportPreset { version, options }))
    }
    pub(super) fn commit(&self, expected: u64, value: ExportPreset) -> Result<(), CommandError> {
        let mut slot = self
            .0
            .lock()
            .map_err(|_| CommandError::Invalid("export preset lock unavailable".into()))?;
        if slot.value.version != expected {
            return Err(CommandError::Stale);
        }
        if slot.publishing != 0 && slot.value != value {
            return Err(CommandError::Invalid(
                "quick export is publishing; retry preset changes when it finishes".into(),
            ));
        }
        slot.value = value;
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetExportPreset {
    pub options: ExportPresetOptions,
    #[serde(default)]
    pub expected_version: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuickExport {
    pub resource: String,
    pub preset_version: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuickExportResult {
    pub status: String,
    pub command: String,
    pub preset_version: u64,
    pub format: QuickExportFormat,
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_pins_version_without_blocking_queries_or_another_engine_on_disk_io() {
        let store = ExportPresetStore::default();
        let owner = store.guard(0).unwrap();
        let other = store.clone();
        let writer = std::thread::spawn(move || {
            assert_eq!(other.get().unwrap().version, 0);
            let (version, preset) = other
                .prepare(SetExportPreset {
                    options: ExportPresetOptions {
                        format: QuickExportFormat::Tiff,
                        ..Default::default()
                    },
                    expected_version: Some(0),
                })
                .unwrap();
            assert!(other.commit(version, preset).is_err());
            assert_eq!(other.get().unwrap().options.format, QuickExportFormat::Png);
        });
        writer.join().unwrap();
        drop(owner);
        let (version, preset) = store
            .prepare(SetExportPreset {
                options: ExportPresetOptions {
                    format: QuickExportFormat::Tiff,
                    ..Default::default()
                },
                expected_version: Some(0),
            })
            .unwrap();
        store.commit(version, preset).unwrap();
        assert_eq!(store.get().unwrap().version, 1);
        assert!(store.guard(0).is_err());
    }
}
