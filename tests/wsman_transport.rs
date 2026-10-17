#![cfg(not(windows))]

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use wmic_compat::backend::SecretWide;
use wmic_compat::backend::wsman::transport::ntlm::{self, Role, Session};
use wmic_compat::backend::wsman::transport::{NtlmHttpTransport, base64, framing, http, tls};
use wmic_compat::backend::wsman::{Endpoint, Tls, Transport, TransportError};

fn hex(text: &str) -> Vec<u8> {
    let text: String = text.split_whitespace().collect();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

const SERVER_CHALLENGE: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];

#[test]
fn base64_round_trips_and_rejects_garbage() {
    assert_eq!(base64::encode(b""), "");
    assert_eq!(base64::encode(b"f"), "Zg==");
    assert_eq!(base64::encode(b"fo"), "Zm8=");
    assert_eq!(base64::encode(b"foo"), "Zm9v");
    assert_eq!(base64::encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64::decode("Zm9vYmFy").unwrap(), b"foobar");
    assert_eq!(base64::decode("Zm8=").unwrap(), b"fo");
    assert_eq!(base64::decode("Zm8").unwrap(), b"fo");
    assert_eq!(base64::decode("Zm9v\r\nYmFy").unwrap(), b"foobar");
    assert!(base64::decode("Zm9v*mFy").is_none());
    assert!(base64::decode("Z").is_none());
    assert!(base64::decode("====").is_none());
    for len in 0..40usize {
        let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
        assert_eq!(base64::decode(&base64::encode(&data)).unwrap(), data);
    }
}

#[test]
fn ms_nlmp_4_2_4_response_vectors() {
    let password = units("Password");
    assert_eq!(
        to_hex(&ntlm::ntowf_v1(&password)),
        "a4f49c406510bdcab6824ee7c30fd852"
    );
    let response_key = ntlm::ntowf_v2(&password, "User", "Domain");
    assert_eq!(to_hex(&response_key), "0c868a403bfd7a93a3001ef22ef02e3f");

    let av = ntlm::encode_av_pairs(&[
        (ntlm::AV_NB_DOMAIN_NAME, ntlm::utf16le("Domain")),
        (ntlm::AV_NB_COMPUTER_NAME, ntlm::utf16le("Server")),
    ]);
    let nt = ntlm::ntlmv2_response(&response_key, &SERVER_CHALLENGE, &[0; 8], &[0xaa; 8], &av);
    assert_eq!(
        to_hex(&nt.blob),
        "0101000000000000 0000000000000000 aaaaaaaaaaaaaaaa 00000000 \
         02000c00 44006f006d00610069006e00 01000c00 530065007200760065007200 00000000 00000000"
            .split_whitespace()
            .collect::<String>()
    );
    assert_eq!(to_hex(&nt.proof), "68cd0ab851e51c96aabc927bebef6a1c");
    assert_eq!(
        to_hex(&nt.session_base_key),
        "8de40ccadbc14a82f15cb0ad0de95ca3"
    );
    let lm = ntlm::lmv2_response(&response_key, &SERVER_CHALLENGE, &[0xaa; 8]);
    assert_eq!(
        to_hex(&lm),
        "86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa"
    );
    assert_eq!(
        to_hex(&ntlm::rc4k(&nt.session_base_key, &[0x55; 16])),
        "c5dad2544fc9799094ce1ce90bc9d03e"
    );
}

#[test]
fn ms_nlmp_4_2_4_sealing_vectors() {
    let exported = [0x55u8; 16];
    assert_eq!(
        to_hex(&ntlm::seal_key(&exported, ntlm::Direction::ClientToServer)),
        "59f600973cc4960a25480a7c196e4c58"
    );
    assert_eq!(
        to_hex(&ntlm::sign_key(&exported, ntlm::Direction::ClientToServer)),
        "4788dc861b4782f35d43fd98fe1a2d39"
    );
    let plain = hex("50006c00610069006e007400650078007400");
    let mut client = Session::new(&exported, Role::Client);
    let (signature, sealed) = client.seal(&plain);
    assert_eq!(to_hex(&sealed), "54e50165bf1936dc996020c1811b0f06fb5f");
    assert_eq!(
        to_hex(&signature),
        "01000000".to_owned() + "7fb38ec5c55d4976" + "00000000"
    );

    let mut server = Session::new(&exported, Role::Server);
    assert_eq!(server.unseal(&signature, &sealed).unwrap(), plain);
    assert!(
        server.unseal(&signature, &sealed).is_err(),
        "a replayed message must fail the sequence check"
    );
}

#[test]
fn sealing_streams_continue_across_messages() {
    let exported = [7u8; 16];
    let mut client = Session::new(&exported, Role::Client);
    let mut server = Session::new(&exported, Role::Server);
    for i in 0..5u8 {
        let plain = vec![i; 40 + usize::from(i)];
        let (signature, sealed) = client.seal(&plain);
        assert_eq!(
            u32::from_le_bytes(signature[12..16].try_into().unwrap()),
            u32::from(i)
        );
        assert_eq!(server.unseal(&signature, &sealed).unwrap(), plain);
        let (signature, sealed) = server.seal(&plain);
        assert_eq!(client.unseal(&signature, &sealed).unwrap(), plain);
    }
}

#[test]
fn negotiate_message_matches_the_captured_layout() {
    let message = ntlm::negotiate_message();
    assert_eq!(message.len(), 40);
    assert_eq!(&message[..12], b"NTLMSSP\0\x01\0\0\0");
    assert_eq!(&message[12..16], &0xE208_8235u32.to_le_bytes());
    assert_eq!(&message[16..20], &[0, 0, 0, 0]);
    assert_eq!(&message[20..24], &40u32.to_le_bytes());
}

fn sealed_parts(key: &[u8; 16], plain: &[u8]) -> ([u8; 16], Vec<u8>) {
    Session::new(key, Role::Server).seal(plain)
}

#[test]
fn framing_matches_the_captured_layout() {
    let key = [0x55u8; 16];
    let plain = b"<s:Envelope/>";
    let body = framing::encrypt(&mut Session::new(&key, Role::Client), plain);
    let (signature, sealed) = Session::new(&key, Role::Client).seal(plain);
    let mut expected = format!(
        "--Encrypted Boundary\r\n\tContent-Type: application/HTTP-SPNEGO-session-encrypted\r\n\tOriginalContent: type=application/soap+xml;charset=UTF-8;Length={}\r\n--Encrypted Boundary\r\n\tContent-Type: application/octet-stream\r\n",
        plain.len()
    )
    .into_bytes();
    expected.extend_from_slice(&16u32.to_le_bytes());
    expected.extend_from_slice(&signature);
    expected.extend_from_slice(&sealed);
    expected.extend_from_slice(b"--Encrypted Boundary--\r\n");
    assert_eq!(body, expected);
    assert_eq!(
        framing::CONTENT_TYPE,
        "multipart/encrypted;protocol=\"application/HTTP-SPNEGO-session-encrypted\";boundary=\"Encrypted Boundary\""
    );
    assert_eq!(
        framing::boundary_of(framing::CONTENT_TYPE).as_deref(),
        Some("Encrypted Boundary")
    );
}

#[test]
fn framing_round_trips_with_swapped_roles() {
    let key = [3u8; 16];
    let mut client = Session::new(&key, Role::Client);
    let mut server = Session::new(&key, Role::Server);
    for text in [
        "<a/>",
        "",
        "x".repeat(100_000).as_str(),
        "caf\u{e9} \u{1f600}",
    ] {
        let request = framing::encrypt(&mut client, text.as_bytes());
        let seen = framing::decrypt(&mut server, &request, framing::BOUNDARY).unwrap();
        assert_eq!(seen, text.as_bytes());
        let reply = framing::encrypt(&mut server, &seen);
        assert_eq!(
            framing::decrypt(&mut client, &reply, framing::BOUNDARY).unwrap(),
            text.as_bytes()
        );
    }
}

#[test]
fn framing_decoder_accepts_the_boundary_variants() {
    let key = [9u8; 16];
    let plain = b"<soap>payload</soap>";
    let head = |tab: &str, trail: &str| {
        format!(
            "--Encrypted Boundary{trail}\r\n{tab}Content-Type: application/HTTP-SPNEGO-session-encrypted\r\n{tab}OriginalContent: type=application/soap+xml;charset=UTF-8;Length={}\r\n--Encrypted Boundary{trail}\r\n{tab}Content-Type: application/octet-stream\r\n",
            plain.len()
        )
    };
    let variants: Vec<(String, &str, &str)> = vec![
        ("tabs".into(), "\t", ""),
        ("no tabs".into(), "", ""),
        ("trailing space".into(), "\t", " "),
        ("trailing tab".into(), "", "\t"),
        ("blank line".into(), "\t", ""),
    ];
    for (name, tab, trail) in variants {
        let (signature, sealed) = sealed_parts(&key, plain);
        let mut body = head(tab, trail).into_bytes();
        if name == "blank line" {
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&signature);
        body.extend_from_slice(&sealed);
        for tail in [
            "--Encrypted Boundary--\r\n",
            "\r\n--Encrypted Boundary--",
            "--Encrypted Boundary--",
        ] {
            let mut full = body.clone();
            full.extend_from_slice(tail.as_bytes());
            let got = framing::decrypt(
                &mut Session::new(&key, Role::Client),
                &full,
                framing::BOUNDARY,
            )
            .unwrap_or_else(|e| panic!("{name} / {tail:?}: {e}"));
            assert_eq!(got, plain, "{name}");
        }
    }
}

#[test]
fn framing_decoder_rejects_damage() {
    let key = [5u8; 16];
    let good = framing::encrypt(
        &mut Session::new(&key, Role::Server),
        b"<soap>payload</soap>",
    );
    let decrypt = |body: &[u8]| {
        framing::decrypt(
            &mut Session::new(&key, Role::Client),
            body,
            framing::BOUNDARY,
        )
    };
    assert!(decrypt(&good).is_ok());

    let at = good.len() - 30;
    let mut flipped = good.clone();
    flipped[at] ^= 1;
    assert!(matches!(
        decrypt(&flipped),
        Err(TransportError::Protocol(_))
    ));

    let mut patched = good.clone();
    let at = patched.windows(9).position(|w| w == b"Length=20").unwrap();
    patched[at + 8] = b'1';
    assert!(matches!(
        decrypt(&patched),
        Err(TransportError::Protocol(_))
    ));

    assert!(matches!(
        decrypt(&good[..good.len() - 40]),
        Err(TransportError::Protocol(_))
    ));
    assert!(matches!(
        decrypt(b"plain text"),
        Err(TransportError::Protocol(_))
    ));
    assert!(matches!(
        decrypt(b"--Encrypted Boundary--\r\n"),
        Err(TransportError::Protocol(_))
    ));
}

fn desc(len: usize, offset: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(len as u16).to_le_bytes());
    out.extend_from_slice(&(len as u16).to_le_bytes());
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out
}

const FILETIME: [u8; 8] = [0x80, 0x5c, 0x12, 0x34, 0x56, 0x78, 0x9a, 0x01];

fn challenge_message(with_timestamp: bool, flags: u32) -> Vec<u8> {
    let mut pairs = vec![
        (ntlm::AV_NB_DOMAIN_NAME, ntlm::utf16le("LAB")),
        (ntlm::AV_NB_COMPUTER_NAME, ntlm::utf16le("LABHOST")),
        (4, ntlm::utf16le("lab.example")),
        (3, ntlm::utf16le("labhost.lab.example")),
    ];
    if with_timestamp {
        pairs.push((ntlm::AV_TIMESTAMP, FILETIME.to_vec()));
    }
    let info = ntlm::encode_av_pairs(&pairs);
    let mut m = b"NTLMSSP\0".to_vec();
    m.extend_from_slice(&2u32.to_le_bytes());
    m.extend_from_slice(&desc(0, 56));
    m.extend_from_slice(&flags.to_le_bytes());
    m.extend_from_slice(&SERVER_CHALLENGE);
    m.extend_from_slice(&[0; 8]);
    m.extend_from_slice(&desc(info.len(), 56));
    m.extend_from_slice(&[10, 0, 0x63, 0x45, 0, 0, 0, 15]);
    m.extend_from_slice(&info);
    m
}

const SERVER_FLAGS: u32 =
    ntlm::CLIENT_FLAGS | ntlm::NEGOTIATE_TARGET_TYPE_SERVER | ntlm::NEGOTIATE_TARGET_INFO;

#[test]
fn challenge_parser_reads_flags_names_and_timestamp() {
    let challenge = ntlm::parse_challenge(&challenge_message(true, SERVER_FLAGS)).unwrap();
    assert_eq!(challenge.flags, 0xE28A_8235);
    assert_eq!(challenge.server_challenge, SERVER_CHALLENGE);
    assert_eq!(challenge.computer_name().as_deref(), Some("LABHOST"));
    assert_eq!(challenge.domain_name().as_deref(), Some("LAB"));
    assert_eq!(challenge.timestamp(), Some(FILETIME));

    let bare = ntlm::parse_challenge(&challenge_message(false, SERVER_FLAGS)).unwrap();
    assert_eq!(bare.timestamp(), None);

    assert!(ntlm::parse_challenge(b"short").is_err());
    let mut wrong_type = challenge_message(true, SERVER_FLAGS);
    wrong_type[8] = 3;
    assert!(ntlm::parse_challenge(&wrong_type).is_err());
    let mut out_of_range = challenge_message(true, SERVER_FLAGS);
    out_of_range[44..48].copy_from_slice(&9999u32.to_le_bytes());
    assert!(ntlm::parse_challenge(&out_of_range).is_err());
}

#[test]
fn authenticate_requires_the_sealing_flags() {
    let negotiate = ntlm::negotiate_message();
    let challenge = ntlm::parse_challenge(&challenge_message(
        true,
        SERVER_FLAGS & !ntlm::NEGOTIATE_KEY_EXCH,
    ))
    .unwrap();
    let password = units("Password");
    let result = ntlm::build_authenticate(
        &negotiate,
        &challenge,
        &ntlm::AuthenticateParams {
            identity: ntlm::Identity {
                user: "User",
                domain: "Domain",
                password: &password,
            },
            target_name: "HTTP/host",
            client_challenge: [1; 8],
            exported_session_key: [2; 16],
            fallback_time: [0; 8],
            channel_binding: [0; 16],
        },
    );
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------------------------
// Loopback server acting as the service side of the same primitives.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Behavior {
    chunked: bool,
    close_after: Option<usize>,
    corrupt_reply: bool,
    plain_error: bool,
    /// Answers every request after this many with an unsealed 200 reply, as an attacker on the
    /// path could.
    forged_after: Option<usize>,
    /// Answers every request after this many with HTTP 401 and no body.
    unauthorized_after: Option<usize>,
    /// Answers the NEGOTIATE request with HTTP 401 and no challenge.
    no_challenge: bool,
    timestamp: bool,
    /// MsvAvChannelBindings the AUTHENTICATE_MESSAGE must carry: zeros without TLS.
    channel_binding: [u8; 16],
}

#[derive(Default, Debug)]
struct ServerLog {
    requests: Vec<String>,
    authenticated: bool,
    checks: Vec<String>,
}

struct Request {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_request<S: Read>(reader: &mut BufReader<S>) -> Option<Request> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    assert!(
        line.starts_with("POST /wsman HTTP/1.1"),
        "request line: {line}"
    );
    let mut headers = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).ok()?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        let (k, v) = trimmed.split_once(':').unwrap();
        headers.push((k.to_owned(), v.trim().to_owned()));
    }
    let length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .map(|(_, v)| v.parse().unwrap())
        .unwrap();
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request { headers, body })
}

fn respond(stream: &mut impl Write, status: &str, headers: &[(&str, String)], body: &[u8]) {
    let mut out = format!("HTTP/1.1 {status} \r\nServer: Microsoft-HTTPAPI/2.0\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    stream.write_all(&bytes).unwrap();
}

fn respond_chunked(stream: &mut impl Write, headers: &[(&str, String)], body: &[u8]) {
    let mut out = String::from("HTTP/1.1 200 \r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("Transfer-Encoding: chunked\r\n\r\n");
    let mut bytes = out.into_bytes();
    for chunk in body.chunks(37) {
        bytes.extend_from_slice(format!("{:X};note=1\r\n", chunk.len()).as_bytes());
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"0\r\nX-Trailer: 1\r\n\r\n");
    stream.write_all(&bytes).unwrap();
}

fn utf16_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16(&units).unwrap()
}

struct Credentials {
    user: &'static str,
    domain: &'static str,
    password: &'static str,
}

const LAB: Credentials = Credentials {
    user: "LabAdmin",
    domain: "CONTOSO",
    password: "Correct Horse 9!",
};

fn authenticate_stage(
    request: &Request,
    negotiate: &[u8],
    challenge: &[u8],
    behavior: &Behavior,
    log: &mut ServerLog,
) -> Option<Session> {
    let header = request
        .header("Authorization")
        .expect("Type3 request has Authorization");
    let token = header.strip_prefix("Negotiate ").expect("Negotiate scheme");
    let message = base64::decode(token).expect("base64 Type3");
    assert_eq!(&message[..12], b"NTLMSSP\0\x03\0\0\0");
    let lm = ntlm::security_buffer(&message, 12).unwrap();
    let nt = ntlm::security_buffer(&message, 20).unwrap();
    let domain = utf16_text(ntlm::security_buffer(&message, 28).unwrap());
    let user = utf16_text(ntlm::security_buffer(&message, 36).unwrap());
    let encrypted_key = ntlm::security_buffer(&message, 52).unwrap();
    let flags = ntlm::le32(&message, 60).unwrap();
    log.checks
        .push(format!("user={user} domain={domain} flags={flags:#010x}"));
    assert_eq!(encrypted_key.len(), 16);
    assert_ne!(flags & ntlm::NEGOTIATE_KEY_EXCH, 0);

    let password = units(LAB.password);
    let response_key = ntlm::ntowf_v2(&password, &user, &domain);
    let (proof, blob) = nt.split_at(16);
    let expected = ntlm::hmac_md5(&response_key, &[&SERVER_CHALLENGE, blob]);
    if expected != proof {
        return None;
    }
    let session_base_key = ntlm::hmac_md5(&response_key, &[proof]);
    let exported: [u8; 16] = ntlm::rc4k(&session_base_key, encrypted_key)
        .try_into()
        .unwrap();
    let mic = ntlm::compute_mic(&exported, negotiate, challenge, &message);
    assert_eq!(
        &message[ntlm::MIC_OFFSET..ntlm::MIC_OFFSET + 16],
        &mic,
        "MIC"
    );

    let pairs = ntlm::parse_av_pairs(&blob[28..blob.len() - 4]).unwrap();
    let find = |id| pairs.iter().find(|(i, _)| *i == id).map(|(_, v)| v.clone());
    let av_flags = u32::from_le_bytes(find(ntlm::AV_FLAGS).unwrap().try_into().unwrap());
    assert_eq!(
        av_flags & ntlm::AV_FLAG_MIC_PRESENT,
        ntlm::AV_FLAG_MIC_PRESENT
    );
    assert_eq!(
        utf16_text(&find(ntlm::AV_TARGET_NAME).unwrap()),
        "HTTP/127.0.0.1"
    );
    assert_eq!(
        find(ntlm::AV_CHANNEL_BINDINGS).unwrap(),
        behavior.channel_binding.to_vec(),
        "channel binding"
    );
    assert_eq!(&blob[..8], &[1, 1, 0, 0, 0, 0, 0, 0]);
    if behavior.timestamp {
        assert_eq!(&blob[8..16], &FILETIME);
        assert_eq!(lm, &[0; 24]);
    } else {
        assert_ne!(lm, &[0; 24]);
    }
    log.authenticated = true;
    Some(Session::new(&exported, Role::Server))
}

fn serve<S: Read + Write>(stream: S, behavior: Behavior) -> ServerLog {
    let mut reader = BufReader::new(stream);
    let mut log = ServerLog::default();
    let mut negotiate = Vec::new();
    let challenge = challenge_message(behavior.timestamp, SERVER_FLAGS);
    let mut session: Option<Session> = None;
    let mut handled = 0usize;
    while let Some(request) = read_request(&mut reader) {
        if session.is_none() {
            if negotiate.is_empty() {
                if behavior.no_challenge {
                    respond(reader.get_mut(), "401", &[], b"");
                    return log;
                }
                let header = request
                    .header("Authorization")
                    .expect("Type1 Authorization");
                negotiate = base64::decode(header.strip_prefix("Negotiate ").unwrap()).unwrap();
                assert_eq!(negotiate, ntlm::negotiate_message());
                assert!(request.body.is_empty());
                respond(
                    reader.get_mut(),
                    "401",
                    &[(
                        "WWW-Authenticate",
                        format!("Negotiate {}", base64::encode(&challenge)),
                    )],
                    b"",
                );
                continue;
            }
            match authenticate_stage(&request, &negotiate, &challenge, &behavior, &mut log) {
                Some(s) => session = Some(s),
                None => {
                    respond(
                        reader.get_mut(),
                        "401",
                        &[
                            ("WWW-Authenticate", "Negotiate".into()),
                            ("Connection", "close".into()),
                        ],
                        b"",
                    );
                    return log;
                }
            }
        } else {
            assert!(
                request.header("Authorization").is_none(),
                "only the first request authenticates"
            );
        }
        let session = session.as_mut().unwrap();
        assert_eq!(request.header("Content-Type"), Some(framing::CONTENT_TYPE));
        let plain = framing::decrypt(session, &request.body, framing::BOUNDARY)
            .expect("server unseals request");
        let text = String::from_utf8(plain).unwrap();
        handled += 1;
        let reply = format!("<s:Envelope><s:Body>echo {handled}: {text}</s:Body></s:Envelope>");
        log.requests.push(text);
        if behavior
            .unauthorized_after
            .is_some_and(|after| handled > after)
        {
            respond(reader.get_mut(), "401", &[], b"");
            continue;
        }
        if behavior.forged_after.is_some_and(|after| handled > after) {
            respond(
                reader.get_mut(),
                "200",
                &[("Content-Type", "application/soap+xml;charset=UTF-8".into())],
                b"<s:Envelope><s:Body>forged</s:Body></s:Envelope>",
            );
        } else if behavior.plain_error {
            respond(
                reader.get_mut(),
                "500",
                &[("Content-Type", "text/plain".into())],
                b"boom",
            );
        } else {
            let mut body = framing::encrypt(session, reply.as_bytes());
            if behavior.corrupt_reply {
                let at = body.len() - 40;
                body[at] ^= 0x80;
            }
            let headers = [("Content-Type", framing::CONTENT_TYPE.to_owned())];
            if behavior.chunked {
                respond_chunked(reader.get_mut(), &headers, &body);
            } else {
                respond(reader.get_mut(), "200", &headers, &body);
            }
        }
        if behavior.close_after == Some(handled) {
            return log;
        }
    }
    log
}

fn spawn(behavior: Behavior) -> (u16, JoinHandle<ServerLog>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        serve(stream, behavior)
    });
    (port, handle)
}

fn endpoint(port: u16, password: &str) -> Endpoint {
    Endpoint {
        host: "127.0.0.1".into(),
        port,
        user: LAB.user.into(),
        domain: LAB.domain.into(),
        password: SecretWide::new(password.into()),
        tls: None,
    }
}

#[test]
fn loopback_handshake_and_sealed_exchange() {
    for timestamp in [true, false] {
        let (port, server) = spawn(Behavior {
            timestamp,
            ..Behavior::default()
        });
        let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
        let first = transport.post("<one/>").unwrap();
        assert_eq!(first.status, 200);
        assert_eq!(
            first.body,
            "<s:Envelope><s:Body>echo 1: <one/></s:Body></s:Envelope>"
        );
        let big = "y".repeat(70_000);
        let second = transport.post(&big).unwrap();
        assert!(second.body.contains("echo 2: yyyy"));
        let third = transport.post("<three \u{e9}/>").unwrap();
        assert!(third.body.contains("echo 3: <three \u{e9}/>"));
        drop(transport);
        let log = server.join().unwrap();
        assert!(log.authenticated);
        assert_eq!(log.requests.len(), 3);
        assert_eq!(log.requests[0], "<one/>");
        assert!(
            log.checks[0].starts_with("user=LabAdmin domain=CONTOSO flags=0xe2888235"),
            "{:?}",
            log.checks
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The same server behind TLS, with the certificates in tests/fixtures/tls.
// ---------------------------------------------------------------------------------------------

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tls")
        .join(name)
}

fn certificate_der(stem: &str) -> Vec<u8> {
    CertificateDer::from_pem_file(fixture(&format!("{stem}.pem")))
        .unwrap()
        .to_vec()
}

/// Serves one TLS connection with the fixture certificate `stem`, then the NTLM server on top.
/// The server expects the channel binding of its own certificate. A failed handshake returns
/// its error text; no request can reach the HTTP layer then.
fn spawn_tls(
    stem: &'static str,
    behavior: Behavior,
) -> (u16, JoinHandle<Result<ServerLog, String>>) {
    let certificates = CertificateDer::pem_file_iter(fixture(&format!("{stem}.pem")))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_file(fixture(&format!("{stem}.key"))).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .unwrap();
    let channel_binding =
        ntlm::channel_binding(&tls::server_end_point(&certificate_der(stem)).unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        while stream.conn.is_handshaking() {
            stream
                .conn
                .complete_io(&mut stream.sock)
                .map_err(|e| e.to_string())?;
        }
        let log = serve(
            &mut stream,
            Behavior {
                channel_binding,
                ..behavior
            },
        );
        // The session ends with close_notify, as a service that closes an idle connection does.
        stream.conn.send_close_notify();
        let _ = stream.conn.complete_io(&mut stream.sock);
        Ok(log)
    });
    (port, handle)
}

fn tls_endpoint(port: u16, ca_file: Option<&str>) -> Endpoint {
    Endpoint {
        tls: Some(Tls {
            ca_file: ca_file.map(fixture),
        }),
        ..endpoint(port, LAB.password)
    }
}

#[test]
fn channel_binding_hashes_the_gss_structure_around_the_end_point_data() {
    // MD5 values from .NET over the same bytes.
    let data = tls::server_end_point(&certificate_der("loopback-rsa-sha256")).unwrap();
    assert_eq!(
        to_hex(&ntlm::channel_binding(&data)),
        "5ce5c455ffba327a9b8235a43d524b63"
    );
    let ecdsa = tls::server_end_point(&certificate_der("other-ecdsa-sha384")).unwrap();
    assert_eq!(
        to_hex(&ntlm::channel_binding(&ecdsa)),
        "df642f49b6117937301a3723832ed222"
    );
    assert_eq!(
        to_hex(&ntlm::channel_binding(b"")),
        "441018525208457705bf09a8ee3c1093"
    );
}

#[test]
fn tls_loopback_binds_the_logon_to_the_server_certificate() {
    let (port, server) = spawn_tls("loopback-rsa-sha256", Behavior::default());
    let endpoint = tls_endpoint(port, Some("loopback-rsa-sha256.pem"));
    let mut transport = NtlmHttpTransport::connect(&endpoint).unwrap();
    let first = transport.post("<one/>").unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(
        first.body,
        "<s:Envelope><s:Body>echo 1: <one/></s:Body></s:Envelope>"
    );
    assert!(transport.is_reusable());
    let second = transport.post("<two/>").unwrap();
    assert!(second.body.contains("echo 2: <two/>"));
    drop(transport);
    let log = server.join().unwrap().expect("handshake succeeds");
    assert!(log.authenticated, "{log:?}");
    assert_eq!(log.requests, ["<one/>", "<two/>"]);
}

#[test]
fn tls_loopback_refuses_an_untrusted_certificate_before_any_request() {
    let (port, server) = spawn_tls("loopback-rsa-sha256", Behavior::default());
    let error = NtlmHttpTransport::connect(&tls_endpoint(port, None))
        .err()
        .expect("refused");
    let TransportError::Tls(reason) = &error else {
        panic!("{error}");
    };
    assert!(reason.starts_with("handshake with 127.0.0.1: "), "{reason}");
    let outcome = server.join().unwrap();
    assert!(
        outcome.is_err(),
        "no request reached the server: {outcome:?}"
    );
}

#[test]
fn tls_loopback_refuses_a_certificate_issued_to_another_name() {
    let (port, server) = spawn_tls("other-ecdsa-sha384", Behavior::default());
    let error = NtlmHttpTransport::connect(&tls_endpoint(port, Some("other-ecdsa-sha384.pem")))
        .err()
        .expect("refused");
    assert!(matches!(error, TransportError::Tls(_)), "{error}");
    assert!(server.join().unwrap().is_err());
}

#[test]
fn tls_loopback_connection_closed_by_the_server_is_not_reusable() {
    let (port, server) = spawn_tls(
        "loopback-rsa-sha256",
        Behavior {
            close_after: Some(1),
            ..Behavior::default()
        },
    );
    let endpoint = tls_endpoint(port, Some("loopback-rsa-sha256.pem"));
    let mut transport = NtlmHttpTransport::connect(&endpoint).unwrap();
    assert_eq!(transport.post("<one/>").unwrap().status, 200);
    assert!(server.join().unwrap().unwrap().authenticated);
    assert!(!transport.is_reusable());
    assert!(matches!(
        transport.post("<two/>"),
        Err(TransportError::Io(_))
    ));
}

#[test]
fn tls_loopback_wrong_password_is_still_an_authentication_error() {
    let (port, server) = spawn_tls("loopback-rsa-sha256", Behavior::default());
    let endpoint = Endpoint {
        password: SecretWide::new("wrong password".into()),
        ..tls_endpoint(port, Some("loopback-rsa-sha256.pem"))
    };
    let mut transport = NtlmHttpTransport::connect(&endpoint).unwrap();
    assert!(matches!(
        transport.post("<one/>"),
        Err(TransportError::Authentication)
    ));
    assert!(!server.join().unwrap().unwrap().authenticated);
}

#[test]
fn a_ca_file_that_cannot_be_read_is_a_tls_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let missing = tls_endpoint(port, Some("missing.pem"));
    let error = NtlmHttpTransport::connect(&missing).err().expect("refused");
    let TransportError::Tls(reason) = &error else {
        panic!("{error}");
    };
    assert!(reason.starts_with("WMIC_WSMAN_CA_FILE "), "{reason}");
    assert!(reason.contains("missing.pem"), "{reason}");
    let empty = tls_endpoint(port, Some("loopback-rsa-sha256.key"));
    let error = NtlmHttpTransport::connect(&empty).err().expect("refused");
    let TransportError::Tls(reason) = &error else {
        panic!("{error}");
    };
    assert!(reason.ends_with("no certificate in the file"), "{reason}");
}

#[test]
fn loopback_wrong_password_is_an_authentication_error() {
    let (port, server) = spawn(Behavior::default());
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, "wrong password")).unwrap();
    assert!(matches!(
        transport.post("<one/>"),
        Err(TransportError::Authentication)
    ));
    assert!(matches!(
        transport.post("<two/>"),
        Err(TransportError::Io(_))
    ));
    assert!(!server.join().unwrap().authenticated);
}

#[test]
fn loopback_chunked_responses_are_reassembled() {
    let (port, server) = spawn(Behavior {
        chunked: true,
        timestamp: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    let text = "z".repeat(5_000);
    let reply = transport.post(&text).unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.body,
        format!("<s:Envelope><s:Body>echo 1: {text}</s:Body></s:Envelope>")
    );
    let again = transport.post("<b/>").unwrap();
    assert!(again.body.contains("echo 2: <b/>"));
    drop(transport);
    server.join().unwrap();
}

#[test]
fn loopback_unsealed_replies_are_protocol_errors() {
    for behavior in [
        Behavior {
            plain_error: true,
            timestamp: true,
            ..Behavior::default()
        },
        Behavior {
            forged_after: Some(0),
            timestamp: true,
            ..Behavior::default()
        },
    ] {
        let (port, server) = spawn(behavior);
        let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
        assert!(matches!(
            transport.post("<one/>"),
            Err(TransportError::Protocol(_))
        ));
        assert!(matches!(
            transport.post("<two/>"),
            Err(TransportError::Io(_))
        ));
        drop(transport);
        server.join().unwrap();
    }
}

#[test]
fn loopback_unsealed_reply_after_a_sealed_one_is_still_rejected() {
    let (port, server) = spawn(Behavior {
        forged_after: Some(1),
        timestamp: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    assert!(transport.post("<one/>").is_ok());
    assert!(matches!(
        transport.post("<two/>"),
        Err(TransportError::Protocol(_))
    ));
    drop(transport);
    server.join().unwrap();
}

#[test]
fn loopback_401_after_the_handshake_is_reported_with_an_empty_body() {
    let (port, server) = spawn(Behavior {
        unauthorized_after: Some(1),
        timestamp: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    assert!(transport.post("<one/>").is_ok());
    let reply = transport.post("<two/>").unwrap();
    assert_eq!((reply.status, reply.body.as_str()), (401, ""));
    assert!(matches!(
        transport.post("<three/>"),
        Err(TransportError::Io(_))
    ));
    assert!(!transport.is_reusable());
    drop(transport);
    server.join().unwrap();
}

#[test]
fn loopback_401_without_a_challenge_is_an_authentication_error() {
    let (port, server) = spawn(Behavior {
        no_challenge: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    assert!(matches!(
        transport.post("<one/>"),
        Err(TransportError::Authentication)
    ));
    drop(transport);
    server.join().unwrap();
}

#[test]
fn loopback_reports_the_computer_name_from_the_challenge() {
    let (port, server) = spawn(Behavior {
        timestamp: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    assert_eq!(transport.server_name(), None);
    transport.post("<one/>").unwrap();
    assert_eq!(transport.server_name().as_deref(), Some("LABHOST"));
    drop(transport);
    server.join().unwrap();
}

#[test]
fn loopback_connection_closed_by_the_peer_is_not_reusable() {
    let (port, server) = spawn(Behavior {
        close_after: Some(2),
        timestamp: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    assert!(transport.is_reusable());
    assert!(transport.post("<one/>").is_ok());
    assert!(
        transport.is_reusable(),
        "the check leaves the stream intact"
    );
    assert!(transport.post("<two/>").is_ok());
    server.join().unwrap();
    assert!(!transport.is_reusable());
}

fn spawn_sequence(
    behavior: Behavior,
    connections: usize,
) -> (
    u16,
    std::sync::mpsc::Receiver<()>,
    JoinHandle<Vec<ServerLog>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done, finished) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut logs = Vec::new();
        for _ in 0..connections {
            let (stream, _) = listener.accept().unwrap();
            logs.push(serve(stream, behavior.clone()));
            done.send(()).unwrap();
        }
        logs
    });
    (port, finished, handle)
}

#[test]
fn a_connection_closed_while_idle_is_replaced_before_an_invoke_and_the_invoke_is_sent_once() {
    use wmic_compat::backend::wsman::client::{Target, WsmanClient, default_connector};
    let (port, finished, server) = spawn_sequence(
        Behavior {
            close_after: Some(1),
            timestamp: true,
            ..Behavior::default()
        },
        2,
    );
    let target = Target {
        endpoint: endpoint(port, LAB.password),
        namespace: r"root\cimv2".into(),
        node: "127.0.0.1".into(),
        server: "127.0.0.1".into(),
        multiple_nodes: false,
    };
    let mut client = WsmanClient::new(default_connector());
    let _ = client.get(&target, "Win32_Process", &[("Handle".into(), "4".into())]);
    finished.recv().unwrap();
    let _ = client.invoke(&target, "Win32_Process", "Terminate", &[], None, &[]);
    finished.recv().unwrap();
    let logs = server.join().unwrap();
    assert_eq!(logs.len(), 2);
    assert!(logs.iter().all(|log| log.authenticated));
    assert_eq!(logs[0].requests.len(), 1);
    assert!(logs[0].requests[0].contains("transfer/Get"));
    assert_eq!(logs[1].requests.len(), 1, "the Invoke is sent once");
    assert!(logs[1].requests[0].contains("Terminate"));
}

#[test]
fn loopback_closed_connection_is_an_io_error_without_reconnect() {
    let (port, server) = spawn(Behavior {
        close_after: Some(1),
        timestamp: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    assert!(transport.post("<one/>").is_ok());
    server.join().unwrap();
    assert!(matches!(
        transport.post("<two/>"),
        Err(TransportError::Io(_))
    ));
    assert!(matches!(
        transport.post("<three/>"),
        Err(TransportError::Io(_))
    ));
}

#[test]
fn loopback_bad_server_signature_is_a_protocol_error() {
    let (port, server) = spawn(Behavior {
        corrupt_reply: true,
        timestamp: true,
        ..Behavior::default()
    });
    let mut transport = NtlmHttpTransport::connect(&endpoint(port, LAB.password)).unwrap();
    assert!(matches!(
        transport.post("<one/>"),
        Err(TransportError::Protocol(_))
    ));
    assert!(matches!(
        transport.post("<two/>"),
        Err(TransportError::Io(_))
    ));
    drop(transport);
    server.join().unwrap();
}

#[test]
fn connect_failure_is_reported_as_connect() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    assert!(matches!(
        NtlmHttpTransport::connect(&endpoint(port, "x")),
        Err(TransportError::Connect(_))
    ));
}

#[test]
fn http_client_reads_content_length_and_close_delimited_bodies() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        read_request(&mut reader).unwrap();
        writer
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nX-A: 1\r\nx-a: 2\r\nContent-Length: 5\r\n\r\nhello")
            .unwrap();
        read_request(&mut reader).unwrap();
        writer
            .write_all(b"HTTP/1.1 200\r\nConnection: close\r\n\r\nuntil close")
            .unwrap();
    });
    let mut connection = http::HttpConnection::connect("127.0.0.1", port, None).unwrap();
    let first = connection.post("/wsman", &[], b"a").unwrap();
    assert_eq!((first.status, first.body.as_slice()), (200, &b"hello"[..]));
    assert_eq!(first.headers_named("X-A").collect::<Vec<_>>(), ["1", "2"]);
    let second = connection.post("/wsman", &[], b"b").unwrap();
    assert_eq!(second.body, b"until close");
    server.join().unwrap();
    assert!(matches!(
        connection.post("/wsman", &[], b"c"),
        Err(TransportError::Io(_))
    ));
}

#[test]
fn http_client_refuses_bodies_far_above_the_envelope_size() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        read_request(&mut reader).unwrap();
        let _ = writer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 134217728\r\n\r\n");
    });
    let mut connection = http::HttpConnection::connect("127.0.0.1", port, None).unwrap();
    assert!(matches!(
        connection.post("/wsman", &[], b"a"),
        Err(TransportError::Protocol(_))
    ));
    server.join().unwrap();
}

#[test]
fn wipe_zeroes_key_material() {
    let mut key = [0xaau8; 16];
    ntlm::wipe(&mut key);
    assert_eq!(key, [0u8; 16]);
}
