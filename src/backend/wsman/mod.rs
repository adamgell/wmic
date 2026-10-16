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
}

/// The listener port, overridable for non-standard listeners and tunnels.
pub fn listener_port() -> u16 {
    std::env::var("WMIC_WSMAN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5985)
}

pub fn wide(text: &str) -> WideText {
    text.into()
}
