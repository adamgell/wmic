use crate::{model::WideText, platform::unix::output::native_newlines, render::OutputError};
use std::{
    io::Write,
    process::{Command, Stdio},
};

/// The absolute path avoids PATH lookup. pbcopy decodes stdin by locale and LC_ALL outranks LANG,
/// so both are set.
pub fn copy(text: &WideText) -> Result<(), OutputError> {
    let unavailable = |_| OutputError("Clipboard unavailable".into());
    let body = String::from_utf16_lossy(&native_newlines(&text.0));
    let mut child = Command::new("/usr/bin/pbcopy")
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(unavailable)?;
    let written = child
        .stdin
        .take()
        .map_or(Ok(()), |mut stdin| stdin.write_all(body.as_bytes()));
    let status = child.wait().map_err(unavailable)?;
    if written.is_err() || !status.success() {
        return Err(OutputError("Clipboard unavailable".into()));
    }
    Ok(())
}
