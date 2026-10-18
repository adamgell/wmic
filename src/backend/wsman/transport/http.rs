//! Blocking HTTP/1.1 client bound to one TCP connection, with or without TLS.

use super::super::{Tls, TransportError, soap::MAX_ENVELOPE_SIZE};
use super::tls::{self, TlsStream};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Socket timeout. It must exceed the `OperationTimeout` the requests carry, or the client gives
/// up before the service answers with its own timeout fault.
pub const IO_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_LINE: u64 = 64 * 1024;
const MAX_HEADERS: usize = 200;
/// The service never sends an envelope above `MaxEnvelopeSize`; the cap leaves room for framing.
const MAX_BODY: usize = 4 * MAX_ENVELOPE_SIZE as usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self.headers_named(name).next()
    }

    pub fn headers_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.headers
            .iter()
            .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// The byte stream under HTTP: the socket itself, or TLS over it.
pub enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream>),
}

impl Stream {
    fn socket(&self) -> &TcpStream {
        match self {
            Self::Plain(socket) => socket,
            Self::Tls(tls) => tls.get_ref(),
        }
    }

    /// True when TLS holds decrypted bytes nobody asked for, the peer ended the session, or the
    /// TLS state is in error. A non-application record still in the socket, such as a session
    /// ticket, is not seen here; the socket check that follows then reports the connection as
    /// not idle, which costs a reconnect but never reuses a stale connection.
    fn is_unusable(&mut self) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Tls(tls) => match tls.conn.process_new_packets() {
                Ok(state) => state.plaintext_bytes_to_read() > 0 || state.peer_has_closed(),
                Err(_) => true,
            },
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(socket) => socket.read(buf),
            Self::Tls(tls) => tls.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(socket) => socket.write(buf),
            Self::Tls(tls) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(socket) => socket.flush(),
            Self::Tls(tls) => tls.flush(),
        }
    }
}

/// One keep-alive connection. After the peer closes it, or after any I/O error, every request fails
/// with `TransportError::Io`; the connection is never reopened.
pub struct HttpConnection {
    reader: BufReader<Stream>,
    host_header: String,
    /// DER of the server certificate when the connection runs over TLS.
    certificate: Option<Vec<u8>>,
    closed: bool,
}

fn protocol(message: impl Into<String>) -> TransportError {
    TransportError::Protocol(message.into())
}

fn open_socket(host: &str, port: u16) -> Result<TcpStream, TransportError> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(TransportError::Connect)?;
    let mut last = io::Error::new(io::ErrorKind::NotFound, "host resolved to no address");
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
                    .and_then(|()| stream.set_nodelay(true))
                    .map_err(TransportError::Connect)?;
                return Ok(stream);
            }
            Err(e) => last = e,
        }
    }
    Err(TransportError::Connect(last))
}

impl HttpConnection {
    /// Opens the connection to `connect`, or to `host` when `connect` is `None`. `host` names
    /// the server in the `Host` header and, with `tls`, in the certificate check; the handshake
    /// runs here and the server certificate is validated before the connection is returned.
    pub fn connect(
        host: &str,
        port: u16,
        connect: Option<&str>,
        tls: Option<&Tls>,
    ) -> Result<Self, TransportError> {
        let socket = open_socket(connect.unwrap_or(host), port)?;
        let (stream, certificate) = match tls {
            Some(settings) => {
                let secured = tls::connect(socket, host, settings)?;
                (
                    Stream::Tls(Box::new(secured.stream)),
                    Some(secured.certificate),
                )
            }
            None => (Stream::Plain(socket), None),
        };
        let host_header = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        Ok(Self {
            reader: BufReader::new(stream),
            host_header,
            certificate,
            closed: false,
        })
    }

    /// DER of the certificate the server presented; `None` on a plain connection.
    pub fn peer_certificate(&self) -> Option<&[u8]> {
        self.certificate.as_deref()
    }

    /// True when the connection is open and holds no unread byte. A peer that closed the socket
    /// or sent data nobody asked for makes the connection unfit for the next request.
    pub fn is_idle(&mut self) -> bool {
        if self.closed || !self.reader.buffer().is_empty() {
            return false;
        }
        if self.reader.get_mut().is_unusable() {
            return false;
        }
        let stream = self.reader.get_ref().socket();
        if stream.set_nonblocking(true).is_err() {
            return false;
        }
        let mut byte = [0u8; 1];
        let quiet = matches!(
            stream.peek(&mut byte),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted)
        );
        quiet && stream.set_nonblocking(false).is_ok()
    }

    /// Sends one POST and reads the complete response.
    pub fn post(
        &mut self,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpResponse, TransportError> {
        if self.closed {
            return Err(TransportError::Io(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection is closed",
            )));
        }
        let result = self.exchange(path, headers, body);
        if result.is_err() {
            self.closed = true;
        }
        result
    }

    fn exchange(
        &mut self,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpResponse, TransportError> {
        let mut head = format!("POST {path} HTTP/1.1\r\nHost: {}\r\n", self.host_header);
        for (name, value) in headers {
            if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
                return Err(protocol("header contains a line break"));
            }
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {}\r\nConnection: Keep-Alive\r\nUser-Agent: wmic-compat\r\n\r\n",
            body.len()
        ));
        let mut request = head.into_bytes();
        request.extend_from_slice(body);
        let stream = self.reader.get_mut();
        stream
            .write_all(&request)
            .and_then(|()| stream.flush())
            .map_err(TransportError::Io)?;
        let response = self.read_response()?;
        if response.header("Connection").is_some_and(|v| {
            v.split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("close"))
        }) {
            self.closed = true;
        }
        Ok(response)
    }

    fn read_line(&mut self) -> Result<String, TransportError> {
        let mut raw = Vec::new();
        (&mut self.reader)
            .take(MAX_LINE)
            .read_until(b'\n', &mut raw)
            .map_err(TransportError::Io)?;
        if raw.last() != Some(&b'\n') {
            return Err(if raw.len() as u64 >= MAX_LINE {
                protocol("HTTP line exceeds the size limit")
            } else {
                TransportError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed by the server",
                ))
            });
        }
        raw.pop();
        if raw.last() == Some(&b'\r') {
            raw.pop();
        }
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }

    fn read_headers(&mut self) -> Result<Vec<(String, String)>, TransportError> {
        let mut headers = Vec::new();
        loop {
            let line = self.read_line()?;
            if line.is_empty() {
                return Ok(headers);
            }
            if headers.len() >= MAX_HEADERS {
                return Err(protocol("too many HTTP headers"));
            }
            let Some((name, value)) = line.split_once(':') else {
                return Err(protocol(format!("malformed HTTP header: {line}")));
            };
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }

    fn read_response(&mut self) -> Result<HttpResponse, TransportError> {
        loop {
            let status_line = self.read_line()?;
            let mut parts = status_line.split_whitespace();
            let version = parts.next().unwrap_or_default();
            let status: u16 = match (version.starts_with("HTTP/1."), parts.next()) {
                (true, Some(code)) => code
                    .parse()
                    .map_err(|_| protocol(format!("bad HTTP status line: {status_line}")))?,
                _ => return Err(protocol(format!("bad HTTP status line: {status_line}"))),
            };
            let headers = self.read_headers()?;
            if (100..200).contains(&status) && status != 101 {
                continue;
            }
            let mut response = HttpResponse {
                status,
                headers,
                body: Vec::new(),
            };
            if version == "HTTP/1.0"
                && !response
                    .header("Connection")
                    .is_some_and(|v| v.eq_ignore_ascii_case("keep-alive"))
            {
                self.closed = true;
            }
            if status == 204 || status == 304 || status == 101 {
                return Ok(response);
            }
            let chunked = response
                .header("Transfer-Encoding")
                .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
            response.body = if chunked {
                self.read_chunked()?
            } else if let Some(length) = response.header("Content-Length") {
                let length: usize = length
                    .parse()
                    .map_err(|_| protocol(format!("bad Content-Length: {length}")))?;
                self.read_exact_body(length)?
            } else {
                self.closed = true;
                let mut body = Vec::new();
                (&mut self.reader)
                    .take(MAX_BODY as u64 + 1)
                    .read_to_end(&mut body)
                    .map_err(TransportError::Io)?;
                if body.len() > MAX_BODY {
                    return Err(protocol("response body exceeds the size limit"));
                }
                body
            };
            return Ok(response);
        }
    }

    fn read_exact_body(&mut self, length: usize) -> Result<Vec<u8>, TransportError> {
        if length > MAX_BODY {
            return Err(protocol("response body exceeds the size limit"));
        }
        let mut body = Vec::new();
        (&mut self.reader)
            .take(length as u64)
            .read_to_end(&mut body)
            .map_err(TransportError::Io)?;
        if body.len() != length {
            return Err(TransportError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed inside the response body",
            )));
        }
        Ok(body)
    }

    fn read_chunked(&mut self) -> Result<Vec<u8>, TransportError> {
        let mut body = Vec::new();
        loop {
            let line = self.read_line()?;
            let size_text = line.split(';').next().unwrap_or_default().trim();
            let size = usize::from_str_radix(size_text, 16)
                .map_err(|_| protocol(format!("bad chunk size: {line}")))?;
            if size == 0 {
                while !self.read_line()?.is_empty() {}
                return Ok(body);
            }
            if body.len().saturating_add(size) > MAX_BODY {
                return Err(protocol("response body exceeds the size limit"));
            }
            body.extend(self.read_exact_body(size)?);
            if !self.read_line()?.is_empty() {
                return Err(protocol("chunk is not followed by a line break"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_timeout_exceeds_the_operation_timeout_in_the_request_header() {
        let seconds: f64 = crate::backend::wsman::soap::OPERATION_TIMEOUT
            .strip_prefix("PT")
            .and_then(|text| text.strip_suffix('S'))
            .and_then(|text| text.parse().ok())
            .expect("OperationTimeout is a duration in seconds");
        assert!(IO_TIMEOUT.as_secs_f64() > seconds);
    }
}
