use crate::{
    backend::macos::{context::Context, provider::ClassProvider, sys::volumes},
    model::{ManagementError, Record},
};
use std::collections::HashSet;

pub struct Volume;

impl ClassProvider for Volume {
    fn class(&self) -> &'static str {
        "Win32_Volume"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let mut seen = HashSet::new();
        let mut records = vec![];
        for mount in volumes::mounts() {
            // The device is the key; a device mounted twice appears once.
            if !seen.insert(mount.device.clone()) {
                continue;
            }
            let kind = mount.kind();
            let label = ctx.wants("Label").then(|| mount.label());
            let longest_name = ctx
                .wants("MaximumFileNameLength")
                .then(|| volumes::max_name_length(&mount.mount_point).unwrap_or(255));
            records.push(
                ctx.record("Win32_Volume")
                    .set("DeviceID", mount.device.as_str())
                    .set("Name", mount.mount_point.as_str())
                    .set("Caption", mount.mount_point.as_str())
                    .set("Label", label)
                    .set("FileSystem", mount.file_system())
                    .set("Capacity", mount.size())
                    .set("FreeSpace", mount.free_space())
                    .set("BlockSize", mount.block_size)
                    .set("NumberOfBlocks", mount.blocks)
                    .set("DriveType", kind.drive_type())
                    .set("BootVolume", mount.is_root())
                    .set("SystemVolume", mount.is_root())
                    .set("Automount", true)
                    .set("Compressed", false)
                    .set("DirtyBitSet", false)
                    .set("MaximumFileNameLength", longest_name)
                    .set("PageFilePresent", mount.mount_point == "/System/Volumes/VM")
                    .set("QuotasEnabled", false)
                    .set("QuotasIncomplete", false)
                    .set("QuotasRebuilding", false)
                    .set("SerialNumber", mount.fsid)
                    .set("SupportsDiskQuotas", false)
                    .set("SupportsFileBasedCompression", true)
                    .set("CreationClassName", "Win32_Volume")
                    .set("SystemCreationClassName", "Win32_ComputerSystem")
                    .set("SystemName", ctx.server())
                    .build(),
            );
        }
        Ok(records)
    }
}
