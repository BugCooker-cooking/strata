//! Versioned single-file SQLite container. No platform UI dependencies.
use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    path::Path,
};

const APP_ID: i64 = 0x53545241;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_METADATA: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectArchive {
    pub metadata: Value,
    pub tiles: Vec<ProjectTile>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectTile {
    pub layer: String,
    pub x: i64,
    pub y: i64,
    pub bit_depth: u8,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("invalid or unsupported project: {0}")]
    Invalid(String),
    #[error("destination already exists; explicit overwrite permission required")]
    Exists,
    #[error("published, but durability could not be confirmed: {0}")]
    PublishedUncertain(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStage {
    BeforeWrite,
    AfterTiles,
    BeforeCommit,
    AfterCommit,
    BeforeVerify,
    BeforePublish,
    AfterPublish,
}

fn connection(path: &Path, read_only: bool) -> Result<Connection, StorageError> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    conn.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;",
    )?;
    if read_only {
        conn.execute_batch("PRAGMA query_only=ON;")?;
    } else {
        conn.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA fullfsync=ON;",
        )?;
    }
    Ok(conn)
}

fn validate(archive: &ProjectArchive) -> Result<Vec<u8>, StorageError> {
    let metadata = serde_json::to_vec(&archive.metadata)?;
    if metadata.len() > MAX_METADATA || archive.tiles.len() > 2048 {
        return Err(StorageError::Invalid("project limits".into()));
    }
    let mut unique = std::collections::HashSet::new();
    let mut total = 0u64;
    for t in &archive.tiles {
        let expected = match t.bit_depth {
            8 => 256 * 256 * 4,
            16 => 256 * 256 * 8,
            _ => return Err(StorageError::Invalid("tile depth".into())),
        };
        if t.bytes.len() != expected || t.layer.len() > 128 || !unique.insert((&t.layer, t.x, t.y))
        {
            return Err(StorageError::Invalid(
                "tile data or duplicate coordinate".into(),
            ));
        }
        total += t.bytes.len() as u64;
    }
    if total > MAX_BYTES {
        return Err(StorageError::Invalid("pixel budget".into()));
    }
    Ok(metadata)
}

pub fn read_project(path: &Path) -> Result<ProjectArchive, StorageError> {
    read_project_inner(path, true)
}
fn read_project_inner(path: &Path, recover: bool) -> Result<ProjectArchive, StorageError> {
    if fs::metadata(path)?.len() > MAX_BYTES + 64 * 1024 * 1024 {
        return Err(StorageError::Invalid("file size".into()));
    }
    let mut journal_name = path.as_os_str().to_os_string();
    journal_name.push("-journal");
    let journal = std::path::PathBuf::from(journal_name);
    if recover && journal.exists() && fs::metadata(&journal)?.len() > 0 {
        // Opening is a read capability. Recover a private copy of a hot rollback
        // journal rather than writing to the user's original during file.open.
        if fs::metadata(&journal)?.len() > MAX_BYTES + 64 * 1024 * 1024 {
            return Err(StorageError::Invalid("journal size".into()));
        }
        let dir = tempfile::tempdir()?;
        let copy = dir.path().join("recovery.strata");
        fs::copy(path, &copy)?;
        fs::copy(&journal, dir.path().join("recovery.strata-journal"))?;
        let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let _: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
        conn.close().map_err(|(_, e)| StorageError::Sql(e))?;
        return read_project_inner(&copy, false);
    }
    let conn = connection(path, true)?;
    let app: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if app != APP_ID || version != 0 {
        return Err(StorageError::Invalid("signature or version".into()));
    }
    let integrity: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(StorageError::Invalid("database integrity".into()));
    }
    let size: i64 = conn.query_row("SELECT length(data) FROM metadata WHERE id=1", [], |r| {
        r.get(0)
    })?;
    if !(0..=MAX_METADATA as i64).contains(&size) {
        return Err(StorageError::Invalid("metadata size".into()));
    }
    let (bytes, hash): (Vec<u8>, Vec<u8>) =
        conn.query_row("SELECT data,hash FROM metadata WHERE id=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    if Sha256::digest(&bytes).as_slice() != hash {
        return Err(StorageError::Invalid("metadata checksum".into()));
    }
    let (count, total): (i64, i64) = conn.query_row(
        "SELECT count(*),coalesce(sum(length(data)),0) FROM tiles",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if !(0..=2048).contains(&count) || !(0..=MAX_BYTES as i64).contains(&total) {
        return Err(StorageError::Invalid("tile budget".into()));
    }
    // Read only validated fixed-size blobs; a corrupted row cannot allocate an arbitrary payload.
    let bad:i64=conn.query_row("SELECT count(*) FROM tiles WHERE depth NOT IN (8,16) OR length(data) != 256*256*4*(depth/8) OR length(layer)>128 OR length(hash)!=32",[],|r|r.get(0))?;
    if bad != 0 {
        return Err(StorageError::Invalid("tile encoding".into()));
    }
    let mut statement =
        conn.prepare("SELECT layer,x,y,depth,data,hash FROM tiles ORDER BY layer,x,y")?;
    let mut rows = statement.query([])?;
    let mut tiles = Vec::new();
    while let Some(row) = rows.next()? {
        let bytes: Vec<u8> = row.get(4)?;
        let hash: Vec<u8> = row.get(5)?;
        if Sha256::digest(&bytes).as_slice() != hash {
            return Err(StorageError::Invalid("tile checksum".into()));
        }
        tiles.push(ProjectTile {
            layer: row.get(0)?,
            x: row.get(1)?,
            y: row.get(2)?,
            bit_depth: row.get(3)?,
            bytes,
        });
    }
    let archive = ProjectArchive {
        metadata: serde_json::from_slice(&bytes)?,
        tiles,
    };
    validate(&archive)?;
    Ok(archive)
}

fn write_transaction(
    path: &Path,
    archive: &ProjectArchive,
    published: bool,
    hook: &mut impl FnMut(SaveStage) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let data = validate(archive)?;
    hook(SaveStage::BeforeWrite)?;
    let mut conn = connection(path, false)?;
    let tx = conn.transaction()?;
    tx.execute_batch("PRAGMA application_id=1398035009; PRAGMA user_version=0;
      CREATE TABLE IF NOT EXISTS metadata(id INTEGER PRIMARY KEY CHECK(id=1),data BLOB NOT NULL,hash BLOB NOT NULL);
      CREATE TABLE IF NOT EXISTS tiles(layer TEXT NOT NULL,x INTEGER NOT NULL,y INTEGER NOT NULL,depth INTEGER NOT NULL,data BLOB NOT NULL,hash BLOB NOT NULL,PRIMARY KEY(layer,x,y));
      CREATE TEMP TABLE wanted(layer TEXT,x INTEGER,y INTEGER,PRIMARY KEY(layer,x,y));")?;
    tx.execute("INSERT INTO metadata VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data,hash=excluded.hash",params![data,Sha256::digest(&data).as_slice()])?;
    for t in &archive.tiles {
        tx.execute(
            "INSERT INTO wanted VALUES(?1,?2,?3)",
            params![t.layer, t.x, t.y],
        )?;
        tx.execute("INSERT INTO tiles VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(layer,x,y) DO UPDATE SET depth=excluded.depth,data=excluded.data,hash=excluded.hash WHERE tiles.hash!=excluded.hash OR tiles.depth!=excluded.depth",params![t.layer,t.x,t.y,t.bit_depth,t.bytes,Sha256::digest(&t.bytes).as_slice()])?;
    }
    tx.execute("DELETE FROM tiles WHERE NOT EXISTS(SELECT 1 FROM wanted WHERE wanted.layer=tiles.layer AND wanted.x=tiles.x AND wanted.y=tiles.y)",[])?;
    hook(SaveStage::AfterTiles)?;
    hook(SaveStage::BeforeCommit)?;
    tx.commit()?;
    let after_commit = |e: StorageError| {
        if published {
            StorageError::PublishedUncertain(e.to_string())
        } else {
            e
        }
    };
    hook(SaveStage::AfterCommit).map_err(&after_commit)?;
    conn.close()
        .map_err(|(_, e)| after_commit(StorageError::Sql(e)))?;
    Ok(())
}

/// Save existing matching projects incrementally, or publish a new standalone
/// database atomically. Callers bind this to an explicit host write capability.
pub fn write_project(
    path: &Path,
    archive: &ProjectArchive,
    overwrite: bool,
    incremental: bool,
) -> Result<(), StorageError> {
    write_project_with_hook(path, archive, overwrite, incremental, |_| Ok(()))
}
pub fn write_project_with_hook(
    path: &Path,
    archive: &ProjectArchive,
    overwrite: bool,
    incremental: bool,
    mut hook: impl FnMut(SaveStage) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    validate(archive)?;
    if path.exists() {
        if !overwrite {
            return Err(StorageError::Exists);
        }
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(StorageError::Invalid("refusing symlink destination".into()));
        }
        if incremental {
            let old = read_project(path)?;
            if old.metadata.get("id") != archive.metadata.get("id") {
                return Err(StorageError::Invalid("destination document changed".into()));
            }
            write_transaction(path, archive, true, &mut hook)?;
            sync_parent(path).map_err(|e| StorageError::PublishedUncertain(e.to_string()))?;
            return Ok(());
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = tempfile::NamedTempFile::new_in(parent)?;
    write_transaction(temp.path(), archive, false, &mut hook)?;
    hook(SaveStage::BeforeVerify)?;
    read_project(temp.path())?;
    temp.as_file().sync_all()?;
    hook(SaveStage::BeforePublish)?;
    if overwrite {
        temp.persist(path).map_err(|e| e.error)?;
    } else {
        temp.persist_noclobber(path).map_err(|e| e.error)?;
    }
    hook(SaveStage::AfterPublish)
        .and_then(|_| sync_parent(path))
        .map_err(|e| StorageError::PublishedUncertain(e.to_string()))?;
    Ok(())
}

pub(crate) fn sync_parent(path: &Path) -> Result<(), StorageError> {
    File::open(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?
    .sync_all()?;
    Ok(())
}
