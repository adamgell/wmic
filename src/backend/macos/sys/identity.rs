use super::{iokit::Entry, sysctl};

fn platform_expert() -> Option<Entry> {
    Entry::matching_service("IOPlatformExpertDevice")
}

fn device_tree(path: &str, key: &str) -> Option<String> {
    Entry::from_path(path)?
        .string(key)
        .filter(|v| !v.trim().is_empty())
}

/// Hardware serial number.
pub fn serial_number() -> Option<String> {
    platform_expert()?
        .string("IOPlatformSerialNumber")
        .filter(|v| !v.is_empty())
}

/// Hardware UUID.
pub fn platform_uuid() -> Option<String> {
    platform_expert()?
        .string("IOPlatformUUID")
        .filter(|v| !v.is_empty())
}

/// Model identifier such as `Mac16,10`.
pub fn model() -> Option<String> {
    sysctl::string("hw.model").filter(|v| !v.is_empty())
}

/// Marketing name such as `Mac mini (2024)`.
pub fn product_name() -> Option<String> {
    device_tree("IODeviceTree:/product", "product-name")
}

/// Orderable model number with region suffix, for example `MU9D3LL/A`.
pub fn sku() -> Option<String> {
    let expert = platform_expert()?;
    let number = expert.string("model-number").filter(|v| !v.is_empty())?;
    let region = expert.string("region-info").unwrap_or_default();
    Some(format!("{number}{region}"))
}

/// Regulatory model number such as `A3238`.
pub fn regulatory_model() -> Option<String> {
    platform_expert()?
        .string("regulatory-model-number")
        .filter(|v| !v.is_empty())
}

/// Logic board serial number.
pub fn logic_board_serial() -> Option<String> {
    platform_expert()?
        .string("mlb-serial-number")
        .filter(|v| !v.is_empty())
}

/// System firmware version such as `mBoot-18000.161.10`.
pub fn firmware_version() -> Option<String> {
    device_tree("IODeviceTree:/chosen", "system-firmware-version")
        .or_else(|| device_tree("IODeviceTree:/options", "system-firmware-version"))
}

/// Memory vendor reported by the boot loader.
pub fn dram_vendor() -> Option<String> {
    device_tree("IODeviceTree:/chosen", "dram-vendor")
}

/// Memory technology reported by the boot loader, for example `LPDDR5`.
pub fn dram_type() -> Option<String> {
    device_tree("IODeviceTree:/chosen", "dram-type")
}

/// Highest performance-cluster frequency in MHz from the power-manager DVFS table.
pub fn max_cpu_mhz() -> Option<u32> {
    if let Some(hz) = sysctl::unsigned("hw.cpufrequency_max") {
        return u32::try_from(hz / 1_000_000).ok().filter(|v| *v > 0);
    }
    let table = Entry::from_path("IODeviceTree:/arm-io/pmgr")?.data("voltage-states5-sram")?;
    let top = table
        .as_chunks::<8>()
        .0
        .iter()
        .map(|pair| u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]))
        .max()?;
    let mhz = if top > 100_000_000 {
        top / 1_000_000
    } else {
        top / 1_000
    };
    (mhz > 0).then_some(mhz)
}
