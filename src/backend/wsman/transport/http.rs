//! Blocking HTTP/1.1 client bound to one TCP connection.

use super::super::TransportError;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const IO_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LINE: u64 = 64 * 1024;
const MAX_HEADERS: usize = 200;
const MAX_BODY: usize = 128 * 1024 * 1024;

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

/// One keep-alive connection. After the peer closes it, or after any I/O error, every request fails
/// with `TransportError::Io`; the connection is never reopened.
pub struct HttpConnection {
    reader: BufReader<TcpStream>,
    host_header: String,
    closed: bool,
}

fn protocol(message: impl Into<String>) -> TransportError {
    TransportError::Protocol(message.into())
}

impl HttpConnection {
    pub fn connect(host: &str, port: u16) -> Result<Self, TransportError> {
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
                    let host_header = if host.contains(':') {
                        format!("[{host}]:{port}")
                    } else {
                        format!("{host}:{port}")
                    };
                    return Ok(Self {
                        reader: BufReader::new(stream),
                        host_header,
                        closed: false,
                    });
                }
                Err(e) => last = e,
            }
        }
        Err(TransportError::Connect(last))
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
