/// Page counts from `host_statistics64` and the page size in bytes.
#[derive(Clone, Copy, Debug)]
pub struct VmStatistics {
    pub page_size: u64,
    pub free: u64,
    pub active: u64,
    pub inactive: u64,
    pub speculative: u64,
    pub wired: u64,
    pub purgeable: u64,
}

impl VmStatistics {
    /// Bytes available without reclaiming wired or active memory.
    pub fn available_bytes(&self) -> u64 {
        (self.free + self.inactive + self.speculative + self.purgeable) * self.page_size
    }
}

pub fn vm_statistics() -> Option<VmStatistics> {
    let mut data = unsafe { std::mem::zeroed::<libc::vm_statistics64>() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    #[allow(deprecated)]
    let host = unsafe { libc::mach_host_self() };
    let status = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&mut data as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    if status != 0 {
        return None;
    }
    let page_size = super::sysctl::unsigned("hw.pagesize")?;
    // The structure is packed, so each field is copied out by value.
    let (free, active, inactive, wired) = (
        data.free_count,
        data.active_count,
        data.inactive_count,
        data.wire_count,
    );
    let (speculative, purgeable) = (data.speculative_count, data.purgeable_count);
    Some(VmStatistics {
        page_size,
        free: free as u64,
        active: active as u64,
        inactive: inactive as u64,
        speculative: speculative as u64,
        wired: wired as u64,
        purgeable: purgeable as u64,
    })
}

/// Cumulative CPU ticks across all processors: user, system, idle, nice.
pub fn cpu_ticks() -> Option<[u32; 4]> {
    let mut data = unsafe { std::mem::zeroed::<libc::host_cpu_load_info>() };
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    #[allow(deprecated)]
    let host = unsafe { libc::mach_host_self() };
    let status = unsafe {
        libc::host_statistics(
            host,
            libc::HOST_CPU_LOAD_INFO,
            (&mut data as *mut libc::host_cpu_load_info).cast(),
            &mut count,
        )
    };
    (status == 0).then_some(data.cpu_ticks)
}

/// Busy share of all processors over `interval`, in whole percent.
pub fn cpu_load_percent(interval: std::time::Duration) -> Option<u8> {
    let before = cpu_ticks()?;
    std::thread::sleep(interval);
    let after = cpu_ticks()?;
    let delta = |i: usize| after[i].wrapping_sub(before[i]) as u64;
    let total: u64 = (0..4).map(delta).sum();
    if total == 0 {
        return None;
    }
    let idle = delta(2);
    Some(((total - idle) * 100 / total) as u8)
}

/// Number of processes visible to the caller. The size query over-reports, so the count comes
/// from a filled buffer.
pub fn process_count() -> Option<usize> {
    let mut capacity = usize::try_from(unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) })
        .ok()?
        .max(1)
        + 64;
    loop {
        let mut pids = vec![0 as libc::pid_t; capacity];
        let bytes = i32::try_from(capacity * std::mem::size_of::<libc::pid_t>()).ok()?;
        let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let filled = usize::try_from(filled).ok().filter(|n| *n > 0)?;
        if filled < capacity {
            return Some(filled);
        }
        capacity *= 2;
    }
}
