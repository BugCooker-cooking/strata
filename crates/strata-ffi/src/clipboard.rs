use crate::*;
use std::io::Write;
use uuid::Uuid;

#[derive(uniffi::Record)]
pub struct ClipboardTransfer {
    pub data: Vec<u8>,
    pub origin_x: i64,
    pub origin_y: i64,
}
fn source(engine: &StrataEngine, turn: Option<String>) -> Result<Source, StrataError> {
    if let Some(turn) = turn {
        let turn = Uuid::parse_str(&turn).map_err(render_error)?;
        crate::ai::require_turn(engine, turn, true).map_err(render_error)?;
        Ok(Source::Ai { turn_id: turn })
    } else {
        Ok(Source::User)
    }
}
#[uniffi::export]
impl StrataEngine {
    pub fn clipboard_export(
        &self,
        resource: String,
        turn_id: Option<String>,
    ) -> Result<ClipboardTransfer, StrataError> {
        let owner = source(self, turn_id)?;
        let image = self
            .lock()?
            .editor
            .clipboard_image(&resource, &owner)
            .map_err(command_error)?;
        let (w, h) = image.size();
        let (x, y) = image.origin();
        let _scratch = strata_pixels::memory::MemoryPool::global()
            .reserve(
                strata_pixels::memory::MemoryKind::Transient,
                image.pixels().byte_len() as u64 * 2 + 8 * 1024 * 1024,
            )
            .map_err(render_error)?;
        let directory = tempfile::tempdir().map_err(render_error)?;
        let path = directory.path().join("clipboard.tiff");
        strata_io::export::export_image(
            &path,
            strata_io::export::ExportFormat::Tiff,
            w,
            h,
            image.pixels(),
            image.icc(),
            false,
        )
        .map_err(render_error)?;
        let data = std::fs::read(path).map_err(render_error)?;
        Ok(ClipboardTransfer {
            data,
            origin_x: x,
            origin_y: y,
        })
    }
    pub fn clipboard_import(
        &self,
        data: Vec<u8>,
        origin_x: i64,
        origin_y: i64,
        turn_id: Option<String>,
    ) -> Result<String, StrataError> {
        let owner = source(self, turn_id)?;
        if data.is_empty()
            || data.len() as u64 > strata_core::MAX_FLAT_IMAGE_BYTES + 16 * 1024 * 1024
        {
            return Err(render_error("clipboard byte limit exceeded"));
        }
        let mut temporary = tempfile::NamedTempFile::new().map_err(render_error)?;
        temporary.write_all(&data).map_err(render_error)?;
        drop(data);
        let bytes = strata_io::decoded_bytes(temporary.path(), strata_io::DEFAULT_DECODE_LIMIT)
            .map_err(render_error)?;
        let pool = strata_pixels::memory::MemoryPool::global();
        let _scratch = pool
            .reserve(
                strata_pixels::memory::MemoryKind::Transient,
                bytes * 2 + 8 * 1024 * 1024,
            )
            .map_err(render_error)?;
        let decoded = strata_io::decode_image(temporary.path()).map_err(render_error)?;
        let icc = if let Some(icc) = decoded.icc_profile {
            let profile = strata_color::resolve_profile(Some(&icc)).map_err(render_error)?;
            if profile.fallback {
                return Err(render_error("clipboard ICC invalid"));
            }
            icc
        } else {
            strata_color::working_profile("srgb").map_err(render_error)?
        };
        let image = strata_core::ClipboardImage::new(
            decoded.pixels,
            decoded.width,
            decoded.height,
            origin_x,
            origin_y,
            icc,
            &pool,
        )
        .map_err(command_error)?;
        self.lock()?
            .editor
            .grant_clipboard_image(image, owner)
            .map_err(command_error)
    }
    pub fn clipboard_confirm_write(
        &self,
        resource: String,
        turn_id: Option<String>,
    ) -> Result<(), StrataError> {
        let owner = source(self, turn_id)?;
        {
            let mut inner = self.lock()?;
            inner
                .editor
                .confirm_clipboard_write(&resource, &owner)
                .map_err(command_error)?;
            let state = inner.editor.snapshot();
            inner.events.push_back(EngineEventDto::StateChanged {
                snapshot_json: json!(state).to_string(),
            });
        }
        self.deliver_events()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Listener;
    impl EngineListener for Listener {
        fn on_event(&self, _: EngineEventDto) {}
    }
    #[test]
    fn ffi_tiff_clipboard_preserves_all_16bit_codes_and_rejects_wrong_origin_owner() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":2,"height":2,"bit_depth":16,"background":[32768,32769,50001,12345]})
                    .to_string(),
            )
            .unwrap();
        let copy: Value =
            serde_json::from_str(&engine.dispatch("edit.copy".into(), "{}".into()).unwrap())
                .unwrap();
        let resource = copy["clipboard_resource"].as_str().unwrap().to_owned();
        let data = engine.clipboard_export(resource.clone(), None).unwrap();
        let second = StrataEngine::new(Arc::new(Listener));
        second
            .dispatch(
                "file.new".into(),
                json!({"width":2,"height":2,"bit_depth":16}).to_string(),
            )
            .unwrap();
        let imported = second.clipboard_import(data.data, -3, 5, None).unwrap();
        let image = second
            .lock()
            .unwrap()
            .editor
            .clipboard_image(&imported, &Source::User)
            .unwrap();
        assert_eq!(
            image.pixels(),
            &strata_pixels::PixelBuffer::Rgba16([32768, 32769, 50001, 12345].repeat(4))
        );
        assert_eq!(image.origin(), (-3, 5));
        assert!(second.clipboard_export(resource, None).is_err());
        assert!(
            second
                .clipboard_import(vec![0, 1, 2, 3], 0, 0, None)
                .is_err()
        );
    }
}
