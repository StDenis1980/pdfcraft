//! Tinta Office fork: signing with a timestamp server (PAdES B-T) and document timestamps,
//! through the tools, against an RFC 3161 server on loopback.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

use pdfcraft_automation::{Automation, Content, ToolError};
use pdfcraft_engine::sign::{self, DigitalId, Name, PrivateKey, Time, der};
use serde_json::{Value, json};

/// A timestamp authority's ID: self-signed, valid from October 2026 for ten years, with the
/// critical time-stamping extended key usage RFC 3161 requires.
fn tsa_id() -> DigitalId {
    let key = PrivateKey::generate_p256().unwrap();
    let name = Name::build("Test TSA", "", "Tinta Tests", "", "ES");
    let alg = key.preferred_digest();
    let sig_alg = key.signature_algorithm(alg);
    let from = Time { year: 2026, month: 10, day: 2, hour: 0, minute: 0, second: 0 };
    let until = Time { year: 2036, ..from };
    let eku = der::seq(&[&der::oid("2.5.29.37"), &der::boolean(true), &der::octets(&der::seq(&[&der::oid("1.3.6.1.5.5.7.3.8")]))]);
    let tbs = der::seq(&[
        &der::explicit(0, &der::int(2)),
        &der::uint(&[0x31]),
        &sig_alg,
        &name.raw,
        &der::seq(&[&from.encode(), &until.encode()]),
        &name.raw,
        &key.public_key().spki(),
        &der::explicit(3, &der::seq(&[&eku])),
    ]);
    let signature = key.sign(alg, &tbs).unwrap();
    let certificate = sign::Certificate::parse(&der::seq(&[&tbs, &sig_alg, &der::bit_string(&signature)])).unwrap();
    DigitalId { key, certificate, chain: Vec::new(), friendly_name: None }
}

/// An HTTP timestamp server on loopback that answers `requests` queries; returns its URL.
fn tsa_server(requests: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/tsr", listener.local_addr().unwrap());
    let id = tsa_id();
    std::thread::spawn(move || {
        for n in 0..requests {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut query = vec![0; length];
            reader.read_exact(&mut query).unwrap();
            let q = sign::timestamp::parse_request(&query).unwrap();
            let time = Time { year: 2026, month: 10, day: 28, hour: 9, minute: 0, second: 0 };
            let reply =
                sign::timestamp::respond(&id.key, &id.certificate, &id.chain, sign::DigestAlg::Sha256, &q, "1.2.3.4", time, n as u64 + 1).unwrap();
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/timestamp-reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            )
            .unwrap();
            stream.write_all(&reply).unwrap();
        }
    });
    url
}

fn workdir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tinta-tsa-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ok(a: &mut Automation, tool: &str, args: Value) -> Value {
    match a.call(tool, &args).unwrap_or_else(|e| panic!("{tool}: {e:?}")).into_iter().next() {
        Some(Content::Json(v)) => v,
        other => panic!("{tool}: {other:?}"),
    }
}

/// A one-page document and a digital ID to sign it with.
fn setup(dir: &std::path::Path) -> (Automation, u64) {
    let mut a = Automation::new().with_root(dir).unwrap();
    ok(&mut a, "sign_id_create", json!({ "name": "Ada Lovelace", "key": "p256", "password": "secret12", "path": "ada.p12" }));
    let doc = ok(&mut a, "doc_create", json!({ "from": "text", "text": "Prueba de firma" }))["doc"].as_u64().unwrap();
    (a, doc)
}

#[test]
fn a_signature_with_a_timestamp_server_carries_its_time() {
    let dir = workdir("sign");
    let (mut a, doc) = setup(&dir);
    let url = tsa_server(1);
    let r = ok(&mut a, "sign_document", json!({ "doc": doc, "id": "ada.p12", "password": "secret12", "out": "signed.pdf", "tsa": url }));
    assert_eq!(r["signature"]["timestamp"], true, "{r}");
    // Reopened, the token still verifies and gives the server's time.
    let mut b = Automation::new().with_root(&dir).unwrap();
    let re = ok(&mut b, "doc_open", json!({ "path": "signed.pdf" }))["doc"].as_u64().unwrap();
    let list = ok(&mut b, "sign_list", json!({ "doc": re }));
    let s = &list["signatures"][0];
    assert_eq!(s["timestamp"], true, "{list}");
    // The test server isn't a trusted authority, so its time is reported but not relied on.
    assert!(s["details"].to_string().contains("2026.10.28 09:00:00 UTC"), "{list}");
}

#[test]
fn a_document_timestamp_is_added_and_validates() {
    let dir = workdir("doc");
    let (mut a, doc) = setup(&dir);
    let url = tsa_server(1);
    let r = ok(&mut a, "sign_timestamp", json!({ "doc": doc, "tsa": url, "out": "stamped.pdf" }));
    assert!(r["bytes"].as_u64().unwrap() > 0, "{r}");
    let mut b = Automation::new().with_root(&dir).unwrap();
    let re = ok(&mut b, "doc_open", json!({ "path": "stamped.pdf" }))["doc"].as_u64().unwrap();
    let list = ok(&mut b, "sign_list", json!({ "doc": re }));
    assert_eq!(list["signatures"].as_array().map(Vec::len), Some(1), "{list}");
    assert!(list.to_string().contains("2026"), "the token's time is reported: {list}");
}

#[test]
fn a_bad_or_silent_server_fails_before_anything_is_written() {
    let dir = workdir("bad");
    let (mut a, doc) = setup(&dir);
    let r = a.call("sign_document", &json!({ "doc": doc, "id": "ada.p12", "password": "secret12", "out": "x.pdf", "tsa": "ftp://tsa.example" }));
    assert!(matches!(r, Err(ToolError::InvalidArgs(_))), "{r:?}");
    // Nobody listens on this port.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let r = a.call("sign_timestamp", &json!({ "doc": doc, "tsa": format!("http://127.0.0.1:{port}/"), "out": "y.pdf" }));
    assert!(matches!(&r, Err(ToolError::Failed(m)) if m.contains("could not be reached")), "{r:?}");
    assert!(!dir.join("x.pdf").exists() && !dir.join("y.pdf").exists());
}
