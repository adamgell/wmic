use crate::{
    backend::macos::{context::Context, provider::ClassProvider, sys::accounts},
    model::{ManagementError, Record},
};

pub struct Group;

/// Names with a leading underscore and `com.apple.` directory groups are system groups.
fn listed(group: &accounts::Group) -> bool {
    !group.name.starts_with('_') && !group.name.starts_with("com.apple.")
}

impl ClassProvider for Group {
    fn class(&self) -> &'static str {
        "Win32_Group"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let mut groups: Vec<accounts::Group> =
            accounts::groups().into_iter().filter(listed).collect();
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        groups.dedup_by(|a, b| a.name == b.name);
        Ok(groups
            .iter()
            .map(|group| {
                ctx.record("Win32_Group")
                    .set("Name", group.name.as_str())
                    .set("Domain", ctx.server())
                    .set("Caption", format!("{}\\{}", ctx.server(), group.name))
                    .set("SIDType", 4u8)
                    .set("LocalAccount", true)
                    .set("Status", "OK")
                    .build()
            })
            .collect())
    }
}
