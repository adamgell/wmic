//! launchd: `launchctl` invocation and output parsing, plist job definitions and the merged
//! service inventory. `launchctl` is the only interface launchd offers, so this is the one module
//! that starts a subprocess.
use super::{accounts, cmdline};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const LAUNCHCTL: &str = "/bin/launchctl";
/// A `launchctl print` of a busy domain stays far below this; a larger stream is cut off.
const OUTPUT_LIMIT: u64 = 32 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Domain {
    System,
    Gui(u32),
}

impl Domain {
    /// Domain specifier as `launchctl` spells it.
    pub fn target(&self) -> String {
        match self {
            Self::System => "system".into(),
            Self::Gui(uid) => format!("gui/{uid}"),
        }
    }

    /// True when the current user may change services in the domain.
    pub fn manageable(&self) -> bool {
        match self {
            Self::System => unsafe { libc::geteuid() == 0 },
            Self::Gui(_) => true,
        }
    }
}

/// Result of one `launchctl` run.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

fn read_bounded(source: impl Read, limit: u64) -> (Vec<u8>, bool) {
    let mut bytes = vec![];
    let _ = source.take(limit).read_to_end(&mut bytes);
    let truncated = bytes.len() as u64 >= limit;
    (bytes, truncated)
}

/// Runs `/bin/launchctl` with a scrubbed environment and no input.
pub fn run(arguments: &[String]) -> Outcome {
    let spawned = Command::new(LAUNCHCTL)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let Ok(mut child) = spawned else {
        return Outcome::default();
    };
    let errors = child
        .stderr
        .take()
        .map(|stream| std::thread::spawn(move || read_bounded(stream, 1 << 20).0));
    let (stdout, truncated) = child
        .stdout
        .take()
        .map_or((vec![], false), |stream| read_bounded(stream, OUTPUT_LIMIT));
    if truncated {
        let _ = child.kill();
    }
    let status = child.wait();
    let stderr = errors
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    Outcome {
        success: status.is_ok_and(|s| s.success()) && !truncated,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

/// One row of the `services = { ... }` block of `launchctl print <domain>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEntry {
    /// 0 when the job has no process.
    pub pid: u32,
    /// Last exit status; `None` when the column shows `-` or a marker such as `(pe)`.
    pub status: Option<i32>,
    pub label: String,
}

/// Rows of the first top-level `services = {` block.
pub fn parse_services(text: &str) -> Vec<RuntimeEntry> {
    let mut inside = false;
    let mut entries = vec![];
    for line in text.lines() {
        let trimmed = line.trim();
        if !inside {
            inside = trimmed == "services = {";
            continue;
        }
        if trimmed == "}" {
            break;
        }
        let mut fields = trimmed.split_whitespace();
        let (Some(pid), Some(status), Some(label)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let pid = if pid == "-" {
            0
        } else if let Ok(pid) = pid.parse() {
            pid
        } else {
            continue;
        };
        entries.push(RuntimeEntry {
            pid,
            status: status.parse().ok(),
            label: label.to_owned(),
        });
    }
    entries
}

/// Override database of `launchctl print-disabled`: label to disabled flag.
pub fn parse_disabled(text: &str) -> HashMap<String, bool> {
    let mut overrides = HashMap::new();
    for line in text.lines() {
        let Some((label, state)) = line.trim().split_once("=>") else {
            continue;
        };
        let label = label.trim();
        let Some(label) = label
            .strip_prefix('"')
            .and_then(|l| l.strip_suffix('"'))
            .filter(|l| !l.is_empty())
        else {
            continue;
        };
        match state.trim() {
            "disabled" | "true" => overrides.insert(label.to_owned(), true),
            "enabled" | "false" => overrides.insert(label.to_owned(), false),
            _ => continue,
        };
    }
    overrides
}

/// Job definition read from a launchd property list.
#[derive(Clone, Debug)]
pub struct Definition {
    pub label: String,
    pub path: PathBuf,
    pub program: Option<String>,
    pub arguments: Vec<String>,
    pub run_at_load: bool,
    pub keep_alive: bool,
    pub user_name: Option<String>,
    pub disabled: bool,
}

impl Definition {
    /// Executable and arguments as one command line; the first argument is the program's own
    /// name, so it is dropped when `Program` is present.
    pub fn command_line(&self) -> Option<String> {
        let executable = self
            .program
            .as_deref()
            .or_else(|| self.arguments.first().map(String::as_str))?;
        let mut parts = vec![executable];
        parts.extend(self.arguments.iter().skip(1).map(String::as_str));
        Some(cmdline::join(&parts))
    }
}

pub fn parse_definition(value: &plist::Value, path: &Path) -> Option<Definition> {
    let dictionary = value.as_dictionary()?;
    let text = |key: &str| {
        dictionary
            .get(key)
            .and_then(plist::Value::as_string)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let flag = |key: &str| {
        dictionary
            .get(key)
            .and_then(plist::Value::as_boolean)
            .unwrap_or(false)
    };
    let label = text("Label").or_else(|| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    })?;
    Some(Definition {
        label,
        path: path.to_owned(),
        program: text("Program"),
        arguments: dictionary
            .get("ProgramArguments")
            .and_then(plist::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_string().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        run_at_load: flag("RunAtLoad"),
        keep_alive: match dictionary.get("KeepAlive") {
            Some(plist::Value::Boolean(value)) => *value,
            Some(plist::Value::Dictionary(conditions)) => !conditions.is_empty(),
            _ => false,
        },
        user_name: text("UserName"),
        disabled: flag("Disabled"),
    })
}

pub fn read_definition(path: &Path) -> Option<Definition> {
    parse_definition(&plist::Value::from_file(path).ok()?, path)
}

/// Definitions of the `.plist` files directly inside a directory, ordered by path.
pub fn definitions(directory: &Path) -> Vec<Definition> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return vec![];
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "plist"))
        .collect();
    paths.sort();
    paths.iter().filter_map(|p| read_definition(p)).collect()
}

/// Directories that hold the definitions of a domain.
pub fn directories(domain: &Domain) -> Vec<PathBuf> {
    match domain {
        Domain::System => vec![
            "/System/Library/LaunchDaemons".into(),
            "/Library/LaunchDaemons".into(),
        ],
        Domain::Gui(_) => {
            let mut paths: Vec<PathBuf> = vec![
                "/System/Library/LaunchAgents".into(),
                "/Library/LaunchAgents".into(),
            ];
            paths.extend(user_agents_directory());
            paths
        }
    }
}

pub fn user_agents_directory() -> Option<PathBuf> {
    accounts::home_directory().map(|home| home.join("Library/LaunchAgents"))
}

/// Process state of a job as the domain reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runtime {
    pub loaded: bool,
    pub pid: u32,
    pub status: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct Service {
    pub label: String,
    pub domain: Domain,
    pub definition: Option<Definition>,
    /// `None` when `launchctl` could not report the domain.
    pub runtime: Option<Runtime>,
    pub disabled: bool,
}

impl Service {
    pub fn running(&self) -> bool {
        self.runtime.is_some_and(|r| r.pid > 0)
    }

    pub fn loaded(&self) -> bool {
        self.runtime.is_some_and(|r| r.loaded)
    }

    fn target(&self) -> String {
        format!("{}/{}", self.domain.target(), self.label)
    }

    pub fn kickstart_arguments(&self) -> Vec<String> {
        vec!["kickstart".into(), self.target()]
    }

    /// Loads the job from its plist; `None` when the job has no plist.
    pub fn bootstrap_arguments(&self) -> Option<Vec<String>> {
        let definition = self.definition.as_ref()?;
        Some(vec![
            "bootstrap".into(),
            self.domain.target(),
            definition.path.to_string_lossy().into_owned(),
        ])
    }

    /// Jobs that launchd restarts are unloaded; others receive SIGTERM.
    pub fn stop_arguments(&self) -> Vec<String> {
        if self.definition.as_ref().is_some_and(|d| d.keep_alive) {
            vec!["bootout".into(), self.target()]
        } else {
            vec!["kill".into(), "SIGTERM".into(), self.target()]
        }
    }

    pub fn enable_arguments(&self, enabled: bool) -> Vec<String> {
        vec![
            (if enabled { "enable" } else { "disable" }).into(),
            self.target(),
        ]
    }
}

/// Merges the runtime rows, override database and plist definitions of one domain.
/// `runtime` is `None` when `launchctl print` failed.
pub fn merge(
    domain: &Domain,
    runtime: Option<Vec<RuntimeEntry>>,
    overrides: &HashMap<String, bool>,
    definitions: Vec<Definition>,
) -> Vec<Service> {
    let known = runtime.is_some();
    let mut rows: HashMap<String, RuntimeEntry> = runtime
        .unwrap_or_default()
        .into_iter()
        .map(|row| (row.label.clone(), row))
        .collect();
    let mut services = vec![];
    let mut seen = std::collections::HashSet::new();
    let state = |row: Option<RuntimeEntry>| {
        known.then_some(match row {
            Some(row) => Runtime {
                loaded: true,
                pid: row.pid,
                status: row.status,
            },
            None => Runtime {
                loaded: false,
                pid: 0,
                status: None,
            },
        })
    };
    for definition in definitions {
        if !seen.insert(definition.label.clone()) {
            continue;
        }
        let label = definition.label.clone();
        services.push(Service {
            disabled: overrides
                .get(&label)
                .copied()
                .unwrap_or(definition.disabled),
            runtime: state(rows.remove(&label)),
            definition: Some(definition),
            label,
            domain: domain.clone(),
        });
    }
    let mut remaining: Vec<RuntimeEntry> = rows.into_values().collect();
    remaining.sort_by(|a, b| a.label.cmp(&b.label));
    for row in remaining {
        services.push(Service {
            disabled: overrides.get(&row.label).copied().unwrap_or(false),
            label: row.label.clone(),
            domain: domain.clone(),
            definition: None,
            runtime: state(Some(row)),
        });
    }
    services
}

fn domain_services(domain: Domain) -> Vec<Service> {
    let printed = run(&["print".into(), domain.target()]);
    let runtime =
        Some(parse_services(&printed.stdout)).filter(|rows| printed.success && !rows.is_empty());
    let disabled = run(&["print-disabled".into(), domain.target()]);
    let overrides = if disabled.success {
        parse_disabled(&disabled.stdout)
    } else {
        HashMap::new()
    };
    let definitions = directories(&domain)
        .iter()
        .flat_map(|d| definitions(d))
        .collect();
    merge(&domain, runtime, &overrides, definitions)
}

/// One service per label; a label present in both domains keeps its running copy, else the
/// system copy.
pub fn combine(groups: Vec<Vec<Service>>) -> Vec<Service> {
    let mut chosen: HashMap<String, Service> = HashMap::new();
    for service in groups.into_iter().flatten() {
        match chosen.get(&service.label) {
            Some(existing) if existing.running() || !service.running() => {}
            _ => {
                chosen.insert(service.label.clone(), service);
            }
        }
    }
    let mut services: Vec<Service> = chosen.into_values().collect();
    services.sort_by_cached_key(|s| s.label.to_ascii_lowercase());
    services
}

/// Every launchd job of the system domain and of the current user's GUI domain.
pub fn services() -> Vec<Service> {
    let uid = unsafe { libc::getuid() };
    combine(vec![
        domain_services(Domain::System),
        domain_services(Domain::Gui(uid)),
    ])
}

/// Windows service return code for a finished state-changing `launchctl` run.
pub fn return_value(outcome: &Outcome) -> u32 {
    if outcome.success {
        return 0;
    }
    let text = outcome.combined().to_ascii_lowercase();
    if ["not permitted", "not privileged", "permission denied"]
        .iter()
        .any(|m| text.contains(m))
    {
        2
    } else if ["could not find service", "no such process"]
        .iter()
        .any(|m| text.contains(m))
    {
        6
    } else {
        8
    }
}
