//! Class providers. A provider registered in `all` replaces the empty placeholder of the same name.
use super::provider::ClassProvider;

mod computer_system;
pub mod empty;
mod hardware;
mod operating_system;
mod processor;
mod system_state;

/// Classes served with an empty instance set until a provider is added to `all`.
pub const EMPTY_CLASSES: &[&str] = &[
    "Win32_TemperatureProbe",
    "Win32_VoltageProbe",
    "Win32_CDROMDrive",
    "Win32_TapeDrive",
    "Win32_Process",
    "Win32_Service",
    "Win32_LogicalDisk",
    "Win32_Volume",
    "Win32_DiskDrive",
    "Win32_DiskPartition",
    "Win32_NetworkAdapter",
    "Win32_NetworkAdapterConfiguration",
    "Win32_UserAccount",
    "Win32_Group",
    "Win32_StartupCommand",
    "Win32_Product",
    "Win32_QuickFixEngineering",
];

pub fn all() -> Vec<Box<dyn ClassProvider>> {
    vec![
        Box::new(operating_system::OperatingSystem),
        Box::new(processor::Processor),
        Box::new(computer_system::ComputerSystem),
        Box::new(hardware::ComputerSystemProduct),
        Box::new(hardware::Bios),
        Box::new(hardware::BaseBoard),
        Box::new(hardware::SystemEnclosure),
        Box::new(hardware::PhysicalMemory),
        Box::new(hardware::PhysicalMemoryArray),
        Box::new(system_state::TimeZone),
        Box::new(system_state::Environment),
        Box::new(system_state::PageFileUsage),
        Box::new(system_state::LogonSession),
    ]
}
