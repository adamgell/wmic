use crate::{
    backend::macos::{context::Context, provider::ClassProvider, sys::time},
    model::{ManagementError, Record},
};
use std::time::UNIX_EPOCH;

pub struct QuickFixEngineering;

const INSTALL_HISTORY: &str = "/Library/Receipts/InstallHistory.plist";

/// Operating-system updates; the history also lists third-party installers.
fn is_system_update(entry: &plist::Dictionary) -> bool {
    let text = |key: &str| entry.get(key).and_then(plist::Value::as_string);
    matches!(
        text("processName"),
        Some("softwareupdated" | "Software Update")
    ) || text("displayName").is_some_and(|n| n.starts_with("macOS"))
}

/// Install date in the `M/D/YYYY` form Windows prints.
fn installed_on(entry: &plist::Dictionary) -> Option<String> {
    let date = entry.get("date")?.as_date()?;
    let seconds = std::time::SystemTime::from(date)
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let local = time::local(seconds);
    Some(format!("{}/{}/{}", local.month, local.day, local.year))
}

impl ClassProvider for QuickFixEngineering {
    fn class(&self) -> &'static str {
        "Win32_QuickFixEngineering"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let history = plist::Value::from_file(INSTALL_HISTORY).ok();
        let entries = history
            .as_ref()
            .and_then(plist::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        Ok(entries
            .iter()
            .filter_map(plist::Value::as_dictionary)
            .filter(|entry| is_system_update(entry))
            .filter_map(|entry| {
                let text = |key: &str| {
                    entry
                        .get(key)
                        .and_then(plist::Value::as_string)
                        .unwrap_or_default()
                };
                let hot_fix = format!("{} {}", text("displayName"), text("displayVersion"));
                let hot_fix = hot_fix.trim();
                (!hot_fix.is_empty()).then(|| {
                    ctx.record("Win32_QuickFixEngineering")
                        .set("HotFixID", hot_fix)
                        .set("Description", "Update")
                        .set("InstalledOn", installed_on(entry))
                        .set("CSName", ctx.server())
                        .build()
                })
            })
            .collect())
    }
}
