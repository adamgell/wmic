//! Kerberos security contexts through the GSS framework of macOS.
//!
//! The framework is Heimdal. Its credential cache belongs to the login session and is shared
//! with `kinit` and `klist`: a ticket acquired here with a password serves later commands of the
//! same session that give no password, and a ticket from `kinit` serves this build.

use super::super::TransportError;
use super::framing::{KERBEROS_PROTOCOL, Sealer};
use super::kerberos::{
    ClientCredential, Failure, NameKind, Plan, SecurityContext, Service, join_wrap_token, protocol,
    split_wrap_token,
};
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    error::{CFError, CFErrorCopyUserInfo, CFErrorRef, kCFErrorDescriptionKey},
    number::CFNumber,
    string::CFString,
};
use std::ffi::c_void;
use std::ptr;

type OmUint32 = u32;

#[repr(C)]
struct Buffer {
    length: usize,
    value: *mut c_void,
}

impl Buffer {
    const EMPTY: Buffer = Buffer {
        length: 0,
        value: ptr::null_mut(),
    };

    fn borrow(bytes: &[u8]) -> Buffer {
        Buffer {
            length: bytes.len(),
            value: bytes.as_ptr() as *mut c_void,
        }
    }

    /// Copies the bytes the library allocated and releases them.
    fn take(&mut self) -> Vec<u8> {
        let bytes = if self.value.is_null() {
            Vec::new()
        } else {
            // The library owns `length` bytes at `value` until gss_release_buffer runs.
            unsafe { std::slice::from_raw_parts(self.value as *const u8, self.length) }.to_vec()
        };
        let mut minor = 0;
        unsafe {
            gss_release_buffer(&mut minor, self);
        }
        bytes
    }
}

/// gssapi.h packs its structures to 2 bytes on x86_64 and leaves the C layout on arm64.
#[cfg_attr(target_arch = "x86_64", repr(C, packed(2)))]
#[cfg_attr(not(target_arch = "x86_64"), repr(C))]
struct Oid {
    length: OmUint32,
    elements: *mut c_void,
}

#[repr(C)]
struct OidSet {
    count: usize,
    elements: *const Oid,
}

#[cfg_attr(target_arch = "x86_64", repr(C, packed(2)))]
#[cfg_attr(not(target_arch = "x86_64"), repr(C))]
struct ChannelBindings {
    initiator_addrtype: OmUint32,
    initiator_address: Buffer,
    acceptor_addrtype: OmUint32,
    acceptor_address: Buffer,
    application_data: Buffer,
}

type NameHandle = *mut c_void;
type CredHandle = *mut c_void;
type ContextHandle = *mut c_void;

const GSS_S_COMPLETE: OmUint32 = 0;
const GSS_S_CONTINUE_NEEDED: OmUint32 = 1;
const GSS_C_GSS_CODE: i32 = 1;
const GSS_C_MECH_CODE: i32 = 2;
const GSS_C_INITIATE: i32 = 1;
const GSS_C_MUTUAL_FLAG: OmUint32 = 2;
const GSS_C_SEQUENCE_FLAG: OmUint32 = 8;
const GSS_C_CONF_FLAG: OmUint32 = 16;
const GSS_C_INTEG_FLAG: OmUint32 = 32;
const REQUESTED_FLAGS: OmUint32 =
    GSS_C_MUTUAL_FLAG | GSS_C_SEQUENCE_FLAG | GSS_C_CONF_FLAG | GSS_C_INTEG_FLAG;

#[link(name = "GSS", kind = "framework")]
unsafe extern "C" {
    static __gss_krb5_mechanism_oid_desc: Oid;
    static __gss_c_nt_hostbased_service_oid_desc: Oid;
    static __gss_krb5_nt_principal_name_oid_desc: Oid;
    static __gss_c_nt_user_name_oid_desc: Oid;

    fn gss_import_name(
        minor: *mut OmUint32,
        input: *const Buffer,
        name_type: *const Oid,
        output: *mut NameHandle,
    ) -> OmUint32;
    fn gss_release_name(minor: *mut OmUint32, name: *mut NameHandle) -> OmUint32;
    fn gss_aapl_initial_cred(
        desired_name: NameHandle,
        desired_mech: *const Oid,
        attributes: core_foundation::dictionary::CFDictionaryRef,
        output_cred: *mut CredHandle,
        error: *mut CFErrorRef,
    ) -> OmUint32;
    fn gss_acquire_cred(
        minor: *mut OmUint32,
        desired_name: NameHandle,
        time_req: OmUint32,
        desired_mechs: *const OidSet,
        cred_usage: i32,
        output_cred: *mut CredHandle,
        actual_mechs: *mut *mut OidSet,
        time_rec: *mut OmUint32,
    ) -> OmUint32;
    fn gss_release_cred(minor: *mut OmUint32, cred: *mut CredHandle) -> OmUint32;
    fn gss_destroy_cred(minor: *mut OmUint32, cred: *mut CredHandle) -> OmUint32;
    fn gss_init_sec_context(
        minor: *mut OmUint32,
        initiator_cred: CredHandle,
        context: *mut ContextHandle,
        target_name: NameHandle,
        mech_type: *const Oid,
        req_flags: OmUint32,
        time_req: OmUint32,
        chan_bindings: *const ChannelBindings,
        input_token: *const Buffer,
        actual_mech_type: *mut *const Oid,
        output_token: *mut Buffer,
        ret_flags: *mut OmUint32,
        time_rec: *mut OmUint32,
    ) -> OmUint32;
    fn gss_delete_sec_context(
        minor: *mut OmUint32,
        context: *mut ContextHandle,
        output_token: *mut Buffer,
    ) -> OmUint32;
    fn gss_wrap(
        minor: *mut OmUint32,
        context: ContextHandle,
        conf_req: i32,
        qop_req: OmUint32,
        input: *const Buffer,
        conf_state: *mut i32,
        output: *mut Buffer,
    ) -> OmUint32;
    fn gss_unwrap(
        minor: *mut OmUint32,
        context: ContextHandle,
        input: *const Buffer,
        output: *mut Buffer,
        conf_state: *mut i32,
        qop_state: *mut OmUint32,
    ) -> OmUint32;
    fn gss_release_buffer(minor: *mut OmUint32, buffer: *mut Buffer) -> OmUint32;
    fn gss_display_status(
        minor: *mut OmUint32,
        status_value: OmUint32,
        status_type: i32,
        mech_type: *const Oid,
        message_context: *mut OmUint32,
        status_string: *mut Buffer,
    ) -> OmUint32;
}

fn krb5_mechanism() -> *const Oid {
    &raw const __gss_krb5_mechanism_oid_desc
}

/// Base of the krb5 error table: protocol error `n` of RFC 4120 section 7.5.9 reaches the
/// caller as `KRB5_BASE + n`.
const KRB5_BASE: i32 = -1765328384;

/// Error codes that name the account or its password. Every other code means Kerberos could
/// not be used for the request.
const DENIED_CODES: [i32; 7] = [
    KRB5_BASE + 1,  // KDC_ERR_NAME_EXP: client account expired
    KRB5_BASE + 12, // KDC_ERR_POLICY
    KRB5_BASE + 18, // KDC_ERR_CLIENT_REVOKED: account disabled or locked
    KRB5_BASE + 21, // KDC_ERR_CLIENT_NOTYET
    KRB5_BASE + 23, // KDC_ERR_KEY_EXPIRED: password expired
    KRB5_BASE + 24, // KDC_ERR_PREAUTH_FAILED: wrong password
    KRB5_BASE + 31, // KRB_AP_ERR_BAD_INTEGRITY: wrong password at a KDC without pre-authentication
];

fn status_text(status: OmUint32, kind: i32) -> String {
    let mut context = 0;
    let mut minor = 0;
    let mut buffer = Buffer::EMPTY;
    let mech = if kind == GSS_C_MECH_CODE {
        krb5_mechanism()
    } else {
        ptr::null()
    };
    let major =
        unsafe { gss_display_status(&mut minor, status, kind, mech, &mut context, &mut buffer) };
    if major != GSS_S_COMPLETE {
        return format!("status {status:#x}");
    }
    String::from_utf8_lossy(&buffer.take()).trim().to_owned()
}

/// Reason text of a GSS failure: the Kerberos text when there is a minor code, else the major
/// status text, each followed by the numeric code.
fn describe(major: OmUint32, minor: OmUint32) -> String {
    if minor != 0 {
        format!("{} ({})", status_text(minor, GSS_C_MECH_CODE), minor as i32)
    } else {
        format!("{} (major {major:#x})", status_text(major, GSS_C_GSS_CODE))
    }
}

fn failure(major: OmUint32, minor: OmUint32) -> Failure {
    let reason = describe(major, minor);
    if DENIED_CODES.contains(&(minor as i32)) {
        Failure::Denied(reason)
    } else {
        Failure::Unavailable(reason)
    }
}

struct Name(NameHandle);

impl Name {
    fn import(text: &str, kind: *const Oid) -> Result<Self, Failure> {
        let mut minor = 0;
        let mut handle: NameHandle = ptr::null_mut();
        let buffer = Buffer::borrow(text.as_bytes());
        let major = unsafe { gss_import_name(&mut minor, &buffer, kind, &mut handle) };
        if major != GSS_S_COMPLETE || handle.is_null() {
            return Err(Failure::Unavailable(format!(
                "name {text:?}: {}",
                describe(major, minor)
            )));
        }
        Ok(Self(handle))
    }
}

impl Drop for Name {
    fn drop(&mut self) {
        let mut minor = 0;
        unsafe {
            gss_release_name(&mut minor, &mut self.0);
        }
    }
}

/// A Kerberos credential handle of the client principal.
pub struct Credential {
    handle: CredHandle,
    /// The credential was acquired with a password into a cache of its own, which is removed
    /// with the credential so that no ticket of the account outlives the command.
    owned_cache: bool,
}

impl Drop for Credential {
    fn drop(&mut self) {
        let mut minor = 0;
        unsafe {
            if self.owned_cache {
                gss_destroy_cred(&mut minor, &mut self.handle);
            } else {
                gss_release_cred(&mut minor, &mut self.handle);
            }
        }
    }
}

/// The minor status (`kGSSMinorErrorCode`) and its text (`NSDescription`) of a GSS error.
fn mechanism_status(error: &CFError) -> (Option<i32>, Option<String>) {
    let info = unsafe { CFErrorCopyUserInfo(error.as_concrete_TypeRef()) };
    if info.is_null() {
        return (None, None);
    }
    let info: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(info) };
    let code = info
        .find(CFString::from_static_string("kGSSMinorErrorCode"))
        .and_then(|value| value.downcast::<CFNumber>())
        .and_then(|number| number.to_i64())
        .map(|value| value as u32 as i32);
    let key = unsafe { CFString::wrap_under_get_rule(kCFErrorDescriptionKey) };
    let text = info
        .find(&key)
        .and_then(|value| value.downcast::<CFString>())
        .map(|text| text.to_string().trim().to_owned())
        .filter(|text| !text.is_empty());
    (code, text)
}

/// Codes of a password logon that leave the password unchecked: the KDC was not reached, or it
/// does not know the account, its realm or the encryption types. NTLM may follow them.
const UNCHECKED_CODES: [i32; 6] = [
    KRB5_BASE + 6,   // KDC_ERR_C_PRINCIPAL_UNKNOWN
    KRB5_BASE + 14,  // KDC_ERR_ETYPE_NOSUPP
    KRB5_BASE + 37,  // KRB_AP_ERR_SKEW
    KRB5_BASE + 68,  // KDC_ERR_WRONG_REALM
    KRB5_BASE + 154, // KRB5_REALM_UNKNOWN
    KRB5_BASE + 156, // KRB5_KDC_UNREACH
];

/// Classifies a `gss_aapl_initial_cred` failure. The CFError code is the major status; the
/// Kerberos code is in the user info. A failure whose code is not known to leave the password
/// unchecked, or that carries no code, is a refusal: NTLM must not send a password the KDC may
/// have rejected, which would count a second bad password.
fn initial_cred_failure(major: OmUint32, error: CFErrorRef) -> Failure {
    let (code, text) = if error.is_null() {
        (None, None)
    } else {
        mechanism_status(&unsafe { CFError::wrap_under_create_rule(error) })
    };
    let text = text.unwrap_or_else(|| status_text(major, GSS_C_GSS_CODE));
    match code {
        Some(code) if UNCHECKED_CODES.contains(&code) => {
            Failure::Unavailable(format!("{text} ({code})"))
        }
        Some(code) => Failure::Denied(format!("{text} ({code})")),
        None => Failure::Denied(format!("{text} (major {major:#x})")),
    }
}

/// The password as UTF-8 in a buffer that never grows, so wiping it removes the only copy this
/// process made. An unpaired surrogate becomes U+FFFD.
fn password_text(password: &[u16]) -> String {
    let mut text = String::with_capacity(password.len() * 3);
    for unit in char::decode_utf16(password.iter().copied()) {
        text.push(unit.unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    text
}

impl Credential {
    fn with_password(name: &Name, password: &[u16]) -> Result<Self, Failure> {
        let mut text = password_text(password);
        let attributes = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("kGSSICPassword"),
                CFString::new(&text).as_CFType(),
            ),
            (
                CFString::new("kGSSICCreateNewCredential"),
                CFBoolean::true_value().as_CFType(),
            ),
        ]);
        // The CFString copy of the password lives until the dictionary is released at the end
        // of this function.
        unsafe {
            text.as_bytes_mut().fill(0);
        }
        let mut handle: CredHandle = ptr::null_mut();
        let mut error: CFErrorRef = ptr::null_mut();
        let major = unsafe {
            gss_aapl_initial_cred(
                name.0,
                krb5_mechanism(),
                attributes.as_concrete_TypeRef(),
                &mut handle,
                &mut error,
            )
        };
        if major != GSS_S_COMPLETE || handle.is_null() {
            if !handle.is_null() {
                drop(Self {
                    handle,
                    owned_cache: true,
                });
            }
            return Err(initial_cred_failure(major, error));
        }
        if !error.is_null() {
            drop(unsafe { CFError::wrap_under_create_rule(error) });
        }
        Ok(Self {
            handle,
            owned_cache: true,
        })
    }

    fn from_cache(name: Option<&Name>) -> Result<Self, Failure> {
        let mechs = OidSet {
            count: 1,
            elements: krb5_mechanism(),
        };
        let mut minor = 0;
        let mut handle: CredHandle = ptr::null_mut();
        let major = unsafe {
            gss_acquire_cred(
                &mut minor,
                name.map_or(ptr::null_mut(), |name| name.0),
                0,
                &mechs,
                GSS_C_INITIATE,
                &mut handle,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if major != GSS_S_COMPLETE || handle.is_null() {
            return Err(Failure::Unavailable(format!(
                "no ticket in the credential cache: {}",
                describe(major, minor)
            )));
        }
        Ok(Self {
            handle,
            owned_cache: false,
        })
    }
}

impl ClientCredential for Credential {
    fn acquire(plan: &Plan, password: &[u16]) -> Result<Self, Failure> {
        let name = plan
            .principal
            .as_deref()
            .map(|principal| Name::import(principal, &raw const __gss_c_nt_user_name_oid_desc))
            .transpose()?;
        match (&name, password.is_empty()) {
            (Some(name), false) => Self::with_password(name, password),
            (None, false) => Err(Failure::Unavailable(
                "a password needs an account name".into(),
            )),
            (name, true) => Self::from_cache(name.as_ref()),
        }
    }

    fn initiate(
        self,
        service: &Service,
        binding: Option<&[u8]>,
    ) -> Result<Box<dyn SecurityContext>, Failure> {
        let kind = match service.kind {
            NameKind::HostBased => &raw const __gss_c_nt_hostbased_service_oid_desc,
            NameKind::Principal => &raw const __gss_krb5_nt_principal_name_oid_desc,
        };
        let target = Name::import(&service.name, kind)?;
        let mut context = Context {
            handle: ptr::null_mut(),
            _credential: self,
            target,
            binding: binding.map(<[u8]>::to_vec),
            token: None,
            complete: false,
        };
        context.advance(None)?;
        Ok(Box::new(context))
    }
}

/// An initiator context. The credential and the target name outlive the handle.
pub struct Context {
    handle: ContextHandle,
    _credential: Credential,
    target: Name,
    binding: Option<Vec<u8>>,
    token: Option<Vec<u8>>,
    complete: bool,
}

impl Context {
    fn advance(&mut self, input: Option<&[u8]>) -> Result<(), Failure> {
        let bindings = self.binding.as_deref().map(|data| ChannelBindings {
            initiator_addrtype: 0,
            initiator_address: Buffer::EMPTY,
            acceptor_addrtype: 0,
            acceptor_address: Buffer::EMPTY,
            application_data: Buffer::borrow(data),
        });
        let input = input.map(Buffer::borrow);
        let mut minor = 0;
        let mut output = Buffer::EMPTY;
        let mut flags = 0;
        let major = unsafe {
            gss_init_sec_context(
                &mut minor,
                self._credential.handle,
                &mut self.handle,
                self.target.0,
                krb5_mechanism(),
                REQUESTED_FLAGS,
                0,
                bindings.as_ref().map_or(ptr::null(), |b| b as *const _),
                input.as_ref().map_or(ptr::null(), |b| b as *const _),
                ptr::null_mut(),
                &mut output,
                &mut flags,
                ptr::null_mut(),
            )
        };
        let token = output.take();
        match major {
            GSS_S_COMPLETE => {
                const NEEDED: OmUint32 = GSS_C_MUTUAL_FLAG | GSS_C_CONF_FLAG | GSS_C_INTEG_FLAG;
                if flags & NEEDED != NEEDED {
                    return Err(Failure::Unavailable(
                        "the service did not grant mutual authentication and sealing".into(),
                    ));
                }
                self.complete = true;
            }
            GSS_S_CONTINUE_NEEDED => {}
            _ => return Err(failure(major, minor)),
        }
        self.token = (!token.is_empty()).then_some(token);
        Ok(())
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            let mut minor = 0;
            unsafe {
                gss_delete_sec_context(&mut minor, &mut self.handle, ptr::null_mut());
            }
        }
    }
}

impl SecurityContext for Context {
    fn take_token(&mut self) -> Option<Vec<u8>> {
        self.token.take()
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn step(&mut self, input: &[u8]) -> Result<(), Failure> {
        self.advance(Some(input))
    }

    fn scheme(&self) -> &'static str {
        "Kerberos"
    }
}

impl Sealer for Context {
    fn protocol(&self) -> &'static str {
        KERBEROS_PROTOCOL
    }

    fn seal(&mut self, plain: &[u8]) -> Result<(Vec<u8>, Vec<u8>), TransportError> {
        let input = Buffer::borrow(plain);
        let mut minor = 0;
        let mut conf_state = 0;
        let mut output = Buffer::EMPTY;
        let major = unsafe {
            gss_wrap(
                &mut minor,
                self.handle,
                1,
                0,
                &input,
                &mut conf_state,
                &mut output,
            )
        };
        let token = output.take();
        if major != GSS_S_COMPLETE {
            return Err(protocol(format!("wrap: {}", describe(major, minor))));
        }
        if conf_state == 0 {
            return Err(protocol("wrap did not seal the message"));
        }
        split_wrap_token(&token, plain.len()).map_err(protocol)
    }

    fn unseal(&mut self, signature: &[u8], sealed: &[u8]) -> Result<Vec<u8>, TransportError> {
        let token = join_wrap_token(signature, sealed);
        let input = Buffer::borrow(&token);
        let mut minor = 0;
        let mut conf_state = 0;
        let mut qop = 0;
        let mut output = Buffer::EMPTY;
        let major = unsafe {
            gss_unwrap(
                &mut minor,
                self.handle,
                &input,
                &mut output,
                &mut conf_state,
                &mut qop,
            )
        };
        let plain = output.take();
        if major != GSS_S_COMPLETE {
            return Err(protocol(format!("unwrap: {}", describe(major, minor))));
        }
        if conf_state == 0 {
            return Err(protocol("the reply was signed but not sealed"));
        }
        Ok(plain)
    }
}
