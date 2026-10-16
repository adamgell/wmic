//! NTLMv2 messages, key schedule and message sealing (MS-NLMP).

use hmac::{Hmac, Mac};
use md4::Md4;
use md5::{Digest, Md5};
use rc4::{KeyInit, StreamCipher, consts::U16};
use std::fmt;
use std::io::Read;

pub const SIGNATURE_LEN: usize = 16;

pub const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
pub const NEGOTIATE_REQUEST_TARGET: u32 = 0x0000_0004;
pub const NEGOTIATE_SIGN: u32 = 0x0000_0010;
pub const NEGOTIATE_SEAL: u32 = 0x0000_0020;
pub const NEGOTIATE_NTLM: u32 = 0x0000_0200;
pub const NEGOTIATE_ALWAYS_SIGN: u32 = 0x0000_8000;
pub const NEGOTIATE_TARGET_TYPE_SERVER: u32 = 0x0002_0000;
pub const NEGOTIATE_EXTENDED_SESSIONSECURITY: u32 = 0x0008_0000;
pub const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
pub const NEGOTIATE_VERSION: u32 = 0x0200_0000;
pub const NEGOTIATE_128: u32 = 0x2000_0000;
pub const NEGOTIATE_KEY_EXCH: u32 = 0x4000_0000;
pub const NEGOTIATE_56: u32 = 0x8000_0000;

/// Flags the client offers in the NEGOTIATE_MESSAGE.
pub const CLIENT_FLAGS: u32 = NEGOTIATE_UNICODE
    | NEGOTIATE_REQUEST_TARGET
    | NEGOTIATE_SIGN
    | NEGOTIATE_SEAL
    | NEGOTIATE_NTLM
    | NEGOTIATE_ALWAYS_SIGN
    | NEGOTIATE_EXTENDED_SESSIONSECURITY
    | NEGOTIATE_VERSION
    | NEGOTIATE_128
    | NEGOTIATE_KEY_EXCH
    | NEGOTIATE_56;

/// Flags the challenge must grant for the sealed channel to exist.
pub const REQUIRED_FLAGS: u32 = NEGOTIATE_SIGN
    | NEGOTIATE_SEAL
    | NEGOTIATE_EXTENDED_SESSIONSECURITY
    | NEGOTIATE_128
    | NEGOTIATE_KEY_EXCH;

pub const AV_EOL: u16 = 0;
pub const AV_NB_COMPUTER_NAME: u16 = 1;
pub const AV_NB_DOMAIN_NAME: u16 = 2;
pub const AV_FLAGS: u16 = 6;
pub const AV_TIMESTAMP: u16 = 7;
pub const AV_TARGET_NAME: u16 = 9;
pub const AV_CHANNEL_BINDINGS: u16 = 10;
/// MsvAvFlags bit: the AUTHENTICATE_MESSAGE carries a MIC.
pub const AV_FLAG_MIC_PRESENT: u32 = 0x2;

/// Offset of the MIC field inside the AUTHENTICATE_MESSAGE.
pub const MIC_OFFSET: usize = 72;
const AUTHENTICATE_PAYLOAD_OFFSET: usize = 88;
const NTLMSSP: &[u8; 8] = b"NTLMSSP\0";
const VERSION: [u8; 8] = [10, 0, 0x63, 0x45, 0, 0, 0, 15];

const CLIENT_SIGN_MAGIC: &[u8] = b"session key to client-to-server signing key magic constant\0";
const SERVER_SIGN_MAGIC: &[u8] = b"session key to server-to-client signing key magic constant\0";
const CLIENT_SEAL_MAGIC: &[u8] = b"session key to client-to-server sealing key magic constant\0";
const SERVER_SEAL_MAGIC: &[u8] = b"session key to server-to-client sealing key magic constant\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtlmError(pub String);

impl fmt::Display for NtlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ntlm: {}", self.0)
    }
}

impl std::error::Error for NtlmError {}

fn fail<T>(message: &str) -> Result<T, NtlmError> {
    Err(NtlmError(message.to_owned()))
}

pub fn md5(data: &[u8]) -> [u8; 16] {
    Md5::digest(data).into()
}

pub fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    let mut mac = <Hmac<Md5> as Mac>::new_from_slice(key).expect("HMAC accepts keys of any length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

fn rc4_handle(key: &[u8; 16]) -> rc4::Rc4<U16> {
    rc4::Rc4::<U16>::new(rc4::Key::<U16>::from_slice(key))
}

/// RC4 with a fresh state, as used for the encrypted random session key.
pub fn rc4k(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    rc4_handle(key).apply_keystream(&mut out);
    out
}

pub fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn units_le(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|u| u.to_le_bytes()).collect()
}

fn uppercase(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => u,
                _ => c,
            }
        })
        .collect()
}

/// MD4 of the UTF-16LE password.
pub fn ntowf_v1(password: &[u16]) -> [u8; 16] {
    Md4::digest(units_le(password)).into()
}

/// HMAC_MD5(NTOWFv1, UTF16LE(UPPER(user) + domain)).
pub fn ntowf_v2(password: &[u16], user: &str, domain: &str) -> [u8; 16] {
    let mut identity = utf16le(&uppercase(user));
    identity.extend(utf16le(domain));
    hmac_md5(&ntowf_v1(password), &[&identity])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtResponse {
    pub proof: [u8; 16],
    /// The client challenge blob ("temp") appended to the proof in the NT response.
    pub blob: Vec<u8>,
    pub session_base_key: [u8; 16],
}

impl NtResponse {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.proof.to_vec();
        out.extend_from_slice(&self.blob);
        out
    }
}

/// `av_pairs` is the complete list including the terminating MsvAvEOL pair.
pub fn ntlmv2_response(
    response_key: &[u8; 16],
    server_challenge: &[u8; 8],
    time: &[u8; 8],
    client_challenge: &[u8; 8],
    av_pairs: &[u8],
) -> NtResponse {
    let mut blob = vec![1, 1, 0, 0, 0, 0, 0, 0];
    blob.extend_from_slice(time);
    blob.extend_from_slice(client_challenge);
    blob.extend_from_slice(&[0; 4]);
    blob.extend_from_slice(av_pairs);
    blob.extend_from_slice(&[0; 4]);
    let proof = hmac_md5(response_key, &[server_challenge, &blob]);
    let session_base_key = hmac_md5(response_key, &[&proof]);
    NtResponse {
        proof,
        blob,
        session_base_key,
    }
}

pub fn lmv2_response(
    response_key: &[u8; 16],
    server_challenge: &[u8; 8],
    client_challenge: &[u8; 8],
) -> [u8; 24] {
    let mut out = [0u8; 24];
    out[..16].copy_from_slice(&hmac_md5(
        response_key,
        &[server_challenge, client_challenge],
    ));
    out[16..].copy_from_slice(client_challenge);
    out
}

pub fn parse_av_pairs(info: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, NtlmError> {
    let mut pairs = Vec::new();
    let mut at = 0;
    while at + 4 <= info.len() {
        let id = u16::from_le_bytes([info[at], info[at + 1]]);
        let len = usize::from(u16::from_le_bytes([info[at + 2], info[at + 3]]));
        at += 4;
        if id == AV_EOL {
            return Ok(pairs);
        }
        let Some(value) = info.get(at..at + len) else {
            return fail("target info pair exceeds its buffer");
        };
        pairs.push((id, value.to_vec()));
        at += len;
    }
    Ok(pairs)
}

/// Serializes the pairs and appends MsvAvEOL.
pub fn encode_av_pairs(pairs: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (id, value) in pairs {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
        out.extend_from_slice(value);
    }
    out.extend_from_slice(&[0; 4]);
    out
}

fn upsert(pairs: &mut Vec<(u16, Vec<u8>)>, id: u16, value: Vec<u8>) {
    match pairs.iter_mut().find(|(existing, _)| *existing == id) {
        Some(pair) => pair.1 = value,
        None => pairs.push((id, value)),
    }
}

/// Reads a security buffer descriptor (length, capacity, offset) at `at` and returns the bytes it points to.
pub fn security_buffer(message: &[u8], at: usize) -> Result<&[u8], NtlmError> {
    let Some(field) = message.get(at..at + 8) else {
        return fail("message ends inside a security buffer descriptor");
    };
    let len = usize::from(u16::from_le_bytes([field[0], field[1]]));
    let offset = u32::from_le_bytes([field[4], field[5], field[6], field[7]]) as usize;
    match offset
        .checked_add(len)
        .and_then(|end| message.get(offset..end))
    {
        Some(bytes) => Ok(bytes),
        None => fail("security buffer lies outside the message"),
    }
}

pub fn le32(message: &[u8], at: usize) -> Result<u32, NtlmError> {
    match message.get(at..at + 4) {
        Some(b) => Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        None => fail("message is truncated"),
    }
}

fn descriptor(len: usize, offset: usize) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&(len as u16).to_le_bytes());
    out[2..4].copy_from_slice(&(len as u16).to_le_bytes());
    out[4..8].copy_from_slice(&(offset as u32).to_le_bytes());
    out
}

pub fn negotiate_message() -> Vec<u8> {
    let mut m = Vec::with_capacity(40);
    m.extend_from_slice(NTLMSSP);
    m.extend_from_slice(&1u32.to_le_bytes());
    m.extend_from_slice(&CLIENT_FLAGS.to_le_bytes());
    m.extend_from_slice(&descriptor(0, 40));
    m.extend_from_slice(&descriptor(0, 40));
    m.extend_from_slice(&VERSION);
    m
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub raw: Vec<u8>,
    pub flags: u32,
    pub server_challenge: [u8; 8],
    pub av_pairs: Vec<(u16, Vec<u8>)>,
}

impl Challenge {
    pub fn av(&self, id: u16) -> Option<&[u8]> {
        self.av_pairs
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, value)| value.as_slice())
    }

    pub fn timestamp(&self) -> Option<[u8; 8]> {
        self.av(AV_TIMESTAMP)?.try_into().ok()
    }

    pub fn computer_name(&self) -> Option<String> {
        self.av(AV_NB_COMPUTER_NAME).map(utf16_to_string)
    }

    pub fn domain_name(&self) -> Option<String> {
        self.av(AV_NB_DOMAIN_NAME).map(utf16_to_string)
    }
}

fn utf16_to_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
}

pub fn parse_challenge(message: &[u8]) -> Result<Challenge, NtlmError> {
    if message.len() < 32 || &message[..8] != NTLMSSP {
        return fail("not an NTLMSSP message");
    }
    if le32(message, 8)? != 2 {
        return fail("not a CHALLENGE_MESSAGE");
    }
    let flags = le32(message, 20)?;
    let mut server_challenge = [0u8; 8];
    server_challenge.copy_from_slice(&message[24..32]);
    let av_pairs = if flags & NEGOTIATE_TARGET_INFO != 0 && message.len() >= 48 {
        parse_av_pairs(security_buffer(message, 40)?)?
    } else {
        Vec::new()
    };
    Ok(Challenge {
        raw: message.to_vec(),
        flags,
        server_challenge,
        av_pairs,
    })
}

#[derive(Clone, Copy)]
pub struct Identity<'a> {
    pub user: &'a str,
    pub domain: &'a str,
    pub password: &'a [u16],
}

pub struct AuthenticateParams<'a> {
    pub identity: Identity<'a>,
    /// Service principal name, `HTTP/<host>` for WinRM.
    pub target_name: &'a str,
    pub client_challenge: [u8; 8],
    pub exported_session_key: [u8; 16],
    /// FILETIME used when the challenge carries no MsvAvTimestamp.
    pub fallback_time: [u8; 8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authenticate {
    pub message: Vec<u8>,
    pub exported_session_key: [u8; 16],
}

pub fn build_authenticate(
    negotiate: &[u8],
    challenge: &Challenge,
    params: &AuthenticateParams<'_>,
) -> Result<Authenticate, NtlmError> {
    let missing = REQUIRED_FLAGS & !challenge.flags;
    if missing != 0 {
        return Err(NtlmError(format!(
            "server did not grant required flags {missing:#010x}"
        )));
    }
    let identity = params.identity;
    let response_key = ntowf_v2(identity.password, identity.user, identity.domain);

    let mut pairs = challenge.av_pairs.clone();
    let server_flags = challenge
        .av(AV_FLAGS)
        .and_then(|v| <[u8; 4]>::try_from(v).ok())
        .map_or(0, u32::from_le_bytes);
    upsert(
        &mut pairs,
        AV_FLAGS,
        (server_flags | AV_FLAG_MIC_PRESENT).to_le_bytes().to_vec(),
    );
    upsert(&mut pairs, AV_CHANNEL_BINDINGS, vec![0; 16]);
    upsert(&mut pairs, AV_TARGET_NAME, utf16le(params.target_name));
    let av_pairs = encode_av_pairs(&pairs);

    let timestamp = challenge.timestamp();
    let time = timestamp.unwrap_or(params.fallback_time);
    let nt = ntlmv2_response(
        &response_key,
        &challenge.server_challenge,
        &time,
        &params.client_challenge,
        &av_pairs,
    );
    let lm = if timestamp.is_some() {
        [0u8; 24]
    } else {
        lmv2_response(
            &response_key,
            &challenge.server_challenge,
            &params.client_challenge,
        )
    };
    let encrypted_key = rc4k(&nt.session_base_key, &params.exported_session_key);
    let flags = (CLIENT_FLAGS & challenge.flags)
        | NEGOTIATE_VERSION
        | (challenge.flags & NEGOTIATE_TARGET_INFO);

    let domain = utf16le(identity.domain);
    let user = utf16le(identity.user);
    let nt_bytes = nt.to_bytes();
    let domain_at = AUTHENTICATE_PAYLOAD_OFFSET;
    let user_at = domain_at + domain.len();
    let workstation_at = user_at + user.len();
    let lm_at = workstation_at;
    let nt_at = lm_at + lm.len();
    let key_at = nt_at + nt_bytes.len();

    let mut m = Vec::with_capacity(key_at + encrypted_key.len());
    m.extend_from_slice(NTLMSSP);
    m.extend_from_slice(&3u32.to_le_bytes());
    m.extend_from_slice(&descriptor(lm.len(), lm_at));
    m.extend_from_slice(&descriptor(nt_bytes.len(), nt_at));
    m.extend_from_slice(&descriptor(domain.len(), domain_at));
    m.extend_from_slice(&descriptor(user.len(), user_at));
    m.extend_from_slice(&descriptor(0, workstation_at));
    m.extend_from_slice(&descriptor(encrypted_key.len(), key_at));
    m.extend_from_slice(&flags.to_le_bytes());
    m.extend_from_slice(&VERSION);
    m.extend_from_slice(&[0; 16]);
    m.extend_from_slice(&domain);
    m.extend_from_slice(&user);
    m.extend_from_slice(&lm);
    m.extend_from_slice(&nt_bytes);
    m.extend_from_slice(&encrypted_key);

    let mic = compute_mic(&params.exported_session_key, negotiate, &challenge.raw, &m);
    m[MIC_OFFSET..MIC_OFFSET + 16].copy_from_slice(&mic);
    Ok(Authenticate {
        message: m,
        exported_session_key: params.exported_session_key,
    })
}

/// MIC over the three messages with the MIC field of the AUTHENTICATE_MESSAGE read as zero.
pub fn compute_mic(
    exported_session_key: &[u8; 16],
    negotiate: &[u8],
    challenge: &[u8],
    authenticate: &[u8],
) -> [u8; 16] {
    let mut zeroed = authenticate.to_vec();
    if let Some(field) = zeroed.get_mut(MIC_OFFSET..MIC_OFFSET + 16) {
        field.fill(0);
    }
    hmac_md5(exported_session_key, &[negotiate, challenge, &zeroed])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}

pub fn sign_key(exported_session_key: &[u8; 16], direction: Direction) -> [u8; 16] {
    let magic = match direction {
        Direction::ClientToServer => CLIENT_SIGN_MAGIC,
        Direction::ServerToClient => SERVER_SIGN_MAGIC,
    };
    md5_concat(exported_session_key, magic)
}

pub fn seal_key(exported_session_key: &[u8; 16], direction: Direction) -> [u8; 16] {
    let magic = match direction {
        Direction::ClientToServer => CLIENT_SEAL_MAGIC,
        Direction::ServerToClient => SERVER_SEAL_MAGIC,
    };
    md5_concat(exported_session_key, magic)
}

fn md5_concat(key: &[u8; 16], magic: &[u8]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(key);
    hasher.update(magic);
    hasher.finalize().into()
}

/// Sealing state of one authenticated connection. The RC4 handles and sequence numbers
/// advance for the lifetime of the connection and are never reset.
pub struct Session {
    send_sign: [u8; 16],
    recv_sign: [u8; 16],
    send_seal: rc4::Rc4<U16>,
    recv_seal: rc4::Rc4<U16>,
    send_seq: u32,
    recv_seq: u32,
}

impl Session {
    pub fn new(exported_session_key: &[u8; 16], role: Role) -> Self {
        let (send, recv) = match role {
            Role::Client => (Direction::ClientToServer, Direction::ServerToClient),
            Role::Server => (Direction::ServerToClient, Direction::ClientToServer),
        };
        Self {
            send_sign: sign_key(exported_session_key, send),
            recv_sign: sign_key(exported_session_key, recv),
            send_seal: rc4_handle(&seal_key(exported_session_key, send)),
            recv_seal: rc4_handle(&seal_key(exported_session_key, recv)),
            send_seq: 0,
            recv_seq: 0,
        }
    }

    /// Returns the 16-byte signature and the sealed message.
    pub fn seal(&mut self, plain: &[u8]) -> ([u8; SIGNATURE_LEN], Vec<u8>) {
        let seq = self.send_seq.to_le_bytes();
        let mut checksum = [0u8; 8];
        checksum.copy_from_slice(&hmac_md5(&self.send_sign, &[&seq, plain])[..8]);
        let mut sealed = plain.to_vec();
        self.send_seal.apply_keystream(&mut sealed);
        self.send_seal.apply_keystream(&mut checksum);
        self.send_seq = self.send_seq.wrapping_add(1);
        let mut signature = [0u8; SIGNATURE_LEN];
        signature[..4].copy_from_slice(&1u32.to_le_bytes());
        signature[4..12].copy_from_slice(&checksum);
        signature[12..].copy_from_slice(&seq);
        (signature, sealed)
    }

    /// Decrypts and verifies the checksum and sequence number. The caller must abandon the
    /// connection on error because the RC4 stream has advanced.
    pub fn unseal(&mut self, signature: &[u8], sealed: &[u8]) -> Result<Vec<u8>, NtlmError> {
        if signature.len() != SIGNATURE_LEN {
            return fail("signature is not 16 bytes");
        }
        let mut plain = sealed.to_vec();
        self.recv_seal.apply_keystream(&mut plain);
        let seq = self.recv_seq.to_le_bytes();
        let expected = hmac_md5(&self.recv_sign, &[&seq, &plain]);
        let mut checksum = [0u8; 8];
        checksum.copy_from_slice(&signature[4..12]);
        self.recv_seal.apply_keystream(&mut checksum);
        let version_ok = signature[..4] == 1u32.to_le_bytes();
        let seq_ok = signature[12..] == seq;
        let checksum_ok = constant_time_eq(&checksum, &expected[..8]);
        if !(version_ok && seq_ok && checksum_ok) {
            return fail("message signature or sequence number mismatch");
        }
        self.recv_seq = self.recv_seq.wrapping_add(1);
        Ok(plain)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn random_bytes<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut out = [0u8; N];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut out)?;
    Ok(out)
}

/// Current time as a Windows FILETIME (100 ns ticks since 1601), little endian.
pub fn filetime_now() -> [u8; 8] {
    const UNIX_TO_FILETIME_SECS: u64 = 11_644_473_600;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ticks =
        (now.as_secs() + UNIX_TO_FILETIME_SECS) * 10_000_000 + u64::from(now.subsec_nanos() / 100);
    ticks.to_le_bytes()
}
