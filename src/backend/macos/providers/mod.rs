//! Class providers. A provider registered in `all` replaces the empty placeholder of the same name.
use super::provider::ClassProvider;

mod computer_system;
mod disk_drive;
mod disk_partition;
pub mod empty;
mod group;
mod hardware;
mod logical_disk;
mod method;
mod network_adapter;
mod network_adapter_configuration;
mod operating_system;
mod process;
mod processor;
mod product;
mod quick_fix_engineering;
mod service;
mod startup_command;
mod system_state;
mod user_account;
mod volume;

/// Classes served with an empty instance set until a provider is added to `all`.
pub const EMPTY_CLASSES: &[&str] = &[
    "Win32_TemperatureProbe",
    "Win32_VoltageProbe",
    "Win32_CDROMDrive",
    "Win32_TapeDrive",
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
        Box::new(logical_disk::LogicalDisk),
        Box::new(volume::Volume),
        Box::new(disk_drive::DiskDrive),
        Box::new(disk_partition::DiskPartition),
        Box::new(network_adapter::NetworkAdapter),
        Box::new(network_adapter_configuration::NetworkAdapterConfiguration),
        Box::new(process::Process),
        Box::new(service::ServiceProvider),
        Box::new(user_account::UserAccount),
        Box::new(group::Group),
        Box::new(startup_command::StartupCommand),
        Box::new(product::Product),
        Box::new(quick_fix_engineering::QuickFixEngineering),
    ]
}
