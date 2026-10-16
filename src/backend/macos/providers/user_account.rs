use crate::{
    backend::macos::{context::Context, provider::ClassProvider, sys::accounts},
    model::{ManagementError, Record},
};

pub struct UserAccount;

/// Root and regular accounts; names with a leading underscore are service accounts. User ids
/// from 0x8000_0000 on are system placeholders such as `nobody` (-2).
pub fn listed(user: &accounts::User) -> bool {
    (user.uid == 0 || (500..0x8000_0000).contains(&user.uid)) && !user.name.starts_with('_')
}

/// Real name: the GECOS field up to its first comma.
fn full_name(gecos: &str) -> Option<&str> {
    gecos
        .split(',')
        .next()
        .map(str::trim)
        .filter(|n| !n.is_empty())
}

impl ClassProvider for UserAccount {
    fn class(&self) -> &'static str {
        "Win32_UserAccount"
    }

    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        let mut users: Vec<accounts::User> = accounts::users().into_iter().filter(listed).collect();
        users.sort_by(|a, b| a.uid.cmp(&b.uid).then_with(|| a.name.cmp(&b.name)));
        users.dedup_by(|a, b| a.name == b.name);
        Ok(users
            .iter()
            .map(|user| {
                ctx.record("Win32_UserAccount")
                    .set("Name", user.name.as_str())
                    .set("Domain", ctx.server())
                    .set("FullName", full_name(&user.gecos))
                    .set("Caption", format!("{}\\{}", ctx.server(), user.name))
                    .set("AccountType", 512u32)
                    .set("SIDType", 1u8)
                    .set("LocalAccount", true)
                    .set("Lockout", false)
                    .set("Status", "OK")
                    .build()
            })
            .collect())
    }
}
