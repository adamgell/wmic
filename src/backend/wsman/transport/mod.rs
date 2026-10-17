//! HTTP/1.1 over TCP, plain or inside TLS, with Kerberos or NTLM authentication and message
//! encryption.
pub mod base64;
pub mod framing;
#[cfg(target_os = "macos")]
pub mod gss;
pub mod http;
pub mod kerberos;
pub mod ntlm;
pub mod tls;

use super::{Authentication, Endpoint, Response, Transport, TransportError};
use crate::backend::SecretWide;
use framing::Sealer;
use http::{HttpConnection, HttpResponse};
use kerberos::{ClientCredential, Failure, SecurityContext};
use ntlm::{AuthenticateParams, Identity, Role, Session};

const PATH: &str = "/wsman";
const SOAP_CONTENT_TYPE: &str = "application/soap+xml;charset=UTF-8";

impl From<ntlm::NtlmError> for TransportError {
    fn from(error: ntlm::NtlmError) -> Self {
        Self::Protocol(error.to_string())
    }
}

/// Extracts the base64 token of `scheme` from the `WWW-Authenticate` headers.
pub fn auth_token<'a>(response: &'a HttpResponse, scheme: &str) -> Option<&'a str> {
    response
        .headers_named("WWW-Authenticate")
        .find_map(|value| {
            let (name, token) = value.trim().split_once(' ')?;
            (name.eq_ignore_ascii_case(scheme) && !token.trim().is_empty()).then(|| token.trim())
        })
}

/// Extracts the base64 token of the `Negotiate` scheme from `WWW-Authenticate` headers.
pub fn negotiate_token(response: &HttpResponse) -> Option<&str> {
    auth_token(response, "Negotiate")
}

/// Scheme names the server offered in `WWW-Authenticate`, for diagnostics.
fn offered_schemes(response: &HttpResponse) -> Vec<String> {
    response
        .headers_named("WWW-Authenticate")
        .map(|value| {
            value
                .trim()
                .split(' ')
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

fn open(endpoint: &Endpoint) -> Result<HttpConnection, TransportError> {
    HttpConnection::connect(
        &endpoint.host,
        endpoint.port,
        endpoint.connect.as_deref(),
        endpoint.tls.as_ref(),
    )
}

/// `tls-server-end-point` channel binding data of the connection; `None` without TLS.
fn binding_data(connection: &HttpConnection) -> Result<Option<Vec<u8>>, TransportError> {
    connection
        .peer_certificate()
        .map(|certificate| tls::server_end_point(certificate).map_err(TransportError::Tls))
        .transpose()
}

/// Opens the connection and chooses the authentication: Kerberos for a domain account whose
/// KDC answers, or for `/AUTHORITY:Kerberos`, else NTLM. Over TLS, both carry the
/// `tls-server-end-point` channel binding of the server certificate.
///
/// The ticket comes from the KDC before the TCP connection opens, and the Kerberos context is
/// established with the service before `connect` returns. When the KDC cannot be used, or the
/// service refuses the ticket, and Kerberos was not asked for, the logon continues with NTLM. A
/// service refusal costs no bad-password count, because the KDC already accepted the account.
/// NTLM credentials are checked by the first `post`.
pub fn connect(endpoint: &Endpoint) -> Result<Box<dyn Transport>, TransportError> {
    connect_with::<kerberos::Credential>(endpoint)
}

/// [`connect`] with the Kerberos credential type as a parameter.
pub fn connect_with<C: ClientCredential>(
    endpoint: &Endpoint,
) -> Result<Box<dyn Transport>, TransportError> {
    let Some(plan) = kerberos::plan(endpoint) else {
        return Ok(Box::new(NtlmHttpTransport::connect(endpoint)?));
    };
    let credential = match C::acquire(&plan, &endpoint.password.expose().0) {
        Ok(credential) => credential,
        Err(failure) => {
            fallback(&plan, failure)?;
            return Ok(Box::new(NtlmHttpTransport::connect(endpoint)?));
        }
    };
    let connection = open(endpoint)?;
    let binding = binding_data(&connection)?;
    let context = match credential.initiate(&plan.service, binding.as_deref()) {
        Ok(context) => context,
        Err(failure) => {
            fallback(&plan, failure)?;
            return Ok(Box::new(NtlmHttpTransport::with_connection(
                connection,
                endpoint,
                binding.as_deref(),
            )));
        }
    };
    let mut transport = KerberosHttpTransport::new(connection, context);
    match transport.establish() {
        Ok(()) => Ok(Box::new(transport)),
        Err(TransportError::Kerberos(failure)) => {
            fallback(&plan, failure)?;
            Ok(Box::new(NtlmHttpTransport::connect(endpoint)?))
        }
        Err(TransportError::Authentication | TransportError::Protocol(_)) if !plan.required => {
            Ok(Box::new(NtlmHttpTransport::connect(endpoint)?))
        }
        Err(error) => Err(error),
    }
}

/// Decides what a Kerberos failure means: with `/AUTHORITY:Kerberos` every failure ends the
/// request; a refused account ends it too; anything else continues with NTLM.
fn fallback(plan: &kerberos::Plan, failure: Failure) -> Result<(), TransportError> {
    match failure {
        failure if plan.required => Err(TransportError::Kerberos(failure)),
        Failure::Denied(_) => Err(TransportError::Kerberos(failure)),
        Failure::Unavailable(_) => Ok(()),
    }
}

/// Reads one sealed reply. Only the sealed channel is trusted: a plaintext body could come from
/// anyone on the path, so only an empty body (HTTP 401, 413) is accepted unsealed.
fn read_sealed(sealer: &mut dyn Sealer, reply: &HttpResponse) -> Result<Response, TransportError> {
    let encrypted = reply.header("Content-Type").filter(|value| {
        value
            .to_ascii_lowercase()
            .starts_with("multipart/encrypted")
    });
    let text = match encrypted {
        Some(content_type) if !reply.body.is_empty() => {
            let boundary =
                framing::boundary_of(content_type).unwrap_or_else(|| framing::BOUNDARY.into());
            let plain = framing::decrypt(sealer, &reply.body, &boundary)?;
            String::from_utf8(plain)
                .map_err(|_| TransportError::Protocol("decrypted body is not UTF-8".into()))?
        }
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
    server_scheme: Option<String>,
    session: Option<Session>,
    unusable: bool,
}

impl NtlmHttpTransport {
    /// Opens the connection and, over TLS, validates the server certificate. Credentials are
    /// checked by the first `post`.
    pub fn connect(endpoint: &Endpoint) -> Result<Self, TransportError> {
        let connection = open(endpoint)?;
        let binding = binding_data(&connection)?;
        Ok(Self::with_connection(
            connection,
            endpoint,
            binding.as_deref(),
        ))
    }

    /// Takes an open connection; `binding` is its `tls-server-end-point` data when it runs over
    /// TLS.
    pub fn with_connection(
        connection: HttpConnection,
        endpoint: &Endpoint,
        binding: Option<&[u8]>,
    ) -> Self {
        Self {
            connection,
            host: endpoint.host.clone(),
            user: endpoint.user.clone(),
            domain: endpoint.domain.clone(),
            password: Some(endpoint.password.clone()),
            channel_binding: binding.map_or([0; 16], ntlm::channel_binding),
            server_name: None,
            server_scheme: None,
            session: None,
            unusable: false,
        }
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
        self.server_scheme = Some("Negotiate".into());
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
        let body = framing::encrypt(session, envelope.as_bytes())?;
        let mut headers = vec![("Content-Type", framing::CONTENT_TYPE)];
        if let Some(value) = &authorization {
            headers.push(("Authorization", value));
        }
        let reply = self.connection.post(PATH, &headers, &body)?;
        if authorization.is_some() && reply.status == 401 {
            return Err(TransportError::Authentication);
        }
        read_sealed(session, &reply)
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

    fn authentication(&self) -> Option<Authentication> {
        self.session.as_ref().map(|_| Authentication {
            mechanism: "NTLM",
            server_scheme: self.server_scheme.clone(),
        })
    }
}

/// Sends SOAP envelopes over one TCP connection authenticated with a Kerberos context and sealed
/// per message with that context.
///
/// The context tokens travel in requests without a body until the context is established, so
/// the service authenticates the ticket, and the client the service, before any envelope is
/// sealed. HTTP 401 during that exchange means the service refused the ticket. Afterwards the
/// connection carries sealed envelopes without an `Authorization` header; the service keeps the
/// authentication for the connection.
pub struct KerberosHttpTransport {
    connection: HttpConnection,
    context: Box<dyn SecurityContext>,
    /// Token the context produced after it completed, sent with the first sealed request.
    pending: Option<Vec<u8>>,
    server_scheme: Option<String>,
    established: bool,
    unusable: bool,
}

impl KerberosHttpTransport {
    pub fn new(connection: HttpConnection, context: Box<dyn SecurityContext>) -> Self {
        Self {
            connection,
            context,
            pending: None,
            server_scheme: None,
            established: false,
            unusable: false,
        }
    }

    /// Exchanges context tokens with the service until the context is complete.
    fn handshake(&mut self) -> Result<(), TransportError> {
        let scheme = self.context.scheme();
        loop {
            let token = self.context.take_token();
            if self.context.is_complete() {
                self.pending = token;
                return Ok(());
            }
            let token = token.ok_or_else(|| {
                TransportError::Protocol("Kerberos context needs a token it did not produce".into())
            })?;
            let header = format!("{scheme} {}", base64::encode(&token));
            let reply = self.connection.post(
                PATH,
                &[
                    ("Authorization", &header),
                    ("Content-Type", SOAP_CONTENT_TYPE),
                ],
                &[],
            )?;
            let server_token = auth_token(&reply, scheme);
            if server_token.is_some() {
                self.server_scheme = Some(scheme.to_owned());
            }
            match (reply.status, server_token) {
                (401, None) => return Err(TransportError::Authentication),
                (status @ (200 | 401), Some(token)) => {
                    let raw = base64::decode(token).ok_or_else(|| {
                        TransportError::Protocol("Kerberos reply token is not base64".into())
                    })?;
                    self.context.step(&raw).map_err(TransportError::Kerberos)?;
                    if status == 401 && self.context.is_complete() {
                        return Err(TransportError::Authentication);
                    }
                }
                (200, None) => {
                    return Err(TransportError::Protocol(
                        "the service accepted the Kerberos token without mutual authentication"
                            .into(),
                    ));
                }
                (status, _) => {
                    return Err(TransportError::Protocol(format!(
                        "HTTP {status} to the Kerberos token, schemes offered: {}",
                        offered_schemes(&reply).join(" ")
                    )));
                }
            }
        }
    }

    /// Establishes the context with the service unless that already happened. A failure leaves
    /// the connection unusable.
    pub fn establish(&mut self) -> Result<(), TransportError> {
        if self.established {
            return Ok(());
        }
        if self.unusable {
            return Err(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "connection is no longer usable",
            )));
        }
        let result = self.handshake();
        match result {
            Ok(()) => self.established = true,
            Err(_) => self.unusable = true,
        }
        result
    }

    fn exchange(&mut self, envelope: &str) -> Result<Response, TransportError> {
        self.establish()?;
        let authorization = self
            .pending
            .take()
            .map(|token| format!("{} {}", self.context.scheme(), base64::encode(&token)));
        let sealer: &mut dyn Sealer = &mut *self.context;
        let body = framing::encrypt(sealer, envelope.as_bytes())?;
        let content_type = framing::content_type(sealer.protocol());
        let mut headers = vec![("Content-Type", content_type.as_str())];
        if let Some(value) = &authorization {
            headers.push(("Authorization", value));
        }
        let reply = self.connection.post(PATH, &headers, &body)?;
        read_sealed(&mut *self.context, &reply)
    }
}

impl Transport for KerberosHttpTransport {
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

    fn is_reusable(&mut self) -> bool {
        !self.unusable && self.connection.is_idle()
    }

    fn authentication(&self) -> Option<Authentication> {
        self.established.then(|| Authentication {
            mechanism: "Kerberos",
            server_scheme: self.server_scheme.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(required: bool) -> kerberos::Plan {
        kerberos::Plan {
            principal: Some("LabAdmin@CONTOSO".into()),
            service: kerberos::service("cm01", None),
            required,
        }
    }

    #[test]
    fn unavailable_kerberos_continues_with_ntlm_when_kerberos_was_not_required() {
        let failure = Failure::Unavailable("no KDC".into());
        assert!(fallback(&plan(false), failure).is_ok());
    }

    #[test]
    fn unavailable_kerberos_ends_the_request_when_kerberos_was_required() {
        let failure = Failure::Unavailable("no KDC".into());
        let result = fallback(&plan(true), failure.clone());
        assert!(matches!(result, Err(TransportError::Kerberos(got)) if got == failure));
    }

    #[test]
    fn a_denied_account_ends_the_request_without_ntlm() {
        for required in [false, true] {
            let failure = Failure::Denied("wrong password".into());
            let result = fallback(&plan(required), failure.clone());
            assert!(
                matches!(result, Err(TransportError::Kerberos(got)) if got == failure),
                "required={required}"
            );
        }
    }
}
