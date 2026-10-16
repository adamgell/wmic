use super::method::{code, integer_argument};
use crate::{
    backend::macos::{
        context::Context,
        provider::{ClassProvider, MethodCall, MethodOutput},
        sys::{fs, host, mach, power, sysctl, time, users},
    },
    model::{ManagementError, Record},
};

pub struct OperatingSystem;

/// Product version such as `26.6.2`.
pub(super) fn product_version() -> Option<String> {
    sysctl::string("kern.osproductversion").filter(|v| !v.is_empty())
}

/// Caption such as `macOS 26.6.2`.
fn caption() -> Option<String> {
    product_version().map(|v| format!("macOS {v}"))
}

/// `Win32_OperatingSystem.Name`; `Win32_Process.OSName` repeats it.
pub(super) fn os_name() -> Option<String> {
    caption().map(|c| format!("{c}|/|/System"))
}

/// Power action named by `Win32Shutdown` flags: 1 shut down, 2 reboot, 8 power off, plus the
/// force bit 4.
fn flag_action(flags: Option<i128>) -> Option<power::Action> {
    let flags = flags?;
    if flags & !0xF != 0 {
        return None;
    }
    match flags & !4 {
        1 | 8 => Some(power::Action::Halt),
        2 => Some(power::Action::Restart),
        _ => None,
    }
}

fn architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "ARM 64-bit Processor"
    } else {
        "64-bit"
    }
}

/// Language and region of the process locale, for example `("en", "US")`.
fn locale() -> Option<(String, String)> {
    let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")?;
    let identifier = value.split(['.', '@']).next().unwrap_or_default();
    let (language, region) = identifier.split_once('_').unwrap_or((identifier, ""));
    (!language.is_empty()).then(|| (language.to_ascii_lowercase(), region.to_ascii_uppercase()))
}

fn locale_code(language: &str, region: &str) -> Option<&'static str> {
    Some(match (language, region) {
        ("en", "US") => "0409",
        ("en", "GB") => "0809",
        ("en", "AU") => "0c09",
        ("en", "CA") => "1009",
        ("en", "NZ") => "1409",
        ("en", "IE") => "1809",
        _ => return None,
    })
}

impl ClassProvider for OperatingSystem {
    fn class(&self) -> &'static str {
        "Win32_OperatingSystem"
    }

    fn implemented_methods(&self) -> &'static [&'static str] {
        &[
            "Reboot",
            "Shutdown",
            "Win32Shutdown",
            "Win32ShutdownTracker",
        ]
    }

    fn call(&self, ctx: &Context, call: &MethodCall<'_>) -> Result<MethodOutput, ManagementError> {
        let action = match call
            .method
            .name
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "reboot" => Some(power::Action::Restart),
            "shutdown" => Some(power::Action::Halt),
            "win32shutdown" | "win32shutdowntracker" => {
                flag_action(integer_argument(call, "Flags"))
            }
            _ => return Err(ctx.not_supported("method")),
        };
        let Some(action) = action else {
            return Ok(code(87));
        };
        if !power::is_root() {
            return Ok(code(5));
        }
        Ok(code(power::request(action)))
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let version = product_version();
        let caption = caption();
        let boot_device = fs::mounted_from("/");
        let memory =
            sysctl::unsigned("hw.memsize_usable").or_else(|| sysctl::unsigned("hw.memsize"));
        let swap = sysctl::swap_usage();
        let total_kb = memory.map(|m| m / 1024);
        let swap_total_kb = swap.map(|(total, _, _)| total / 1024);
        let swap_free_kb = swap.map(|(_, free, _)| free / 1024);
        let total_virtual_kb = total_kb.zip(swap_total_kb).map(|(a, b)| a + b);
        let free_physical_kb = mach::vm_statistics().map(|v| v.available_bytes() / 1024);
        let free_virtual_kb = free_physical_kb.zip(swap_free_kb).map(|(a, b)| a + b);
        let (now, _) = time::now();
        let language = locale();
        Ok(vec![
            ctx.record("Win32_OperatingSystem")
                .set("Caption", caption)
                .set("Name", os_name())
                .set("Version", version)
                .set("BuildNumber", sysctl::string("kern.osversion"))
                .set("Manufacturer", "Apple Inc.")
                .set("OSArchitecture", architecture())
                .set("OSType", 2u16)
                .set("ProductType", 1u32)
                .set("Status", "OK")
                .set("Primary", true)
                .set("Distributed", false)
                .set("Debug", false)
                .set("PortableOperatingSystem", false)
                .set("ServicePackMajorVersion", 0u16)
                .set("ServicePackMinorVersion", 0u16)
                .set("CreationClassName", "Win32_OperatingSystem")
                .set("CSCreationClassName", "Win32_ComputerSystem")
                .set("CSName", host::local_host_name())
                .set("RegisteredUser", host::user_name())
                .set("CodeSet", "65001")
                .set("SystemDirectory", "/System")
                .set("WindowsDirectory", "/System")
                .set("SystemDrive", "/")
                .set("BootDevice", boot_device.clone())
                .set("SystemDevice", boot_device)
                .set(
                    "Locale",
                    language
                        .as_ref()
                        .and_then(|(l, r)| locale_code(l, r))
                        .map(str::to_owned),
                )
                .set(
                    "OSLanguage",
                    language
                        .as_ref()
                        .filter(|(l, _)| l == "en")
                        .map(|_| 1033u32),
                )
                .set(
                    "LastBootUpTime",
                    sysctl::boot_time().map(|(s, us)| time::cim_datetime(s, us)),
                )
                .set("LocalDateTime", time::cim_now())
                .set(
                    "InstallDate",
                    time::change_time("/System/Library/CoreServices/SystemVersion.plist")
                        .map(|(s, us)| time::cim_datetime(s, us)),
                )
                .set("CurrentTimeZone", time::local(now).offset_minutes)
                .set("TotalVisibleMemorySize", total_kb)
                .set("FreePhysicalMemory", free_physical_kb)
                .set("TotalVirtualMemorySize", total_virtual_kb)
                .set("FreeVirtualMemory", free_virtual_kb)
                .set("SizeStoredInPagingFiles", swap_total_kb)
                .set("FreeSpaceInPagingFiles", swap_free_kb)
                .set("MaxProcessMemorySize", total_virtual_kb)
                .set("MaxNumberOfProcesses", sysctl::unsigned("kern.maxproc"))
                .set("NumberOfProcesses", mach::process_count())
                .set("NumberOfUsers", users::distinct_user_count())
                .build(),
        ])
    }
}
