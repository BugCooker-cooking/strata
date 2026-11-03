use crate::{Document, Editor, commands::CommandError};
use serde_json::json;
use strata_io::{
    project::{ProjectSource, ProjectTile, StorageError},
    recovery::{RecoveryInfo, RecoveryStore},
};

struct Checkpoint<'a> {
    document: &'a Document,
    info: &'a RecoveryInfo,
}
impl ProjectSource for Checkpoint<'_> {
    fn metadata(&self) -> Result<serde_json::Value, StorageError> {
        Ok(
            json!({"id":self.document.id(),"recovery":self.info,"document":self.document.metadata()?}),
        )
    }
    fn visit_tiles(
        &self,
        visit: &mut dyn FnMut(ProjectTile) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        self.document.visit_tiles(visit)
    }
}
impl Editor {
    /// Trusted, app-owned checkpoint. Does not grant file resources, change the
    /// saved state, document path, revision, history or external execution log.
    pub fn checkpoint_recovery(
        &self,
        store: &RecoveryStore,
        label: &str,
    ) -> Result<Option<RecoveryInfo>, CommandError> {
        if !self.dirty() {
            return Ok(None);
        }
        let document = self.document().ok_or(CommandError::NoDocument)?;
        let _io = document
            .memory_pool()
            .reserve(
                strata_pixels::memory::MemoryKind::Transient,
                10 * 1024 * 1024,
            )
            .map_err(crate::DocumentError::from)?;
        let info = RecoveryInfo {
            version: 1,
            session_id: self.snapshot().session_id.to_string(),
            label: label.into(),
            original_path: self
                .project_path()
                .map(|p| p.to_string_lossy().into_owned()),
            saved_unix_seconds: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            revision: self.revision(),
        };
        store.write(
            &info,
            &Checkpoint {
                document,
                info: &info,
            },
        )?;
        Ok(Some(info))
    }
}
