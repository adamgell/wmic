//! Probe for a WinRM listener.
//!
//! `wsman-probe HOST PORT [DOMAIN\]USER` reads the password from standard input, authenticates with
//! NTLM, posts an Identify request and prints the decrypted reply.
//! `wsman-probe --challenge HOST PORT` sends only the NTLM negotiate message and prints the parsed
//! challenge; no credential is involved.

#[cfg(windows)]
fn main() {
    eprintln!("wsman-probe is not available on Windows");
    std::process::exit(2);
}

#[cfg(not(windows))]
fn main() {
    std::process::exit(probe::run());
}

#[cfg(not(windows))]
mod probe {
    use std::io::BufRead;
    use wmic_compat::backend::SecretWide;
    use wmic_compat::backend::wsman::transport::http::HttpConnection;
    use wmic_compat::backend::wsman::transport::{
        NtlmHttpTransport, base64, negotiate_token, ntlm,
    };
    use wmic_compat::backend::wsman::{Endpoint, Transport};

    const IDENTIFY: &str = "<s:Envelope xmlns:s=\"http://www.w3.org/2003/05/soap-envelope\" xmlns:wsmid=\"http://schemas.dmtf.org/wbem/wsman/identity/1/wsmanidentity.xsd\"><s:Header/><s:Body><wsmid:Identify/></s:Body></s:Envelope>";

    pub fn run() -> i32 {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["--challenge", host, port] => challenge(host, port),
            [host, port, user] => identify(host, port, user),
            _ => Err(
                "usage: wsman-probe HOST PORT [DOMAIN\\]USER   (password on stdin)\n       wsman-probe --challenge HOST PORT"
                    .into(),
            ),
        };
        match result {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                1
            }
        }
    }

    fn port_of(text: &str) -> Result<u16, String> {
        text.parse().map_err(|_| format!("bad port: {text}"))
    }

    fn challenge(host: &str, port: &str) -> Result<(), String> {
        let mut connection =
            HttpConnection::connect(host, port_of(port)?).map_err(|e| e.to_string())?;
        let plain = connection
            .post(
                "/wsman",
                &[("Content-Type", "application/soap+xml;charset=UTF-8")],
                b"",
            )
            .map_err(|e| e.to_string())?;
        println!(
            "unauthenticated POST: HTTP {}, WWW-Authenticate: {:?}, Connection: {:?}",
            plain.status,
            plain.headers_named("WWW-Authenticate").collect::<Vec<_>>(),
            plain.header("Connection")
        );
        let mut connection =
            HttpConnection::connect(host, port_of(port)?).map_err(|e| e.to_string())?;
        let negotiate = ntlm::negotiate_message();
        let header = format!("Negotiate {}", base64::encode(&negotiate));
        let reply = connection
            .post(
                "/wsman",
                &[
                    ("Authorization", &header),
                    ("Content-Type", "application/soap+xml;charset=UTF-8"),
                ],
                b"",
            )
            .map_err(|e| e.to_string())?;
        println!(
            "negotiate POST: HTTP {}, Connection: {:?}",
            reply.status,
            reply.header("Connection")
        );
        let token = negotiate_token(&reply).ok_or("no Negotiate token in the reply")?;
        let raw = base64::decode(token).ok_or("token is not base64")?;
        let challenge = ntlm::parse_challenge(&raw).map_err(|e| e.to_string())?;
        println!("challenge bytes: {}", raw.len());
        println!("flags: {:#010x}", challenge.flags);
        println!(
            "required flags granted: {}",
            challenge.flags & ntlm::REQUIRED_FLAGS == ntlm::REQUIRED_FLAGS
        );
        println!("computer name: {:?}", challenge.computer_name());
        println!("domain name: {:?}", challenge.domain_name());
        println!(
            "timestamp: {:?}",
            challenge.timestamp().map(u64::from_le_bytes)
        );
        println!(
            "av pairs: {:?}",
            challenge
                .av_pairs
                .iter()
                .map(|(id, v)| (*id, v.len()))
                .collect::<Vec<_>>()
        );
        Ok(())
    }

    fn identify(host: &str, port: &str, account: &str) -> Result<(), String> {
        let (domain, user) = account.split_once('\\').unwrap_or(("", account));
        let mut password = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut password)
            .map_err(|e| e.to_string())?;
        let password = password.trim_end_matches(['\r', '\n']);
        let endpoint = Endpoint {
            host: host.into(),
            port: port_of(port)?,
            user: user.into(),
            domain: domain.into(),
            password: SecretWide::new(password.into()),
        };
        let mut transport = NtlmHttpTransport::connect(&endpoint).map_err(|e| e.to_string())?;
        let reply = transport.post(IDENTIFY).map_err(|e| e.to_string())?;
        println!("computer name: {:?}", transport.server_name());
        println!("HTTP {}", reply.status);
        println!("{}", reply.body);
        if (200..300).contains(&reply.status) {
            Ok(())
        } else {
            Err(format!("the service answered HTTP {}", reply.status))
        }
    }
}
