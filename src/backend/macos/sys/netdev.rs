//! Network controllers behind interfaces, from the IORegistry.
use super::{iokit::Entry, registry};
use std::collections::HashMap;

/// `kIONetworkLinkActive` in `IOLinkStatus`.
const LINK_ACTIVE: i64 = 0x2;

#[derive(Clone, Debug, Default)]
pub struct Controller {
    pub vendor: Option<String>,
    pub model: Option<String>,
    /// Bits per second.
    pub link_speed: Option<u64>,
    pub link_status: Option<i64>,
}

impl Controller {
    pub fn link_active(&self) -> Option<bool> {
        self.link_status.map(|s| s & LINK_ACTIVE != 0)
    }

    /// Vendor and model joined, without repeating a vendor the model already starts with.
    pub fn product_name(&self) -> Option<String> {
        let model = self.model.as_deref()?;
        Some(match self.vendor.as_deref() {
            Some(vendor) if !model.starts_with(vendor) => format!("{vendor} {model}"),
            _ => model.to_owned(),
        })
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn controller(entry: &Entry) -> Controller {
    Controller {
        vendor: non_empty(entry.string("IOVendor")),
        model: non_empty(entry.string("IOModel")),
        link_speed: entry
            .number("IOLinkSpeed")
            .and_then(|v| u64::try_from(v).ok())
            .filter(|v| *v > 0),
        link_status: entry.number("IOLinkStatus"),
    }
}

/// Controller of every network interface that has one, keyed by BSD name.
pub fn controllers() -> HashMap<String, Controller> {
    let mut found = HashMap::new();
    for interface in Entry::matching_services("IONetworkInterface") {
        let Some(name) = interface.string("BSD Name") else {
            continue;
        };
        if let Some(parent) = registry::ancestor_conforming_to(&interface, "IONetworkController", 3)
        {
            found.insert(name, controller(&parent));
        }
    }
    found
}
