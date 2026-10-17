use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{host, identity, mach, sysctl},
    },
    model::{ManagementError, Record},
};

pub struct Processor;

fn architecture() -> u16 {
    if cfg!(target_arch = "aarch64") { 12 } else { 9 }
}

impl ClassProvider for Processor {
    fn class(&self) -> &'static str {
        "Win32_Processor"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let name = sysctl::string("machdep.cpu.brand_string").filter(|v| !v.is_empty());
        let manufacturer = if cfg!(target_arch = "aarch64") {
            Some("Apple".to_owned())
        } else {
            sysctl::string("machdep.cpu.vendor").filter(|v| !v.is_empty())
        };
        let clock = identity::max_cpu_mhz();
        let l2 = sysctl::unsigned("hw.perflevel0.l2cachesize")
            .or_else(|| sysctl::unsigned("hw.l2cachesize"))
            .map(|bytes| bytes / 1024);
        let l3 = sysctl::unsigned("hw.l3cachesize")
            .filter(|bytes| *bytes > 0)
            .map(|bytes| bytes / 1024);
        let load = if ctx.wants("LoadPercentage") {
            mach::cpu_load_percent(std::time::Duration::from_millis(100))
        } else {
            None
        };
        let logical = sysctl::unsigned("hw.logicalcpu");
        Ok(vec![
            ctx.record("Win32_Processor")
                .set("DeviceID", "CPU0")
                .set("Name", name.clone())
                .set("Caption", name.clone())
                .set("Description", name)
                .set("Manufacturer", manufacturer)
                .set("CreationClassName", "Win32_Processor")
                .set("SystemCreationClassName", "Win32_ComputerSystem")
                .set("SystemName", host::local_host_name())
                .set("Role", "CPU")
                .set("Architecture", architecture())
                .set("AddressWidth", 64u16)
                .set("DataWidth", 64u16)
                .set("NumberOfCores", sysctl::unsigned("hw.physicalcpu"))
                .set("NumberOfLogicalProcessors", logical)
                .set("ThreadCount", logical)
                .set("NumberOfEnabledCore", sysctl::unsigned("hw.activecpu"))
                .set("MaxClockSpeed", clock)
                .set("CurrentClockSpeed", clock)
                .set("L2CacheSize", l2)
                .set("L3CacheSize", l3)
                .set("LoadPercentage", load)
                .set("Status", "OK")
                .set("Availability", 3u16)
                .set("CpuStatus", 1u16)
                .set("ProcessorType", 3u16)
                .build(),
        ])
    }
}
