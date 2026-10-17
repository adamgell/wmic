#![cfg(target_os = "macos")]
mod support;
use std::collections::HashSet;
use support::request;
use wmic_compat::{
    backend::{Management, macos::MacManagement},
    model::*,
};

fn objects(text: &str) -> Vec<Record> {
    match MacManagement::new()
        .unwrap()
        .execute(&request(text))
        .unwrap()
    {
        ManagementResult::Objects(set) => set.objects,
        other => panic!("{text} returned {other:?}"),
    }
}

fn value(record: &Record, name: &str) -> CimValue {
    record
        .properties
        .iter()
        .find(|p| !p.system && p.name.eq_ignore_case(name))
        .unwrap_or_else(|| panic!("no property {name}"))
        .value
        .clone()
}

fn text(record: &Record, name: &str) -> Option<String> {
    match value(record, name) {
        CimValue::String(s) | CimValue::DateTime(s) => Some(s.to_string_lossy()),
        CimValue::Null => None,
        other => panic!("{name} holds {other:?}"),
    }
}

fn number(record: &Record, name: &str) -> Option<i128> {
    match value(record, name) {
        CimValue::Null => None,
        CimValue::Uint16(v) => Some(v.into()),
        CimValue::Uint32(v) => Some(v.into()),
        CimValue::Uint64(v) => Some(v.into()),
        other => panic!("{name} holds {other:?}"),
    }
}

fn flag(record: &Record, name: &str) -> Option<bool> {
    match value(record, name) {
        CimValue::Null => None,
        CimValue::Boolean(b) => Some(b),
        other => panic!("{name} holds {other:?}"),
    }
}

fn list(record: &Record, name: &str) -> Option<Vec<String>> {
    match value(record, name) {
        CimValue::Null => None,
        CimValue::Array(items) => Some(
            items
                .into_iter()
                .map(|item| match item {
                    CimValue::String(s) => s.to_string_lossy(),
                    other => panic!("{name} holds {other:?}"),
                })
                .collect(),
        ),
        other => panic!("{name} holds {other:?}"),
    }
}

#[test]
fn logical_disks_list_the_root_file_system() {
    let disks = objects("logicaldisk get *");
    let root = disks
        .iter()
        .find(|d| text(d, "DeviceID").as_deref() == Some("/"))
        .expect("root volume");
    assert_eq!(text(root, "Name").as_deref(), Some("/"));
    assert_eq!(number(root, "DriveType"), Some(3));
    assert_eq!(number(root, "MediaType"), Some(12));
    assert_eq!(
        text(root, "Description").as_deref(),
        Some("Local Fixed Disk")
    );
    assert!(!text(root, "FileSystem").unwrap().is_empty());
    assert!(!text(root, "VolumeName").unwrap().is_empty());
    assert!(number(root, "Size").unwrap() > 0);
    assert!(number(root, "FreeSpace").unwrap() <= number(root, "Size").unwrap());
    assert!(number(root, "MaximumComponentLength").unwrap() > 0);
    assert_eq!(text(root, "ProviderName"), None);
    let serial = text(root, "VolumeSerialNumber").unwrap();
    assert!(
        serial.len() == 8
            && serial
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F')),
        "{serial}"
    );
    for disk in &disks {
        let id = text(disk, "DeviceID").unwrap();
        assert!(id == "/" || id.starts_with("/Volumes/"), "{id}");
        assert_eq!(text(disk, "SystemName"), text(root, "SystemName"));
    }
}

#[test]
fn logical_disk_selection_by_condition_and_path() {
    assert!(!objects("logicaldisk where \"DriveType=3\" get DeviceID").is_empty());
    assert!(
        objects("logicaldisk where \"DriveType=4\" get DeviceID")
            .iter()
            .all(|d| { text(d, "ProviderName").is_some() })
    );
    let hit = objects("path Win32_LogicalDisk.DeviceID=\"/\" get Size");
    assert_eq!(hit.len(), 1);
    assert!(objects("path Win32_LogicalDisk.DeviceID=\"/no/such/volume\" get Size").is_empty());
}

#[test]
fn volumes_have_unique_devices_and_a_boot_volume() {
    let volumes = objects("volume get *");
    let mut devices = HashSet::new();
    for volume in &volumes {
        assert!(devices.insert(text(volume, "DeviceID").unwrap()));
        assert!(number(volume, "Capacity").unwrap() >= number(volume, "FreeSpace").unwrap());
        assert_eq!(text(volume, "Caption"), text(volume, "Name"));
        assert_eq!(text(volume, "DriveLetter"), None);
        assert_eq!(flag(volume, "Automount"), Some(true));
        assert!(number(volume, "SerialNumber").is_some());
    }
    let root: Vec<_> = volumes
        .iter()
        .filter(|v| text(v, "Name").as_deref() == Some("/"))
        .collect();
    assert_eq!(root.len(), 1);
    assert_eq!(flag(root[0], "BootVolume"), Some(true));
    assert_eq!(flag(root[0], "SystemVolume"), Some(true));
    assert!(text(root[0], "DeviceID").unwrap().starts_with("/dev/"));
    assert!(number(root[0], "BlockSize").unwrap() > 0);
    for other in volumes
        .iter()
        .filter(|v| text(v, "Name").as_deref() != Some("/"))
    {
        assert_eq!(flag(other, "BootVolume"), Some(false));
    }
}

#[test]
fn disk_drives_and_partitions_agree() {
    let drives = objects("diskdrive get *");
    let partitions = objects("partition get *");
    assert!(
        !drives.is_empty(),
        "supported macOS runners must expose a system disk"
    );
    assert!(
        !partitions.is_empty(),
        "the system disk must expose its partition map"
    );
    for drive in &drives {
        let id = text(drive, "DeviceID").unwrap();
        assert!(id.starts_with("/dev/disk"), "{id}");
        assert_eq!(text(drive, "Name").as_deref(), Some(id.as_str()));
        let index = number(drive, "Index").unwrap();
        assert_eq!(id, format!("/dev/disk{index}"));
        let size = number(drive, "Size").unwrap();
        let sector = number(drive, "BytesPerSector").unwrap();
        assert!(size > 0 && sector > 0);
        assert_eq!(number(drive, "TotalSectors"), Some(size / sector));
        assert!(!text(drive, "Manufacturer").unwrap().is_empty());
        assert_eq!(text(drive, "Description").as_deref(), Some("Disk drive"));
        for name in [
            "TotalHeads",
            "TotalCylinders",
            "SectorsPerTrack",
            "TotalTracks",
        ] {
            assert_eq!(number(drive, name), None, "{name}");
        }
        let owned: Vec<_> = partitions
            .iter()
            .filter(|p| number(p, "DiskIndex") == Some(index))
            .collect();
        assert_eq!(number(drive, "Partitions"), Some(owned.len() as i128));
        let mut names = HashSet::new();
        for partition in owned {
            let slot = number(partition, "Index").unwrap();
            let name = format!("Disk #{index}, Partition #{slot}");
            assert_eq!(text(partition, "DeviceID").as_deref(), Some(name.as_str()));
            assert_eq!(text(partition, "Name").as_deref(), Some(name.as_str()));
            assert!(names.insert(name));
            assert!(
                number(partition, "StartingOffset").unwrap() + number(partition, "Size").unwrap()
                    <= size
            );
            let block = number(partition, "BlockSize").unwrap();
            assert_eq!(
                number(partition, "NumberOfBlocks"),
                Some(number(partition, "Size").unwrap() / block)
            );
            assert_eq!(flag(partition, "PrimaryPartition"), Some(true));
        }
    }
    for partition in &partitions {
        let disk = number(partition, "DiskIndex").unwrap();
        assert!(drives.iter().any(|d| number(d, "Index") == Some(disk)));
    }
}

#[test]
fn network_adapters_are_numbered_and_formatted() {
    let adapters = objects("nic get *");
    assert!(
        !adapters.is_empty(),
        "supported macOS runners must expose a network adapter"
    );
    let mut seen = HashSet::new();
    for adapter in &adapters {
        let index = number(adapter, "InterfaceIndex").unwrap();
        assert!(index > 0);
        assert!(
            seen.insert(index),
            "interface index {index} is listed twice"
        );
        assert_eq!(number(adapter, "Index"), Some(index));
        assert_eq!(text(adapter, "DeviceID"), Some(index.to_string()));
        assert_eq!(
            text(adapter, "Caption"),
            Some(format!("[{index:08}] {}", text(adapter, "Name").unwrap()))
        );
        assert_eq!(text(adapter, "Description"), text(adapter, "Name"));
        let service = text(adapter, "ServiceName").unwrap();
        assert!(
            ["en", "bridge", "vmnet"]
                .iter()
                .any(|p| service.starts_with(p)),
            "{service}"
        );
        if let Some(mac) = text(adapter, "MACAddress") {
            let groups: Vec<_> = mac.split(':').collect();
            assert_eq!(groups.len(), 6, "{mac}");
            assert!(
                groups
                    .iter()
                    .all(|g| g.len() == 2
                        && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'))),
                "{mac}"
            );
        }
        assert_eq!(
            text(adapter, "AdapterType").as_deref(),
            Some("Ethernet 802.3")
        );
        assert_eq!(flag(adapter, "Installed"), Some(true));
        let status = number(adapter, "NetConnectionStatus").unwrap();
        assert!(status == 2 || status == 7);
        if status == 7 {
            assert_eq!(number(adapter, "Speed"), None);
        }
        if service.starts_with("bridge") || service.starts_with("vmnet") {
            assert_eq!(flag(adapter, "PhysicalAdapter"), Some(false));
        }
        assert!(text(adapter, "TimeOfLastReset").is_some());
    }
}

#[test]
fn network_configurations_follow_the_adapters() {
    let adapters = objects("nic get *");
    let configurations = objects("nicconfig get *");
    assert!(!adapters.is_empty(), "no network adapters returned");
    assert_eq!(
        configurations.len(),
        adapters.len(),
        "one configuration is required for every adapter"
    );
    let mut paired = 0;
    let mut configured = HashSet::new();
    for configuration in &configurations {
        let index = number(configuration, "InterfaceIndex");
        assert!(index.is_some(), "configuration has no interface index");
        assert!(
            configured.insert(index),
            "duplicate configuration for {index:?}"
        );
        assert_eq!(number(configuration, "Index"), index);
        let Some(adapter) = adapters
            .iter()
            .find(|a| number(a, "InterfaceIndex") == index)
        else {
            panic!("configuration {index:?} has no matching adapter");
        };
        paired += 1;
        assert_eq!(number(adapter, "Index"), number(configuration, "Index"));
        assert_eq!(text(adapter, "Caption"), text(configuration, "Caption"));
        assert_eq!(text(adapter, "Name"), text(configuration, "Description"));
        assert_eq!(
            text(adapter, "MACAddress"),
            text(configuration, "MACAddress")
        );
        assert_eq!(
            text(adapter, "ServiceName"),
            text(configuration, "ServiceName")
        );
        let addresses = list(configuration, "IPAddress");
        let subnets = list(configuration, "IPSubnet");
        assert_eq!(
            addresses.as_ref().map(Vec::len),
            subnets.as_ref().map(Vec::len)
        );
        let enabled = flag(configuration, "IPEnabled").unwrap();
        let routable = addresses.iter().flatten().any(|a| !a.starts_with("fe80:"));
        assert_eq!(enabled, routable);
        for (address, subnet) in addresses.iter().flatten().zip(subnets.iter().flatten()) {
            if address.contains(':') {
                assert!(subnet.parse::<u8>().unwrap() <= 128);
                assert!(!address.contains('%'));
            } else {
                assert!(subnet.parse::<std::net::Ipv4Addr>().is_ok(), "{subnet}");
            }
        }
        for gateway in list(configuration, "DefaultIPGateway").iter().flatten() {
            assert!(gateway.parse::<std::net::IpAddr>().is_ok(), "{gateway}");
        }
        assert_eq!(flag(configuration, "IPXEnabled"), Some(false));
        assert_eq!(
            flag(configuration, "DNSEnabledForWINSResolution"),
            Some(false)
        );
    }
    assert_eq!(
        paired,
        adapters.len(),
        "every adapter needs a matching configuration"
    );
    let expected: HashSet<_> = adapters
        .iter()
        .map(|adapter| number(adapter, "InterfaceIndex"))
        .collect();
    assert_eq!(
        configured, expected,
        "adapter/configuration identities differ"
    );
    for configuration in objects("nicconfig where IPEnabled=TRUE get Index,IPEnabled") {
        assert_eq!(flag(&configuration, "IPEnabled"), Some(true));
    }
}
