use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{accounts, time},
    },
    model::{ManagementError, Record},
};
use std::{
    collections::HashSet,
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

/// One installed application bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub version: Option<String>,
    /// Bundle identifier; the bundle path when the bundle has none.
    pub identifier: String,
    pub path: PathBuf,
    pub installed: Option<String>,
    pub package: String,
}

/// Bundles under the roots, in root order. A bundle is listed once per `(identifier, name,
/// version)`, the key of `Win32_Product`; the first root wins.
pub fn scan(roots: &[PathBuf]) -> Vec<Entry> {
    let mut seen = HashSet::new();
    let mut entries = vec![];
    for bundle in roots.iter().flat_map(|root| bundles(root)) {
        let package = bundle
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = package.strip_suffix(".app").unwrap_or(&package);
        let info = info(&bundle);
        let name = first_text(info.as_ref(), &["CFBundleDisplayName", "CFBundleName"])
            .unwrap_or_else(|| stem.to_owned());
        let version = first_text(
            info.as_ref(),
            &["CFBundleShortVersionString", "CFBundleVersion"],
        );
        let identifier = first_text(info.as_ref(), &["CFBundleIdentifier"])
            .unwrap_or_else(|| bundle.to_string_lossy().into_owned());
        if !seen.insert((identifier.clone(), name.clone(), version.clone())) {
            continue;
        }
        entries.push(Entry {
            name,
            version,
            identifier,
            installed: install_date(&bundle),
            path: bundle,
            package,
        });
    }
    entries
}

/// `/Applications` and the session user's `~/Applications`.
pub fn roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    roots.extend(accounts::home_directory().map(|home| home.join("Applications")));
    roots
}

impl ClassProvider for Product {
    fn class(&self) -> &'static str {
        "Win32_Product"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(scan(&roots())
            .iter()
            .map(|entry| {
                ctx.record("Win32_Product")
                    .set("Name", entry.name.as_str())
                    .set("Caption", entry.name.as_str())
                    .set("Description", entry.name.as_str())
                    .set("Version", entry.version.as_deref())
                    .set("IdentifyingNumber", entry.identifier.as_str())
                    .set("InstallLocation", entry.path.to_string_lossy().into_owned())
                    .set("InstallDate", entry.installed.as_deref())
                    .set("PackageName", entry.package.as_str())
                    .set("InstallState", 5u16)
                    .set("AssignmentType", 1u16)
                    .build()
            })
            .collect())
    }
}
