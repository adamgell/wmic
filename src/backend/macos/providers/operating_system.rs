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
fn flag_action(flags: Option<i128>) -> Option<power::Request> {
    let flags = flags?;
    if flags & !0xF != 0 {
        return None;
    }
    let action = match flags & !4 {
        1 | 8 => power::Action::Halt,
        2 => power::Action::Restart,
        _ => return None,
    };
    Some(power::Request {
        action,
        force: flags & 4 != 0,
    })
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

impl OperatingSystem {
    fn call_with_power(
        &self,
        ctx: &Context,
        call: &MethodCall<'_>,
        runtime: &impl power::Runtime,
    ) -> Result<MethodOutput, ManagementError> {
        let action = match call
            .method
            .name
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "reboot" => Some(power::Request {
                action: power::Action::Restart,
                force: false,
            }),
            "shutdown" => Some(power::Request {
                action: power::Action::Halt,
                force: false,
            }),
            "win32shutdown" | "win32shutdowntracker" => {
                flag_action(integer_argument(call, "Flags"))
            }
            _ => return Err(ctx.not_supported("method")),
        };
        let Some(action) = action else {
            return Ok(code(87));
        };
        Ok(code(power::request(action, runtime)))
    }
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
        self.call_with_power(ctx, call, &power::Native)
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

#[cfg(test)]
mod power_tests {
    use super::*;
    use crate::{
        backend::macos::{
            provider::registry,
            sys::power::tests::{Call, FakeRuntime},
        },
        model::{CimValue, Property, WideText},
    };

    fn invoke(method: &str, flags: Option<CimValue>, runtime: &FakeRuntime) -> MethodOutput {
        let entry = registry().get("Win32_OperatingSystem").unwrap();
        let method = entry
            .info
            .schema
            .methods
            .iter()
            .find(|schema| schema.name.eq_ignore_case(method))
            .unwrap();
        let path: WideText = "Win32_OperatingSystem".into();
        let input: Vec<_> = flags
            .into_iter()
            .map(|value| Property {
                name: "Flags".into(),
                value,
                cim_type: 19,
                system: false,
                qualifiers: vec![],
            })
            .collect();
        OperatingSystem
            .call_with_power(
                &Context::unrestricted(),
                &MethodCall {
                    method,
                    path: &path,
                    instance: None,
                    input: &input,
                },
                runtime,
            )
            .unwrap()
    }

    fn assert_code(output: MethodOutput, expected: u32) {
        assert_eq!(output.return_value, Some(CimValue::Uint32(expected)));
        assert!(output.out_parameters.is_empty());
    }

    #[test]
    fn force_flag_is_not_discarded() {
        for (normal, forced) in [(1, 5), (2, 6), (8, 12)] {
            assert_ne!(
                flag_action(Some(normal)),
                flag_action(Some(forced)),
                "force bit must change the power request for flags {forced}"
            );
        }
    }

    #[test]
    fn reboot_and_shutdown_default_to_ordinary_requests() {
        for (method, flag) in [("Reboot", "-r"), ("Shutdown", "-h")] {
            let runtime = FakeRuntime::new(true);
            assert_code(invoke(method, None, &runtime), 0);
            assert_eq!(runtime.privilege_checks.get(), 1);
            assert_eq!(
                *runtime.calls.borrow(),
                [Call::Shutdown(
                    "/sbin/shutdown".into(),
                    vec![flag.into(), "now".into()]
                )]
            );
        }
    }

    #[test]
    fn shutdown_flags_select_exact_ordinary_or_forced_operation() {
        for method in ["Win32Shutdown", "Win32ShutdownTracker"] {
            for (flags, operation) in [
                (
                    1,
                    Call::Shutdown("/sbin/shutdown".into(), vec!["-h".into(), "now".into()]),
                ),
                (
                    2,
                    Call::Shutdown("/sbin/shutdown".into(), vec!["-r".into(), "now".into()]),
                ),
                (
                    8,
                    Call::Shutdown("/sbin/shutdown".into(), vec!["-h".into(), "now".into()]),
                ),
                (5, Call::Force(8)),
                (6, Call::Force(0)),
                (12, Call::Force(8)),
            ] {
                let runtime = FakeRuntime::new(true);
                assert_code(invoke(method, Some(CimValue::Uint32(flags)), &runtime), 0);
                assert_eq!(runtime.privilege_checks.get(), 1);
                assert_eq!(
                    *runtime.calls.borrow(),
                    [operation],
                    "{method} flags {flags}"
                );
            }
        }
    }

    #[test]
    fn all_power_methods_deny_fake_unprivileged_caller_without_actions() {
        for method in ["Reboot", "Shutdown"] {
            let runtime = FakeRuntime::new(false);
            assert_code(invoke(method, None, &runtime), 5);
            assert_eq!(runtime.privilege_checks.get(), 1);
            assert!(runtime.calls.borrow().is_empty());
        }
        for method in ["Win32Shutdown", "Win32ShutdownTracker"] {
            for flags in [1, 2, 8, 5, 6, 12] {
                let runtime = FakeRuntime::new(false);
                assert_code(invoke(method, Some(CimValue::Uint32(flags)), &runtime), 5);
                assert_eq!(runtime.privilege_checks.get(), 1);
                assert!(runtime.calls.borrow().is_empty());
            }
        }
    }

    #[test]
    fn invalid_flags_return_87_before_privilege_check_or_any_action() {
        let invalid = [
            None,
            Some(CimValue::Null),
            Some(CimValue::String("1".into())),
            Some(CimValue::Boolean(true)),
            Some(CimValue::Sint64(-1)),
            Some(CimValue::Uint64(u64::MAX)),
        ];
        for method in ["Win32Shutdown", "Win32ShutdownTracker"] {
            for root in [false, true] {
                for flags in invalid.iter().cloned().chain(
                    [0, 3, 4, 7, 9, 10, 11, 13, 14, 15, 16, u32::MAX]
                        .map(|flags| Some(CimValue::Uint32(flags))),
                ) {
                    let runtime = FakeRuntime::new(root);
                    assert_code(invoke(method, flags, &runtime), 87);
                    assert_eq!(runtime.privilege_checks.get(), 0);
                    assert!(runtime.calls.borrow().is_empty());
                }
            }
        }
    }

    #[test]
    fn action_failures_keep_the_existing_method_output_shape() {
        for (error, expected) in [(libc::EPERM, 5), (libc::EIO, 8)] {
            let mut runtime = FakeRuntime::new(true);
            runtime.error = Some(error);
            assert_code(invoke("Reboot", None, &runtime), expected);
        }
        let mut runtime = FakeRuntime::new(true);
        runtime.exit_status = 1 << 8;
        assert_code(invoke("Shutdown", None, &runtime), 8);
    }
}
