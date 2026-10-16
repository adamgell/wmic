//! Identity classes backed by the platform expert and the device tree.
use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{identity, sysctl},
    },
    model::{ManagementError, Record},
};

pub struct ComputerSystemProduct;
pub struct Bios;
pub struct BaseBoard;
pub struct SystemEnclosure;
pub struct PhysicalMemory;
pub struct PhysicalMemoryArray;

impl ClassProvider for ComputerSystemProduct {
    fn class(&self) -> &'static str {
        "Win32_ComputerSystemProduct"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(vec![
            ctx.record("Win32_ComputerSystemProduct")
                .set("IdentifyingNumber", identity::serial_number())
                .set("Name", identity::model())
                .set("Vendor", "Apple Inc.")
                .set("Version", identity::product_name())
                .set("UUID", identity::platform_uuid())
                .set("SKUNumber", identity::sku())
                .set("Caption", "Computer System Product")
                .set("Description", "Computer System Product")
                .build(),
        ])
    }
}

impl ClassProvider for Bios {
    fn class(&self) -> &'static str {
        "Win32_BIOS"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let firmware = identity::firmware_version();
        Ok(vec![
            ctx.record("Win32_BIOS")
                .set("Name", firmware.clone())
                .set("Caption", firmware.clone())
                .set("Description", firmware.clone())
                .set("SMBIOSBIOSVersion", firmware.clone())
                .set("SoftwareElementID", firmware.clone())
                .set("Version", firmware.as_ref().map(|f| format!("APPLE - {f}")))
                .set("BIOSVersion", firmware.map(|f| vec![f]))
                .set("Manufacturer", "Apple Inc.")
                .set("SerialNumber", identity::serial_number())
                .set("SMBIOSPresent", false)
                .set("PrimaryBIOS", true)
                .set("Status", "OK")
                .set("SoftwareElementState", 3u16)
                .set("TargetOperatingSystem", 0u16)
                .build(),
        ])
    }
}

impl ClassProvider for BaseBoard {
    fn class(&self) -> &'static str {
        "Win32_BaseBoard"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(vec![
            ctx.record("Win32_BaseBoard")
                .set("Manufacturer", "Apple Inc.")
                .set("Product", identity::model())
                .set("SerialNumber", identity::logic_board_serial())
                .set("Version", identity::regulatory_model())
                .set("Tag", "Base Board")
                .set("Name", "Base Board")
                .set("Caption", "Base Board")
                .set("Description", "Base Board")
                .set("CreationClassName", "Win32_BaseBoard")
                .set("HostingBoard", true)
                .set("Status", "OK")
                .build(),
        ])
    }
}

/// SMBIOS chassis type from the marketing name, else from the model identifier.
fn chassis_type(product: Option<&str>, model: Option<&str>) -> Option<u16> {
    let by_prefix = |text: &str| -> Option<u16> {
        Some(match text {
            t if t.starts_with("Mac mini") || t.starts_with("Macmini") => 35,
            t if t.starts_with("Mac Studio") => 35,
            t if t.starts_with("iMac") => 13,
            t if t.starts_with("Mac Pro") || t.starts_with("MacPro") => 7,
            t if t.starts_with("MacBook") => 10,
            _ => return None,
        })
    };
    product
        .and_then(by_prefix)
        .or_else(|| model.and_then(by_prefix))
}

impl ClassProvider for SystemEnclosure {
    fn class(&self) -> &'static str {
        "Win32_SystemEnclosure"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let chassis = chassis_type(
            identity::product_name().as_deref(),
            identity::model().as_deref(),
        );
        Ok(vec![
            ctx.record("Win32_SystemEnclosure")
                .set("Manufacturer", "Apple Inc.")
                .set("SerialNumber", identity::serial_number())
                .set("ChassisTypes", chassis.map(|c| vec![c]))
                .set("Tag", "System Enclosure")
                .set("Name", "System Enclosure")
                .set("Caption", "System Enclosure")
                .set("Description", "System Enclosure")
                .set("CreationClassName", "Win32_SystemEnclosure")
                .set("LockPresent", false)
                .set("Status", "OK")
                .build(),
        ])
    }
}

impl ClassProvider for PhysicalMemory {
    fn class(&self) -> &'static str {
        "Win32_PhysicalMemory"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(vec![
            ctx.record("Win32_PhysicalMemory")
                .set("Tag", "Physical Memory 0")
                .set("Name", "Physical Memory")
                .set("Caption", "Physical Memory")
                .set("Description", "Physical Memory")
                .set("CreationClassName", "Win32_PhysicalMemory")
                .set("BankLabel", "BANK 0")
                .set("DeviceLocator", "SoC")
                .set("Capacity", sysctl::unsigned("hw.memsize"))
                .set("Manufacturer", identity::dram_vendor())
                .set("FormFactor", 0u16)
                .set("DataWidth", 64u16)
                .set("TotalWidth", 64u16)
                .build(),
        ])
    }
}

impl ClassProvider for PhysicalMemoryArray {
    fn class(&self) -> &'static str {
        "Win32_PhysicalMemoryArray"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let bytes = sysctl::unsigned("hw.memsize");
        Ok(vec![
            ctx.record("Win32_PhysicalMemoryArray")
                .set("Tag", "Physical Memory Array 0")
                .set("Name", "Physical Memory Array")
                .set("Caption", "Physical Memory Array")
                .set("Description", "Physical Memory Array")
                .set("CreationClassName", "Win32_PhysicalMemoryArray")
                .set("MaxCapacity", bytes.map(|b| b / 1024))
                .set("MaxCapacityEx", bytes)
                .set("MemoryDevices", 1u16)
                .set("Use", 3u16)
                .set("Location", 3u16)
                .set("MemoryErrorCorrection", 3u16)
                .build(),
        ])
    }
}
