//! Tinta Office fork: the network transport for RFC 3161 timestamps (PAdES B-T signatures and
//! document timestamps). The sign crate builds and checks the request and the token and never
//! opens a socket; this posts the request to the timestamp server the user chose, and nothing
//! else. Nothing is sent unless a server is given.
//!
//! The token is signed by the server, so the sign crate's checks of its imprint and signature
//! are what make it trustworthy: plain `http://` servers (most public ones) are fine.

use std::time::Duration;

use pdfcraft_sign::{SignError, TimestampAuthority};

/// The largest reply read: a token with its certificate chain is a few KiB.
const MAX_REPLY: u64 = 1 << 20;
/// The longest a server may take before signing gives up.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The longest URL taken.
const MAX_URL: usize = 2048;

/// An RFC 3161 server reached over HTTP(S) (RFC 3161 §3.4: a POST of
/// `application/timestamp-query`, answered with `application/timestamp-reply`).
pub struct HttpTsa {
    url: String,
    agent: ureq::Agent,
}

impl HttpTsa {
    /// A server at `url` (`http://` or `https://`). `user_agent` names the product and its
    /// version, nothing about the person.
    pub fn new(url: &str, user_agent: &str) -> Result<HttpTsa, SignError> {
        let url = url.trim();
        let scheme_ok = ["http://", "https://"].iter().any(|s| url.get(..s.len()).is_some_and(|p| p.eq_ignore_ascii_case(s)));
        if !scheme_ok || url.len() > MAX_URL || url.chars().any(char::is_whitespace) {
            return Err(SignError::Pdf(format!("the timestamp server must be an http:// or https:// address, not {url:?}")));
        }
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .user_agent(user_agent)
            .build()
            .new_agent();
        Ok(HttpTsa { url: url.to_owned(), agent })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl TimestampAuthority for HttpTsa {
    fn timestamp(&self, request: &[u8]) -> Result<Vec<u8>, SignError> {
        let failed = |why: String| SignError::Pdf(format!("the timestamp server {} {why}", self.url));
        let mut reply = self
            .agent
            .post(&self.url)
            .header("Content-Type", "application/timestamp-query")
            .header("Accept", "application/timestamp-reply")
            .send(request)
            .map_err(|e| failed(format!("could not be reached: {e}")))?;
        let status = reply.status();
        if !status.is_success() {
            return Err(failed(format!("answered HTTP {}", status.as_u16())));
        }
        reply.body_mut().with_config().limit(MAX_REPLY).read_to_vec().map_err(|e| failed(format!("sent an unreadable reply: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// A one-request HTTP server on loopback answering `status` and `body`; returns its URL and
    /// what it received (headers, then body).
    fn serve(status: &'static str, body: Vec<u8>) -> (String, std::thread::JoinHandle<(String, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/tsa", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                head.push_str(&line);
            }
            let length = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                .unwrap_or(0);
            let mut sent = vec![0; length];
            reader.read_exact(&mut sent).unwrap();
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/timestamp-reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
            (head, sent)
        });
        (url, handle)
    }

    #[test]
    fn posts_the_query_and_returns_the_reply() {
        let (url, server) = serve("200 OK", b"reply bytes".to_vec());
        let tsa = HttpTsa::new(&url, "TintaPDF/test").unwrap();
        assert_eq!(tsa.timestamp(b"query bytes").unwrap(), b"reply bytes");
        let (head, sent) = server.join().unwrap();
        assert!(head.starts_with("POST /tsa "), "{head}");
        assert!(head.to_ascii_lowercase().contains("content-type: application/timestamp-query"), "{head}");
        assert_eq!(sent, b"query bytes");
    }

    #[test]
    fn a_refusal_is_an_error_naming_the_server() {
        let (url, server) = serve("500 Internal Server Error", Vec::new());
        let e = HttpTsa::new(&url, "TintaPDF/test").unwrap().timestamp(b"q").unwrap_err().to_string();
        assert!(e.contains(&url) && e.contains("HTTP 500"), "{e}");
        server.join().unwrap();
    }

    #[test]
    fn an_unreachable_server_is_an_error() {
        // A port nobody listens on: bind one, then let it go.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let e = HttpTsa::new(&format!("http://127.0.0.1:{port}/"), "TintaPDF/test").unwrap().timestamp(b"q").unwrap_err().to_string();
        assert!(e.contains("could not be reached"), "{e}");
    }

    #[test]
    fn only_web_addresses_are_servers() {
        for bad in ["", "ftp://tsa.example", "file:///etc/passwd", "tsa.example", "http://a b"] {
            assert!(HttpTsa::new(bad, "TintaPDF/test").is_err(), "{bad:?}");
        }
        assert_eq!(HttpTsa::new(" HTTP://tsa.example/tsr ", "TintaPDF/test").unwrap().url(), "HTTP://tsa.example/tsr");
    }
}
