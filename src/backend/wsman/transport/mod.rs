//! HTTP/1.1 over TCP with NTLM authentication and message encryption.
pub mod base64;
pub mod framing;
pub mod http;
pub mod ntlm;

use super::{Endpoint, Response, Transport, TransportError};
use crate::backend::SecretWide;
use http::{HttpConnection, HttpResponse};
use ntlm::{AuthenticateParams, Identity, Role, Session};

const PATH: &str = "/wsman";
const SOAP_CONTENT_TYPE: &str = "application/soap+xml;charset=UTF-8";

impl From<ntlm::NtlmError> for TransportError {
    fn from(error: ntlm::NtlmError) -> Self {
        Self::Protocol(error.to_string())
    }
}

/// Sends SOAP envelopes over one TCP connection authenticated with NTLM and sealed per message.
///
/// The NTLM exchange runs on the first `post`; the closing NTLM message travels with the first
/// sealed envelope. The security context belongs to the connection, so a connection that closes
/// or fails makes every later `post` return `TransportError::Io`.
pub struct NtlmHttpTransport {
    connection: HttpConnection,
    host: String,
    user: String,
    domain: String,
    password: SecretWide,
    session: Option<Session>,
    unusable: bool,
}

/// Extracts the base64 token of the `Negotiate` scheme from `WWW-Authenticate` headers.
pub fn negotiate_token(response: &HttpResponse) -> Option<&str> {
    response
        .headers_named("WWW-Authenticate")
        .find_map(|value| {
            let (scheme, token) = value.trim().split_once(' ')?;
            (scheme.eq_ignore_ascii_case("negotiate") && !token.trim().is_empty())
                .then(|| token.trim())
        })
}

impl NtlmHttpTransport {
    /// Opens the TCP connection. Credentials are checked by the first `post`.
    pub fn connect(endpoint: &Endpoint) -> Result<Self, TransportError> {
        Ok(Self {
            connection: HttpConnection::connect(&endpoint.host, endpoint.port)?,
            host: endpoint.host.clone(),
            user: endpoint.user.clone(),
            domain: endpoint.domain.clone(),
            password: endpoint.password.clone(),
            session: None,
            unusable: false,
        })
    }

    /// Runs the NEGOTIATE/CHALLENGE legs and returns the AUTHENTICATE header value with the
    /// sealing state derived from it.
    fn authenticate(&mut self) -> Result<(String, Session), TransportError> {
        let negotiate = ntlm::negotiate_message();
        let header = format!("Negotiate {}", base64::encode(&negotiate));
        let reply = self.connection.post(
            PATH,
            &[
                ("Authorization", &header),
                ("Content-Type", SOAP_CONTENT_TYPE),
            ],
            &[],
        )?;
        if reply.status != 401 {
            return Err(TransportError::Protocol(format!(
                "HTTP {} to the NTLM negotiate request",
                reply.status
            )));
        }
        let token = negotiate_token(&reply)
            .ok_or_else(|| TransportError::Protocol("HTTP 401 without an NTLM challenge".into()))?;
        let raw = base64::decode(token)
            .ok_or_else(|| TransportError::Protocol("challenge is not base64".into()))?;
        let challenge = ntlm::parse_challenge(&raw)?;

        let exported_session_key = ntlm::random_bytes::<16>().map_err(TransportError::Io)?;
        let client_challenge = ntlm::random_bytes::<8>().map_err(TransportError::Io)?;
        let target_name = format!("HTTP/{}", self.host);
        let authenticate = ntlm::build_authenticate(
            &negotiate,
            &challenge,
            &AuthenticateParams {
                identity: Identity {
                    user: &self.user,
                    domain: &self.domain,
                    password: &self.password.expose().0,
                },
                target_name: &target_name,
                client_challenge,
                exported_session_key,
                fallback_time: ntlm::filetime_now(),
            },
        )?;
        Ok((
            format!("Negotiate {}", base64::encode(&authenticate.message)),
            Session::new(&authenticate.exported_session_key, Role::Client),
        ))
    }

    fn exchange(&mut self, envelope: &str) -> Result<Response, TransportError> {
        let mut authorization = None;
        if self.session.is_none() {
            let (header, session) = self.authenticate()?;
            authorization = Some(header);
            self.session = Some(session);
        }
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| TransportError::Protocol("no security context".into()))?;
        let body = framing::encrypt(session, envelope.as_bytes());
        let mut headers = vec![("Content-Type", framing::CONTENT_TYPE)];
        if let Some(value) = &authorization {
            headers.push(("Authorization", value));
        }
        let reply = self.connection.post(PATH, &headers, &body)?;
        if authorization.is_some() && reply.status == 401 {
            return Err(TransportError::Authentication);
        }
        let encrypted = reply.header("Content-Type").filter(|value| {
            value
                .to_ascii_lowercase()
                .starts_with("multipart/encrypted")
        });
        let text = match encrypted {
            Some(content_type) if !reply.body.is_empty() => {
                let boundary =
                    framing::boundary_of(content_type).unwrap_or_else(|| framing::BOUNDARY.into());
                let plain = framing::decrypt(session, &reply.body, &boundary)?;
                String::from_utf8(plain)
                    .map_err(|_| TransportError::Protocol("decrypted body is not UTF-8".into()))?
            }
            _ => String::from_utf8_lossy(&reply.body).into_owned(),
        };
        Ok(Response {
            status: reply.status,
            body: text,
        })
    }
}

impl Transport for NtlmHttpTransport {
    fn post(&mut self, envelope: &str) -> Result<Response, TransportError> {
        if self.unusable {
            return Err(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "connection is no longer usable",
            )));
        }
        let result = self.exchange(envelope);
        if result.is_err() {
            self.unusable = true;
        }
        result
    }
}
