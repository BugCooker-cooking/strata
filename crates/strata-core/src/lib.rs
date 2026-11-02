//! Strata document model and events. M0: version and image documents only.

use strata_pixels::{PixelBuffer, PixelError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub source_profile: String,
    pub profile_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    DocumentOpened(DocumentInfo),
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: EngineEvent);
}

/// Validated immutable source pixels. Display conversion never replaces this data.
pub struct ImageDocument {
    info: DocumentInfo,
    pixels: PixelBuffer,
    icc_profile: Option<Vec<u8>>,
}

impl ImageDocument {
    pub fn new(
        width: u32,
        height: u32,
        pixels: PixelBuffer,
        icc_profile: Option<Vec<u8>>,
        source_profile: String,
        profile_fallback: bool,
    ) -> Result<Self, PixelError> {
        pixels.validate(width, height)?;
        let info = DocumentInfo {
            width,
            height,
            bit_depth: pixels.format().bit_depth(),
            source_profile,
            profile_fallback,
        };
        Ok(Self {
            info,
            pixels,
            icc_profile,
        })
    }
    pub fn info(&self) -> &DocumentInfo {
        &self.info
    }
    pub fn pixels(&self) -> &PixelBuffer {
        &self.pixels
    }
    pub fn icc_profile(&self) -> Option<&[u8]> {
        self.icc_profile.as_deref()
    }
}

#[derive(Default)]
pub struct Engine {
    document: Option<ImageDocument>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }
    /// M0 bootstrap only. M1 moves this operation behind the command executor.
    /// Returns an event for delivery AFTER the host's transaction lock is released.
    pub fn set_document(&mut self, doc: ImageDocument) -> EngineEvent {
        let event = EngineEvent::DocumentOpened(doc.info.clone());
        self.document = Some(doc);
        event
    }
    pub fn document(&self) -> Option<&ImageDocument> {
        self.document.as_ref()
    }
}

/// The version of the portable core.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_pixels::PixelBuffer;

    fn doc(w: u32, h: u32) -> ImageDocument {
        ImageDocument::new(
            w,
            h,
            PixelBuffer::Rgba8(vec![0; (w * h * 4) as usize]),
            None,
            "sRGB (assumed)".into(),
            false,
        )
        .unwrap()
    }

    #[test]
    fn version_matches_cargo_package() {
        assert_eq!(super::version(), "0.0.1");
    }

    #[test]
    fn set_document_returns_event_after_commit() {
        let mut engine = Engine::new();
        assert!(engine.document().is_none());
        let event = engine.set_document(doc(4, 3));
        assert_eq!(
            event,
            EngineEvent::DocumentOpened(engine.document().unwrap().info().clone())
        );
    }

    #[test]
    fn second_document_replaces_first() {
        let mut engine = Engine::new();
        engine.set_document(doc(4, 3));
        engine.set_document(doc(8, 6));
        assert_eq!(engine.document().unwrap().info().width, 8);
    }

    #[test]
    fn document_requires_valid_pixels_and_derives_bit_depth() {
        assert!(
            ImageDocument::new(
                2,
                2,
                PixelBuffer::Rgba8(vec![0; 3]),
                None,
                "sRGB".into(),
                false
            )
            .is_err()
        );
        let pixels = PixelBuffer::Rgba16(vec![1, 32768, 32769, 65535]);
        let document = ImageDocument::new(
            1,
            1,
            pixels.clone(),
            Some(vec![1, 2, 3]),
            "P3".into(),
            false,
        )
        .unwrap();
        assert_eq!(document.info().bit_depth, 16);
        assert_eq!(document.pixels(), &pixels);
        assert_eq!(document.icc_profile(), Some([1, 2, 3].as_slice()));
    }
}
