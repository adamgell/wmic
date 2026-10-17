use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{
            interfaces::{self, Interface},
            netconfig,
            netdev::{self, Controller},
            sysctl, time,
        },
    },
    model::{ManagementError, Record},
};

pub struct NetworkAdapter;

/// One adapter the Win32 network classes report.
pub(super) struct Adapter {
    /// Kernel interface index; Win32 `Index` and `DeviceID`. It stays the same when another
    /// interface appears or disappears.
    pub index: u32,
    pub interface: Interface,
    pub controller: Option<Controller>,
    /// Product name, else the BSD name.
    pub name: String,
}

impl Adapter {
    /// `[00000007] Name`, as the Win32 classes format captions.
    pub fn caption(&self) -> String {
        format!("[{:08}] {}", self.index, self.name)
    }

    /// Ethernet-class interfaces other than the physical ports.
    pub fn is_virtual(&self) -> bool {
        self.interface.name.starts_with("bridge") || self.interface.name.starts_with("vmnet")
    }
}

/// True for `en0`, `bridge0`, `vmnet1` and similar names.
fn reported(name: &str) -> bool {
    ["en", "bridge", "vmnet"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|unit| !unit.is_empty() && unit.bytes().all(|b| b.is_ascii_digit()))
    })
}

pub(super) fn adapters() -> Vec<Adapter> {
    let mut controllers = netdev::controllers();
    interfaces::list()
        .into_iter()
        .filter(|i| reported(&i.name))
        .map(|interface| {
            let controller = controllers.remove(&interface.name);
            let name = controller
                .as_ref()
                .and_then(Controller::product_name)
                .unwrap_or_else(|| interface.name.clone());
            Adapter {
                index: interface.index,
                interface,
                controller,
                name,
            }
        })
        .collect()
}

impl ClassProvider for NetworkAdapter {
    fn class(&self) -> &'static str {
        "Win32_NetworkAdapter"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let ports = netconfig::port_names();
        let reset =
            sysctl::boot_time().map(|(seconds, micros)| time::cim_datetime(seconds, micros));
        Ok(records(ctx, &adapters(), &ports, reset.as_deref()))
    }
}

/// Map an explicit interface/controller snapshot without querying macOS.
pub(super) fn records(
    ctx: &Context,
    adapters: &[Adapter],
    ports: &std::collections::HashMap<String, String>,
    reset: Option<&str>,
) -> Vec<Record> {
    adapters
        .iter()
        .map(|adapter| {
            let interface = &adapter.interface;
            let controller = adapter.controller.as_ref();
            let linked = interface.is_running()
                && controller.and_then(Controller::link_active).unwrap_or(true);
            let speed = controller
                .and_then(|c| c.link_speed)
                .or(interface.baudrate)
                .filter(|_| linked);
            let connection = ports
                .get(&interface.name)
                .map_or(interface.name.as_str(), String::as_str);
            ctx.record("Win32_NetworkAdapter")
                .set("DeviceID", adapter.index.to_string())
                .set("Index", adapter.index)
                .set("InterfaceIndex", interface.index)
                .set("Name", adapter.name.as_str())
                .set("Description", adapter.name.as_str())
                .set("ProductName", adapter.name.as_str())
                .set("Caption", adapter.caption())
                .set("NetConnectionID", connection)
                .set("ServiceName", interface.name.as_str())
                .set("MACAddress", interface.mac_text())
                .set("AdapterType", "Ethernet 802.3")
                .set("AdapterTypeId", 0u16)
                .set(
                    "PhysicalAdapter",
                    controller.is_some() && !adapter.is_virtual(),
                )
                .set("Manufacturer", controller.and_then(|c| c.vendor.as_deref()))
                .set("Speed", speed)
                .set("NetEnabled", interface.is_up() && interface.is_running())
                .set("NetConnectionStatus", if linked { 2u16 } else { 7u16 })
                .set("Installed", true)
                .set("Availability", 3u16)
                .set("ConfigManagerErrorCode", 0u32)
                .set("ConfigManagerUserConfig", false)
                .set("PowerManagementSupported", false)
                .set("TimeOfLastReset", reset)
                .set("CreationClassName", "Win32_NetworkAdapter")
                .set("SystemCreationClassName", "Win32_ComputerSystem")
                .set("SystemName", ctx.server())
                .build()
        })
        .collect()
}
