//! Private, atomic recovery copies. Never modify an original project and never
//! interpret an original_path as a read or overwrite capability.
use crate::project::{
    ProjectSource, StorageError, read_project_metadata, sync_parent, write_project_source,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryInfo {
    pub version: u32,
    pub session_id: String,
    pub label: String,
    pub original_path: Option<String>,
    pub saved_unix_seconds: u64,
    pub revision: u64,
}
impl RecoveryInfo {
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.version != 1
            || !valid_id(&self.session_id)
            || self.label.len() > 1024
            || self.original_path.as_ref().is_some_and(|s| s.len() > 16384)
        {
            return Err(StorageError::Invalid("recovery metadata".into()));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct RecoveryEntry {
    pub id: String,
    pub path: PathBuf,
    pub info: Option<RecoveryInfo>,
    pub error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct RecoveryStore {
    root: PathBuf,
}
fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
impl RecoveryStore {
    /// Trusted host chooses an app-owned directory, outside the model catalog.
    pub fn new(root: &Path) -> Result<Self, StorageError> {
        fs::create_dir_all(root)?;
        if !fs::symlink_metadata(root)?.is_dir() {
            return Err(StorageError::Invalid(
                "recovery root must be a directory".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            root: fs::canonicalize(root)?,
        })
    }
    pub fn path(&self, id: &str) -> Result<PathBuf, StorageError> {
        if !valid_id(id) || !fs::symlink_metadata(&self.root)?.is_dir() {
            return Err(StorageError::Invalid(
                "recovery identifier or directory".into(),
            ));
        }
        Ok(self.root.join(format!("{id}.strata")))
    }
    pub fn write(
        &self,
        info: &RecoveryInfo,
        source: &impl ProjectSource,
    ) -> Result<(), StorageError> {
        info.validate()?;
        let path = self.path(&info.session_id)?;
        if !path.exists() && self.list()?.len() >= 128 {
            return Err(StorageError::Invalid("recovery entry limit".into()));
        }
        // First checkpoint/document replacement is atomically published. Later
        // checkpoints update changed blobs in a FULL-sync SQLite transaction;
        // rollback journals retain the last committed copy after a crash.
        let metadata = source.metadata()?;
        let incremental = path.exists()
            && read_project_metadata(&path).is_ok_and(|old| {
                old.get("id").is_some()
                    && old.get("id") == metadata.get("id")
                    && old.pointer("/recovery/session_id")
                        == metadata.pointer("/recovery/session_id")
            });
        write_project_source(&path, source, true, incremental)
    }
    pub fn list(&self) -> Result<Vec<RecoveryEntry>, StorageError> {
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.root)? {
            let item = item?;
            if !item.file_type()?.is_file() {
                continue;
            }
            let path = item.path();
            if path.extension().and_then(|s| s.to_str()) != Some("strata") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            if !valid_id(&id) {
                continue;
            }
            if entries.len() >= 128 {
                return Err(StorageError::Invalid("too many recovery entries".into()));
            }
            let result = read_project_metadata(&path).and_then(|metadata| {
                let info: RecoveryInfo =
                    serde_json::from_value(metadata.get("recovery").cloned().ok_or_else(
                        || StorageError::Invalid("missing recovery metadata".into()),
                    )?)?;
                info.validate()?;
                if info.session_id != id {
                    return Err(StorageError::Invalid("recovery identifier mismatch".into()));
                }
                Ok(info)
            });
            let (info, error) = match result {
                Ok(info) => (Some(info), None),
                Err(e) => (None, Some(e.to_string())),
            };
            entries.push(RecoveryEntry {
                id,
                path,
                info,
                error,
            });
        }
        entries.sort_by_key(|e| {
            std::cmp::Reverse(e.info.as_ref().map_or(0, |i| i.saved_unix_seconds))
        });
        Ok(entries)
    }
    pub fn remove(&self, id: &str) -> Result<(), StorageError> {
        let path = self.path(id)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        // A crash-recovered private copy intentionally leaves the source hot
        // journal untouched. Explicit discard removes that session's sidecar.
        let mut journal = path.as_os_str().to_os_string();
        journal.push("-journal");
        match fs::remove_file(PathBuf::from(journal)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        sync_parent(&path)
    }
}
