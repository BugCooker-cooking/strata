use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use strata_io::{
    export::{ExportFormat, export_image},
    project::{ProjectArchive, StorageError, read_project, write_project},
};

#[derive(Clone, Debug, PartialEq)]
struct FileStamp {
    size: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}
fn stamp(path: &Path) -> Result<Option<FileStamp>, CommandError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(FileStamp {
            size: m.len(),
            modified: m.modified().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (m.dev(), m.ino())
            },
        })),
        Ok(_) => Err(CommandError::Permission),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StorageError::Io(e).into()),
    }
}
#[derive(Clone)]
pub(super) struct Resource {
    path: PathBuf,
    source: Source,
    write: bool,
    overwrite: bool,
    stamp: Option<FileStamp>,
    issued: Instant,
}
pub(super) struct WriteCandidate {
    resource: String,
    path: PathBuf,
    stamp: Option<FileStamp>,
    overwrite: bool,
    state: StateId,
    data: WriteData,
}
enum WriteData {
    Project {
        archive: ProjectArchive,
        incremental: bool,
    },
    Image {
        pixels: PixelBuffer,
        width: u32,
        height: u32,
        icc: Vec<u8>,
        format: ExportFormat,
    },
}

impl Editor {
    /// Trusted host call after a file dialog or explicit CLI resource binding.
    /// This API is never registered in the model command catalog.
    pub fn grant_read(&mut self, path: &Path, source: Source) -> Result<String, CommandError> {
        let path = fs::canonicalize(path).map_err(StorageError::Io)?;
        let saved = stamp(&path)?;
        if saved.is_none() {
            return Err(CommandError::Permission);
        }
        self.grant(Resource {
            path,
            source,
            write: false,
            overwrite: false,
            stamp: saved,
            issued: Instant::now(),
        })
    }
    pub fn grant_write(
        &mut self,
        path: &Path,
        overwrite: bool,
        source: Source,
    ) -> Result<String, CommandError> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = fs::canonicalize(parent).map_err(StorageError::Io)?;
        let path = parent.join(path.file_name().ok_or(CommandError::Permission)?);
        let saved = stamp(&path)?;
        if saved.is_some() && !overwrite {
            return Err(StorageError::Exists.into());
        }
        self.grant(Resource {
            path,
            source,
            write: true,
            overwrite,
            stamp: saved,
            issued: Instant::now(),
        })
    }
    fn grant(&mut self, resource: Resource) -> Result<String, CommandError> {
        self.resources
            .retain(|_, r| r.issued.elapsed() < Duration::from_secs(3600));
        if self.resources.len() >= 1024 {
            return Err(CommandError::Permission);
        }
        let id = Uuid::new_v4().to_string();
        self.resources.insert(id.clone(), resource);
        Ok(id)
    }
    pub(super) fn resource(
        &self,
        id: &str,
        request: &Request,
        write: bool,
    ) -> Result<&Resource, CommandError> {
        let resource = self.resources.get(id).ok_or(CommandError::Permission)?;
        if resource.source != request.source
            || resource.write != write
            || resource.issued.elapsed() >= Duration::from_secs(3600)
            || stamp(&resource.path)? != resource.stamp
        {
            return Err(CommandError::Permission);
        }
        Ok(resource)
    }
    pub(super) fn prepare_open(
        &self,
        request: &Request,
        params: Value,
    ) -> Result<(Document, Option<PathBuf>), CommandError> {
        let p: OpenFile = parse(params)?;
        if self.dirty() && !p.discard_changes {
            return Err(CommandError::UnsavedChanges);
        }
        let resource = self.resource(&p.resource, request, false)?;
        use std::io::Read;
        let mut signature = [0u8; 16];
        let mut file = fs::File::open(&resource.path).map_err(StorageError::Io)?;
        let count = file.read(&mut signature).map_err(StorageError::Io)?;
        if count == 16 && &signature == b"SQLite format 3\0" {
            let doc = Document::from_archive(read_project(&resource.path)?)?;
            return Ok((doc, Some(resource.path.clone())));
        }
        let image = strata_io::decode_image(&resource.path)?;
        let doc = DocumentCandidate::from_image(
            image.width,
            image.height,
            image.pixels,
            image.icc_profile,
            resource
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        )?
        .into_document();
        Ok((doc, None))
    }
    pub(super) fn prepare_place(
        &self,
        request: &Request,
        doc: &mut Document,
        params: Value,
    ) -> Result<(String, Value), CommandError> {
        let p: PlaceFile = parse(params)?;
        if doc.layers.len() >= crate::MAX_LAYERS {
            return Err(DocumentError::ResourceLimit.into());
        }
        let resource = self.resource(&p.resource, request, false)?;
        let image = strata_io::decode_image(&resource.path)?;
        let added = u64::from(image.width.div_ceil(256))
            * u64::from(image.height.div_ceil(256))
            * doc
                .format
                .byte_count(256, 256)
                .map_err(DocumentError::from)? as u64;
        let existing: u64 = doc.layers.iter().map(|l| l.pixels.referenced_bytes()).sum();
        if existing + added > crate::MAX_DOCUMENT_BYTES {
            return Err(DocumentError::ResourceLimit.into());
        }
        let profile = strata_color::resolve_profile(image.icc_profile.as_deref())
            .map_err(DocumentError::from)?;
        // Promote before transforming, quantize only after transforming. Alpha
        // uses the same full-range depth mapping as the RGB samples.
        let source = match (image.pixels, doc.format) {
            (PixelBuffer::Rgba8(v), PixelFormat::Rgba16) => {
                PixelBuffer::Rgba16(v.into_iter().map(|v| u16::from(v) * 257).collect())
            }
            (pixels, _) => pixels,
        };
        let converted = strata_color::convert_to_profile(
            &source,
            image.width,
            image.height,
            &profile.icc,
            &doc.working_icc,
        )
        .map_err(DocumentError::from)?;
        let pixels = match (converted, doc.format) {
            (PixelBuffer::Rgba16(v), PixelFormat::Rgba8) => PixelBuffer::Rgba8(
                v.into_iter()
                    .map(|v| ((u32::from(v) + 128) / 257) as u8)
                    .collect(),
            ),
            (pixels, _) => pixels,
        };
        let name = resource
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut layer = Layer::new(
            name,
            TiledPixels::from_dense(image.width, image.height, &pixels, tile_limit(doc.format))
                .map_err(DocumentError::from)?,
        );
        layer.info.x = p.x;
        layer.info.y = p.y;
        let id = layer.id();
        let index = doc
            .active
            .and_then(|id| doc.layers.iter().position(|l| l.id() == id))
            .map_or(doc.layers.len(), |index| index + 1);
        doc.layers.insert(index, layer);
        doc.active = Some(id);
        self.resource(&p.resource, request, false)?;
        Ok((p.resource, json!({"layer_id":id})))
    }
    pub(super) fn prepare_write(
        &self,
        request: &Request,
        id: &str,
        params: Value,
    ) -> Result<WriteCandidate, CommandError> {
        let doc = self.document.as_ref().ok_or(CommandError::NoDocument)?;
        let (resource_id, export) = if id == "file.export" {
            let p: ExportFile = parse(params)?;
            let plan = export::ExportPlan::new(doc, &p)?;
            (p.resource, Some(plan))
        } else {
            let p: SaveFile = parse(params)?;
            (p.resource, None)
        };
        let resource = self.resource(&resource_id, request, true)?;
        let data = if let Some(export) = export {
            WriteData::Image {
                pixels: export.pixels(doc)?,
                width: doc.frame.width,
                height: doc.frame.height,
                icc: export.icc,
                format: export.format,
            }
        } else {
            WriteData::Project {
                archive: doc.to_archive()?,
                incremental: id == "file.save"
                    && self.project_path.as_ref() == Some(&resource.path),
            }
        };
        Ok(WriteCandidate {
            resource: resource_id,
            path: resource.path.clone(),
            stamp: resource.stamp.clone(),
            overwrite: resource.overwrite,
            state: doc.state_id(),
            data,
        })
    }
    pub(super) fn commit_write(
        &mut self,
        request: &Request,
        id: &str,
        write: WriteCandidate,
    ) -> Result<(), CommandError> {
        self.resource(&write.resource, request, true)?;
        if stamp(&write.path)? != write.stamp {
            return Err(CommandError::Permission);
        }
        let project = matches!(write.data, WriteData::Project { .. });
        let result = match write.data {
            WriteData::Project {
                archive,
                incremental,
            } => write_project(&write.path, &archive, write.overwrite, incremental),
            WriteData::Image {
                pixels,
                width,
                height,
                icc,
                format,
            } => export_image(
                &write.path,
                format,
                width,
                height,
                &pixels,
                &icc,
                write.overwrite,
            ),
        };
        self.executions.push(json!({"command":id,"call_id":request.call_id,"target":write.path,"status":match &result {Ok(_)=>"completed",Err(StorageError::PublishedUncertain(_))=>"durability_uncertain",Err(_)=>"failed"},"undoable":false}));
        if self.executions.len() > 256 {
            self.executions.remove(0);
        }
        // Write capabilities are single-use, including failures of uncertain
        // durability. Re-authorize a target before another attempt.
        self.resources.remove(&write.resource);
        result?;
        if project {
            self.saved = Some(write.state);
            self.project_path = Some(write.path);
        }
        Ok(())
    }
}
