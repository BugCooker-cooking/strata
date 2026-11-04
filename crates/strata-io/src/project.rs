//! Versioned single-file SQLite container. No platform UI dependencies.
use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    path::Path,
};

const APP_ID: i64 = 0x53545241;
pub const MAX_PROJECT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_BYTES: u64 = MAX_PROJECT_BYTES;
const MAX_TILES: usize = 65536;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
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
    /// v0 implicitly had four channels. v1 can also store compact coverage planes.
    pub channels: u8,
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
        "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA temp_store=FILE; PRAGMA cache_size=-8192; PRAGMA mmap_size=0;",
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
    if metadata.len() > MAX_METADATA || archive.tiles.len() > MAX_TILES {
        return Err(StorageError::Invalid("project limits".into()));
    }
    let mut unique = std::collections::HashSet::new();
    let mut total = 0u64;
    for t in &archive.tiles {
        let expected = tile_bytes(t.bit_depth, t.channels)?;
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

/// Bounded discovery only. Opening a document still verifies every tile.
pub fn read_project_metadata(path: &Path) -> Result<Value, StorageError> {
    let mut journal = path.as_os_str().to_os_string();
    journal.push("-journal");
    if fs::metadata(std::path::PathBuf::from(journal)).is_ok_and(|m| m.len() > 0) {
        return read_project_stream(path, |_, _| Ok(()));
    }
    let conn = connection(path, true)?;
    let app: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if app != APP_ID || !matches!(version, 0 | 1) {
        return Err(StorageError::Invalid("signature or version".into()));
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
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn read_project(path: &Path) -> Result<ProjectArchive, StorageError> {
    let mut tiles = Vec::new();
    let mut total = 0u64;
    let metadata = read_project_stream(path, |_, tile| {
        if let Some(tile) = tile {
            total += tile.bytes.len() as u64;
            if total > MAX_ARCHIVE_BYTES {
                return Err(StorageError::Invalid(
                    "use streaming project reader for large files".into(),
                ));
            }
            tiles.push(tile);
        }
        Ok(())
    })?;
    Ok(ProjectArchive { metadata, tiles })
}

/// Metadata arrives first (tile=None), followed by one checked block at a time.
pub fn read_project_stream(
    path: &Path,
    mut visit: impl FnMut(&Value, Option<ProjectTile>) -> Result<(), StorageError>,
) -> Result<Value, StorageError> {
    read_project_inner(path, true, &mut visit)
}
fn read_project_inner(
    path: &Path,
    recover: bool,
    visit: &mut impl FnMut(&Value, Option<ProjectTile>) -> Result<(), StorageError>,
) -> Result<Value, StorageError> {
    if fs::metadata(path)?.len() > MAX_BYTES + 256 * 1024 * 1024 {
        return Err(StorageError::Invalid("file size".into()));
    }
    let mut journal_name = path.as_os_str().to_os_string();
    journal_name.push("-journal");
    let journal = std::path::PathBuf::from(journal_name);
    if recover && journal.exists() && fs::metadata(&journal)?.len() > 0 {
        // Opening is a read capability. Recover a private copy of a hot rollback
        // journal rather than writing to the user's original during file.open.
        if fs::metadata(&journal)?.len() > MAX_BYTES + 256 * 1024 * 1024 {
            return Err(StorageError::Invalid("journal size".into()));
        }
        let dir = tempfile::tempdir()?;
        let copy = dir.path().join("recovery.strata");
        fs::copy(path, &copy)?;
        fs::copy(&journal, dir.path().join("recovery.strata-journal"))?;
        let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let _: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
        conn.close().map_err(|(_, e)| StorageError::Sql(e))?;
        return read_project_inner(&copy, false, visit);
    }
    let conn = connection(path, true)?;
    let app: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if app != APP_ID || !matches!(version, 0 | 1) {
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
    if !(0..=MAX_TILES as i64).contains(&count) || !(0..=MAX_BYTES as i64).contains(&total) {
        return Err(StorageError::Invalid("tile budget".into()));
    }
    // Read only validated fixed-size blobs; a corrupted row cannot allocate an arbitrary payload.
    let channels = if version == 0 { "4" } else { "channels" };
    let bad:i64=conn.query_row(&format!("SELECT count(*) FROM tiles WHERE depth NOT IN (8,16) OR {channels} NOT IN (1,4) OR length(data) != 256*256*{channels}*(depth/8) OR length(layer)>128 OR length(hash)!=32"),[],|r|r.get(0))?;
    if bad != 0 {
        return Err(StorageError::Invalid("tile encoding".into()));
    }
    let mut statement = conn.prepare(&format!(
        "SELECT layer,x,y,depth,data,hash,{channels} FROM tiles ORDER BY layer,x,y"
    ))?;
    let mut rows = statement.query([])?;
    let metadata = serde_json::from_slice(&bytes)?;
    visit(&metadata, None)?;
    while let Some(row) = rows.next()? {
        let bytes: Vec<u8> = row.get(4)?;
        let hash: Vec<u8> = row.get(5)?;
        if Sha256::digest(&bytes).as_slice() != hash {
            return Err(StorageError::Invalid("tile checksum".into()));
        }
        visit(
            &metadata,
            Some(ProjectTile {
                layer: row.get(0)?,
                x: row.get(1)?,
                y: row.get(2)?,
                bit_depth: row.get(3)?,
                channels: row.get(6)?,
                bytes,
            }),
        )?;
    }
    Ok(metadata)
}

/// A repeatable immutable snapshot, consumed one tile at a time.
pub trait ProjectSource {
    fn metadata(&self) -> Result<Value, StorageError>;
    fn visit_tiles(
        &self,
        visit: &mut dyn FnMut(ProjectTile) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>;
}
impl ProjectSource for ProjectArchive {
    fn metadata(&self) -> Result<Value, StorageError> {
        Ok(self.metadata.clone())
    }
    fn visit_tiles(
        &self,
        visit: &mut dyn FnMut(ProjectTile) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        for tile in &self.tiles {
            visit(tile.clone())?;
        }
        Ok(())
    }
}
fn write_transaction(
    path: &Path,
    source: &impl ProjectSource,
    data: &[u8],
    published: bool,
    hook: &mut impl FnMut(SaveStage) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    hook(SaveStage::BeforeWrite)?;
    let mut conn = connection(path, false)?;
    let tx = conn.transaction()?;
    tx.execute_batch("PRAGMA application_id=1398035009;
      CREATE TABLE IF NOT EXISTS metadata(id INTEGER PRIMARY KEY CHECK(id=1),data BLOB NOT NULL,hash BLOB NOT NULL);
      CREATE TABLE IF NOT EXISTS tiles(layer TEXT NOT NULL,x INTEGER NOT NULL,y INTEGER NOT NULL,depth INTEGER NOT NULL,data BLOB NOT NULL,hash BLOB NOT NULL,channels INTEGER NOT NULL DEFAULT 4,PRIMARY KEY(layer,x,y));
      CREATE TEMP TABLE wanted(layer TEXT,x INTEGER,y INTEGER,PRIMARY KEY(layer,x,y));")?;
    let has_channels: i64 = tx.query_row(
        "SELECT count(*) FROM pragma_table_info('tiles') WHERE name='channels'",
        [],
        |r| r.get(0),
    )?;
    if has_channels == 0 {
        tx.execute_batch("ALTER TABLE tiles ADD COLUMN channels INTEGER NOT NULL DEFAULT 4;")?;
    }
    tx.execute_batch("PRAGMA user_version=1;")?;
    tx.execute("INSERT INTO metadata VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data,hash=excluded.hash",params![data,Sha256::digest(data).as_slice()])?;
    let mut total = 0u64;
    let mut count = 0usize;
    source.visit_tiles(&mut |t| {
        let expected = tile_bytes(t.bit_depth,t.channels)?;
        total += t.bytes.len() as u64; count += 1;
        if t.bytes.len() != expected || total > MAX_BYTES || count > MAX_TILES || t.layer.len() > 128 { return Err(StorageError::Invalid("tile budget or encoding".into())); }
        tx.execute(
            "INSERT INTO wanted VALUES(?1,?2,?3)",
            params![t.layer, t.x, t.y],
        )?;
        tx.execute("INSERT INTO tiles(layer,x,y,depth,data,hash,channels) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(layer,x,y) DO UPDATE SET depth=excluded.depth,data=excluded.data,hash=excluded.hash,channels=excluded.channels WHERE tiles.hash!=excluded.hash OR tiles.depth!=excluded.depth OR tiles.channels!=excluded.channels",params![t.layer,t.x,t.y,t.bit_depth,t.bytes,Sha256::digest(&t.bytes).as_slice(),t.channels])?;
        Ok(())
    })?;
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

fn tile_bytes(depth: u8, channels: u8) -> Result<usize, StorageError> {
    if !matches!(depth, 8 | 16) || !matches!(channels, 1 | 4) {
        return Err(StorageError::Invalid("tile depth or channels".into()));
    }
    Ok(256 * 256 * usize::from(channels) * usize::from(depth / 8))
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
    hook: impl FnMut(SaveStage) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    validate(archive)?;
    write_project_source_with_hook(path, archive, overwrite, incremental, hook)
}

pub fn write_project_source(
    path: &Path,
    source: &impl ProjectSource,
    overwrite: bool,
    incremental: bool,
) -> Result<(), StorageError> {
    write_project_source_with_hook(path, source, overwrite, incremental, |_| Ok(()))
}
pub fn write_project_source_with_hook(
    path: &Path,
    source: &impl ProjectSource,
    overwrite: bool,
    incremental: bool,
    mut hook: impl FnMut(SaveStage) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let metadata = source.metadata()?;
    let data = serde_json::to_vec(&metadata)?;
    if data.len() > MAX_METADATA {
        return Err(StorageError::Invalid("metadata limit".into()));
    }
    if path.exists() {
        if !overwrite {
            return Err(StorageError::Exists);
        }
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(StorageError::Invalid("refusing symlink destination".into()));
        }
        if incremental {
            let old = read_project_stream(path, |_, _| Ok(()))?;
            if old.get("id") != metadata.get("id") {
                return Err(StorageError::Invalid("destination document changed".into()));
            }
            write_transaction(path, source, &data, true, &mut hook)?;
            sync_parent(path).map_err(|e| StorageError::PublishedUncertain(e.to_string()))?;
            return Ok(());
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = tempfile::NamedTempFile::new_in(parent)?;
    write_transaction(temp.path(), source, &data, false, &mut hook)?;
    hook(SaveStage::BeforeVerify)?;
    read_project_stream(temp.path(), |_, _| Ok(()))?;
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
