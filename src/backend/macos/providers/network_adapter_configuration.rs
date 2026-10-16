use super::network_adapter::adapters;
use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{host, netconfig, time},
    },
    model::{ManagementError, Record},
};
use std::net::Ipv6Addr;

pub struct NetworkAdapterConfiguration;

fn link_local(address: &Ipv6Addr) -> bool {
    address.segments()[0] & 0xffc0 == 0xfe80
}

fn or_null(values: Vec<String>) -> Option<Vec<String>> {
    (!values.is_empty()).then_some(values)
}

fn lease_time(seconds: i64) -> String {
    time::cim_datetime(seconds, 0)
}

impl ClassProvider for NetworkAdapterConfiguration {
    fn class(&self) -> &'static str {
        "Win32_NetworkAdapterConfiguration"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let state = netconfig::network_state();
        let host_label = host::host_label();
        let records = adapters()
            .iter()
            .map(|adapter| {
                let interface = &adapter.interface;
                let enabled = !interface.ipv4.is_empty()
                    || interface
                        .ipv6
                        .iter()
                        .any(|(address, _)| !link_local(address));
                let mut addresses = vec![];
                let mut subnets = vec![];
                for (address, mask) in &interface.ipv4 {
                    addresses.push(address.to_string());
                    subnets.push(mask.to_string());
                }
                for (address, prefix) in &interface.ipv6 {
                    addresses.push(address.to_string());
                    subnets.push(prefix.to_string());
                }
                let service = state.service(&interface.name);
                let dhcp = service.and_then(|s| s.dhcp.as_ref());
                let dhcp_enabled = if service.is_some_and(|s| s.dhcp_configured || s.dhcp.is_some())
                {
                    Some(true)
                } else if interface.ipv4.is_empty() {
                    None
                } else {
                    Some(false)
                };
                let dns = enabled.then(|| state.dns(&interface.name));
                ctx.record("Win32_NetworkAdapterConfiguration")
                    .set("Index", adapter.index)
                    .set("InterfaceIndex", interface.index)
                    .set("Description", adapter.name.as_str())
                    .set("Caption", adapter.caption())
                    .set("ServiceName", interface.name.as_str())
                    .set("MACAddress", interface.mac_text())
                    .set("IPEnabled", enabled)
                    .set("IPAddress", or_null(addresses))
                    .set("IPSubnet", or_null(subnets))
                    .set("DefaultIPGateway", or_null(state.gateways(&interface.name)))
                    .set("DHCPEnabled", dhcp_enabled)
                    .set("DHCPServer", dhcp.and_then(|d| d.server.as_deref()))
                    .set(
                        "DHCPLeaseObtained",
                        dhcp.and_then(|d| d.lease_start).map(lease_time),
                    )
                    .set(
                        "DHCPLeaseExpires",
                        dhcp.and_then(|d| d.lease_end).map(lease_time),
                    )
                    .set(
                        "DNSServerSearchOrder",
                        dns.and_then(|d| or_null(d.servers.clone())),
                    )
                    .set("DNSDomain", dns.and_then(|d| d.domain.as_deref()))
                    .set(
                        "DNSDomainSuffixSearchOrder",
                        dns.and_then(|d| or_null(d.search_domains.clone())),
                    )
                    .set("DNSHostName", host_label.as_deref())
                    .set("DNSEnabledForWINSResolution", false)
                    .set("DomainDNSRegistrationEnabled", false)
                    .set("FullDNSRegistrationEnabled", false)
                    .set("MTU", interface.mtu)
                    .set("IPFilterSecurityEnabled", false)
                    .set("IPXEnabled", false)
                    .build()
            })
            .collect();
        Ok(records)
    }
}
