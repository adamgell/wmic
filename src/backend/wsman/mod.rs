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

/// One WinRM endpoint and the account used against it.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub domain: String,
    pub password: SecretWide,
    /// `None` sends HTTP; `Some` runs HTTP over TLS with the server certificate validated.
    pub tls: Option<Tls>,
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
    /// The server rejected the credentials (HTTP 401 after the NTLM exchange).
    Authentication,
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
            Self::Protocol(m) => write!(f, "protocol: {m}"),
            Self::Tls(m) => write!(f, "tls: {m}"),
            Self::Io(e) => write!(f, "io: {e}"),
        }
    }
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
}

pub const PORT_VARIABLE: &str = "WMIC_WSMAN_PORT";
pub const SCHEME_VARIABLE: &str = "WMIC_WSMAN_SCHEME";
pub const CA_FILE_VARIABLE: &str = "WMIC_WSMAN_CA_FILE";

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
}

impl std::fmt::Display for InvalidSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Port(value) => write!(f, "{PORT_VARIABLE} is not a port number: {value}"),
            Self::Scheme(value) => write!(f, "{SCHEME_VARIABLE} is not http or https: {value}"),
        }
    }
}

/// Where and how the listener of every node is reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub scheme: Scheme,
    pub port: u16,
    /// Extra trust anchors for HTTPS; ignored for HTTP.
    pub ca_file: Option<PathBuf>,
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
/// `WMIC_WSMAN_PORT` (default 5985 for `http`, 5986 for `https`) and `WMIC_WSMAN_CA_FILE`.
pub fn listener() -> Result<Listener, InvalidSetting> {
    let scheme =
        setting(SCHEME_VARIABLE, parse_scheme, InvalidSetting::Scheme)?.unwrap_or(Scheme::Http);
    let port = setting(PORT_VARIABLE, parse_port, InvalidSetting::Port)?
        .unwrap_or_else(|| scheme.default_port());
    let ca_file = std::env::var_os(CA_FILE_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    Ok(Listener {
        scheme,
        port,
        ca_file,
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
    use super::{Scheme, parse_port, parse_scheme};

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
}
