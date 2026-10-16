use crate::{
    backend::macos::{
        context::Context,
        provider::ClassProvider,
        sys::{
            accounts, host,
            launchd::{self, Domain},
        },
    },
    model::{ManagementError, Record},
};
use std::path::PathBuf;

pub struct StartupCommand;

impl ClassProvider for StartupCommand {
    fn class(&self) -> &'static str {
        "Win32_StartupCommand"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let uid = accounts::session_uid();
        let user = accounts::user_name(uid).unwrap_or_else(host::user_name);
        let mut sources: Vec<(PathBuf, String)> =
            vec![("/Library/LaunchAgents".into(), "Public".into())];
        sources.extend(launchd::user_agents_directory(uid).map(|d| (d, user)));
        let overrides = launchd::disabled_overrides(&Domain::Gui(uid));
        let mut records = vec![];
        for (directory, owner) in sources {
            for definition in launchd::definitions(&directory) {
                let Some(command) = definition
                    .command_line()
                    .filter(|_| definition.starts_at_login(&overrides))
                else {
                    continue;
                };
                records.push(
                    ctx.record("Win32_StartupCommand")
                        .set("Name", definition.label.as_str())
                        .set("Caption", definition.label.as_str())
                        .set("Description", definition.label.as_str())
                        .set("Command", command)
                        .set("Location", directory.to_string_lossy().into_owned())
                        .set("User", owner.as_str())
                        .build(),
                );
            }
        }
        Ok(records)
    }
}
