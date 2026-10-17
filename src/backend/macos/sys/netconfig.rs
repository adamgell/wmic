//! IP configuration and port names from SystemConfiguration.
use super::cfvalue::{self, Dictionary};
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    string::CFString,
};
use std::{collections::HashMap, ffi::c_void};

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCDynamicStoreCreate(
        allocator: *const c_void,
        name: *const c_void,
        callout: *const c_void,
        context: *const c_void,
    ) -> *const c_void;
    fn SCDynamicStoreCopyValue(store: *const c_void, key: *const c_void) -> *const c_void;
    fn SCDynamicStoreCopyKeyList(store: *const c_void, pattern: *const c_void) -> *const c_void;
    fn SCNetworkInterfaceCopyAll() -> *const c_void;
    fn SCNetworkInterfaceGetBSDName(interface: *const c_void) -> *const c_void;
    fn SCNetworkInterfaceGetLocalizedDisplayName(interface: *const c_void) -> *const c_void;
}

/// Dynamic store session; the reference is released on drop.
struct Store(CFType);

impl Store {
    fn open() -> Option<Store> {
        let name = CFString::new("wmic");
        let raw = unsafe {
            SCDynamicStoreCreate(
                std::ptr::null(),
                name.as_concrete_TypeRef().cast(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        // The Create rule hands over one reference.
        (!raw.is_null()).then(|| Store(unsafe { CFType::wrap_under_create_rule(raw.cast()) }))
    }

    fn value(&self, key: &str) -> Option<CFType> {
        let key = CFString::new(key);
        let raw = unsafe {
            SCDynamicStoreCopyValue(
                self.0.as_CFTypeRef().cast(),
                key.as_concrete_TypeRef().cast(),
            )
        };
        // The Copy rule hands over one reference.
        (!raw.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(raw.cast()) })
    }

    fn dictionary(&self, key: &str) -> Option<Dictionary> {
        cfvalue::dictionary(&self.value(key)?)
    }

    fn keys(&self, pattern: &str) -> Vec<String> {
        let pattern = CFString::new(pattern);
        let raw = unsafe {
            SCDynamicStoreCopyKeyList(
                self.0.as_CFTypeRef().cast(),
                pattern.as_concrete_TypeRef().cast(),
            )
        };
        if raw.is_null() {
            return vec![];
        }
        // The Copy rule hands over one reference.
        let list = unsafe { CFType::wrap_under_create_rule(raw.cast()) };
        let Some(list) = cfvalue::array(&list) else {
            return vec![];
        };
        list.iter()
            .filter_map(|key| key.downcast::<CFString>().map(|s| s.to_string()))
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Dns {
    pub servers: Vec<String>,
    pub domain: Option<String>,
    pub search_domains: Vec<String>,
}

impl Dns {
    fn read(dictionary: Option<&Dictionary>) -> Dns {
        let Some(dictionary) = dictionary else {
            return Dns::default();
        };
        Dns {
            servers: cfvalue::strings(dictionary, "ServerAddresses"),
            domain: cfvalue::text(dictionary, "DomainName").filter(|d| !d.is_empty()),
            search_domains: cfvalue::strings(dictionary, "SearchDomains"),
        }
    }

    fn is_empty(&self) -> bool {
        self.servers.is_empty() && self.domain.is_none() && self.search_domains.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Dhcp {
    pub server: Option<String>,
    /// Unix time.
    pub lease_start: Option<i64>,
    /// Unix time.
    pub lease_end: Option<i64>,
}

/// Configuration of the network service bound to one interface.
#[derive(Clone, Debug, Default)]
pub struct Service {
    pub id: String,
    pub interface: String,
    pub ipv4_router: Option<String>,
    pub ipv6_router: Option<String>,
    /// The service obtains its address by DHCP.
    pub dhcp_configured: bool,
    pub dhcp: Option<Dhcp>,
    pub dns: Dns,
}

#[derive(Clone, Debug, Default)]
pub struct NetworkState {
    pub services: Vec<Service>,
    pub primary_ipv4_interface: Option<String>,
    pub primary_ipv4_router: Option<String>,
    pub primary_ipv6_interface: Option<String>,
    pub primary_ipv6_router: Option<String>,
    pub global_dns: Dns,
}

impl NetworkState {
    pub fn service(&self, interface: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.interface == interface)
    }

    /// IPv4 gateway first, then IPv6.
    pub fn gateways(&self, interface: &str) -> Vec<String> {
        let service = self.service(interface);
        let v4 = service.and_then(|s| s.ipv4_router.clone()).or_else(|| {
            (self.primary_ipv4_interface.as_deref() == Some(interface))
                .then(|| self.primary_ipv4_router.clone())
                .flatten()
        });
        let v6 = service.and_then(|s| s.ipv6_router.clone()).or_else(|| {
            (self.primary_ipv6_interface.as_deref() == Some(interface))
                .then(|| self.primary_ipv6_router.clone())
                .flatten()
        });
        v4.into_iter().chain(v6).collect()
    }

    /// DNS of the interface's service, else the global resolver configuration.
    pub fn dns(&self, interface: &str) -> &Dns {
        self.service(interface)
            .map(|s| &s.dns)
            .filter(|d| !d.is_empty())
            .unwrap_or(&self.global_dns)
    }
}

/// Router address without a zone suffix.
fn router(dictionary: &Dictionary) -> Option<String> {
    let text = cfvalue::text(dictionary, "Router")?;
    let address = text.split('%').next().unwrap_or_default();
    (!address.is_empty()).then(|| address.to_owned())
}

/// Four-byte server identifier as dotted decimal.
fn dotted(bytes: &[u8]) -> Option<String> {
    let octets: [u8; 4] = bytes.try_into().ok()?;
    Some(std::net::Ipv4Addr::from(octets).to_string())
}

fn dhcp(dictionary: &Dictionary) -> Dhcp {
    let server = cfvalue::text(dictionary, "ServerIdentifier")
        .filter(|s| s.parse::<std::net::Ipv4Addr>().is_ok())
        .or_else(|| cfvalue::bytes(dictionary, "Option_54").and_then(|b| dotted(&b)));
    Dhcp {
        server,
        lease_start: cfvalue::unix_time(dictionary, "LeaseStartTime"),
        lease_end: cfvalue::unix_time(dictionary, "LeaseExpirationTime"),
    }
}

fn service(store: &Store, id: &str, interface: String) -> Service {
    let state = |leaf: &str| store.dictionary(&format!("State:/Network/Service/{id}/{leaf}"));
    let configured = store
        .dictionary(&format!("Setup:/Network/Service/{id}/IPv4"))
        .and_then(|d| cfvalue::text(&d, "ConfigMethod"))
        .is_some_and(|method| method.eq_ignore_ascii_case("DHCP"));
    Service {
        id: id.to_owned(),
        interface,
        ipv4_router: state("IPv4").as_ref().and_then(router),
        ipv6_router: state("IPv6").as_ref().and_then(router),
        dhcp_configured: configured,
        dhcp: state("DHCP").as_ref().map(dhcp),
        dns: Dns::read(state("DNS").as_ref()),
    }
}

/// Current dynamic network configuration; empty when the store is unreachable.
pub fn network_state() -> NetworkState {
    let Some(store) = Store::open() else {
        return NetworkState::default();
    };
    let mut state = NetworkState::default();
    for key in store.keys("State:/Network/Service/[^/]+/IPv4") {
        let id = key
            .strip_prefix("State:/Network/Service/")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or_default();
        let interface = store
            .dictionary(&key)
            .and_then(|d| cfvalue::text(&d, "InterfaceName"));
        if let Some(interface) = interface.filter(|_| !id.is_empty()) {
            state.services.push(service(&store, id, interface));
        }
    }
    if let Some(global) = store.dictionary("State:/Network/Global/IPv4") {
        state.primary_ipv4_interface = cfvalue::text(&global, "PrimaryInterface");
        state.primary_ipv4_router = router(&global);
    }
    if let Some(global) = store.dictionary("State:/Network/Global/IPv6") {
        state.primary_ipv6_interface = cfvalue::text(&global, "PrimaryInterface");
        state.primary_ipv6_router = router(&global);
    }
    state.global_dns = Dns::read(store.dictionary("State:/Network/Global/DNS").as_ref());
    state
}

/// User-visible port names such as `Ethernet` and `Wi-Fi`, keyed by BSD name.
pub fn port_names() -> HashMap<String, String> {
    let raw = unsafe { SCNetworkInterfaceCopyAll() };
    if raw.is_null() {
        return HashMap::new();
    }
    // The Copy rule hands over one reference.
    let all = unsafe { CFType::wrap_under_create_rule(raw.cast()) };
    let Some(all) = all.downcast::<CFArray>() else {
        return HashMap::new();
    };
    let mut names = HashMap::new();
    for item in all.get_all_values() {
        // Get-rule strings stay valid while `all` holds the interface.
        let bsd = unsafe { SCNetworkInterfaceGetBSDName(item) };
        let display = unsafe { SCNetworkInterfaceGetLocalizedDisplayName(item) };
        if bsd.is_null() || display.is_null() {
            continue;
        }
        let bsd = unsafe { CFString::wrap_under_get_rule(bsd.cast()) }.to_string();
        let display = unsafe { CFString::wrap_under_get_rule(display.cast()) }.to_string();
        names.insert(bsd, display);
    }
    names
}
