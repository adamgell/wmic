//! Kerberos for the WS-Management transport: which account uses it, the names it asks the KDC
//! for, and the RFC 4121 wrap tokens inside the `multipart/encrypted` framing.
//!
//! The security context itself comes from the GSS framework of macOS ([`super::gss`]). On other
//! platforms the context cannot be created and every account uses NTLM.

use super::super::{Authority, Endpoint, TransportError};
use super::framing::Sealer;
use std::net::IpAddr;

/// Why Kerberos did not authenticate the account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The KDC or the service refused the account: wrong password, expired or disabled account.
    Denied(String),
    /// Kerberos cannot serve this account or node: no KDC answers, the realm or the principal is
    /// unknown, or the platform has no Kerberos library. NTLM is tried instead unless Kerberos
    /// was asked for.
    Unavailable(String),
}

impl Failure {
    pub fn reason(&self) -> &str {
        match self {
            Self::Denied(reason) | Self::Unavailable(reason) => reason,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(reason) => write!(f, "Kerberos refused the account: {reason}"),
            Self::Unavailable(reason) => write!(f, "Kerberos is not available: {reason}"),
        }
    }
}

/// GSS name type of a service name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// `service@host` (GSS_C_NT_HOSTBASED_SERVICE).
    HostBased,
    /// `service/host[@REALM]` (GSS_KRB5_NT_PRINCIPAL_NAME).
    Principal,
}

/// The service principal the ticket is requested for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub kind: NameKind,
}

/// The Kerberos attempt an endpoint calls for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Client principal `name@REALM`; `None` uses the default credential of the ticket cache.
    pub principal: Option<String>,
    pub service: Service,
    /// `/AUTHORITY:Kerberos` was given, so a failure is reported instead of trying NTLM.
    pub required: bool,
}

/// Decides whether the endpoint tries Kerberos. `None` goes straight to NTLM: no account, a
/// local account form such as `name` or `.\name`, or `/AUTHORITY:NTLMDOMAIN:`, without
/// `/AUTHORITY:Kerberos`.
pub fn plan(endpoint: &Endpoint) -> Option<Plan> {
    let principal = principal(&endpoint.user, &endpoint.domain, &endpoint.host);
    match &endpoint.authority {
        Authority::Ntlm => None,
        Authority::Kerberos(spn) => Some(Plan {
            principal,
            service: service(&endpoint.host, spn.as_deref()),
            required: true,
        }),
        Authority::Negotiate => principal.map(|principal| Plan {
            principal: Some(principal),
            service: service(&endpoint.host, None),
            required: false,
        }),
    }
}

/// The client principal of a domain account, or `None` for an account form Kerberos cannot
/// name.
///
/// The realm is the domain in upper case. A domain without a dot is a NetBIOS name; when the
/// node is a DNS name whose first label after the host matches it, the realm is the DNS suffix
/// of the node, which is the usual relation between a NetBIOS domain and its DNS domain. A user
/// in `name@realm` form with no domain is used as typed, with the realm in upper case.
pub fn principal(user: &str, domain: &str, host: &str) -> Option<String> {
    let user = user.trim();
    let domain = domain.trim();
    if user.is_empty() || domain == "." {
        return None;
    }
    if domain.is_empty() || user.contains('@') {
        let (name, realm) = user.rsplit_once('@')?;
        let realm = realm.trim_end_matches('.');
        if name.is_empty() || realm.is_empty() {
            return None;
        }
        return Some(format!("{name}@{}", realm.to_ascii_uppercase()));
    }
    Some(format!("{user}@{}", realm(domain, host)))
}

/// The realm of a domain name typed with an account or a principal, for a request to `host`.
fn realm(domain: &str, host: &str) -> String {
    let domain = domain.trim_end_matches('.');
    if domain.contains('.') {
        return domain.to_ascii_uppercase();
    }
    dns_suffix(host)
        .filter(|suffix| {
            suffix
                .split('.')
                .next()
                .is_some_and(|label| label.eq_ignore_ascii_case(domain))
        })
        .map(str::to_ascii_uppercase)
        .unwrap_or_else(|| domain.to_ascii_uppercase())
}

/// The part of a DNS host name after its first label; `None` for a single label or an address.
fn dns_suffix(host: &str) -> Option<&str> {
    let host = host.trim().trim_end_matches('.');
    if host.parse::<IpAddr>().is_ok() {
        return None;
    }
    host.split_once('.')
        .map(|(_, suffix)| suffix)
        .filter(|suffix| !suffix.is_empty())
}

/// The service name: `HTTP@host` for the node as typed, or the principal of `/AUTHORITY`.
/// `service/host` forms are Kerberos principal names, `service@host` forms are host-based names,
/// the `DOMAIN\host` form of classic WMIC names the HTTP service of that host in the realm of
/// the domain, and a bare host becomes `HTTP@host`. An empty part of `DOMAIN\host` takes the
/// node for the host and the library's realm for the domain.
pub fn service(host: &str, explicit: Option<&str>) -> Service {
    let explicit = explicit.map(str::trim).filter(|spn| !spn.is_empty());
    match explicit {
        None => Service {
            name: format!("HTTP@{}", host.trim()),
            kind: NameKind::HostBased,
        },
        Some(spn) if spn.contains('/') => Service {
            name: spn.to_owned(),
            kind: NameKind::Principal,
        },
        Some(spn) if spn.contains('\\') => {
            let (domain, server) = spn.split_once('\\').unwrap_or_default();
            let (domain, server) = (domain.trim(), server.trim());
            let server = if server.is_empty() {
                host.trim()
            } else {
                server
            };
            let name = if domain.is_empty() {
                format!("HTTP/{server}")
            } else {
                format!("HTTP/{server}@{}", realm(domain, host))
            };
            Service {
                name,
                kind: NameKind::Principal,
            }
        }
        Some(spn) if spn.contains('@') => Service {
            name: spn.to_owned(),
            kind: NameKind::HostBased,
        },
        Some(spn) => Service {
            name: format!("HTTP@{spn}"),
            kind: NameKind::HostBased,
        },
    }
}

/// Length of the RFC 4121 wrap token header.
pub const WRAP_HEADER_LEN: usize = 16;
const WRAP_TOKEN_ID: [u8; 2] = [0x05, 0x04];
const WRAP_FLAG_SEALED: u8 = 0x02;

/// Length of the confounder that starts the encrypted part of a wrap token for every RFC 3962
/// and RFC 8009 encryption type.
const CONFOUNDER_LEN: usize = 16;

/// Splits a wrap token with confidentiality into the signature and the data of the WSMV framing.
/// The data is the last `plain_len` bytes of the token and the signature is everything before
/// them.
///
/// RFC 4121 lets the sender rotate the encrypted part right by RRC bytes. A token with RRC 0 is
/// rotated here so the filler, the header copy and the checksum move in front of the confounder;
/// the data is then the ciphertext of the plaintext alone. With AES and SHA-1 this is RRC 28 and
/// a 60-byte signature. The rotation changes no protected byte: the header copy inside the
/// ciphertext is defined with RRC 0.
pub fn split_wrap_token(token: &[u8], plain_len: usize) -> Result<(Vec<u8>, Vec<u8>), String> {
    if token.len() < WRAP_HEADER_LEN || token[..2] != WRAP_TOKEN_ID {
        return Err("wrap token is not an RFC 4121 token".into());
    }
    if token[2] & WRAP_FLAG_SEALED == 0 {
        return Err("wrap token is not sealed".into());
    }
    let mut header: [u8; WRAP_HEADER_LEN] = token[..WRAP_HEADER_LEN].try_into().unwrap_or_default();
    let body = &token[WRAP_HEADER_LEN..];
    let at = body
        .len()
        .checked_sub(plain_len)
        .ok_or("wrap token is shorter than the plaintext")?;
    if u16::from_be_bytes([header[6], header[7]]) != 0 {
        let mut signature = header.to_vec();
        signature.extend_from_slice(&body[..at]);
        return Ok((signature, body[at..].to_vec()));
    }
    let rrc = at
        .checked_sub(CONFOUNDER_LEN)
        .ok_or("wrap token has no room for the confounder")?;
    let rrc_field =
        u16::try_from(rrc).map_err(|_| "wrap token trailer does not fit the RRC field")?;
    header[6..8].copy_from_slice(&rrc_field.to_be_bytes());
    let (front, trailer) = body.split_at(body.len() - rrc);
    let mut signature = header.to_vec();
    signature.extend_from_slice(trailer);
    signature.extend_from_slice(&front[..CONFOUNDER_LEN]);
    Ok((signature, front[CONFOUNDER_LEN..].to_vec()))
}

/// Reverses [`split_wrap_token`]: the token as the GSS library reads it, with the RRC field
/// left as the sender set it.
pub fn join_wrap_token(signature: &[u8], data: &[u8]) -> Vec<u8> {
    let mut token = Vec::with_capacity(signature.len() + data.len());
    token.extend_from_slice(signature);
    token.extend_from_slice(data);
    token
}

/// A GSS security context after the initiator produced its first token.
pub trait SecurityContext: Sealer {
    /// Token to send to the service; `None` once every token has been taken.
    fn take_token(&mut self) -> Option<Vec<u8>>;
    /// True when the context is established and can seal messages.
    fn is_complete(&self) -> bool;
    /// Feeds a token from the service into the context.
    fn step(&mut self, input: &[u8]) -> Result<(), Failure>;
    /// Scheme the first token was sent with: `Kerberos` for a raw Kerberos token.
    fn scheme(&self) -> &'static str;
}

/// A credential of the client principal: a ticket-granting ticket.
pub trait ClientCredential: Sized {
    /// Acquires the credential: with a password through the KDC, without one from the ticket
    /// cache. `password` is UTF-16 as the command line holds it.
    fn acquire(plan: &Plan, password: &[u16]) -> Result<Self, Failure>;
    /// Starts the security context for `service` and produces the first token. `binding` is the
    /// `tls-server-end-point` channel binding data of the TLS connection, when there is one.
    fn initiate(
        self,
        service: &Service,
        binding: Option<&[u8]>,
    ) -> Result<Box<dyn SecurityContext>, Failure>;
}

#[cfg(target_os = "macos")]
pub use super::gss::Credential;

/// Platforms without the GSS framework: Kerberos is never available and NTLM is used.
#[cfg(not(target_os = "macos"))]
pub struct Credential;

#[cfg(not(target_os = "macos"))]
impl ClientCredential for Credential {
    fn acquire(_plan: &Plan, _password: &[u16]) -> Result<Self, Failure> {
        Err(Failure::Unavailable(
            "Kerberos needs the GSS framework of macOS".into(),
        ))
    }

    fn initiate(
        self,
        _service: &Service,
        _binding: Option<&[u8]>,
    ) -> Result<Box<dyn SecurityContext>, Failure> {
        Err(Failure::Unavailable(
            "Kerberos needs the GSS framework of macOS".into(),
        ))
    }
}

/// Protocol error of the sealing layer.
pub fn protocol(message: impl Into<String>) -> TransportError {
    TransportError::Protocol(format!("Kerberos: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realms_follow_the_domain_form() {
        assert_eq!(
            principal("LabAdmin", "CONTOSO", "cm01.contoso.com"),
            Some("LabAdmin@CONTOSO.COM".into())
        );
        assert_eq!(
            principal("LabAdmin", "contoso.com", "cm01"),
            Some("LabAdmin@CONTOSO.COM".into())
        );
        assert_eq!(
            principal("LabAdmin", "CONTOSO", "cm01"),
            Some("LabAdmin@CONTOSO".into())
        );
        assert_eq!(
            principal("LabAdmin", "CONTOSO", "192.168.50.20"),
            Some("LabAdmin@CONTOSO".into())
        );
        assert_eq!(
            principal("LabAdmin", "OTHER", "cm01.contoso.com"),
            Some("LabAdmin@OTHER".into())
        );
        assert_eq!(
            principal("LabAdmin@contoso.com", "", "cm01"),
            Some("LabAdmin@CONTOSO.COM".into())
        );
        for (user, domain) in [
            ("", "CONTOSO"),
            ("Administrator", ""),
            ("admin", "."),
            ("@x", ""),
            ("x@", ""),
        ] {
            assert_eq!(
                principal(user, domain, "cm01.contoso.com"),
                None,
                "{user:?} {domain:?}"
            );
        }
    }

    #[test]
    fn service_names_take_the_authority_form() {
        assert_eq!(
            service("cm01.contoso.com", None),
            Service {
                name: "HTTP@cm01.contoso.com".into(),
                kind: NameKind::HostBased
            }
        );
        assert_eq!(service("cm01", Some("  ")), service("cm01", None));
        assert_eq!(
            service("cm01", Some("HTTP/cm01.contoso.com@CONTOSO.COM")),
            Service {
                name: "HTTP/cm01.contoso.com@CONTOSO.COM".into(),
                kind: NameKind::Principal
            }
        );
        assert_eq!(
            service("cm01", Some("HTTP@cm01.contoso.com")),
            Service {
                name: "HTTP@cm01.contoso.com".into(),
                kind: NameKind::HostBased
            }
        );
        assert_eq!(
            service("cm01", Some("cm01.contoso.com")),
            Service {
                name: "HTTP@cm01.contoso.com".into(),
                kind: NameKind::HostBased
            }
        );
        assert_eq!(
            service("cm01.contoso.com", Some("CONTOSO\\CM01")),
            Service {
                name: "HTTP/CM01@CONTOSO.COM".into(),
                kind: NameKind::Principal
            }
        );
        assert_eq!(
            service("cm01", Some("contoso.com\\cm01")).name,
            "HTTP/cm01@CONTOSO.COM"
        );
    }

    fn token(rrc: u16, body: &[u8]) -> Vec<u8> {
        let mut t = vec![0x05, 0x04, 0x06, 0xff, 0x00, 0x00];
        t.extend_from_slice(&rrc.to_be_bytes());
        t.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 7]);
        t.extend_from_slice(body);
        t
    }

    #[test]
    fn an_unrotated_token_is_rotated_so_the_data_is_the_ciphertext_of_the_plaintext() {
        // Confounder 1..=16, plaintext 17..=20, filler, header copy and checksum 21..=26.
        let body: Vec<u8> = (1..=26).collect();
        let (signature, data) = split_wrap_token(&token(0, &body), 4).unwrap();
        assert_eq!(&signature[..6], &[0x05, 0x04, 0x06, 0xff, 0x00, 0x00]);
        assert_eq!(&signature[6..8], &6u16.to_be_bytes());
        assert_eq!(&signature[8..16], &[0, 0, 0, 0, 0, 0, 0, 7]);
        assert_eq!(&signature[16..22], &[21, 22, 23, 24, 25, 26]);
        assert_eq!(&signature[22..], &(1..=16).collect::<Vec<u8>>()[..]);
        assert_eq!(data, vec![17, 18, 19, 20]);
        let joined = join_wrap_token(&signature, &data);
        let mut rotated = body.clone();
        rotated.rotate_right(6);
        assert_eq!(&joined[16..], &rotated[..]);
    }

    #[test]
    fn an_aes_token_has_the_windows_layout() {
        // AES with SHA-1: confounder 16, header copy 16, checksum 12, no filler.
        let plain_len = 100;
        let body = vec![0u8; 16 + plain_len + 16 + 12];
        let (signature, data) = split_wrap_token(&token(0, &body), plain_len).unwrap();
        assert_eq!(&signature[6..8], &28u16.to_be_bytes());
        assert_eq!((signature.len(), data.len()), (60, plain_len));
    }

    #[test]
    fn a_rotated_token_keeps_its_rrc_and_ends_with_the_data() {
        let body: Vec<u8> = (1..=10).collect();
        let (signature, data) = split_wrap_token(&token(3, &body), 7).unwrap();
        assert_eq!(&signature[6..8], &3u16.to_be_bytes());
        assert_eq!(&signature[16..], &[1, 2, 3]);
        assert_eq!(data, vec![4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn damaged_tokens_are_refused() {
        assert!(split_wrap_token(&[5, 4, 6], 0).is_err());
        let mut unsealed = token(0, &[1; 20]);
        unsealed[2] = 0x04;
        assert!(split_wrap_token(&unsealed, 1).is_err());
        let mut other = token(0, &[1; 20]);
        other[1] = 0x03;
        assert!(split_wrap_token(&other, 1).is_err());
        assert!(split_wrap_token(&token(0, &[1, 2, 3]), 4).is_err());
        assert!(split_wrap_token(&token(9, &[1, 2, 3]), 4).is_err());
        assert!(split_wrap_token(&token(0, &[1; 20]), 5).is_err());
    }

    fn endpoint(user: &str, domain: &str, authority: Authority) -> Endpoint {
        Endpoint {
            host: "cm01.contoso.com".into(),
            connect: None,
            port: 5985,
            user: user.into(),
            domain: domain.into(),
            password: Default::default(),
            tls: None,
            authority,
        }
    }

    #[test]
    fn the_plan_selects_kerberos_for_domain_accounts_and_explicit_authority() {
        let domain = plan(&endpoint("LabAdmin", "CONTOSO", Authority::Negotiate)).unwrap();
        assert_eq!(domain.principal, Some("LabAdmin@CONTOSO.COM".into()));
        assert_eq!(domain.service.name, "HTTP@cm01.contoso.com");
        assert!(!domain.required);
        assert_eq!(
            plan(&endpoint("Administrator", "", Authority::Negotiate)),
            None
        );
        assert_eq!(
            plan(&endpoint("Administrator", ".", Authority::Negotiate)),
            None
        );
        assert_eq!(plan(&endpoint("", "", Authority::Negotiate)), None);

        let explicit = plan(&endpoint(
            "Administrator",
            "",
            Authority::Kerberos(Some("HTTP/other.contoso.com".into())),
        ))
        .unwrap();
        assert_eq!(explicit.principal, None);
        assert_eq!(explicit.service.kind, NameKind::Principal);
        assert!(explicit.required);
        let default = plan(&endpoint("LabAdmin", "CONTOSO", Authority::Kerberos(None))).unwrap();
        assert_eq!(default.service.name, "HTTP@cm01.contoso.com");
        assert!(default.required);
    }

    #[test]
    fn an_ntlm_domain_authority_never_tries_kerberos() {
        assert_eq!(
            plan(&endpoint("LabAdmin", "CONTOSO", Authority::Ntlm)),
            None
        );
    }

    #[test]
    fn naming_tolerates_trailing_dots_empty_parts_and_qualified_users() {
        assert_eq!(
            principal("LabAdmin", "contoso.com.", "cm01"),
            Some("LabAdmin@CONTOSO.COM".into())
        );
        assert_eq!(
            principal("LabAdmin@contoso.com.", "", "cm01"),
            Some("LabAdmin@CONTOSO.COM".into())
        );
        assert_eq!(
            principal("LabAdmin@contoso.com", "OTHER", "cm01"),
            Some("LabAdmin@CONTOSO.COM".into())
        );
        assert_eq!(
            service("cm01.contoso.com", Some("CONTOSO\\")).name,
            "HTTP/cm01.contoso.com@CONTOSO.COM"
        );
        assert_eq!(service("cm01", Some("\\cm01")).name, "HTTP/cm01");
    }
}
