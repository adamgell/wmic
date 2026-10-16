//! Physical disks and their partitions from the IORegistry.
use super::{cfvalue, iokit::Entry, registry};

/// One partition-scheme child of a physical disk.
#[derive(Clone, Debug)]
pub struct Partition {
    pub bsd_name: String,
    /// One-based slot in the partition map.
    pub slot: u32,
    pub size: Option<u64>,
    pub offset: Option<u64>,
    pub block_size: Option<u64>,
    pub content: Option<String>,
}

/// One whole disk backed by a block storage device.
#[derive(Clone, Debug)]
pub struct Disk {
    pub bsd_name: String,
    pub number: u32,
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub serial_number: Option<String>,
    pub firmware: Option<String>,
    pub interconnect: Option<String>,
    pub size: Option<u64>,
    pub block_size: Option<u64>,
    pub removable: bool,
    pub partitions: Vec<Partition>,
}

fn trimmed(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Number in a whole-disk BSD name such as `disk4`.
fn disk_number(bsd_name: &str) -> Option<u32> {
    bsd_name.strip_prefix("disk")?.parse().ok()
}

/// Disk and slot numbers in a partition BSD name such as `disk4s2`.
fn partition_numbers(bsd_name: &str) -> Option<(u32, u32)> {
    let (disk, slot) = bsd_name.strip_prefix("disk")?.split_once('s')?;
    Some((disk.parse().ok()?, slot.parse().ok()?))
}

fn unsigned(entry: &Entry, key: &str) -> Option<u64> {
    entry.number(key).and_then(|v| u64::try_from(v).ok())
}

/// Block storage device that directly serves a whole-disk medium.
/// A medium layered on another medium (synthesized APFS containers) or on a disk image has none.
fn storage_device(medium: &Entry) -> Option<Entry> {
    let mut current = registry::parent(medium)?;
    for _ in 0..6 {
        if registry::conforms_to(&current, "IOMedia") {
            return None;
        }
        if registry::conforms_to(&current, "IOBlockStorageDevice") {
            let class = registry::class_name(&current).unwrap_or_default();
            return (!class.contains("HDIX") && !class.contains("DiskImage")).then_some(current);
        }
        current = registry::parent(&current)?;
    }
    None
}

fn disk(medium: &Entry, bsd_name: String, number: u32) -> Option<Disk> {
    let device = storage_device(medium)?;
    let characteristics = registry::dictionary(&device, "Device Characteristics");
    let text = |key: &str| trimmed(characteristics.as_ref().and_then(|d| cfvalue::text(d, key)));
    let interconnect = trimmed(
        registry::dictionary(&device, "Protocol Characteristics")
            .and_then(|d| cfvalue::text(&d, "Physical Interconnect")),
    );
    Some(Disk {
        bsd_name,
        number,
        model: text("Product Name"),
        vendor: text("Vendor Name"),
        serial_number: text("Serial Number"),
        firmware: text("Product Revision Level"),
        interconnect,
        size: unsigned(medium, "Size"),
        block_size: unsigned(medium, "Preferred Block Size"),
        removable: medium.boolean("Removable").unwrap_or(false),
        partitions: vec![],
    })
}

fn partition(medium: &Entry, bsd_name: String) -> Option<Partition> {
    let slot = u32::try_from(medium.number("Partition ID")?).ok()?;
    Some(Partition {
        bsd_name,
        slot,
        size: unsigned(medium, "Size"),
        offset: unsigned(medium, "Base"),
        block_size: unsigned(medium, "Preferred Block Size"),
        content: trimmed(medium.string("Content")),
    })
}

/// Physical disks in disk-number order, each with its partitions in slot order.
pub fn physical_disks() -> Vec<Disk> {
    let mut disks = vec![];
    let mut partitions = vec![];
    for medium in Entry::matching_services("IOMedia") {
        let Some(bsd_name) = medium.string("BSD Name") else {
            continue;
        };
        if medium.boolean("Whole") == Some(true) {
            if let Some(number) = disk_number(&bsd_name)
                && let Some(found) = disk(&medium, bsd_name, number)
            {
                disks.push(found);
            }
        } else if let Some((number, _)) = partition_numbers(&bsd_name)
            && let Some(found) = partition(&medium, bsd_name)
        {
            partitions.push((number, found));
        }
    }
    disks.sort_by_key(|d| d.number);
    for (number, found) in partitions {
        if let Some(owner) = disks.iter_mut().find(|d| d.number == number) {
            owner.partitions.push(found);
        }
    }
    for owner in &mut disks {
        owner.partitions.sort_by_key(|p| p.slot);
    }
    disks
}
