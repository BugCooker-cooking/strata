use serde::Serialize;
#[derive(Clone, Debug, Default, Serialize)]
pub struct ProcessMetrics {
    pub peak_rss_bytes: Option<u64>,
    pub resident_bytes: Option<u64>,
    pub physical_footprint_bytes: Option<u64>,
    pub disk_read_bytes: Option<u64>,
    pub disk_write_bytes: Option<u64>,
}
pub fn process_metrics() -> ProcessMetrics {
    let mut result = ProcessMetrics::default();
    #[cfg(unix)]
    // SAFETY: getrusage writes exactly the initialized, correctly sized struct.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0 {
            let factor = if cfg!(target_os = "macos") { 1 } else { 1024 };
            result.peak_rss_bytes = u64::try_from(usage.ru_maxrss)
                .ok()
                .and_then(|v| v.checked_mul(factor));
        }
    }
    #[cfg(target_os = "macos")]
    // SAFETY: the flavor and buffer are the matching libproc v2 ABI; querying
    // our own process does not inspect another application or document.
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        if libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        ) == 0
        {
            result.resident_bytes = Some(info.ri_resident_size);
            result.physical_footprint_bytes = Some(info.ri_phys_footprint);
            result.disk_read_bytes = Some(info.ri_diskio_bytesread);
            result.disk_write_bytes = Some(info.ri_diskio_byteswritten);
        }
    }
    result
}
