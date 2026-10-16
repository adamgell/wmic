use super::method::{code, text_argument};
use crate::{
    backend::macos::{
        context::Context,
        provider::{ClassProvider, MethodCall, MethodOutput},
        sys::{
            accounts, host,
            launchd::{self, Domain, Service},
        },
    },
    model::{CimValue, ManagementError, Record},
};

pub struct ServiceProvider;

/// Windows start mode of a job: disabled jobs, then jobs launchd starts on its own, then the rest.
fn start_mode(service: &Service) -> &'static str {
    if service.disabled {
        "Disabled"
    } else if service
        .definition
        .as_ref()
        .is_some_and(|d| d.run_at_load || d.keep_alive)
    {
        "Auto"
    } else {
        "Manual"
    }
}

fn start_name(service: &Service) -> String {
    match (
        service
            .definition
            .as_ref()
            .and_then(|d| d.user_name.as_deref()),
        &service.domain,
    ) {
        (Some(user), _) => user.to_owned(),
        (None, Domain::System) => "LocalSystem".into(),
        (None, Domain::Gui(uid)) => accounts::user_name(*uid).unwrap_or_else(host::user_name),
    }
}

fn start(service: &Service) -> u32 {
    if service.running() {
        return 10;
    }
    if service.disabled && !service.loaded() {
        return 14;
    }
    if !service.domain.manageable() {
        return 2;
    }
    let arguments = if service.loaded() {
        service.kickstart_arguments()
    } else if let Some(arguments) = service.bootstrap_arguments() {
        arguments
    } else {
        return 8;
    };
    launchd::return_value(&launchd::run(&arguments))
}

fn stop(service: &Service) -> u32 {
    if !service.running() {
        return 6;
    }
    if !service.domain.manageable() {
        return 2;
    }
    launchd::return_value(&launchd::run(&service.stop_arguments()))
}

/// `Some(true)` enables the job, `Some(false)` disables it.
fn requested_enabled(mode: &str) -> Option<bool> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "auto" | "automatic" | "boot" | "system" | "manual" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

fn change_start_mode(service: &Service, call: &MethodCall<'_>) -> u32 {
    let Some(enabled) = text_argument(call, "StartMode").and_then(|m| requested_enabled(&m)) else {
        return 21;
    };
    if !service.domain.manageable() {
        return 2;
    }
    launchd::return_value(&launchd::run(&service.enable_arguments(enabled)))
}

fn instance_name(call: &MethodCall<'_>) -> Option<String> {
    let record = call.instance?;
    let property = record
        .properties
        .iter()
        .find(|p| !p.system && p.name.eq_ignore_case("Name"))?;
    match &property.value {
        CimValue::String(name) => Some(name.to_string_lossy()),
        _ => None,
    }
}

impl ClassProvider for ServiceProvider {
    fn class(&self) -> &'static str {
        "Win32_Service"
    }

    fn implemented_methods(&self) -> &'static [&'static str] {
        &["StartService", "StopService", "ChangeStartMode"]
    }

    fn call(&self, ctx: &Context, call: &MethodCall<'_>) -> Result<MethodOutput, ManagementError> {
        let name = call.method.name.to_string_lossy().to_ascii_lowercase();
        if !matches!(
            name.as_str(),
            "startservice" | "stopservice" | "changestartmode"
        ) {
            return Err(ctx.not_supported("method"));
        }
        let label = instance_name(call);
        let services = launchd::services();
        let Some(service) = services.iter().find(|s| Some(&s.label) == label.as_ref()) else {
            return Ok(code(21));
        };
        Ok(code(match name.as_str() {
            "startservice" => start(service),
            "stopservice" => stop(service),
            _ => change_start_mode(service, call),
        }))
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let records = launchd::services()
            .into_iter()
            .map(|service| {
                let command = service.definition.as_ref().and_then(|d| d.command_line());
                let runtime = service.runtime;
                let running = runtime.map(|r| r.pid > 0);
                let exit_code = runtime
                    .and_then(|r| r.status)
                    .and_then(|s| u32::try_from(s).ok())
                    .unwrap_or(0);
                ctx.record("Win32_Service")
                    .set("Name", service.label.as_str())
                    .set("DisplayName", service.label.as_str())
                    .set("Caption", service.label.as_str())
                    .set("PathName", command)
                    .set(
                        "State",
                        running.map(|r| if r { "Running" } else { "Stopped" }),
                    )
                    .set("Started", running)
                    .set("ProcessId", runtime.map(|r| r.pid))
                    .set("StartMode", start_mode(&service))
                    .set("StartName", start_name(&service))
                    .set("ServiceType", "Own Process")
                    .set("Status", if runtime.is_some() { "OK" } else { "Unknown" })
                    .set("AcceptStop", true)
                    .set("AcceptPause", false)
                    .set("DesktopInteract", false)
                    .set("ErrorControl", "Normal")
                    .set("ExitCode", exit_code)
                    .set("SystemName", ctx.server())
                    .set("CreationClassName", "Win32_Service")
                    .set("SystemCreationClassName", "Win32_ComputerSystem")
                    .set("TagId", 0u32)
                    .set("CheckPoint", 0u32)
                    .set("WaitHint", 0u32)
                    .set("ServiceSpecificExitCode", 0u32)
                    .set("DelayedAutoStart", false)
                    .build()
            })
            .collect();
        Ok(records)
    }
}
