use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{host, identity, sysctl, time},
    },
    model::{ManagementError, Record},
};

pub struct ComputerSystem;

pub(super) fn system_type() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "ARM64-based PC"
    } else {
        "x64-based PC"
    }
}

impl ClassProvider for ComputerSystem {
    fn class(&self) -> &'static str {
        "Win32_ComputerSystem"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let name = host::local_host_name();
        let user = host::user_name();
        let model = identity::model();
        let laptop = model.as_deref().is_some_and(|m| m.starts_with("MacBook"));
        let (now, _) = time::now();
        let local = time::local(now);
        let domain = host::domain_suffix().unwrap_or_else(|| "WORKGROUP".into());
        let safe_boot = sysctl::unsigned("kern.safeboot").unwrap_or(0) != 0;
        Ok(vec![
            ctx.record("Win32_ComputerSystem")
                .set("Name", name.clone())
                .set("Caption", name)
                .set("DNSHostName", host::host_name())
                .set("Description", host::computer_name())
                .set("CreationClassName", "Win32_ComputerSystem")
                .set("Domain", domain)
                .set("PartOfDomain", false)
                .set("Workgroup", "WORKGROUP")
                .set("Manufacturer", "Apple Inc.")
                .set("Model", model)
                .set("SystemFamily", identity::product_name())
                .set("SystemSKUNumber", identity::sku())
                .set("SystemType", system_type())
                .set("TotalPhysicalMemory", sysctl::unsigned("hw.memsize"))
                .set("NumberOfProcessors", sysctl::unsigned("hw.packages"))
                .set(
                    "NumberOfLogicalProcessors",
                    sysctl::unsigned("hw.logicalcpu"),
                )
                .set("UserName", format!("{}\\{user}", ctx.server()))
                .set("PrimaryOwnerName", user)
                .set(
                    "BootupState",
                    if safe_boot {
                        "Fail-safe boot"
                    } else {
                        "Normal boot"
                    },
                )
                .set("Status", "OK")
                .set("PCSystemType", if laptop { 2u16 } else { 1u16 })
                .set(
                    "HypervisorPresent",
                    sysctl::unsigned("kern.hv_vmm_present").map(|v| v != 0),
                )
                .set("CurrentTimeZone", local.offset_minutes)
                .set("DaylightInEffect", local.dst)
                .set("Roles", ["LM_Workstation"])
                .set("ThermalState", 3u16)
                .set("PowerState", 0u16)
                .build(),
        ])
    }
}
