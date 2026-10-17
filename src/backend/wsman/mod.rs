//! Remote WMI over WS-Management for hosts without DCOM.
pub mod transport;

use crate::backend::SecretWide;
use crate::model::WideText;

/// One WinRM endpoint and the account used against it.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub domain: String,
    pub password: SecretWide,
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
    Io(std::io::Error),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "connect: {e}"),
            Self::Authentication => write!(f, "authentication failed"),
            Self::Protocol(m) => write!(f, "protocol: {m}"),
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
const DEFAULT_PORT: u16 = 5985;

/// `WMIC_WSMAN_PORT` holds something other than a port number from 1 to 65535.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidPort(pub String);

impl std::fmt::Display for InvalidPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{PORT_VARIABLE} is not a port number: {}", self.0)
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

/// The listener port, overridable for non-standard listeners and tunnels.
pub fn listener_port() -> Result<u16, InvalidPort> {
    match std::env::var(PORT_VARIABLE) {
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_PORT),
        Err(std::env::VarError::NotUnicode(raw)) => {
            Err(InvalidPort(raw.to_string_lossy().into_owned()))
        }
        Ok(text) => parse_port(&text).ok_or(InvalidPort(text)),
    }
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
    use super::parse_port;

    #[test]
    fn port_values_outside_1_to_65535_are_rejected() {
        assert_eq!(parse_port("5986"), Some(5986));
        assert_eq!(parse_port(" 1 "), Some(1));
        assert_eq!(parse_port("65535"), Some(65535));
        for bad in ["0", "65536", "-1", "", "http", "59 85"] {
            assert_eq!(parse_port(bad), None, "{bad:?}");
        }
    }
}
