//! Known nonempty source snapshots prove mappings even without live hardware.
use super::{
    disk_drive, disk_partition,
    network_adapter::{self, Adapter},
    network_adapter_configuration,
};
use crate::{
    backend::macos::{
        context::Context,
        sys::{
            disks::{Disk, Partition},
            interfaces::Interface,
            netconfig::{Dhcp, Dns, NetworkState, Service},
            netdev::Controller,
        },
    },
    model::{CimValue, Record},
};
use std::collections::HashMap;

fn value<'a>(record: &'a Record, name: &str) -> &'a CimValue {
    &record
        .properties
        .iter()
        .find(|p| !p.system && p.name.eq_ignore_case(name))
        .unwrap()
        .value
}
fn string(text: &str) -> CimValue {
    CimValue::String(text.into())
}

fn disks() -> Vec<Disk> {
    let disk = Disk {
        bsd_name: "disk7".into(),
        number: 7,
        model: Some("Fixture SSD".into()),
        vendor: Some("Fixture Vendor".into()),
        serial_number: Some("TEST-SERIAL".into()),
        firmware: Some("1.2".into()),
        interconnect: Some("PCI-Express".into()),
        size: Some(1_048_576),
        block_size: Some(512),
        removable: false,
        partitions: vec![Partition {
            bsd_name: "disk7s2".into(),
            slot: 2,
            size: Some(524_288),
            offset: Some(4096),
            block_size: Some(512),
            content: Some("7C3457EF-0000-11AA-AA11-00306543ECAC".into()),
        }],
    };
    let mut usb = disk.clone();
    usb.bsd_name = "disk9".into();
    usb.number = 9;
    usb.vendor = None;
    usb.size = None;
    usb.block_size = Some(0);
    usb.removable = true;
    usb.interconnect = Some("USB".into());
    usb.partitions.clear();
    vec![disk, usb]
}

#[test]
fn explicit_disk_snapshot_maps_both_disks_and_retains_missing_values() {
    let records = disk_drive::records(&Context::unrestricted(), &disks());
    assert_eq!(records.len(), 2);
    for (name, expected) in [
        ("DeviceID", string("/dev/disk7")),
        ("Index", CimValue::Uint32(7)),
        ("Model", string("Fixture SSD")),
        ("Manufacturer", string("Fixture Vendor")),
        ("SerialNumber", string("TEST-SERIAL")),
        ("Size", CimValue::Uint64(1_048_576)),
        ("BytesPerSector", CimValue::Uint32(512)),
        ("TotalSectors", CimValue::Uint64(2048)),
        ("Partitions", CimValue::Uint32(1)),
        ("InterfaceType", string("SCSI")),
    ] {
        assert_eq!(value(&records[0], name), &expected, "{name}");
    }
    assert_eq!(value(&records[1], "DeviceID"), &string("/dev/disk9"));
    assert_eq!(
        value(&records[1], "Manufacturer"),
        &string("(Standard disk drives)")
    );
    assert_eq!(value(&records[1], "InterfaceType"), &string("USB"));
    assert_eq!(value(&records[1], "Size"), &CimValue::Null);
    assert_eq!(value(&records[1], "TotalSectors"), &CimValue::Null);
    assert_eq!(
        value(&records[1], "Capabilities"),
        &CimValue::Array(vec![
            CimValue::Uint16(3),
            CimValue::Uint16(4),
            CimValue::Uint16(7)
        ])
    );
}

#[test]
fn explicit_partition_snapshot_maps_owner_offset_and_type() {
    let records = disk_partition::records(&Context::unrestricted(), &disks());
    assert_eq!(records.len(), 1);
    for (name, expected) in [
        ("DeviceID", string("Disk #7, Partition #1")),
        ("Index", CimValue::Uint32(1)),
        ("DiskIndex", CimValue::Uint32(7)),
        ("Size", CimValue::Uint64(524_288)),
        ("StartingOffset", CimValue::Uint64(4096)),
        ("NumberOfBlocks", CimValue::Uint64(1024)),
        ("Type", string("Apple_APFS")),
        ("PrimaryPartition", CimValue::Boolean(true)),
        ("Bootable", CimValue::Null),
    ] {
        assert_eq!(value(&records[0], name), &expected, "{name}");
    }
}

fn adapters() -> Vec<Adapter> {
    let interface = Interface {
        name: "en7".into(),
        index: 7,
        flags: (libc::IFF_UP | libc::IFF_RUNNING) as u32,
        mac: Some([0, 1, 2, 0xab, 0xcd, 0xef]),
        mtu: Some(1500),
        baudrate: Some(100_000_000),
        ipv4: vec![(
            "192.0.2.7".parse().unwrap(),
            "255.255.255.0".parse().unwrap(),
        )],
        ipv6: vec![("2001:db8::7".parse().unwrap(), 64)],
    };
    let mut virtual_interface = interface.clone();
    virtual_interface.name = "vmnet2".into();
    virtual_interface.index = 9;
    virtual_interface.flags = 0;
    virtual_interface.mac = None;
    virtual_interface.ipv4.clear();
    virtual_interface.ipv6 = vec![("fe80::9".parse().unwrap(), 64)];
    vec![
        Adapter {
            index: 7,
            interface,
            name: "Fixture Ethernet".into(),
            controller: Some(Controller {
                vendor: Some("Fixture".into()),
                model: Some("Ethernet".into()),
                link_speed: Some(1_000_000_000),
                link_status: Some(2),
            }),
        },
        Adapter {
            index: 9,
            interface: virtual_interface,
            name: "vmnet2".into(),
            controller: None,
        },
    ]
}

#[test]
fn explicit_adapter_snapshot_maps_linked_physical_and_disconnected_virtual_ports() {
    let records = network_adapter::records(
        &Context::unrestricted(),
        &adapters(),
        &HashMap::from([("en7".into(), "Fixture Port".into())]),
        None,
    );
    assert_eq!(records.len(), 2);
    for (name, expected) in [
        ("Index", CimValue::Uint32(7)),
        ("DeviceID", string("7")),
        ("Caption", string("[00000007] Fixture Ethernet")),
        ("MACAddress", string("00:01:02:AB:CD:EF")),
        ("NetConnectionID", string("Fixture Port")),
        ("PhysicalAdapter", CimValue::Boolean(true)),
        ("Speed", CimValue::Uint64(1_000_000_000)),
        ("NetConnectionStatus", CimValue::Uint16(2)),
    ] {
        assert_eq!(value(&records[0], name), &expected, "{name}");
    }
    assert_eq!(value(&records[1], "Index"), &CimValue::Uint32(9));
    assert_eq!(
        value(&records[1], "PhysicalAdapter"),
        &CimValue::Boolean(false)
    );
    assert_eq!(value(&records[1], "Speed"), &CimValue::Null);
    assert_eq!(value(&records[1], "MACAddress"), &CimValue::Null);
    assert_eq!(
        value(&records[1], "NetConnectionStatus"),
        &CimValue::Uint16(7)
    );
}

#[test]
fn explicit_network_state_maps_addresses_dhcp_dns_and_link_local_only_ports() {
    let state = NetworkState {
        services: vec![Service {
            id: "fixture-service".into(),
            interface: "en7".into(),
            ipv4_router: Some("192.0.2.1".into()),
            dhcp_configured: true,
            dhcp: Some(Dhcp {
                server: Some("192.0.2.2".into()),
                ..Default::default()
            }),
            dns: Dns {
                servers: vec!["192.0.2.53".into()],
                domain: Some("example.invalid".into()),
                search_domains: vec!["example.invalid".into()],
            },
            ..Default::default()
        }],
        ..Default::default()
    };
    let records = network_adapter_configuration::records(
        &Context::unrestricted(),
        &adapters(),
        &state,
        Some("fixture-host"),
    );
    assert_eq!(records.len(), 2);
    for (name, expected) in [
        ("Index", CimValue::Uint32(7)),
        ("IPEnabled", CimValue::Boolean(true)),
        (
            "IPAddress",
            CimValue::Array(vec![string("192.0.2.7"), string("2001:db8::7")]),
        ),
        (
            "IPSubnet",
            CimValue::Array(vec![string("255.255.255.0"), string("64")]),
        ),
        ("DHCPEnabled", CimValue::Boolean(true)),
        ("DHCPServer", string("192.0.2.2")),
        (
            "DefaultIPGateway",
            CimValue::Array(vec![string("192.0.2.1")]),
        ),
        (
            "DNSServerSearchOrder",
            CimValue::Array(vec![string("192.0.2.53")]),
        ),
        ("DNSDomain", string("example.invalid")),
        ("DNSHostName", string("fixture-host")),
    ] {
        assert_eq!(value(&records[0], name), &expected, "{name}");
    }
    assert_eq!(value(&records[1], "IPEnabled"), &CimValue::Boolean(false));
    assert_eq!(value(&records[1], "DHCPEnabled"), &CimValue::Null);
    assert_eq!(value(&records[1], "DNSServerSearchOrder"), &CimValue::Null);
    assert_eq!(value(&records[1], "DefaultIPGateway"), &CimValue::Null);
}
