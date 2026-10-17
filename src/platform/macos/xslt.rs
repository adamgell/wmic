//! Stylesheet files run through the system libxslt and libxml2.
//!
//! The result is serialized into a buffer without an encoder, so the output stays UTF-8 even when
//! the stylesheet declares `encoding="utf-16"`.
use crate::{
    model::{TransformSpec, WideText},
    render::RenderError,
};
use std::{
    ffi::{CString, c_char, c_int, c_void},
    marker::PhantomData,
    ptr::{self, NonNull},
    rc::Rc,
    sync::{Once, OnceLock},
};

macro_rules! opaque {
    ($($name:ident),*) => {$(
        #[repr(C)]
        struct $name {
            _private: [u8; 0],
        }
    )*};
}
opaque!(
    XmlDoc,
    XmlNode,
    XmlOutputBuffer,
    XsltStylesheet,
    XsltTransformContext,
    XsltSecurityPrefs
);

type ErrorHandler = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;
type EntityLoader = unsafe extern "C" fn(*const c_char, *const c_char, *mut c_void) -> *mut c_void;
type DocumentLoader =
    unsafe extern "C" fn(*const c_char, *mut c_void, c_int, *mut c_void, c_int) -> *mut XmlDoc;
type SecurityCheck =
    unsafe extern "C" fn(*mut XsltSecurityPrefs, *mut XsltTransformContext, *const c_char) -> c_int;

#[link(name = "xml2")]
unsafe extern "C" {
    fn xmlInitParser();
    fn xmlReadMemory(
        buffer: *const c_char,
        size: c_int,
        url: *const c_char,
        encoding: *const c_char,
        options: c_int,
    ) -> *mut XmlDoc;
    fn xmlFreeDoc(doc: *mut XmlDoc);
    fn xmlDocGetRootElement(doc: *const XmlDoc) -> *mut XmlNode;
    fn xmlAllocOutputBuffer(encoder: *mut c_void) -> *mut XmlOutputBuffer;
    fn xmlOutputBufferGetContent(out: *mut XmlOutputBuffer) -> *const u8;
    fn xmlOutputBufferGetSize(out: *mut XmlOutputBuffer) -> usize;
    fn xmlOutputBufferClose(out: *mut XmlOutputBuffer) -> c_int;
    fn xmlSetGenericErrorFunc(context: *mut c_void, handler: ErrorHandler);
    fn xmlSetExternalEntityLoader(loader: EntityLoader);
    fn xmlNoNetExternalEntityLoader(
        url: *const c_char,
        id: *const c_char,
        context: *mut c_void,
    ) -> *mut c_void;
}

#[link(name = "xslt")]
unsafe extern "C" {
    fn xsltParseStylesheetDoc(doc: *mut XmlDoc) -> *mut XsltStylesheet;
    fn xsltFreeStylesheet(style: *mut XsltStylesheet);
    fn xsltNewTransformContext(
        style: *mut XsltStylesheet,
        doc: *mut XmlDoc,
    ) -> *mut XsltTransformContext;
    fn xsltFreeTransformContext(context: *mut XsltTransformContext);
    fn xsltQuoteOneUserParam(
        context: *mut XsltTransformContext,
        name: *const u8,
        value: *const u8,
    ) -> c_int;
    fn xsltApplyStylesheetUser(
        style: *mut XsltStylesheet,
        doc: *mut XmlDoc,
        params: *const *const c_char,
        output: *const c_char,
        profile: *mut c_void,
        context: *mut XsltTransformContext,
    ) -> *mut XmlDoc;
    fn xsltSaveResultTo(
        out: *mut XmlOutputBuffer,
        result: *mut XmlDoc,
        style: *mut XsltStylesheet,
    ) -> c_int;
    fn xsltNewSecurityPrefs() -> *mut XsltSecurityPrefs;
    fn xsltFreeSecurityPrefs(prefs: *mut XsltSecurityPrefs);
    fn xsltSetSecurityPrefs(
        prefs: *mut XsltSecurityPrefs,
        option: c_int,
        check: SecurityCheck,
    ) -> c_int;
    fn xsltSetCtxtSecurityPrefs(
        prefs: *mut XsltSecurityPrefs,
        context: *mut XsltTransformContext,
    ) -> c_int;
    fn xsltSetDefaultSecurityPrefs(prefs: *mut XsltSecurityPrefs);
    fn xsltSecurityForbid(
        prefs: *mut XsltSecurityPrefs,
        context: *mut XsltTransformContext,
        value: *const c_char,
    ) -> c_int;
    fn xsltSetGenericErrorFunc(context: *mut c_void, handler: ErrorHandler);
    fn xsltSetCtxtParseOptions(context: *mut XsltTransformContext, options: c_int) -> c_int;
    fn xsltSetLoaderFunc(loader: DocumentLoader);
    static xsltDocDefaultLoader: Option<DocumentLoader>;
}

const PARSE_NOERROR: c_int = 1 << 5;
const PARSE_NOWARNING: c_int = 1 << 6;
const PARSE_NOBLANKS: c_int = 1 << 8;
const PARSE_NONET: c_int = 1 << 11;
const PARSE_NOCDATA: c_int = 1 << 14;
const PARSE_IGNORE_ENC: c_int = 1 << 21;
const STYLESHEET_OPTIONS: c_int = PARSE_NONET | PARSE_NOCDATA | PARSE_NOERROR | PARSE_NOWARNING;
const INPUT_OPTIONS: c_int =
    PARSE_NONET | PARSE_NOBLANKS | PARSE_NOERROR | PARSE_NOWARNING | PARSE_IGNORE_ENC;
/// Documents loaded by `document()`, `xsl:include` and `xsl:import`: no network, no entity
/// substitution and no external subset.
const SECONDARY_OPTIONS: c_int = PARSE_NONET | PARSE_NOCDATA;

const SECURITY_WRITE_FILE: c_int = 2;
const SECURITY_CREATE_DIRECTORY: c_int = 3;
const SECURITY_READ_NETWORK: c_int = 4;
const SECURITY_WRITE_NETWORK: c_int = 5;

const SCRIPT_NAMESPACE: &str = "urn:schemas-microsoft-com:xslt";

fn format_error() -> RenderError {
    RenderError::new("Invalid XSL format (or) file name.")
}

fn invalid_xml() -> RenderError {
    RenderError::status(0xACB5, "Invalid XML content.")
}

unsafe extern "C" fn discard(_: *mut c_void, _: *const c_char) {}

/// `xsltSetLoaderFunc` overwrites `xsltDocDefaultLoader`, so the original is kept before the
/// replacement is installed.
static DEFAULT_LOADER: OnceLock<DocumentLoader> = OnceLock::new();

unsafe extern "C" fn load_secondary(
    uri: *const c_char,
    dict: *mut c_void,
    _options: c_int,
    context: *mut c_void,
    kind: c_int,
) -> *mut XmlDoc {
    match DEFAULT_LOADER.get() {
        Some(load) => unsafe { load(uri, dict, SECONDARY_OPTIONS, context, kind) },
        None => ptr::null_mut(),
    }
}

fn new_forbidding_prefs() -> Option<NonNull<XsltSecurityPrefs>> {
    let prefs = NonNull::new(unsafe { xsltNewSecurityPrefs() })?;
    for option in [
        SECURITY_WRITE_FILE,
        SECURITY_CREATE_DIRECTORY,
        SECURITY_READ_NETWORK,
        SECURITY_WRITE_NETWORK,
    ] {
        unsafe { xsltSetSecurityPrefs(prefs.as_ptr(), option, xsltSecurityForbid) };
    }
    Some(prefs)
}

/// Diagnostics never reach stderr. The libxml2 handler is per thread, so it is set on every
/// entry; the libxslt handler and the default security preferences are process wide.
fn engine() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        xmlInitParser();
        xmlSetExternalEntityLoader(xmlNoNetExternalEntityLoader);
        xsltSetGenericErrorFunc(ptr::null_mut(), Some(discard));
        if let Some(default) = xsltDocDefaultLoader {
            let _ = DEFAULT_LOADER.set(default);
            xsltSetLoaderFunc(load_secondary);
        }
        if let Some(prefs) = new_forbidding_prefs() {
            xsltSetDefaultSecurityPrefs(prefs.as_ptr());
        }
    });
    unsafe { xmlSetGenericErrorFunc(ptr::null_mut(), Some(discard)) };
}

struct Doc(NonNull<XmlDoc>);
impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { xmlFreeDoc(self.0.as_ptr()) }
    }
}

struct Context(NonNull<XsltTransformContext>);
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { xsltFreeTransformContext(self.0.as_ptr()) }
    }
}

struct Prefs(NonNull<XsltSecurityPrefs>);
impl Drop for Prefs {
    fn drop(&mut self) {
        unsafe { xsltFreeSecurityPrefs(self.0.as_ptr()) }
    }
}

fn read_document(
    bytes: &[u8],
    url: &std::ffi::CStr,
    encoding: *const c_char,
    options: c_int,
) -> Option<Doc> {
    let size = c_int::try_from(bytes.len()).ok()?;
    NonNull::new(unsafe {
        xmlReadMemory(bytes.as_ptr().cast(), size, url.as_ptr(), encoding, options)
    })
    .map(Doc)
}

fn decode(bytes: &[u8]) -> String {
    let utf16 = |rest: &[u8], big: bool| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| {
                if big {
                    u16::from_be_bytes(*p)
                } else {
                    u16::from_le_bytes(*p)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, false),
        [0xfe, 0xff, rest @ ..] => utf16(rest, true),
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// A `script` element in the vendor namespace needs a script engine that this platform lacks.
fn declares_script(source: &str) -> bool {
    let mut from = 0;
    while let Some(found) = source[from..].find(SCRIPT_NAMESPACE) {
        let at = from + found;
        from = at + SCRIPT_NAMESPACE.len();
        let head = source[..at]
            .strip_suffix(['"', '\''])
            .map(str::trim_end)
            .and_then(|h| h.strip_suffix('='))
            .map(str::trim_end);
        let Some(head) = head else { continue };
        let name_start = head
            .rfind(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')))
            .map_or(0, |i| i + 1);
        let tag = match &head[name_start..] {
            "xmlns" => "<script".to_string(),
            declaration => match declaration.strip_prefix("xmlns:") {
                Some(prefix) => format!("<{prefix}:script"),
                None => continue,
            },
        };
        let mut rest = source;
        while let Some(i) = rest.find(&tag) {
            rest = &rest[i + tag.len()..];
            if rest.starts_with(|c: char| c.is_whitespace() || matches!(c, '>' | '/')) {
                return true;
            }
        }
    }
    false
}

/// Stylesheets with a document type declaration are refused.
fn declares_doctype(source: &str) -> bool {
    source.contains("<!DOCTYPE")
}

/// A stylesheet that stays valid for the lifetime of the value. It never leaves its thread.
pub struct Prepared {
    style: NonNull<XsltStylesheet>,
    parameters: Vec<(CString, CString)>,
    _thread: PhantomData<Rc<()>>,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        unsafe { xsltFreeStylesheet(self.style.as_ptr()) }
    }
}

pub fn prepare(spec: &TransformSpec) -> Result<Prepared, RenderError> {
    engine();
    let path = spec.name.to_string_lossy();
    let bytes = std::fs::read(&path).map_err(|_| format_error())?;
    let source = decode(&bytes);
    if declares_script(&source) {
        return Err(RenderError::new(
            "Invalid XSL format (or) file name. Script extensions are not supported on this platform.",
        ));
    }
    if declares_doctype(&source) {
        return Err(format_error());
    }
    let url = CString::new(path).map_err(|_| format_error())?;
    let doc =
        read_document(&bytes, &url, ptr::null(), STYLESHEET_OPTIONS).ok_or_else(format_error)?;
    let style =
        NonNull::new(unsafe { xsltParseStylesheetDoc(doc.0.as_ptr()) }).ok_or_else(format_error)?;
    // The stylesheet owns its document after a successful parse.
    std::mem::forget(doc);
    let mut latest: Vec<&(WideText, WideText)> = vec![];
    for pair in spec.parameters.iter().rev() {
        if !latest.iter().any(|kept| kept.0 == pair.0) {
            latest.push(pair)
        }
    }
    latest.reverse();
    let parameters = latest
        .into_iter()
        .map(|(name, value)| {
            Ok((
                CString::new(name.to_string_lossy()).map_err(|_| format_error())?,
                CString::new(value.to_string_lossy()).map_err(|_| format_error())?,
            ))
        })
        .collect::<Result<_, RenderError>>();
    match parameters {
        Ok(parameters) => Ok(Prepared {
            style,
            parameters,
            _thread: PhantomData,
        }),
        Err(e) => {
            unsafe { xsltFreeStylesheet(style.as_ptr()) };
            Err(e)
        }
    }
}

impl Prepared {
    pub fn apply(&self, xml: &WideText) -> Result<WideText, RenderError> {
        engine();
        let text = String::from_utf16_lossy(&xml.0);
        let input = read_document(
            text.as_bytes(),
            c"wmic.xml",
            c"UTF-8".as_ptr(),
            INPUT_OPTIONS,
        )
        .ok_or_else(invalid_xml)?;
        let context =
            NonNull::new(unsafe { xsltNewTransformContext(self.style.as_ptr(), input.0.as_ptr()) })
                .map(Context)
                .ok_or_else(format_error)?;
        // The status is the previous option set; only a negative value is an error.
        if unsafe { xsltSetCtxtParseOptions(context.0.as_ptr(), SECONDARY_OPTIONS) } < 0 {
            return Err(format_error());
        }
        let prefs = new_forbidding_prefs().map(Prefs).ok_or_else(format_error)?;
        if unsafe { xsltSetCtxtSecurityPrefs(prefs.0.as_ptr(), context.0.as_ptr()) } != 0 {
            return Err(format_error());
        }
        for (name, value) in &self.parameters {
            let status = unsafe {
                xsltQuoteOneUserParam(
                    context.0.as_ptr(),
                    name.as_ptr().cast(),
                    value.as_ptr().cast(),
                )
            };
            if status != 0 {
                return Err(format_error());
            }
        }
        let result = NonNull::new(unsafe {
            xsltApplyStylesheetUser(
                self.style.as_ptr(),
                input.0.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                context.0.as_ptr(),
            )
        })
        .map(Doc)
        .ok_or_else(format_error)?;
        let buffer = unsafe { xmlAllocOutputBuffer(ptr::null_mut()) };
        if buffer.is_null() {
            return Err(format_error());
        }
        let written = unsafe { xsltSaveResultTo(buffer, result.0.as_ptr(), self.style.as_ptr()) };
        let bytes = (written >= 0).then(|| unsafe {
            let size = xmlOutputBufferGetSize(buffer);
            let content = xmlOutputBufferGetContent(buffer);
            if content.is_null() {
                vec![]
            } else {
                std::slice::from_raw_parts(content, size).to_vec()
            }
        });
        unsafe { xmlOutputBufferClose(buffer) };
        let mut text = String::from_utf8_lossy(&bytes.ok_or_else(format_error)?).into_owned();
        // Markup output writes a carriage return as a reference; text output never does.
        if !unsafe { xmlDocGetRootElement(result.0.as_ptr()) }.is_null() {
            text = text.replace("&#13;", "\r");
        }
        drop(context);
        drop(prefs);
        Ok(WideText(text.encode_utf16().collect()))
    }
}

pub fn transform(xml: &WideText, spec: &TransformSpec) -> Result<WideText, RenderError> {
    prepare(spec)?.apply(xml)
}
