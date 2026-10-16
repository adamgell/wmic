//! Network interfaces and their addresses from `getifaddrs`.
use std::{
    ffi::CStr,
    net::{Ipv4Addr, Ipv6Addr},
};

/// Leading fields of the kernel `if_data` record that `ifa_data` points to for link entries.
#[repr(C)]
struct LinkData {
    ifi_type: u8,
    ifi_typelen: u8,
    ifi_physical: u8,
    ifi_addrlen: u8,
    ifi_hdrlen: u8,
    ifi_recvquota: u8,
    ifi_xmitquota: u8,
    ifi_unused1: u8,
    ifi_mtu: u32,
    ifi_metric: u32,
    ifi_baudrate: u32,
}

#[derive(Clone, Debug)]
pub struct Interface {
    pub name: String,
    pub index: u32,
    pub flags: u32,
    pub mac: Option<[u8; 6]>,
    pub mtu: Option<u32>,
    pub baudrate: Option<u64>,
    pub ipv4: Vec<(Ipv4Addr, Ipv4Addr)>,
    /// Address and prefix length.
    pub ipv6: Vec<(Ipv6Addr, u8)>,
}

impl Interface {
    pub fn is_up(&self) -> bool {
        self.flags & libc::IFF_UP as u32 != 0
    }

    pub fn is_running(&self) -> bool {
        self.flags & libc::IFF_RUNNING as u32 != 0
    }

    pub fn mac_text(&self) -> Option<String> {
        self.mac.map(|m| {
            m.iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(":")
        })
    }
}

/// Bytes of a socket address from `offset` on, zero-filled past the length the kernel recorded.
/// Netmask addresses are stored trimmed, so bytes beyond `sa_len` are zero by definition.
unsafe fn address_bytes<const N: usize>(address: *const libc::sockaddr, offset: usize) -> [u8; N] {
    let mut out = [0u8; N];
    let length = unsafe { (*address).sa_len } as usize;
    for (i, slot) in out.iter_mut().enumerate() {
        if offset + i < length {
            *slot = unsafe { *address.cast::<u8>().add(offset + i) };
        }
    }
    out
}

fn ipv4_at(address: *const libc::sockaddr) -> Option<Ipv4Addr> {
    if address.is_null() {
        return None;
    }
    // `sin_addr` follows the length, family and port fields.
    let bytes = unsafe { address_bytes::<4>(address, 4) };
    Some(Ipv4Addr::from(bytes))
}

fn ipv6_at(address: *const libc::sockaddr) -> Option<Ipv6Addr> {
    if address.is_null() {
        return None;
    }
    // `sin6_addr` follows the length, family, port and flow-info fields.
    let bytes = unsafe { address_bytes::<16>(address, 8) };
    Some(Ipv6Addr::from(bytes))
}

/// Link-local addresses carry the zone in bytes 2 and 3; the visible address omits it.
fn without_zone(address: Ipv6Addr) -> Ipv6Addr {
    let mut bytes = address.octets();
    if bytes[0] == 0xfe && bytes[1] & 0xc0 == 0x80 {
        bytes[2] = 0;
        bytes[3] = 0;
    }
    Ipv6Addr::from(bytes)
}

fn hardware_address(address: *const libc::sockaddr) -> Option<[u8; 6]> {
    let link = address.cast::<libc::sockaddr_dl>();
    let (name_length, address_length, length) = unsafe {
        (
            usize::from((*link).sdl_nlen),
            usize::from((*link).sdl_alen),
            usize::from((*link).sdl_len),
        )
    };
    let start = std::mem::offset_of!(libc::sockaddr_dl, sdl_data) + name_length;
    if address_length != 6 || start + 6 > length {
        return None;
    }
    let bytes = unsafe { address_bytes::<6>(address, start) };
    Some(bytes)
}

fn entry<'a>(list: &'a mut Vec<Interface>, name: &str) -> &'a mut Interface {
    if let Some(at) = list.iter().position(|i| i.name == name) {
        return &mut list[at];
    }
    let index = std::ffi::CString::new(name)
        .map(|c| unsafe { libc::if_nametoindex(c.as_ptr()) })
        .unwrap_or(0);
    list.push(Interface {
        name: name.to_owned(),
        index,
        flags: 0,
        mac: None,
        mtu: None,
        baudrate: None,
        ipv4: vec![],
        ipv6: vec![],
    });
    list.last_mut().expect("entry was just pushed")
}

/// Every interface once, in interface-index order.
pub fn list() -> Vec<Interface> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return vec![];
    }
    let mut interfaces: Vec<Interface> = vec![];
    let mut cursor = head;
    while !cursor.is_null() {
        // The list stays valid until `freeifaddrs`.
        let item = unsafe { &*cursor };
        cursor = item.ifa_next;
        if item.ifa_name.is_null() {
            continue;
        }
        let name = unsafe { CStr::from_ptr(item.ifa_name) }
            .to_string_lossy()
            .into_owned();
        let found = entry(&mut interfaces, &name);
        found.flags = item.ifa_flags;
        if item.ifa_addr.is_null() {
            continue;
        }
        match i32::from(unsafe { (*item.ifa_addr).sa_family }) {
            libc::AF_LINK => {
                found.mac = hardware_address(item.ifa_addr);
                if !item.ifa_data.is_null() {
                    let data = unsafe { &*item.ifa_data.cast::<LinkData>() };
                    found.mtu = Some(data.ifi_mtu).filter(|v| *v > 0);
                    found.baudrate = Some(u64::from(data.ifi_baudrate)).filter(|v| *v > 0);
                }
            }
            libc::AF_INET => {
                if let Some(address) = ipv4_at(item.ifa_addr) {
                    let mask = ipv4_at(item.ifa_netmask).unwrap_or(Ipv4Addr::UNSPECIFIED);
                    found.ipv4.push((address, mask));
                }
            }
            libc::AF_INET6 => {
                if let Some(address) = ipv6_at(item.ifa_addr) {
                    let prefix = ipv6_at(item.ifa_netmask)
                        .map_or(0, |mask| mask.octets().iter().map(|b| b.count_ones()).sum());
                    found.ipv6.push((without_zone(address), prefix as u8));
                }
            }
            _ => {}
        }
    }
    unsafe { libc::freeifaddrs(head) };
    interfaces.sort_by_key(|i| i.index);
    interfaces
}
