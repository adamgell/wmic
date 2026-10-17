use super::{
    method::{code, integer_argument, text_argument},
    operating_system,
};
use crate::{
    backend::macos::{
        context::Context,
        provider::{ClassProvider, MethodCall, MethodOutput},
        sys::{accounts, cmdline, proc, spawn, time},
    },
    model::{CimValue, ManagementError, Property, Record},
};
use std::path::Path;

pub struct Process;

/// Win32 base priority for a nice value.
fn base_priority(nice: i8) -> u32 {
    match nice {
        0 => 8,
        n if n < 0 => 13,
        _ => 4,
    }
}

/// Nice value for a Win32 priority class: idle, below normal, normal, above normal, high and
/// realtime.
fn nice_for_class(class: i128) -> Option<i32> {
    Some(match class {
        64 => 19,
        16384 => 10,
        32 => 0,
        32768 => -5,
        128 => -10,
        256 => -20,
        _ => return None,
    })
}

fn out_parameter(name: &str, value: CimValue) -> Property {
    Property {
        name: name.into(),
        value,
        cim_type: 8,
        system: false,
        qualifiers: vec![],
    }
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Process id of the instance a method runs on.
fn target_pid(call: &MethodCall<'_>) -> Option<i32> {
    let record = call.instance?;
    let value = record
        .properties
        .iter()
        .find(|p| !p.system && p.name.eq_ignore_case("ProcessId"))?;
    match value.value {
        CimValue::Uint32(pid) => i32::try_from(pid).ok(),
        _ => None,
    }
}

fn create(call: &MethodCall<'_>) -> MethodOutput {
    let line = text_argument(call, "CommandLine").unwrap_or_default();
    let arguments = cmdline::split(&line);
    if arguments.first().is_none_or(|program| program.is_empty()) {
        return code(21);
    }
    let directory = text_argument(call, "CurrentDirectory").filter(|d| !d.is_empty());
    if directory.as_deref().is_some_and(|d| !Path::new(d).is_dir()) {
        return code(9);
    }
    match spawn::spawn(&arguments, directory.as_deref()) {
        Ok(pid) => MethodOutput {
            return_value: Some(CimValue::Uint32(0)),
            out_parameters: vec![Property {
                name: "ProcessId".into(),
                value: CimValue::Uint32(pid as u32),
                cim_type: 19,
                system: false,
                qualifiers: vec![],
            }],
        },
        Err(libc::ENOENT | libc::ENOTDIR) => code(9),
        Err(libc::EACCES | libc::EPERM) => code(2),
        Err(libc::EINVAL) => code(21),
        Err(_) => code(8),
    }
}

fn terminate(pid: i32) -> u32 {
    if pid <= 1 {
        return 3;
    }
    if unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
        return 0;
    }
    match last_errno() {
        libc::EPERM => 2,
        libc::ESRCH => 21,
        _ => 8,
    }
}

fn set_priority(pid: i32, call: &MethodCall<'_>) -> u32 {
    let Some(nice) = integer_argument(call, "Priority").and_then(nice_for_class) else {
        return 21;
    };
    if pid <= 0 {
        return 3;
    }
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) } == 0 {
        return 0;
    }
    match last_errno() {
        libc::EPERM | libc::EACCES => 2,
        libc::ESRCH => 21,
        _ => 8,
    }
}

fn get_owner(ctx: &Context, pid: i32) -> MethodOutput {
    let Some(process) = proc::find(pid) else {
        return code(21);
    };
    let Some(user) = accounts::user_name(process.uid) else {
        return code(8);
    };
    MethodOutput {
        return_value: Some(CimValue::Uint32(0)),
        out_parameters: vec![
            out_parameter("User", CimValue::String(user.as_str().into())),
            out_parameter("Domain", CimValue::String(ctx.server().into())),
        ],
    }
}

impl ClassProvider for Process {
    fn class(&self) -> &'static str {
        "Win32_Process"
    }

    fn implemented_methods(&self) -> &'static [&'static str] {
        &["Create", "Terminate", "GetOwner", "SetPriority"]
    }

    fn call(&self, ctx: &Context, call: &MethodCall<'_>) -> Result<MethodOutput, ManagementError> {
        let name = call.method.name.to_string_lossy().to_ascii_lowercase();
        if name == "create" {
            return Ok(create(call));
        }
        let Some(pid) = target_pid(call) else {
            return match name.as_str() {
                "terminate" | "getowner" | "setpriority" => Ok(code(21)),
                _ => Err(ctx.not_supported("method")),
            };
        };
        Ok(match name.as_str() {
            "terminate" => code(terminate(pid)),
            "getowner" => get_owner(ctx, pid),
            "setpriority" => code(set_priority(pid, call)),
            _ => return Err(ctx.not_supported("method")),
        })
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let table =
            proc::list().ok_or_else(|| ctx.error(0x80041001, "Generic failure", "query"))?;
        let version = operating_system::product_version();
        let os_name = operating_system::os_name();
        let wants_task = [
            "ThreadCount",
            "WorkingSetSize",
            "VirtualSize",
            "PageFaults",
            "KernelModeTime",
            "UserModeTime",
        ]
        .iter()
        .any(|p| ctx.wants(p));
        let mut arguments = ctx.wants("CommandLine").then(proc::ArgumentReader::new);
        let mut records = Vec::with_capacity(table.len());
        for process in table {
            let path = proc::executable_path(process.pid);
            let name = path
                .as_deref()
                .and_then(|p| p.rsplit('/').next())
                .map(str::to_owned)
                .or_else(|| proc::short_name(process.pid))
                .or_else(|| Some(process.command.clone()))
                .filter(|n| !n.is_empty());
            let command_line = arguments
                .as_mut()
                .filter(|_| process.pid > 0)
                .and_then(|reader| reader.read(process.pid))
                .map(|items| cmdline::join(&items))
                .filter(|line| !line.is_empty());
            let task = if wants_task && process.pid > 0 {
                proc::task(process.pid)
            } else {
                None
            };
            records.push(
                ctx.record("Win32_Process")
                    .set("Handle", process.pid.to_string())
                    .set("ProcessId", process.pid as u32)
                    .set("ParentProcessId", process.parent as u32)
                    .set("Name", name.clone())
                    .set("Caption", name.clone())
                    .set("Description", name)
                    .set("ExecutablePath", path)
                    .set("CommandLine", command_line)
                    .set(
                        "CreationDate",
                        process.started.map(|(s, us)| time::cim_datetime(s, us)),
                    )
                    .set("Priority", base_priority(process.nice))
                    .set("ThreadCount", task.map(|t| t.threads))
                    .set("WorkingSetSize", task.map(|t| t.resident_size))
                    .set("VirtualSize", task.map(|t| t.virtual_size))
                    .set("PageFaults", task.map(|t| t.faults))
                    .set("KernelModeTime", task.map(|t| t.system_time))
                    .set("UserModeTime", task.map(|t| t.user_time))
                    .set("CSName", ctx.server())
                    .set("OSName", os_name.clone())
                    .set("WindowsVersion", version.clone())
                    .set("CreationClassName", "Win32_Process")
                    .set("CSCreationClassName", "Win32_ComputerSystem")
                    .set("OSCreationClassName", "Win32_OperatingSystem")
                    .build(),
            );
        }
        Ok(records)
    }
}
