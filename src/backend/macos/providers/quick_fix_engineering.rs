use crate::{
    backend::macos::{context::Context, provider::ClassProvider, sys::time},
    model::{ManagementError, Record},
};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

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

/// Install time of a history entry.
fn installed_at(entry: &plist::Dictionary) -> Option<SystemTime> {
    entry.get("date")?.as_date().map(SystemTime::from)
}

/// Install date in the `M/D/YYYY` form Windows prints.
fn installed_on(entry: &plist::Dictionary) -> Option<String> {
    let seconds = installed_at(entry)?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let local = time::local(seconds);
    Some(format!("{}/{}/{}", local.month, local.day, local.year))
}

/// `HotFixID` of an entry: display name and version; `None` when both are empty.
fn hot_fix_id(entry: &plist::Dictionary) -> Option<String> {
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(plist::Value::as_string)
            .unwrap_or_default()
    };
    let id = format!("{} {}", text("displayName"), text("displayVersion"));
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// One entry per `HotFixID`, the newest install date winning; first-seen order is kept. The
/// history repeats an update each time its payload is installed again.
pub fn newest_per_hot_fix<'a>(
    entries: impl IntoIterator<Item = &'a plist::Dictionary>,
) -> Vec<(String, &'a plist::Dictionary)> {
    let mut chosen: Vec<(String, &plist::Dictionary)> = vec![];
    let mut position: HashMap<String, usize> = HashMap::new();
    for entry in entries.into_iter().filter(|e| is_system_update(e)) {
        let Some(id) = hot_fix_id(entry) else {
            continue;
        };
        match position.get(&id) {
            Some(&at) => {
                if installed_at(entry) > installed_at(chosen[at].1) {
                    chosen[at].1 = entry;
                }
            }
            None => {
                position.insert(id.clone(), chosen.len());
                chosen.push((id, entry));
            }
        }
    }
    chosen
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
        Ok(
            newest_per_hot_fix(entries.iter().filter_map(plist::Value::as_dictionary))
                .into_iter()
                .map(|(hot_fix, entry)| {
                    ctx.record("Win32_QuickFixEngineering")
                        .set("HotFixID", hot_fix.as_str())
                        .set("Description", "Update")
                        .set("InstalledOn", installed_on(entry))
                        .set("CSName", ctx.server())
                        .build()
                })
                .collect(),
        )
    }
}
