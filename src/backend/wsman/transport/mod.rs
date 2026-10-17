//! HTTP/1.1 over TCP, plain or inside TLS, with NTLM authentication and message encryption.
pub mod base64;
pub mod framing;
pub mod http;
pub mod ntlm;
pub mod tls;

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
/// or fails makes every later `post` return `TransportError::Io`. An HTTP 401 reply after the
/// handshake is returned once with an empty body and ends the connection the same way.
///
/// Over TLS, the NTLM exchange carries the `tls-server-end-point` channel binding of the server
/// certificate, so a server that checks channel bindings refuses the logon when it arrives over
/// another TLS connection.
pub struct NtlmHttpTransport {
    connection: HttpConnection,
    host: String,
    user: String,
    domain: String,
    /// Held until the NTLM exchange has used it.
    password: Option<SecretWide>,
    /// MsvAvChannelBindings value: the hash of the TLS channel binding, zeros without TLS.
    channel_binding: [u8; 16],
    server_name: Option<String>,
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
    /// Opens the connection and, over TLS, validates the server certificate. Credentials are
    /// checked by the first `post`.
    pub fn connect(endpoint: &Endpoint) -> Result<Self, TransportError> {
        let connection =
            HttpConnection::connect(&endpoint.host, endpoint.port, endpoint.tls.as_ref())?;
        let channel_binding = match connection.peer_certificate() {
            Some(certificate) => {
                let data = tls::server_end_point(certificate).map_err(TransportError::Tls)?;
                ntlm::channel_binding(&data)
            }
            None => [0; 16],
        };
        Ok(Self {
            connection,
            host: endpoint.host.clone(),
            user: endpoint.user.clone(),
            domain: endpoint.domain.clone(),
            password: Some(endpoint.password.clone()),
            channel_binding,
            server_name: None,
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
        let token = negotiate_token(&reply).ok_or(TransportError::Authentication)?;
        let raw = base64::decode(token)
            .ok_or_else(|| TransportError::Protocol("challenge is not base64".into()))?;
        let challenge = ntlm::parse_challenge(&raw)?;

        self.server_name = challenge.computer_name();
        let password = self
            .password
            .take()
            .ok_or_else(|| TransportError::Protocol("credentials already used".into()))?;
        let mut exported_session_key = ntlm::random_bytes::<16>().map_err(TransportError::Io)?;
        let client_challenge = ntlm::random_bytes::<8>().map_err(TransportError::Io)?;
        let target_name = format!("HTTP/{}", self.host);
        let mut params = AuthenticateParams {
            identity: Identity {
                user: &self.user,
                domain: &self.domain,
                password: &password.expose().0,
            },
            target_name: &target_name,
            client_challenge,
            exported_session_key,
            fallback_time: ntlm::filetime_now(),
            channel_binding: self.channel_binding,
        };
        ntlm::wipe(&mut exported_session_key);
        let built = ntlm::build_authenticate(&negotiate, &challenge, &params);
        ntlm::wipe(&mut params.exported_session_key);
        let mut authenticate = built?;
        let session = Session::new(&authenticate.exported_session_key, Role::Client);
        ntlm::wipe(&mut authenticate.exported_session_key);
        Ok((
            format!("Negotiate {}", base64::encode(&authenticate.message)),
            session,
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
            // Only the sealed channel is trusted: a plaintext body could come from anyone on the
            // path, so only an empty body (HTTP 401, 413) is accepted unsealed.
            _ if reply.body.is_empty() => String::new(),
            _ => {
                return Err(TransportError::Protocol(
                    "reply body is not encrypted".into(),
                ));
            }
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
        let rejected = matches!(&result, Ok(reply) if reply.status == 401);
        if result.is_err() || rejected {
            self.unusable = true;
        }
        result
    }

    fn server_name(&self) -> Option<String> {
        self.server_name.clone()
    }

    fn is_reusable(&mut self) -> bool {
        !self.unusable && self.connection.is_idle()
    }
}
