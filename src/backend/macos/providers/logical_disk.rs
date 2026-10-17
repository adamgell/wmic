use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::volumes::{self, Mount},
    },
    model::{ManagementError, Record},
};

pub struct LogicalDisk;

/// The root and user-visible volumes under `/Volumes`.
fn listed(mount: &Mount) -> bool {
    (mount.is_root() || mount.mount_point.starts_with("/Volumes/")) && !mount.hidden_from_browsing()
}

impl ClassProvider for LogicalDisk {
    fn class(&self) -> &'static str {
        "Win32_LogicalDisk"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let records = volumes::mounts()
            .iter()
            .filter(|m| listed(m))
            .map(|mount| {
                let kind = mount.kind();
                let label = ctx.wants("VolumeName").then(|| mount.label());
                let longest_name = ctx
                    .wants("MaximumComponentLength")
                    .then(|| volumes::max_name_length(&mount.mount_point).unwrap_or(255));
                let provider = (kind == volumes::Kind::Network).then_some(mount.device.as_str());
                ctx.record("Win32_LogicalDisk")
                    .set("DeviceID", mount.mount_point.as_str())
                    .set("Name", mount.mount_point.as_str())
                    .set("Caption", mount.mount_point.as_str())
                    .set("Description", kind.description())
                    .set("VolumeName", label)
                    .set("FileSystem", mount.file_system())
                    .set("Size", mount.size())
                    .set("FreeSpace", mount.free_space())
                    .set("DriveType", kind.drive_type())
                    .set("MediaType", kind.media_type())
                    .set("ProviderName", provider)
                    .set("VolumeSerialNumber", format!("{:08X}", mount.fsid))
                    .set("MaximumComponentLength", longest_name)
                    .set("Compressed", false)
                    .set("SupportsFileBasedCompression", true)
                    .set("SupportsDiskQuotas", false)
                    .set("QuotasDisabled", true)
                    .set("QuotasIncomplete", false)
                    .set("QuotasRebuilding", false)
                    .set("VolumeDirty", false)
                    .set("Access", 0u16)
                    .set("Availability", 3u16)
                    .set("CreationClassName", "Win32_LogicalDisk")
                    .set("SystemCreationClassName", "Win32_ComputerSystem")
                    .set("SystemName", ctx.server())
                    .set("Status", "OK")
                    .build()
            })
            .collect();
        Ok(records)
    }
}
