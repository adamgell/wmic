//! Independent ordinary-output, append, record and clipboard destinations.
use super::*;
use crate::{
    model::*,
    render::{DestinationProfile, OutputError, encode_output},
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
struct FileDestination {
    file: File,
    profile: DestinationProfile,
}
fn path(text: &WideText) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&text.0).into()
    }
    #[cfg(not(windows))]
    {
        text.to_string_lossy().into()
    }
}
fn io_error(e: std::io::Error) -> OutputError {
    OutputError(e.to_string())
}
fn open_file(name: &WideText, append: bool) -> Result<FileDestination, OutputError> {
    let mut profile = DestinationProfile::utf16_file(false);
    let mut new = true;
    if append && let Ok(mut old) = File::open(path(name)) {
        new = false;
        let mut prefix = [0; 3];
        let n = old.read(&mut prefix).map_err(io_error)?;
        if !(n >= 2 && prefix[..2] == [255, 254] || n >= 3 && prefix == [239, 187, 191]) {
            profile = DestinationProfile::pipe(code_page());
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(path(name))
        .map_err(io_error)?;
    if new || !append {
        file.write_all(&[255, 254]).map_err(io_error)?;
    }
    Ok(FileDestination { file, profile })
}
fn code_page() -> u32 {
    #[cfg(windows)]
    {
        unsafe { windows::Win32::Globalization::GetACP() }
    }
    #[cfg(not(windows))]
    {
        1252
    }
}
fn file_text(text: &WideText, strip_final: bool) -> WideText {
    let units = if strip_final && text.0.ends_with(&[13, 13, 10]) {
        &text.0[..text.0.len() - 3]
    } else {
        &text.0
    };
    let mut out = vec![];
    let mut i = 0;
    while i < units.len() {
        if units[i..].starts_with(&[13, 13, 10]) {
            out.extend([13, 10]);
            i += 3
        } else {
            out.push(units[i]);
            i += 1
        }
    }
    WideText(out)
}
fn write_file(file: &mut FileDestination, text: &WideText) -> Result<(), OutputError> {
    file.file
        .write_all(&encode_output(text, &file.profile)?)
        .map_err(io_error)
}
#[derive(Default, Clone)]
struct RecordOutput {
    context: Option<RenderContext>,
    node: WideText,
    text: WideText,
}
pub struct RoutedOutput<T: OutputSink> {
    pub inner: T,
    output: Option<FileDestination>,
    append: Option<FileDestination>,
    append_name: Option<WideText>,
    managed: bool,
    record_name: Option<WideText>,
    record_enabled: bool,
    history: Vec<RecordOutput>,
    output_name: Option<WideText>,
    inherited_output: bool,
    last_iteration: u32,
    clipboard: bool,
    clipboard_append: bool,
    clipboard_text: WideText,
    records: Vec<RecordOutput>,
    empty: bool,
}
impl<T: OutputSink> RoutedOutput<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            output: None,
            append: None,
            append_name: None,
            managed: false,
            record_name: None,
            record_enabled: true,
            history: vec![],
            output_name: None,
            inherited_output: false,
            last_iteration: 0,
            clipboard: false,
            clipboard_append: false,
            clipboard_text: WideText::default(),
            records: vec![],
            empty: true,
        }
    }
}
impl<T: OutputSink> OutputSink for RoutedOutput<T> {
    fn consume_query_key(&mut self) -> Result<(), OutputError> {
        self.inner.consume_query_key()
    }
    fn clear_wait_prompt(&mut self) -> Result<(), OutputError> {
        self.inner.clear_wait_prompt()
    }
    fn reset_command(&mut self, s: &SessionSettings) -> Result<(), OutputError> {
        self.output = None;
        self.append = None;
        self.append_name = None;
        self.output_name = None;
        self.records.clear();
        self.empty = true;
        self.managed = false;
        self.record_enabled = true;
        self.inherited_output = false;
        self.clipboard = false;
        self.clipboard_append = false;
        self.clipboard_text = WideText::default();
        if self.record_name != s.record {
            self.history.clear();
        }
        self.record_name = s.record.clone();
        Ok(())
    }
    fn console_input(&self) -> bool {
        self.inner.console_input()
    }
    fn record_user(&self) -> WideText {
        self.inner.record_user()
    }
    fn prompt(&mut self, text: &WideText) -> Result<(), OutputError> {
        self.inner.prompt(text)
    }
    fn command(&mut self, command: &ParsedCommand) -> Result<(), OutputError> {
        self.record_enabled = !matches!(command.verb, Verb::Interactive | Verb::Exit)
            && !command
                .modifiers
                .iter()
                .any(|m| m.name.eq_ignore_case("every"));
        self.inherited_output = !command
            .modifiers
            .iter()
            .any(|m| m.scope == ModifierScope::Global && m.name.eq_ignore_case("output"));
        if command
            .modifiers
            .iter()
            .any(|m| m.scope == ModifierScope::Global && m.name.eq_ignore_case("record"))
        {
            self.history.clear();
        }
        Ok(())
    }
    fn read_line(&mut self) -> Result<Option<WideText>, OutputError> {
        self.inner.read_line()
    }
    fn read_secret(&mut self) -> Result<Option<WideText>, OutputError> {
        self.inner.read_secret()
    }
    fn configure(&mut self, s: &SessionSettings) -> Result<(), OutputError> {
        self.output = None;
        self.append = None;
        if self.record_name != s.record {
            self.history.clear();
        }
        self.record_name = s.record.clone();
        self.output_name = None;
        self.last_iteration = 0;
        self.records.clear();
        self.clipboard_text = WideText::default();
        self.empty = true;
        self.managed = false;
        self.append_name = None;
        self.clipboard = s.output.eq_ignore_case("clipboard");
        self.clipboard_append = s.append.eq_ignore_case("clipboard");
        if !s.output.eq_ignore_case("stdout") && !self.clipboard {
            self.output = Some(open_file(&s.output, false)?);
            self.output_name = Some(s.output.clone());
        }
        if !s.append.eq_ignore_case("stdout") && !self.clipboard_append {
            self.append_name = Some(s.append.clone());
        }
        if let Some(name) = &self.record_name {
            let p = path(name);
            if p.is_dir()
                || p.parent()
                    .is_some_and(|p| !p.as_os_str().is_empty() && !p.is_dir())
            {
                return Err(OutputError("Invalid record file".into()));
            }
        }
        Ok(())
    }
    fn command_context(&mut self, context: &RenderContext) -> Result<(), OutputError> {
        self.records.push(RecordOutput {
            context: Some(context.clone()),
            node: context.issued_from.clone(),
            text: WideText::default(),
        });
        Ok(())
    }
    fn context(&mut self, context: &RenderContext, node: &WideText) -> Result<(), OutputError> {
        if !self.managed && self.records.last().is_some_and(|r| r.text.0.is_empty()) {
            self.records.pop();
        }
        self.managed = true;
        if context.every_count > self.last_iteration
            && let Some(name) = &self.output_name
        {
            self.output = Some(open_file(name, false)?);
        }
        self.last_iteration = context.every_count;
        self.records.push(RecordOutput {
            context: Some(context.clone()),
            node: node.clone(),
            text: WideText::default(),
        });
        Ok(())
    }
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        if stream == OutputStream::Stderr {
            if !self.managed
                && let Some(record) = self.records.last_mut()
            {
                record.text.0.extend(file_text(text, false).0);
            }
            return self.inner.write(stream, text);
        }
        if text.0.is_empty() {
            return Ok(());
        }
        self.empty = false;
        let body = file_text(text, true);
        if let Some(record) = self.records.last_mut() {
            record.text.0.extend(&body.0)
        }
        if self.clipboard || self.clipboard_append {
            self.clipboard_text.0.extend(&body.0)
        }
        if self.append.is_none()
            && let Some(name) = &self.append_name
        {
            self.append = Some(open_file(name, true)?);
        }
        if let Some(file) = &mut self.append {
            if file.profile.utf16 {
                write_file(file, &file_text(text, false))?;
            } else {
                let mut bytes = ansi_prefix(&body, file.profile.code_page)?;
                bytes.extend([13, 10]);
                file.file.write_all(&bytes).map_err(io_error)?;
            }
        }
        if let Some(file) = &mut self.output {
            write_file(file, &body)
        } else if self.clipboard {
            Ok(())
        } else {
            self.inner.write(stream, text)
        }
    }
    fn finish(&mut self) -> Result<(), OutputError> {
        if self.empty && self.managed {
            if let Some(file) = &mut self.output {
                write_file(file, &"\r\n".into())?;
            }
            if self.record_name.is_some() && self.record_enabled {
                self.inner
                    .write(OutputStream::Stdout, &"\r\r\n\r\r\n".into())?;
            }
            if let Some(record) = self.records.last_mut() {
                record.text = "\r\n".into();
            }
        }
        if let Some(file) = &mut self.output {
            if self.inherited_output && !self.empty {
                write_file(file, &"\r\n".into())?;
            }
            file.file.flush().map_err(io_error)?;
        }
        if let Some(file) = &mut self.append {
            file.file.flush().map_err(io_error)?;
        }
        if self.clipboard || self.clipboard_append {
            self.inner.clipboard(&self.clipboard_text)?;
        }
        let user = self.record_user();
        if self.record_enabled
            && let Some(name) = &self.record_name
        {
            self.history.extend(
                self.records
                    .iter()
                    .filter(|r| !r.text.0.is_empty())
                    .cloned(),
            );
            let text = record_xml(&user, &self.history);
            let mut file = File::create(path(name)).map_err(io_error)?;
            file.write_all(&encode_output(&text, &DestinationProfile::pipe(65001))?)
                .map_err(io_error)?;
            file.flush().map_err(io_error)?;
        }
        Ok(())
    }
}
fn escape(s: &WideText) -> WideText {
    let mut out = WideText::default();
    for &u in &s.0 {
        out.0.extend(match u {
            38 => "&amp;".encode_utf16().collect(),
            60 => "&lt;".encode_utf16().collect(),
            62 => "&gt;".encode_utf16().collect(),
            34 => "&quot;".encode_utf16().collect(),
            _ => vec![u],
        })
    }
    out
}
fn record_xml(user: &WideText, records: &[RecordOutput]) -> WideText {
    let mut out = WideText::from("<WMIRECORD USER=\"");
    out.0.extend(escape(user).0);
    out.0.extend("\">".encode_utf16());
    for record in records {
        if let Some(c) = &record.context {
            out.0.extend(
                format!("<RECORD SEQUENCENUM=\"{}\" ISSUEDFROM=\"", c.sequence).encode_utf16(),
            );
            out.0.extend(escape(&c.issued_from).0);
            out.0.extend("\" STARTTIME=\"".encode_utf16());
            out.0.extend(escape(&c.started_at).0);
            out.0.extend("\"><REQUEST><COMMANDLINE>".encode_utf16());
            out.0
                .extend(escape_text(&without_record(&c.command_line)).0);
            out.0
                .extend("</COMMANDLINE></REQUEST><OUTPUT TARGETNODE=\"".encode_utf16());
            out.0.extend(escape(&record.node).0);
            out.0
                .extend(format!("\" ITERATION=\"{}\"><![CDATA[", c.every_count + 1).encode_utf16());
            // Split CDATA terminators so provider data cannot corrupt the record document.
            out.0.extend(
                crate::render::replace(&record.text, &"]]>".into(), &"]]]]><![CDATA[>".into()).0,
            );
            out.0.extend("]]></OUTPUT></RECORD>".encode_utf16());
        }
    }
    out.0.extend("</WMIRECORD>\r\n".encode_utf16());
    out
}
fn without_record(text: &WideText) -> WideText {
    let mut out = WideText::default();
    let mut at = 0;
    let mut skip_next = false;
    while at < text.0.len() {
        let start = at;
        let mut quoted = false;
        while at < text.0.len() {
            let u = text.0[at];
            if !quoted && matches!(u, 32 | 9) {
                break;
            }
            if u == 34 {
                quoted = !quoted
            }
            at += 1;
        }
        let token = WideText(text.0[start..at].to_vec())
            .to_string_lossy()
            .to_ascii_lowercase();
        if !token.is_empty() {
            let remove = skip_next || token.starts_with("/record:");
            skip_next = token == "/record:";
            if !remove {
                out.0.extend(&text.0[start..at]);
            }
        }
        if at < text.0.len() {
            out.0.push(text.0[at]);
            at += 1;
        }
    }
    out
}

fn escape_text(text: &WideText) -> WideText {
    let mut out = WideText::default();
    for &u in &text.0 {
        out.0.extend(match u {
            38 => "&amp;".encode_utf16().collect(),
            60 => "&lt;".encode_utf16().collect(),
            62 => "&gt;".encode_utf16().collect(),
            _ => vec![u],
        })
    }
    out
}
fn ansi_prefix(text: &WideText, code_page: u32) -> Result<Vec<u8>, OutputError> {
    #[cfg(windows)]
    {
        use windows::{
            Win32::Globalization::*,
            core::{BOOL, PCSTR},
        };
        let mut out = vec![];
        for unit in std::char::decode_utf16(text.0.iter().copied()) {
            let ch = match unit {
                Ok(ch) => ch,
                Err(_) => break,
            };
            let mut storage = [0u16; 2];
            let units = ch.encode_utf16(&mut storage);
            let mut default = BOOL(0);
            let count = unsafe {
                WideCharToMultiByte(
                    code_page,
                    WC_NO_BEST_FIT_CHARS,
                    units,
                    None,
                    PCSTR::null(),
                    Some(&mut default),
                )
            };
            if count == 0 || default.as_bool() {
                break;
            }
            let mut bytes = vec![0; count as usize];
            unsafe {
                WideCharToMultiByte(
                    code_page,
                    WC_NO_BEST_FIT_CHARS,
                    units,
                    Some(&mut bytes),
                    PCSTR::null(),
                    Some(&mut default),
                )
            };
            out.extend(bytes);
        }
        Ok(out)
    }
    #[cfg(not(windows))]
    {
        encode_output(text, &DestinationProfile::pipe(code_page))
    }
}
