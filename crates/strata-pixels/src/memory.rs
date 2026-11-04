//! Shared, process-wide pixel/cache reservations. A reservation lives as long as
//! the allocation, including pinned readers and staged transactions. Disk-backed
//! source/history blocks and disposable caches register for global LRU eviction.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct MemoryLimits {
    pub cpu_bytes: u64,
    pub gpu_bytes: u64,
    pub disk_bytes: u64,
}
impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            cpu_bytes: 2 * 1024 * 1024 * 1024,
            gpu_bytes: 512 * 1024 * 1024,
            disk_bytes: 32 * 1024 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryKind {
    Source,
    CpuCache,
    Transient,
    Gpu,
    Disk,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryStats {
    pub backing_read_bytes: u64,
    pub backing_write_bytes: u64,
    pub source_bytes: u64,
    pub cpu_cache_bytes: u64,
    pub transient_bytes: u64,
    pub gpu_bytes: u64,
    pub disk_bytes: u64,
    pub cpu_bytes: u64,
    pub peak_cpu_bytes: u64,
    pub peak_gpu_bytes: u64,
    pub peak_disk_bytes: u64,
    pub evictions: u64,
    pub reloads: u64,
}
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("global {kind:?} budget exhausted (requested {bytes} bytes)")]
    Exhausted { kind: MemoryKind, bytes: u64 },
    #[error("pixel backing store failed: {0}")]
    Backing(#[from] std::io::Error),
    #[error("pixel backing checksum mismatch")]
    Corrupt,
}
pub(crate) trait Evictable: Send + Sync {
    fn kind(&self) -> MemoryKind;
    fn used(&self) -> u64;
    fn evict(&self) -> Result<bool, MemoryError>;
}
struct State {
    stats: MemoryStats,
    entries: HashMap<usize, (Weak<dyn Evictable>, u64)>,
}
struct Pool {
    limits: MemoryLimits,
    state: Mutex<State>,
    clock: AtomicU64,
    backing: Mutex<BackingStore>,
}
#[derive(Default)]
struct BackingStore {
    file: Option<std::fs::File>,
    end: u64,
    free: BTreeMap<u64, u64>,
}
pub(crate) struct DiskSlot {
    pool: MemoryPool,
    offset: u64,
    len: u64,
    hash: [u8; 32],
    _reservation: Reservation,
}
impl fmt::Debug for DiskSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiskSlot")
            .field("offset", &self.offset)
            .field("len", &self.len)
            .finish()
    }
}
impl Drop for DiskSlot {
    fn drop(&mut self) {
        let mut backing = self
            .pool
            .0
            .backing
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (mut offset, mut len) = (self.offset, self.len);
        if let Some((&previous, &size)) = backing.free.range(..offset).next_back()
            && previous + size == offset
        {
            backing.free.remove(&previous);
            offset = previous;
            len += size;
        }
        if let Some(&size) = backing.free.get(&(offset + len)) {
            backing.free.remove(&(offset + len));
            len += size;
        }
        if offset + len == backing.end {
            backing.end = offset;
            if let Some(file) = &backing.file {
                let _ = file.set_len(backing.end);
            }
        } else {
            backing.free.insert(offset, len);
        }
    }
}
#[derive(Clone)]
pub struct MemoryPool(Arc<Pool>);
impl fmt::Debug for MemoryPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryPool")
            .field("limits", &self.0.limits)
            .field("stats", &self.stats())
            .finish()
    }
}
impl MemoryPool {
    pub fn new(limits: MemoryLimits) -> Self {
        Self(Arc::new(Pool {
            limits,
            state: Mutex::new(State {
                stats: MemoryStats::default(),
                entries: HashMap::new(),
            }),
            clock: AtomicU64::new(1),
            backing: Mutex::new(BackingStore::default()),
        }))
    }
    pub fn global() -> Self {
        static GLOBAL: OnceLock<MemoryPool> = OnceLock::new();
        GLOBAL
            .get_or_init(|| Self::new(MemoryLimits::default()))
            .clone()
    }
    pub fn limits(&self) -> MemoryLimits {
        self.0.limits
    }
    pub fn stats(&self) -> MemoryStats {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats
            .clone()
    }
    pub(crate) fn tick(&self) -> u64 {
        self.0.clock.fetch_add(1, Ordering::Relaxed)
    }
    pub(crate) fn register(&self, entry: Arc<dyn Evictable>) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.entries.len().is_multiple_of(256) {
            state.entries.retain(|_, (e, _)| e.strong_count() > 0);
        }
        let key = Arc::as_ptr(&entry) as *const () as usize;
        state
            .entries
            .insert(key, (Arc::downgrade(&entry), self.tick()));
    }
    fn evicted(&self, key: usize, generation: u64) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        // A simultaneous reload may already have registered a new generation.
        if state
            .entries
            .get(&key)
            .is_some_and(|(_, g)| *g == generation)
        {
            state.entries.remove(&key);
        }
        state.stats.evictions += 1;
    }
    pub(crate) fn reloaded(&self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats
            .reloads += 1;
    }
    pub(crate) fn spill(&self, bytes: &[u8]) -> Result<DiskSlot, MemoryError> {
        let reservation = self.reserve(MemoryKind::Disk, bytes.len() as u64)?;
        let mut backing = self.0.backing.lock().unwrap_or_else(|e| e.into_inner());
        if backing.file.is_none() {
            backing.file = Some(tempfile::tempfile()?);
        }
        let len = bytes.len() as u64;
        let free = backing
            .free
            .iter()
            .find(|&(_, &size)| size >= len)
            .map(|(&offset, &size)| (offset, size));
        let offset = if let Some((offset, size)) = free {
            backing.free.remove(&offset);
            if size > len {
                backing.free.insert(offset + len, size - len);
            }
            offset
        } else {
            if backing
                .end
                .checked_add(len)
                .is_none_or(|v| v > self.0.limits.disk_bytes)
            {
                return Err(MemoryError::Exhausted {
                    kind: MemoryKind::Disk,
                    bytes: len,
                });
            }
            let offset = backing.end;
            backing.end += len;
            offset
        };
        let file = backing.file.as_mut().expect("backing file initialized");
        let result = file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| file.write_all(bytes));
        if let Err(e) = result {
            backing.free.insert(offset, len);
            return Err(e.into());
        }
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats
            .backing_write_bytes += len;
        Ok(DiskSlot {
            pool: self.clone(),
            offset,
            len,
            hash: Sha256::digest(bytes).into(),
            _reservation: reservation,
        })
    }
    pub(crate) fn reload(&self, slot: &DiskSlot, bytes: &mut [u8]) -> Result<(), MemoryError> {
        let mut backing = self.0.backing.lock().unwrap_or_else(|e| e.into_inner());
        let file = backing
            .file
            .as_mut()
            .expect("live disk slots have a backing file");
        file.seek(SeekFrom::Start(slot.offset))?;
        file.read_exact(bytes)?;
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats
            .backing_read_bytes += bytes.len() as u64;
        if Sha256::digest(bytes).as_slice() != slot.hash {
            return Err(MemoryError::Corrupt);
        }
        Ok(())
    }
    fn try_reserve(&self, kind: MemoryKind, bytes: u64) -> Option<Reservation> {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let s = &mut state.stats;
        let (used, limit) = match kind {
            MemoryKind::Gpu => (s.gpu_bytes, self.0.limits.gpu_bytes),
            MemoryKind::Disk => (s.disk_bytes, self.0.limits.disk_bytes),
            _ => (s.cpu_bytes, self.0.limits.cpu_bytes),
        };
        if used.checked_add(bytes).is_none_or(|v| v > limit) {
            return None;
        }
        match kind {
            MemoryKind::Source => s.source_bytes += bytes,
            MemoryKind::CpuCache => s.cpu_cache_bytes += bytes,
            MemoryKind::Transient => s.transient_bytes += bytes,
            MemoryKind::Gpu => s.gpu_bytes += bytes,
            MemoryKind::Disk => s.disk_bytes += bytes,
        }
        s.cpu_bytes = s.source_bytes + s.cpu_cache_bytes + s.transient_bytes;
        s.peak_cpu_bytes = s.peak_cpu_bytes.max(s.cpu_bytes);
        s.peak_gpu_bytes = s.peak_gpu_bytes.max(s.gpu_bytes);
        s.peak_disk_bytes = s.peak_disk_bytes.max(s.disk_bytes);
        Some(Reservation {
            pool: self.clone(),
            kind,
            bytes,
        })
    }
    pub fn reserve(&self, kind: MemoryKind, bytes: u64) -> Result<Reservation, MemoryError> {
        let limit = match kind {
            MemoryKind::Gpu => self.0.limits.gpu_bytes,
            MemoryKind::Disk => self.0.limits.disk_bytes,
            _ => self.0.limits.cpu_bytes,
        };
        if bytes > limit {
            return Err(MemoryError::Exhausted { kind, bytes });
        }
        if let Some(r) = self.try_reserve(kind, bytes) {
            return Ok(r);
        }
        if kind != MemoryKind::Disk {
            let mut entries: Vec<_> = self
                .0
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entries
                .iter()
                .filter_map(|(&key, (entry, generation))| {
                    entry.upgrade().map(|e| (key, *generation, e))
                })
                .collect();
            entries.retain(|(_, _, e)| {
                if kind == MemoryKind::Gpu {
                    e.kind() == MemoryKind::Gpu
                } else {
                    e.kind() != MemoryKind::Gpu
                }
            });
            // Source admission must not destroy a newer composite merely
            // because it is rebuildable. Cache the atomic timestamp once:
            // concurrent readers may update it while sorting.
            entries.sort_by_cached_key(|(_, _, e)| e.used());
            let mut failure = None;
            for (key, generation, entry) in entries {
                match entry.evict() {
                    Ok(true) => {
                        self.evicted(key, generation);
                    }
                    Ok(false) => {}
                    Err(e) => failure = Some(e),
                }
                if let Some(r) = self.try_reserve(kind, bytes) {
                    return Ok(r);
                }
            }
            if let Some(e) = failure {
                return Err(e);
            }
        }
        Err(MemoryError::Exhausted { kind, bytes })
    }
    /// Release idle caches and spill unpinned source blocks under system pressure.
    pub fn trim(&self) -> Result<(), MemoryError> {
        let entries: Vec<_> = self
            .0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .iter()
            .filter_map(|(&key, (entry, generation))| {
                entry.upgrade().map(|e| (key, *generation, e))
            })
            .collect();
        for (key, generation, entry) in entries {
            if entry.evict()? {
                self.evicted(key, generation);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damaged_backing_is_reported_and_failed_writes_release_the_disk_reservation() {
        let pool = MemoryPool::new(MemoryLimits {
            cpu_bytes: 1024,
            gpu_bytes: 1024,
            disk_bytes: 1024,
        });
        let slot = pool.spill(&[1, 2, 3, 4]).unwrap();
        {
            let mut backing = pool.0.backing.lock().unwrap();
            let file = backing.file.as_mut().unwrap();
            file.seek(SeekFrom::Start(slot.offset)).unwrap();
            file.write_all(&[9]).unwrap();
        }
        assert!(matches!(
            pool.reload(&slot, &mut [0; 4]),
            Err(MemoryError::Corrupt)
        ));
        drop(slot);
        assert_eq!(pool.stats().disk_bytes, 0);
        let temporary = tempfile::NamedTempFile::new().unwrap();
        pool.0.backing.lock().unwrap().file = Some(std::fs::File::open(temporary.path()).unwrap());
        assert!(pool.spill(&[1, 2, 3, 4]).is_err());
        assert_eq!(pool.stats().disk_bytes, 0);
    }
}

#[derive(Debug)]
pub struct Reservation {
    pool: MemoryPool,
    kind: MemoryKind,
    bytes: u64,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut state = self.pool.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let s = &mut state.stats;
        match self.kind {
            MemoryKind::Source => s.source_bytes -= self.bytes,
            MemoryKind::CpuCache => s.cpu_cache_bytes -= self.bytes,
            MemoryKind::Transient => s.transient_bytes -= self.bytes,
            MemoryKind::Gpu => s.gpu_bytes -= self.bytes,
            MemoryKind::Disk => s.disk_bytes -= self.bytes,
        }
        s.cpu_bytes = s.source_bytes + s.cpu_cache_bytes + s.transient_bytes;
    }
}

/// A disposable cached allocation. Eviction never invalidates a pinned reader.
pub struct CacheSlot<T> {
    data: Mutex<Option<Arc<T>>>,
    pool: MemoryPool,
    used: AtomicU64,
    kind: MemoryKind,
}
impl<T: Send + Sync + 'static> CacheSlot<T> {
    pub fn new(pool: &MemoryPool, value: T) -> Arc<Self> {
        Self::from_arc(pool, Arc::new(value))
    }
    pub fn from_arc(pool: &MemoryPool, value: Arc<T>) -> Arc<Self> {
        Self::with_kind(pool, value, MemoryKind::CpuCache)
    }
    pub fn with_kind(pool: &MemoryPool, value: Arc<T>, kind: MemoryKind) -> Arc<Self> {
        let slot = Arc::new(Self {
            data: Mutex::new(Some(value)),
            pool: pool.clone(),
            used: AtomicU64::new(pool.tick()),
            kind,
        });
        pool.register(slot.clone());
        slot
    }
    pub fn get(&self) -> Option<Arc<T>> {
        self.used.store(self.pool.tick(), Ordering::Relaxed);
        self.data.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
impl<T: Send + Sync> Evictable for CacheSlot<T> {
    fn kind(&self) -> MemoryKind {
        self.kind
    }
    fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }
    fn evict(&self) -> Result<bool, MemoryError> {
        if let Ok(mut data) = self.data.try_lock()
            && data.as_ref().is_some_and(|v| Arc::strong_count(v) == 1)
        {
            *data = None;
            return Ok(true);
        }
        Ok(false)
    }
}
