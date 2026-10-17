use std::os::unix::fs::MetadataExt;

/// Wall-clock fields of a Unix time in the process time zone.
#[derive(Clone, Copy, Debug)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Minutes east of UTC in effect at that instant.
    pub offset_minutes: i32,
    pub dst: bool,
}

pub fn local(seconds: i64) -> Local {
    let time = seconds as libc::time_t;
    let mut fields = unsafe { std::mem::zeroed::<libc::tm>() };
    unsafe { libc::localtime_r(&time, &mut fields) };
    Local {
        year: fields.tm_year + 1900,
        month: (fields.tm_mon + 1) as u32,
        day: fields.tm_mday as u32,
        hour: fields.tm_hour as u32,
        minute: fields.tm_min as u32,
        second: fields.tm_sec as u32,
        offset_minutes: (fields.tm_gmtoff / 60) as i32,
        dst: fields.tm_isdst > 0,
    }
}

/// Current Unix time as seconds and microseconds.
pub fn now() -> (i64, u32) {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (elapsed.as_secs() as i64, elapsed.subsec_micros())
}

/// CIM datetime text `yyyymmddHHMMSS.mmmmmm+UUU` with the offset in effect at that instant.
pub fn cim_datetime(seconds: i64, micros: u32) -> String {
    let t = local(seconds);
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}.{:06}{}{:03}",
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
        micros.min(999_999),
        if t.offset_minutes < 0 { '-' } else { '+' },
        t.offset_minutes.abs()
    )
}

pub fn cim_now() -> String {
    let (seconds, micros) = now();
    cim_datetime(seconds, micros)
}

/// Inode change time of a path, as seconds and microseconds.
pub fn change_time(path: &str) -> Option<(i64, u32)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.ctime(), (metadata.ctime_nsec() / 1000) as u32))
}
