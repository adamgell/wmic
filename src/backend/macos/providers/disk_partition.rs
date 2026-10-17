use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::disks::{self, Disk},
    },
    model::{ManagementError, Record},
};

pub struct DiskPartition;

/// Names the partition types whose IORegistry content is a GUID.
fn partition_type(content: &str) -> String {
    match content.to_ascii_uppercase().as_str() {
        "7C3457EF-0000-11AA-AA11-00306543ECAC" => "Apple_APFS".into(),
        "69646961-6700-11AA-AA11-00306543ECAC" => "Apple_APFS_ISC".into(),
        "52637672-7900-11AA-AA11-00306543ECAC" => "Apple_APFS_Recovery".into(),
        "48465300-0000-11AA-AA11-00306543ECAC" => "Apple_HFS".into(),
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B" => "EFI".into(),
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" => "Microsoft Basic Data".into(),
        _ => content.into(),
    }
}

impl ClassProvider for DiskPartition {
    fn class(&self) -> &'static str {
        "Win32_DiskPartition"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(records(ctx, &disks::physical_disks()))
    }
}

/// Map partitions from the same explicit disk snapshot.
pub(super) fn records(ctx: &Context, disks: &[Disk]) -> Vec<Record> {
    let mut records = vec![];
    for disk in disks {
        for partition in &disk.partitions {
            let index = partition.slot.saturating_sub(1);
            let name = format!("Disk #{}, Partition #{index}", disk.number);
            let kind = partition.content.as_deref().map(partition_type);
            let blocks = partition
                .size
                .zip(partition.block_size)
                .filter(|(_, block)| *block > 0)
                .map(|(size, block)| size / block);
            let bootable = (kind.as_deref() == Some("EFI")).then_some(true);
            records.push(
                ctx.record("Win32_DiskPartition")
                    .set("DeviceID", name.as_str())
                    .set("Name", name.as_str())
                    .set("Caption", name.as_str())
                    .set("Description", name.as_str())
                    .set("Index", index)
                    .set("DiskIndex", disk.number)
                    .set("Size", partition.size)
                    .set("StartingOffset", partition.offset)
                    .set("BlockSize", partition.block_size)
                    .set("NumberOfBlocks", blocks)
                    .set("Type", kind)
                    .set("Bootable", bootable)
                    .set("PrimaryPartition", true)
                    .set("CreationClassName", "Win32_DiskPartition")
                    .set("SystemCreationClassName", "Win32_ComputerSystem")
                    .set("SystemName", ctx.server())
                    .build(),
            );
        }
    }
    records
}
