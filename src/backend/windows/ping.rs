use crate::model::WideText;
use std::net::{SocketAddr, ToSocketAddrs};
use windows::Win32::{Foundation::HANDLE, NetworkManagement::IpHelper::*, Networking::WinSock::*};
struct IcmpHandle(HANDLE);
impl Drop for IcmpHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = IcmpCloseHandle(self.0);
        }
    }
}
pub fn reachable(node: &WideText, timeout: u32) -> bool {
    let Ok(node) = String::from_utf16(&node.0) else {
        return false;
    };
    let Ok(mut addresses) = (node.as_str(), 0).to_socket_addrs() else {
        return true;
    };
    let Some(address) = addresses.next() else {
        return true;
    };
    let data: [u8; 32] = std::array::from_fn(|i| b'a' + (i % 23) as u8);
    let options = IP_OPTION_INFORMATION {
        Ttl: 128,
        ..Default::default()
    };
    // The reply buffer is aligned for either native reply structure, including 32-bit pointers.
    let mut reply = vec![0usize; 8184usize.div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        match address {
            SocketAddr::V4(a) => {
                let Ok(handle) = IcmpCreateFile() else {
                    return false;
                };
                let handle = IcmpHandle(handle);
                let count = IcmpSendEcho(
                    handle.0,
                    u32::from_ne_bytes(a.ip().octets()),
                    data.as_ptr().cast(),
                    data.len() as u16,
                    Some(&options),
                    reply.as_mut_ptr().cast(),
                    8184,
                    timeout,
                );
                count > 0 && (*reply.as_ptr().cast::<ICMP_ECHO_REPLY>()).Status == 0
            }
            SocketAddr::V6(a) => {
                let Ok(handle) = Icmp6CreateFile() else {
                    return false;
                };
                let handle = IcmpHandle(handle);
                let source = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    ..Default::default()
                };
                let destination = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_addr: IN6_ADDR {
                        u: IN6_ADDR_0 {
                            Byte: a.ip().octets(),
                        },
                    },
                    Anonymous: SOCKADDR_IN6_0 {
                        sin6_scope_id: a.scope_id(),
                    },
                    ..Default::default()
                };
                let count = Icmp6SendEcho2(
                    handle.0,
                    None,
                    None,
                    None,
                    &source,
                    &destination,
                    data.as_ptr().cast(),
                    data.len() as u16,
                    Some(&options),
                    reply.as_mut_ptr().cast(),
                    8184,
                    timeout,
                );
                count > 0 && (*reply.as_ptr().cast::<ICMPV6_ECHO_REPLY_LH>()).Status == 0
            }
        }
    }
}
