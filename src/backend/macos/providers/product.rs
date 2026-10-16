use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{accounts, time},
    },
    model::{ManagementError, Record},
};
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub struct Product;

/// Application bundles directly inside a directory, ordered by name.
fn bundles(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return vec![];
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app") && p.is_dir())
        .collect();
    paths.sort();
    paths
}

fn info(bundle: &Path) -> Option<plist::Dictionary> {
    plist::Value::from_file(bundle.join("Contents/Info.plist"))
        .ok()?
        .into_dictionary()
}

fn first_text(info: Option<&plist::Dictionary>, keys: &[&str]) -> Option<String> {
    let info = info?;
    keys.iter().find_map(|key| {
        info.get(key)
            .and_then(plist::Value::as_string)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    })
}

/// Modification time of the bundle directory as `yyyymmdd` in local time.
fn install_date(bundle: &Path) -> Option<String> {
    let modified = std::fs::metadata(bundle).ok()?.modified().ok()?;
    let seconds = modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let date = time::local(seconds);
    Some(format!("{:04}{:02}{:02}", date.year, date.month, date.day))
}

impl ClassProvider for Product {
    fn class(&self) -> &'static str {
        "Win32_Product"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let mut roots = vec![PathBuf::from("/Applications")];
        roots.extend(accounts::home_directory().map(|home| home.join("Applications")));
        let mut records = vec![];
        for bundle in roots.iter().flat_map(|root| bundles(root)) {
            let file_name = bundle
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let stem = file_name.strip_suffix(".app").unwrap_or(&file_name);
            let info = info(&bundle);
            let name = first_text(info.as_ref(), &["CFBundleDisplayName", "CFBundleName"])
                .unwrap_or_else(|| stem.to_owned());
            records.push(
                ctx.record("Win32_Product")
                    .set("Name", name.as_str())
                    .set("Caption", name.as_str())
                    .set("Description", name.as_str())
                    .set(
                        "Version",
                        first_text(
                            info.as_ref(),
                            &["CFBundleShortVersionString", "CFBundleVersion"],
                        ),
                    )
                    .set(
                        "IdentifyingNumber",
                        first_text(info.as_ref(), &["CFBundleIdentifier"]),
                    )
                    .set("InstallLocation", bundle.to_string_lossy().into_owned())
                    .set("InstallDate", install_date(&bundle))
                    .set("PackageName", file_name.as_str())
                    .set("InstallState", 5u16)
                    .set("AssignmentType", 1u16)
                    .build(),
            );
        }
        Ok(records)
    }
}
