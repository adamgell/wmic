//! Process table readers: `kinfo_proc` from sysctl, `proc_pidinfo` task data and `KERN_PROCARGS2`.
use std::{
    ffi::c_int,
    mem::{offset_of, size_of},
    sync::OnceLock,
};

/// The leading, fixed-offset fields of the kernel `struct kinfo_proc`; padding covers the rest.
/// Both LP64 macOS architectures share this layout.
#[repr(C)]
#[derive(Clone, Copy)]
struct KinfoProc {
    started: libc::timeval,
    _before_pid: [u8; 24],
    pid: i32,
    _before_nice: [u8; 198],
    nice: i8,
    comm: [u8; 17],
    _before_uid: [u8; 160],
    uid: u32,
    _before_ppid: [u8; 136],
    ppid: i32,
    _tail: [u8; 84],
}

const _: () = {
    assert!(size_of::<KinfoProc>() == 648);
    assert!(offset_of!(KinfoProc, started) == 0);
    assert!(offset_of!(KinfoProc, pid) == 40);
    assert!(offset_of!(KinfoProc, nice) == 242);
    assert!(offset_of!(KinfoProc, comm) == 243);
    assert!(offset_of!(KinfoProc, uid) == 420);
    assert!(offset_of!(KinfoProc, ppid) == 560);
};

const CTL_KERN: c_int = 1;
const KERN_PROC: c_int = 14;
const KERN_PROC_ALL: c_int = 0;
const KERN_PROC_PID: c_int = 1;
const KERN_PROCARGS2: c_int = 49;
const PROC_PIDTASKINFO: c_int = 4;
const PATH_BUFFER: usize = 4096;
/// Upper bound for the argument buffer, whatever `kern.argmax` reports.
const ARGUMENT_LIMIT: usize = 4 << 20;

/// One row of the kernel process table.
#[derive(Clone, Debug)]
pub struct Process {
    pub pid: i32,
    pub parent: i32,
    pub uid: u32,
    pub nice: i8,
    /// Kernel command name, truncated to 16 bytes.
    pub command: String,
    /// Start time as seconds and microseconds; `None` when the kernel reports none.
    pub started: Option<(i64, u32)>,
}

fn convert(row: &KinfoProc) -> Process {
    let end = row
        .comm
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(row.comm.len());
    Process {
        pid: row.pid,
        parent: row.ppid,
        uid: row.uid,
        nice: row.nice,
        command: String::from_utf8_lossy(&row.comm[..end]).into_owned(),
        started: (row.started.tv_sec > 0)
            .then(|| (row.started.tv_sec, row.started.tv_usec.max(0) as u32)),
    }
}

/// Table rows for a sysctl name; the table can grow between the size query and the read.
fn table(mib: &mut [c_int]) -> Option<Vec<Process>> {
    for _ in 0..8 {
        let mut length = 0usize;
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                std::ptr::null_mut(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        let mut buffer = vec![0u8; length + length / 8 + 4 * size_of::<KinfoProc>()];
        length = buffer.len();
        let status = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                buffer.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        if status == 0 {
            buffer.truncate(length);
            return Some(
                buffer
                    .as_chunks::<{ size_of::<KinfoProc>() }>()
                    .0
                    .iter()
                    // SAFETY: the chunk holds exactly one integer-only structure.
                    .map(|chunk| unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast()) })
                    .map(|row: KinfoProc| convert(&row))
                    .collect(),
            );
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            return None;
        }
    }
    None
}

/// Every process, including those of other users.
pub fn list() -> Option<Vec<Process>> {
    table(&mut [CTL_KERN, KERN_PROC, KERN_PROC_ALL, 0])
}

pub fn find(pid: i32) -> Option<Process> {
    table(&mut [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid])?
        .into_iter()
        .find(|p| p.pid == pid)
}

/// Absolute path of the executable image.
pub fn executable_path(pid: i32) -> Option<String> {
    let mut buffer = vec![0u8; PATH_BUFFER];
    let length =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// Process name up to 32 bytes, longer than the kernel command name.
pub fn short_name(pid: i32) -> Option<String> {
    let mut buffer = vec![0u8; 64];
    let length = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// Memory and CPU totals of a task, readable only for processes of the caller's user.
#[derive(Clone, Copy, Debug)]
pub struct Task {
    pub virtual_size: u64,
    pub resident_size: u64,
    /// User CPU time in 100 ns units.
    pub user_time: u64,
    /// System CPU time in 100 ns units.
    pub system_time: u64,
    pub faults: u32,
    pub threads: u32,
}

/// Mach absolute time ticks to 100 ns units.
#[allow(deprecated)]
fn hundred_nanoseconds(ticks: u64) -> u64 {
    static BASE: OnceLock<(u32, u32)> = OnceLock::new();
    let (numerator, denominator) = *BASE.get_or_init(|| {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        if unsafe { libc::mach_timebase_info(&mut info) } == 0 && info.denom != 0 {
            (info.numer, info.denom)
        } else {
            (1, 1)
        }
    });
    (ticks as u128 * numerator as u128 / denominator as u128 / 100) as u64
}

pub fn task(pid: i32) -> Option<Task> {
    let mut info = unsafe { std::mem::zeroed::<libc::proc_taskinfo>() };
    let size = size_of::<libc::proc_taskinfo>() as c_int;
    let length = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    (length == size).then(|| Task {
        virtual_size: info.pti_virtual_size,
        resident_size: info.pti_resident_size,
        user_time: hundred_nanoseconds(info.pti_total_user),
        system_time: hundred_nanoseconds(info.pti_total_system),
        faults: info.pti_faults.max(0) as u32,
        threads: info.pti_threadnum.max(0) as u32,
    })
}

/// Reads `KERN_PROCARGS2` into one reusable buffer.
pub struct ArgumentReader {
    buffer: Vec<u8>,
}

impl ArgumentReader {
    pub fn new() -> Self {
        let limit = super::sysctl::unsigned("kern.argmax")
            .map_or(ARGUMENT_LIMIT, |v| (v as usize).min(ARGUMENT_LIMIT));
        Self {
            buffer: vec![0u8; limit.max(4096)],
        }
    }

    /// Arguments of a process of the caller's user; `None` for other users' processes.
    pub fn read(&mut self, pid: i32) -> Option<Vec<String>> {
        let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
        let mut length = self.buffer.len();
        let status = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                self.buffer.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        if status != 0 {
            return None;
        }
        parse_arguments(&self.buffer[..length.min(self.buffer.len())])
    }
}

impl Default for ArgumentReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Splits a `KERN_PROCARGS2` buffer: an `int` argument count, the executable path, NUL padding,
/// then the arguments; the environment after them is ignored.
pub fn parse_arguments(buffer: &[u8]) -> Option<Vec<String>> {
    let count = i32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?);
    let count = usize::try_from(count).ok()?;
    let mut rest = &buffer[4..];
    let path_end = rest.iter().position(|b| *b == 0)?;
    rest = &rest[path_end..];
    let padding = rest.iter().take_while(|b| **b == 0).count();
    rest = &rest[padding..];
    let mut arguments = Vec::with_capacity(count);
    for _ in 0..count {
        let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        arguments.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = &rest[(end + 1).min(rest.len())..];
    }
    Some(arguments)
}
