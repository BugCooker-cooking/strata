//! Canvas-axis references are document metadata, never exported image samples.
use crate::{DocumentError, GuideId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_GUIDES: usize = 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuideAxis {
    Horizontal,
    Vertical,
}
impl GuideAxis {
    pub(crate) fn component(self) -> usize {
        match self {
            Self::Vertical => 0,
            Self::Horizontal => 1,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Guide {
    pub id: GuideId,
    #[schemars(length(max = 256))]
    pub name: String,
    pub axis: GuideAxis,
    #[schemars(range(min = -1000000000000.0, max = 1000000000000.0))]
    pub position: f64,
}
impl Guide {
    pub(crate) fn validate(&self) -> Result<(), DocumentError> {
        if !self.id.is_valid()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
            || !self.position.is_finite()
            || self.position.abs() > 1e12
        {
            return Err(DocumentError::Invalid("guide id/name/coordinate".into()));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuideRef {
    Id(GuideId),
    Name(String),
}
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    pub rulers_visible: bool,
    pub guides_visible: bool,
    pub guides_locked: bool,
    pub snap_enabled: bool,
    /// Native pointer tolerance in screen points, converted to canvas by host.
    #[schemars(range(min = 0, max = 64))]
    pub snap_tolerance: f64,
    pub snap_artboard: bool,
    pub version: u64,
}
impl Default for Presentation {
    fn default() -> Self {
        Self {
            rulers_visible: true,
            guides_visible: true,
            guides_locked: false,
            snap_enabled: false,
            snap_tolerance: 6.,
            snap_artboard: true,
            version: 0,
        }
    }
}
