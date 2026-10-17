use super::method::{code, text_argument};
use crate::{
    backend::macos::{
        context::Context,
        provider::{ClassProvider, MethodCall, MethodOutput},
        sys::{
            accounts, host,
            launchd::{self, Domain, Outcome, Service},
        },
    },
    model::{CimValue, ManagementError, Record},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Exit code Windows reports for a service that ended abnormally (`ERROR_PROCESS_ABORTED`).
const ABORTED: u32 = 1067;
/// How long one enumeration of the launchd inventory serves later method calls.
const INVENTORY_TTL: Duration = Duration::from_secs(5);

/// Runs one `launchctl` invocation.
pub type Runner<'a> = &'a dyn Fn(&[String]) -> Outcome;

/// Short-lived copy of the launchd inventory: a method call on several targets enumerates once.
pub struct Inventory {
    ttl: Duration,
    cached: Mutex<Option<(Instant, Arc<Vec<Service>>)>>,
}

impl Inventory {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            cached: Mutex::new(None),
        }
    }

    /// The cached inventory while it is younger than the time to live, else a fresh load. The
    /// lock is held during the load so that concurrent callers share one enumeration.
    pub fn get(&self, load: impl FnOnce() -> Vec<Service>) -> Arc<Vec<Service>> {
        let mut cached = self.cached.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((loaded, services)) = cached.as_ref()
            && loaded.elapsed() < self.ttl
        {
            return Arc::clone(services);
        }
        let services = Arc::new(load());
        *cached = Some((Instant::now(), Arc::clone(&services)));
        services
    }

    /// Always loads, and replaces the cached copy.
    pub fn refresh(&self, load: impl FnOnce() -> Vec<Service>) -> Arc<Vec<Service>> {
        self.invalidate();
        self.get(load)
    }

    /// Drops the cached copy; a state change makes it stale.
    pub fn invalidate(&self) {
        *self.cached.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub struct ServiceProvider {
    inventory: Inventory,
}

impl Default for ServiceProvider {
    fn default() -> Self {
        Self {
            inventory: Inventory::new(INVENTORY_TTL),
        }
    }
}

/// Windows start mode of a job: disabled jobs, then jobs launchd starts on its own, then the rest.
pub fn start_mode(service: &Service) -> &'static str {
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

/// Windows exit code of a job's last launchd status. A negative status is a terminating signal.
pub fn exit_code(status: Option<i32>) -> u32 {
    status.map_or(0, |s| u32::try_from(s).unwrap_or(ABORTED))
}

pub fn start(service: &Service, run: Runner<'_>) -> u32 {
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
    launchd::return_value(&run(&arguments))
}

pub fn stop(service: &Service, run: Runner<'_>) -> u32 {
    if !service.running() {
        return 6;
    }
    if !service.domain.manageable() {
        return 2;
    }
    launchd::return_value(&run(&service.stop_arguments()))
}

#[derive(PartialEq)]
enum Requested {
    Disabled,
    Auto,
    Manual,
}

fn requested(mode: &str) -> Option<Requested> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "auto" | "automatic" | "boot" | "system" => Some(Requested::Auto),
        "manual" => Some(Requested::Manual),
        "disabled" => Some(Requested::Disabled),
        _ => None,
    }
}

/// Sets the override database entry of the job.
fn set_enabled(service: &Service, enabled: bool, run: Runner<'_>) -> u32 {
    if !service.domain.manageable() {
        return 2;
    }
    launchd::return_value(&run(&service.enable_arguments(enabled)))
}

/// `Disabled` disables the job. Any other mode enables a disabled job. An enabled job keeps the
/// mode its plist gives it, so the call succeeds only when that mode is the requested one and
/// returns 1 (not supported) otherwise.
pub fn change_start_mode(service: &Service, mode: &str, run: Runner<'_>) -> u32 {
    match requested(mode) {
        None => 21,
        Some(Requested::Disabled) => set_enabled(service, false, run),
        Some(_) if service.disabled => set_enabled(service, true, run),
        Some(mode) => {
            if (mode == Requested::Auto) == (start_mode(service) == "Auto") {
                0
            } else {
                1
            }
        }
    }
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

/// Win32 record of a job.
pub fn record(ctx: &Context, service: &Service) -> Record {
    let command = service.definition.as_ref().and_then(|d| d.command_line());
    let runtime = service.runtime;
    let running = runtime.map(|r| r.pid > 0);
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
        .set("StartMode", start_mode(service))
        .set("StartName", start_name(service))
        .set("ServiceType", "Own Process")
        .set("Status", if runtime.is_some() { "OK" } else { "Unknown" })
        .set("AcceptStop", true)
        .set("AcceptPause", false)
        .set("DesktopInteract", false)
        .set("ErrorControl", "Normal")
        .set("ExitCode", exit_code(runtime.and_then(|r| r.status)))
        .set("SystemName", ctx.server())
        .set("CreationClassName", "Win32_Service")
        .set("SystemCreationClassName", "Win32_ComputerSystem")
        .set("TagId", 0u32)
        .set("CheckPoint", 0u32)
        .set("WaitHint", 0u32)
        .set("ServiceSpecificExitCode", 0u32)
        .set("DelayedAutoStart", false)
        .build()
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
        let services = self.inventory.get(launchd::services);
        let Some(service) = services.iter().find(|s| Some(&s.label) == label.as_ref()) else {
            return Ok(code(21));
        };
        let result = match name.as_str() {
            "startservice" => start(service, &launchd::run),
            "stopservice" => stop(service, &launchd::run),
            _ => {
                let mode = text_argument(call, "StartMode").unwrap_or_default();
                change_start_mode(service, &mode, &launchd::run)
            }
        };
        if result == 0 {
            self.inventory.invalidate();
        }
        Ok(code(result))
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let services = self.inventory.refresh(launchd::services);
        Ok(services.iter().map(|s| record(ctx, s)).collect())
    }
}
