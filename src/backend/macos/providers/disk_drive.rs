use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::disks::{self, Disk},
    },
    model::{ManagementError, Record},
};

pub struct DiskDrive;

/// Win32 reports SCSI for every internal bus; only USB and FireWire keep their own names.
fn interface_type(disk: &Disk) -> &'static str {
    match disk.interconnect.as_deref() {
        Some(bus) if bus.eq_ignore_ascii_case("USB") => "USB",
        Some(bus) if bus.contains("FireWire") || bus.contains("1394") => "1394",
        _ => "SCSI",
    }
}

fn capabilities(disk: &Disk) -> (Vec<u16>, Vec<&'static str>) {
    let mut codes = vec![3, 4];
    let mut names = vec!["Random Access", "Supports Writing"];
    if disk.removable {
        codes.push(7);
        names.push("Supports Removable Media");
    }
    (codes, names)
}

impl ClassProvider for DiskDrive {
    fn class(&self) -> &'static str {
        "Win32_DiskDrive"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(records(ctx, &disks::physical_disks()))
    }
}

/// Map a disk snapshot without reading IORegistry.
pub(super) fn records(ctx: &Context, disks: &[Disk]) -> Vec<Record> {
    disks
        .iter()
        .map(|disk| {
            let device = format!("/dev/{}", disk.bsd_name);
            let sectors = disk
                .size
                .zip(disk.block_size)
                .filter(|(_, block)| *block > 0)
                .map(|(size, block)| size / block);
            let (codes, names) = capabilities(disk);
            let media = if disk.removable {
                "Removable Media"
            } else {
                "Fixed hard disk media"
            };
            ctx.record("Win32_DiskDrive")
                .set("DeviceID", device.as_str())
                .set("Name", device.as_str())
                .set("Index", disk.number)
                .set("Caption", disk.model.as_deref())
                .set("Model", disk.model.as_deref())
                .set("SerialNumber", disk.serial_number.as_deref())
                .set("FirmwareRevision", disk.firmware.as_deref())
                .set(
                    "Manufacturer",
                    disk.vendor.as_deref().unwrap_or("(Standard disk drives)"),
                )
                .set("Size", disk.size)
                .set("BytesPerSector", disk.block_size)
                .set("TotalSectors", sectors)
                .set("Partitions", disk.partitions.len() as u32)
                .set("InterfaceType", interface_type(disk))
                .set("MediaType", media)
                .set("MediaLoaded", true)
                .set("Capabilities", codes)
                .set("CapabilityDescriptions", names)
                .set("Description", "Disk drive")
                .set("ConfigManagerErrorCode", 0u32)
                .set("ConfigManagerUserConfig", false)
                .set("CreationClassName", "Win32_DiskDrive")
                .set("SystemCreationClassName", "Win32_ComputerSystem")
                .set("SystemName", ctx.server())
                .build()
        })
        .collect()
}
