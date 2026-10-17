//! Classes describing the running session: time zone, environment, paging and logons.
use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{host, sysctl, time, users},
    },
    model::{ManagementError, Record},
};

pub struct TimeZone;
pub struct Environment;
pub struct PageFileUsage;
pub struct LogonSession;

/// IANA name of the system zone from the `/etc/localtime` link.
fn zone_name() -> Option<String> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let target = target.to_string_lossy();
    let (_, name) = target.split_once("zoneinfo/")?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// Smallest and largest UTC offsets in minutes over the next twelve months.
fn offset_range(now: i64) -> (i32, i32) {
    let mut low = i32::MAX;
    let mut high = i32::MIN;
    for month in 0..12 {
        let offset = time::local(now + month * 30 * 86_400).offset_minutes;
        low = low.min(offset);
        high = high.max(offset);
    }
    (low, high)
}

impl ClassProvider for TimeZone {
    fn class(&self) -> &'static str {
        "Win32_TimeZone"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let (now, _) = time::now();
        let (standard, daylight) = offset_range(now);
        let name = zone_name();
        let caption = name.as_ref().map(|n| {
            format!(
                "(UTC{}{:02}:{:02}) {n}",
                if standard < 0 { '-' } else { '+' },
                standard.abs() / 60,
                standard.abs() % 60
            )
        });
        Ok(vec![
            ctx.record("Win32_TimeZone")
                .set("Caption", caption)
                .set("Description", name.clone())
                .set("StandardName", name.clone())
                .set("DaylightName", name)
                .set("Bias", -standard)
                .set("StandardBias", 0u32)
                .set("DaylightBias", -(daylight - standard))
                .build(),
        ])
    }
}

impl ClassProvider for Environment {
    fn class(&self) -> &'static str {
        "Win32_Environment"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let owner = format!("{}\\{}", ctx.server(), host::user_name());
        let mut variables: Vec<(String, String)> = std::env::vars_os()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect();
        variables.sort();
        Ok(variables
            .into_iter()
            .map(|(name, value)| {
                ctx.record("Win32_Environment")
                    .set("Name", name.clone())
                    .set("VariableValue", value)
                    .set("UserName", owner.clone())
                    .set("SystemVariable", false)
                    .set("Caption", name.clone())
                    .set("Description", name)
                    .set("Status", "OK")
                    .build()
            })
            .collect())
    }
}

impl ClassProvider for PageFileUsage {
    fn class(&self) -> &'static str {
        "Win32_PageFileUsage"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let Some((total, _, used)) = sysctl::swap_usage().filter(|(total, _, _)| *total > 0) else {
            return Ok(vec![]);
        };
        let name = "/private/var/vm/swapfile";
        let megabytes = |bytes: u64| bytes / (1024 * 1024);
        Ok(vec![
            ctx.record("Win32_PageFileUsage")
                .set("Name", name)
                .set("Caption", name)
                .set("Description", name)
                .set("AllocatedBaseSize", megabytes(total))
                .set("CurrentUsage", megabytes(used))
                .set("PeakUsage", megabytes(used))
                .set("TempPageFile", false)
                .build(),
        ])
    }
}

impl ClassProvider for LogonSession {
    fn class(&self) -> &'static str {
        "Win32_LogonSession"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(users::sessions()
            .into_iter()
            .map(|session| {
                let id = if session.line.is_empty() {
                    session.pid.to_string()
                } else {
                    session.line.clone()
                };
                let logon_type = if session.host.is_empty() { 2u32 } else { 10u32 };
                ctx.record("Win32_LogonSession")
                    .set("LogonId", id)
                    .set("LogonType", logon_type)
                    .set(
                        "StartTime",
                        time::cim_datetime(session.seconds, session.micros),
                    )
                    .build()
            })
            .collect())
    }
}
