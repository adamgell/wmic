//! TLS for the WS-Management connection and the channel binding of its server certificate.

use super::super::{CA_FILE_VARIABLE, Tls, TransportError};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use rustls_platform_verifier::Verifier;
use std::net::TcpStream;
use std::sync::Arc;

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// A connection after the handshake, with the certificate the server presented.
pub struct Secured {
    pub stream: TlsStream,
    /// DER encoding of the end-entity certificate.
    pub certificate: Vec<u8>,
}

fn failure(message: impl Into<String>) -> TransportError {
    TransportError::Tls(message.into())
}

/// Builds the client configuration: platform trust store plus the anchors of the CA file.
fn client_config(settings: &Tls) -> Result<Arc<ClientConfig>, TransportError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = match &settings.ca_file {
        Some(path) => {
            let anchors = CertificateDer::pem_file_iter(path)
                .and_then(|iter| iter.collect::<Result<Vec<_>, _>>())
                .map_err(|e| failure(format!("{CA_FILE_VARIABLE} {}: {e}", path.display())))?;
            if anchors.is_empty() {
                return Err(failure(format!(
                    "{CA_FILE_VARIABLE} {}: no certificate in the file",
                    path.display()
                )));
            }
            Verifier::new_with_extra_roots(anchors, provider.clone())
        }
        None => Verifier::new(provider.clone()),
    }
    .map_err(|e| failure(format!("trust store: {e}")))?;
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| failure(format!("protocol versions: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Runs the handshake on `socket` and validates the certificate of `host`. A certificate the
/// trust store does not accept, or one issued to another name, ends the connection before any
/// request is sent.
pub fn connect(socket: TcpStream, host: &str, settings: &Tls) -> Result<Secured, TransportError> {
    let name = ServerName::try_from(host.to_owned())
        .map_err(|_| failure(format!("{host} is not a valid server name")))?;
    let connection = ClientConnection::new(client_config(settings)?, name)
        .map_err(|e| failure(format!("client setup: {e}")))?;
    let mut stream = StreamOwned::new(connection, socket);
    while stream.conn.is_handshaking() {
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|e| failure(format!("handshake with {host}: {}", describe(&e))))?;
    }
    let certificate = stream
        .conn
        .peer_certificates()
        .and_then(|chain| chain.first())
        .map(|cert| cert.to_vec())
        .ok_or_else(|| failure("the server sent no certificate"))?;
    Ok(Secured {
        stream,
        certificate,
    })
}

/// The TLS error inside an I/O error, when there is one. A verdict of the platform trust
/// store is printed with its own text.
fn describe(error: &std::io::Error) -> String {
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::InvalidCertificate(rustls::CertificateError::Other(other))) => {
            format!("invalid peer certificate: {other}")
        }
        Some(tls) => tls.to_string(),
        None => error.to_string(),
    }
}

// --- Channel binding ---------------------------------------------------------------------------

const PREFIX: &[u8] = b"tls-server-end-point:";

const OID_SHA1_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x05];
const OID_MD5_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x04];
const OID_MD2_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x02];
const OID_RSA_PSS: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0A];
const OID_SHA256_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B];
const OID_SHA384_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C];
const OID_SHA512_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D];
const OID_ECDSA_SHA1: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x01];
const OID_ECDSA_SHA256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
const OID_ECDSA_SHA384: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03];
const OID_ECDSA_SHA512: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x04];
const OID_SHA1: &[u8] = &[0x2B, 0x0E, 0x03, 0x02, 0x1A];
const OID_SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
const OID_SHA384: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02];
const OID_SHA512: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03];

const TAG_SEQUENCE: u8 = 0x30;
const TAG_OID: u8 = 0x06;
const TAG_CONTEXT_0: u8 = 0xA0;

/// One DER element: its tag, its content, and the offset after it.
fn element(bytes: &[u8], at: usize) -> Option<(u8, &[u8], usize)> {
    let tag = *bytes.get(at)?;
    let first = *bytes.get(at + 1)?;
    let (length, start) = if first < 0x80 {
        (usize::from(first), at + 2)
    } else {
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 {
            return None;
        }
        let digits = bytes.get(at + 2..at + 2 + count)?;
        let length = digits
            .iter()
            .fold(0usize, |acc, digit| (acc << 8) | usize::from(*digit));
        (length, at + 2 + count)
    };
    let end = start.checked_add(length)?;
    Some((tag, bytes.get(start..end)?, end))
}

fn expect(bytes: &[u8], at: usize, tag: u8) -> Option<(&[u8], usize)> {
    let (found, content, next) = element(bytes, at)?;
    (found == tag).then_some((content, next))
}

/// Hash function RFC 5929 assigns to the certificate's signature algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hash {
    Sha256,
    Sha384,
    Sha512,
}

fn dotted(oid: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut value: u64 = 0;
    for (index, byte) in oid.iter().enumerate() {
        value = (value << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            if index == 0 || parts.is_empty() && index > 0 && value >= 80 {
                parts.push((value / 40).min(2).to_string());
                parts.push((value - 40 * (value / 40).min(2)).to_string());
            } else {
                parts.push(value.to_string());
            }
            value = 0;
        }
    }
    parts.join(".")
}

/// The signature algorithm OID of the certificate and, for RSASSA-PSS, the hash OID of its
/// parameters.
fn signature_algorithm(certificate: &[u8]) -> Option<(&[u8], Option<&[u8]>)> {
    let (outer, _) = expect(certificate, 0, TAG_SEQUENCE)?;
    let (_, after_tbs) = expect(outer, 0, TAG_SEQUENCE)?;
    let (algorithm, _) = expect(outer, after_tbs, TAG_SEQUENCE)?;
    let (oid, after_oid) = expect(algorithm, 0, TAG_OID)?;
    if oid != OID_RSA_PSS {
        return Some((oid, None));
    }
    // RSASSA-PSS-params ::= SEQUENCE { hashAlgorithm [0] AlgorithmIdentifier DEFAULT sha1, ... }
    let Some((parameters, _)) = expect(algorithm, after_oid, TAG_SEQUENCE) else {
        return Some((oid, Some(OID_SHA1)));
    };
    let Some((tagged, _)) = expect(parameters, 0, TAG_CONTEXT_0) else {
        return Some((oid, Some(OID_SHA1)));
    };
    let (identifier, _) = expect(tagged, 0, TAG_SEQUENCE)?;
    let (hash, _) = expect(identifier, 0, TAG_OID)?;
    Some((oid, Some(hash)))
}

/// Hash for `tls-server-end-point` (RFC 5929 section 4.1): the hash of the signature algorithm,
/// with MD5 and SHA-1 replaced by SHA-256.
pub fn binding_hash(certificate: &[u8]) -> Result<Hash, String> {
    let (oid, pss_hash) =
        signature_algorithm(certificate).ok_or("certificate is not DER encoded")?;
    let hash = match (oid, pss_hash) {
        (OID_SHA256_RSA | OID_ECDSA_SHA256, _) => Hash::Sha256,
        (OID_SHA384_RSA | OID_ECDSA_SHA384, _) => Hash::Sha384,
        (OID_SHA512_RSA | OID_ECDSA_SHA512, _) => Hash::Sha512,
        (OID_SHA1_RSA | OID_MD5_RSA | OID_MD2_RSA | OID_ECDSA_SHA1, _) => Hash::Sha256,
        (OID_RSA_PSS, Some(OID_SHA1 | OID_SHA256)) => Hash::Sha256,
        (OID_RSA_PSS, Some(OID_SHA384)) => Hash::Sha384,
        (OID_RSA_PSS, Some(OID_SHA512)) => Hash::Sha512,
        (OID_RSA_PSS, Some(other)) => {
            return Err(format!(
                "no channel binding hash for RSASSA-PSS with hash {}",
                dotted(other)
            ));
        }
        (other, _) => {
            return Err(format!(
                "no channel binding hash for signature algorithm {}",
                dotted(other)
            ));
        }
    };
    Ok(hash)
}

fn digest(hash: Hash, data: &[u8]) -> Vec<u8> {
    let algorithm = match hash {
        Hash::Sha256 => &ring::digest::SHA256,
        Hash::Sha384 => &ring::digest::SHA384,
        Hash::Sha512 => &ring::digest::SHA512,
    };
    ring::digest::digest(algorithm, data).as_ref().to_vec()
}

/// The `tls-server-end-point` channel binding data of a server certificate: the prefix followed
/// by the certificate hash.
pub fn server_end_point(certificate: &[u8]) -> Result<Vec<u8>, String> {
    let hash = binding_hash(certificate)?;
    let mut out = PREFIX.to_vec();
    out.extend(digest(hash, certificate));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tls/");
        CertificateDer::from_pem_file(format!("{path}{name}.pem"))
            .unwrap()
            .to_vec()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hash_follows_the_signature_algorithm_with_sha1_raised_to_sha256() {
        assert_eq!(
            binding_hash(&fixture("loopback-rsa-sha256")),
            Ok(Hash::Sha256)
        );
        assert_eq!(
            binding_hash(&fixture("other-ecdsa-sha384")),
            Ok(Hash::Sha384)
        );
        assert_eq!(binding_hash(&fixture("legacy-rsa-sha1")), Ok(Hash::Sha256));
    }

    #[test]
    fn end_point_data_matches_independently_computed_digests() {
        // Digests from .NET HashAlgorithm over the DER of the same files.
        let rsa = server_end_point(&fixture("loopback-rsa-sha256")).unwrap();
        assert_eq!(&rsa[..21], b"tls-server-end-point:");
        assert_eq!(
            hex(&rsa[21..]),
            "d227ede37c81c70c10f116bcda713caa23c06fdecfbadcb1aa94f8c147d8b86a"
        );
        let ecdsa = server_end_point(&fixture("other-ecdsa-sha384")).unwrap();
        assert_eq!(
            hex(&ecdsa[21..]),
            "30dab1ad07e98abecccc05be606eb7362e6792486301e6fdb7d147fc4de77fff119414cf9901c7a990bbdade8da92457"
        );
        let sha1 = server_end_point(&fixture("legacy-rsa-sha1")).unwrap();
        assert_eq!(
            hex(&sha1[21..]),
            "7a43a6323b9e06d3a8509f9a39503e5c6b00f78cf5749481335a77196f5f345d"
        );
    }

    /// Builds `SEQUENCE { SEQUENCE {} (tbs), SEQUENCE { OID, params } }`.
    fn certificate_with_algorithm(oid: &[u8], params: &[u8]) -> Vec<u8> {
        let mut algorithm = vec![TAG_OID, oid.len() as u8];
        algorithm.extend_from_slice(oid);
        algorithm.extend_from_slice(params);
        let mut body = vec![TAG_SEQUENCE, 0, TAG_SEQUENCE, algorithm.len() as u8];
        body.extend(algorithm);
        let mut out = vec![TAG_SEQUENCE, body.len() as u8];
        out.extend(body);
        out
    }

    #[test]
    fn pss_takes_the_hash_from_its_parameters() {
        // hashAlgorithm [0] { SEQUENCE { OID sha384 } }
        let mut params = vec![
            TAG_SEQUENCE,
            15,
            TAG_CONTEXT_0,
            13,
            TAG_SEQUENCE,
            11,
            TAG_OID,
            9,
        ];
        params.extend_from_slice(OID_SHA384);
        assert_eq!(
            binding_hash(&certificate_with_algorithm(OID_RSA_PSS, &params)),
            Ok(Hash::Sha384)
        );
        assert_eq!(
            binding_hash(&certificate_with_algorithm(OID_RSA_PSS, &[0x30, 0])),
            Ok(Hash::Sha256),
            "absent hashAlgorithm defaults to SHA-1, raised to SHA-256"
        );
    }

    #[test]
    fn unknown_algorithms_and_damaged_encodings_are_refused() {
        let ed25519 = [0x2B, 0x65, 0x70];
        assert_eq!(
            binding_hash(&certificate_with_algorithm(&ed25519, &[])),
            Err("no channel binding hash for signature algorithm 1.3.101.112".into())
        );
        assert!(binding_hash(&[]).is_err());
        assert!(binding_hash(&[0x30, 0x05, 0x30, 0x00, 0x02, 0x01, 0x00]).is_err());
        assert!(binding_hash(&[0x30, 0x84, 0xFF, 0xFF, 0xFF, 0xFF]).is_err());
    }

    #[test]
    fn oids_print_in_dotted_form() {
        assert_eq!(dotted(OID_SHA256_RSA), "1.2.840.113549.1.1.11");
        assert_eq!(dotted(OID_SHA256), "2.16.840.1.101.3.4.2.1");
    }
}
