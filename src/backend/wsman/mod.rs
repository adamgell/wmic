//! Remote WMI over WS-Management for hosts without DCOM.
pub mod transport;

use crate::backend::SecretWide;
use crate::model::WideText;
use std::path::PathBuf;

/// TLS settings of an endpoint reached over HTTPS.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tls {
    /// PEM file with trust anchors accepted in addition to the platform trust store.
    pub ca_file: Option<PathBuf>,
}

/// Authentication the command asks for with `/AUTHORITY`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Authority {
    /// Kerberos for a domain account when a KDC answers, else NTLM.
    #[default]
    Negotiate,
    /// `/AUTHORITY:Kerberos:<spn>`: Kerberos only, for the named service principal. `None`
    /// is the default principal `HTTP@host`.
    Kerberos(Option<String>),
    /// `/AUTHORITY:NTLMDOMAIN:<domain>`: NTLM only, as classic WMIC.
    Ntlm,
}

impl Authority {
    /// Reads an `/AUTHORITY` value without regard to case: `Kerberos` or `Kerberos:<spn>` asks
    /// for Kerberos, `NTLMDOMAIN:<domain>` for NTLM; every other value leaves the choice to the
    /// transport.
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        match text.split_once(':') {
            Some((scheme, spn)) if scheme.trim().eq_ignore_ascii_case("kerberos") => {
                let spn = spn.trim();
                Self::Kerberos((!spn.is_empty()).then(|| spn.to_owned()))
            }
            Some((scheme, _)) if scheme.trim().eq_ignore_ascii_case("ntlmdomain") => Self::Ntlm,
            None if text.eq_ignore_ascii_case("kerberos") => Self::Kerberos(None),
            _ => Self::Negotiate,
        }
    }
}

/// One WinRM endpoint and the account used against it.
#[derive(Clone, Debug)]
pub struct Endpoint {
    /// Node as typed: the name of the service principal, the `Host` header, the TLS name and
    /// the `__SERVER` fallback.
    pub host: String,
    /// Address the TCP connection goes to when it is not `host`, for example a tunnel endpoint.
    pub connect: Option<String>,
    pub port: u16,
    pub user: String,
    pub domain: String,
    pub password: SecretWide,
    /// `None` sends HTTP; `Some` runs HTTP over TLS with the server certificate validated.
    pub tls: Option<Tls>,
    pub authority: Authority,
}

impl Endpoint {
    pub fn scheme(&self) -> Scheme {
        if self.tls.is_some() {
            Scheme::Https
        } else {
            Scheme::Http
        }
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

#[derive(Debug)]
pub enum TransportError {
    /// TCP connection to the listener failed.
    Connect(std::io::Error),
    /// The server rejected the credentials (HTTP 401 after the NTLM exchange or to a Kerberos
    /// token).
    Authentication,
    /// Kerberos ended the request: the KDC refused the account, or Kerberos was asked for and
    /// could not be used.
    Kerberos(transport::kerberos::Failure),
    /// The HTTP or encryption framing did not match what the service sends.
    Protocol(String),
    /// The TLS handshake failed, including a server certificate that was not accepted, or the
    /// trust anchors could not be loaded. No request was sent.
    Tls(String),
    Io(std::io::Error),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "connect: {e}"),
            Self::Authentication => write!(f, "authentication failed"),
            Self::Kerberos(failure) => write!(f, "{failure}"),
            Self::Protocol(m) => write!(f, "protocol: {m}"),
            Self::Tls(m) => write!(f, "tls: {m}"),
            Self::Io(e) => write!(f, "io: {e}"),
        }
    }
}

/// The mechanism that authenticated a connection and the scheme the server answered with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authentication {
    /// `Kerberos` or `NTLM`.
    pub mechanism: &'static str,
    /// Scheme name of the `WWW-Authenticate` header the server sent during the exchange.
    pub server_scheme: Option<String>,
}

/// Posts one SOAP envelope to `/wsman` and returns the decrypted reply with its HTTP status.
pub trait Transport {
    fn post(&mut self, envelope: &str) -> Result<Response, TransportError>;

    /// NetBIOS computer name the service reported while authenticating; known after the first
    /// successful `post`.
    fn server_name(&self) -> Option<String> {
        None
    }

    /// False when the connection must not carry the next request: the peer closed it or left
    /// unread data on it.
    fn is_reusable(&mut self) -> bool {
        true
    }

    /// How the connection authenticated; known after the first successful `post`.
    fn authentication(&self) -> Option<Authentication> {
        None
    }
}

pub const PORT_VARIABLE: &str = "WMIC_WSMAN_PORT";
pub const SCHEME_VARIABLE: &str = "WMIC_WSMAN_SCHEME";
pub const CA_FILE_VARIABLE: &str = "WMIC_WSMAN_CA_FILE";
pub const CONNECT_VARIABLE: &str = "WMIC_WSMAN_CONNECT";

/// URL scheme of the listener, which fixes the default port and whether TLS is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub fn default_port(self) -> u16 {
        match self {
            Self::Http => 5985,
            Self::Https => 5986,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

/// An environment variable holds a value the listener settings cannot use.
#[derive(Debug, PartialEq, Eq)]
pub enum InvalidSetting {
    /// `WMIC_WSMAN_PORT` holds something other than a port number from 1 to 65535.
    Port(String),
    /// `WMIC_WSMAN_SCHEME` holds something other than `http` or `https`.
    Scheme(String),
    /// `WMIC_WSMAN_CA_FILE` is set while the scheme is `http`, which would send the logon in
    /// plain text while the caller expects TLS.
    CaFileWithoutTls(String),
    /// `WMIC_WSMAN_CONNECT` is empty or holds blanks, so it names no address.
    Connect(String),
}

impl std::fmt::Display for InvalidSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Port(value) => write!(f, "{PORT_VARIABLE} is not a port number: {value}"),
            Self::Scheme(value) => write!(f, "{SCHEME_VARIABLE} is not http or https: {value}"),
            Self::CaFileWithoutTls(value) => write!(
                f,
                "{CA_FILE_VARIABLE} is set to {value} but {SCHEME_VARIABLE} is not https"
            ),
            Self::Connect(value) => write!(f, "{CONNECT_VARIABLE} is not an address: {value}"),
        }
    }
}

/// Where and how the listener of every node is reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub scheme: Scheme,
    pub port: u16,
    /// Extra trust anchors for HTTPS; `listener()` refuses it with HTTP.
    pub ca_file: Option<PathBuf>,
    /// Address the TCP connection of every node goes to instead of the node name.
    pub connect: Option<String>,
}

impl Listener {
    pub fn tls(&self) -> Option<Tls> {
        match self.scheme {
            Scheme::Http => None,
            Scheme::Https => Some(Tls {
                ca_file: self.ca_file.clone(),
            }),
        }
    }
}

/// True when the environment names the listener port, which marks a tunnel or a non-standard
/// listener.
pub fn port_is_set() -> bool {
    std::env::var_os(PORT_VARIABLE).is_some()
}

fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

/// An address is one word: a host name or an IP address, without blanks.
fn parse_connect(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && !text.contains(char::is_whitespace)).then(|| text.to_owned())
}

fn parse_scheme(text: &str) -> Option<Scheme> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("http") {
        Some(Scheme::Http)
    } else if text.eq_ignore_ascii_case("https") {
        Some(Scheme::Https)
    } else {
        None
    }
}

fn setting<T>(
    variable: &str,
    parse: impl FnOnce(&str) -> Option<T>,
    invalid: impl FnOnce(String) -> InvalidSetting,
) -> Result<Option<T>, InvalidSetting> {
    match std::env::var(variable) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(raw)) => {
            Err(invalid(raw.to_string_lossy().into_owned()))
        }
        Ok(text) => parse(&text).map(Some).ok_or(invalid(text)),
    }
}

/// The listener settings from the environment: `WMIC_WSMAN_SCHEME` (default `http`),
/// `WMIC_WSMAN_PORT` (default 5985 for `http`, 5986 for `https`), `WMIC_WSMAN_CA_FILE` and
/// `WMIC_WSMAN_CONNECT`.
pub fn listener() -> Result<Listener, InvalidSetting> {
    let scheme = setting(SCHEME_VARIABLE, parse_scheme, InvalidSetting::Scheme)?;
    let port = setting(PORT_VARIABLE, parse_port, InvalidSetting::Port)?;
    let ca_file = std::env::var_os(CA_FILE_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let connect = setting(CONNECT_VARIABLE, parse_connect, InvalidSetting::Connect)?;
    listener_of(scheme, port, ca_file, connect)
}

/// Combines the parsed settings; `None` is a variable that is not set.
fn listener_of(
    scheme: Option<Scheme>,
    port: Option<u16>,
    ca_file: Option<PathBuf>,
    connect: Option<String>,
) -> Result<Listener, InvalidSetting> {
    let scheme = scheme.unwrap_or(Scheme::Http);
    if let Some(path) = &ca_file
        && scheme != Scheme::Https
    {
        return Err(InvalidSetting::CaFileWithoutTls(
            path.to_string_lossy().into_owned(),
        ));
    }
    Ok(Listener {
        scheme,
        port: port.unwrap_or_else(|| scheme.default_port()),
        ca_file,
        connect,
    })
}

pub fn wide(text: &str) -> WideText {
    text.into()
}

pub mod catalog;
pub mod client;
pub mod convert;
pub mod fault;
pub mod manage;
pub mod parse;
pub mod path;
pub mod record;
pub mod soap;
mod status_text;
#[cfg(target_os = "macos")]
pub use crate::backend::macos::schema;
#[cfg(not(target_os = "macos"))]
#[path = "../macos/schema.rs"]
pub mod schema;

#[cfg(test)]
mod tests {
    use super::{
        Authority, InvalidSetting, Listener, Scheme, Tls, listener_of, parse_connect, parse_port,
        parse_scheme,
    };
    use std::path::PathBuf;

    #[test]
    fn port_values_outside_1_to_65535_are_rejected() {
        assert_eq!(parse_port("5986"), Some(5986));
        assert_eq!(parse_port(" 1 "), Some(1));
        assert_eq!(parse_port("65535"), Some(65535));
        for bad in ["0", "65536", "-1", "", "http", "59 85"] {
            assert_eq!(parse_port(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn scheme_values_are_http_or_https_in_any_case() {
        assert_eq!(parse_scheme("http"), Some(Scheme::Http));
        assert_eq!(parse_scheme(" HTTPS "), Some(Scheme::Https));
        for bad in ["", "https://", "tls", "ssl", "5986"] {
            assert_eq!(parse_scheme(bad), None, "{bad:?}");
        }
        assert_eq!(Scheme::Http.default_port(), 5985);
        assert_eq!(Scheme::Https.default_port(), 5986);
    }

    #[test]
    fn the_port_follows_the_scheme_and_a_ca_file_needs_https() {
        let plain = listener_of(None, None, None, None).unwrap();
        assert_eq!(
            (plain.scheme, plain.port, plain.tls()),
            (Scheme::Http, 5985, None)
        );
        let secure = listener_of(Some(Scheme::Https), None, None, None).unwrap();
        assert_eq!(secure.port, 5986);
        assert_eq!(secure.tls(), Some(Tls { ca_file: None }));
        let tunnel = listener_of(
            Some(Scheme::Https),
            Some(25986),
            Some("ca.pem".into()),
            Some("127.0.0.1".into()),
        )
        .unwrap();
        assert_eq!(
            tunnel,
            Listener {
                scheme: Scheme::Https,
                port: 25986,
                ca_file: Some(PathBuf::from("ca.pem")),
                connect: Some("127.0.0.1".into()),
            }
        );
        assert_eq!(
            listener_of(None, None, Some("ca.pem".into()), None),
            Err(InvalidSetting::CaFileWithoutTls("ca.pem".into()))
        );
        assert_eq!(
            listener_of(Some(Scheme::Http), Some(5986), Some("ca.pem".into()), None),
            Err(InvalidSetting::CaFileWithoutTls("ca.pem".into()))
        );
    }

    #[test]
    fn authority_values_select_the_mechanism() {
        assert_eq!(Authority::parse("Kerberos"), Authority::Kerberos(None));
        assert_eq!(Authority::parse(" kerberos: "), Authority::Kerberos(None));
        assert_eq!(
            Authority::parse("Kerberos :CONTOSO\\CM01"),
            Authority::Kerberos(Some("CONTOSO\\CM01".into()))
        );
        assert_eq!(
            Authority::parse("KERBEROS:HTTP/cm01.contoso.com"),
            Authority::Kerberos(Some("HTTP/cm01.contoso.com".into()))
        );
        assert_eq!(Authority::parse("NTLMDOMAIN:CONTOSO"), Authority::Ntlm);
        assert_eq!(Authority::parse("ntlmdomain:"), Authority::Ntlm);
        assert_eq!(Authority::parse(""), Authority::Negotiate);
        assert_eq!(Authority::parse("Kerberosx"), Authority::Negotiate);
    }

    #[test]
    fn the_connect_address_is_one_word() {
        assert_eq!(parse_connect(" 127.0.0.1 "), Some("127.0.0.1".into()));
        assert_eq!(parse_connect("tunnel.local"), Some("tunnel.local".into()));
        assert_eq!(parse_connect("::1"), Some("::1".into()));
        for bad in ["", "  ", "a b", "host\tname"] {
            assert_eq!(parse_connect(bad), None, "{bad:?}");
        }
    }
}
