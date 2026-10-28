//! Tinta Office fork: Sign as ▸ Timestamp asks an RFC 3161 server (here on loopback) and embeds
//! its token (PAdES B-T); the choice is remembered with the preferences.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

use crate::support;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::sign::{self, DigitalId, Name, PrivateKey, Time, der};
use pdfcraft_ui_egui::PdfCraftApp;

const FIXTURE: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >> endobj
trailer << /Root 1 0 R >>
%%EOF";

/// A self-signed timestamp authority with the critical time-stamping extended key usage.
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

/// A loopback timestamp server answering one query; returns its URL.
fn tsa_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/tsr", listener.local_addr().unwrap());
    let id = tsa_id();
    std::thread::spawn(move || {
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
        let reply = sign::timestamp::respond(&id.key, &id.certificate, &id.chain, sign::DigestAlg::Sha256, &q, "1.2.3.4", time, 1).unwrap();
        let mut stream = stream;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/timestamp-reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            reply.len()
        )
        .unwrap();
        stream.write_all(&reply).unwrap();
    });
    url
}

#[test]
fn signing_with_a_timestamp_server_embeds_its_token_and_remembers_it() {
    let dir = std::env::temp_dir().join(format!("tinta-sign-timestamp-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("signed.pdf");
    let save = out.to_string_lossy().into_owned();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("form.pdf", None, FIXTURE.to_vec()).expect("opens");
        app.save_override = Some(save);
        app
    });
    h.run_steps(6);
    let url = tsa_server();
    assert!(h.state_mut().execute("sign.certify_invisible"));
    h.run_steps(2);
    {
        let d = h.state_mut().sign_draft.as_mut().unwrap();
        assert!(!d.timestamp.on, "off until chosen");
        d.new_id.create = false;
        d.new_id.file = concat!(env!("CARGO_MANIFEST_DIR"), "/../sign/tests/data/ec-p256.p12").into();
        d.new_id.file_password = "test".into();
    }
    h.run_steps(1);
    h.get_by_label("Continue").click();
    h.run_steps(4);
    h.get_by_label("Continue").click();
    h.run_steps(2);
    h.get_by_label("Add a timestamp from a server").click();
    h.run_steps(2);
    {
        let d = h.state_mut().sign_draft.as_mut().unwrap();
        assert!(d.timestamp.on, "the checkbox turns it on");
        d.timestamp.url = url.clone();
        d.password = "test".into();
    }
    // The dialog grows by the server row; let it settle before clicking.
    h.run_steps(3);
    support::outside_menus(&h, "Sign").click();
    h.run_steps(6);
    let s = h.state();
    assert_eq!(s.sign_draft.as_ref().and_then(|d| d.error.clone()), None);
    let sig = s.session.get(s.views[0].id).unwrap().signatures.iter().find(|x| x.signed).cloned().unwrap();
    assert!(sig.timestamp, "the server's token is embedded");
    let saved = std::fs::read(&out).unwrap();
    assert!(saved.windows(b"ETSI.CAdES.detached".len()).any(|w| w == b"ETSI.CAdES.detached"));
    // Remembered for the next signing, and with the preferences.
    assert!(s.sign_timestamp.on && s.sign_timestamp.url == url);
    let prefs = s.persist();
    let mut fresh = PdfCraftApp::new();
    fresh.restore(&prefs);
    assert_eq!(fresh.sign_timestamp, s.sign_timestamp);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreachable_timestamp_server_leaves_the_document_unsigned() {
    let dir = std::env::temp_dir().join(format!("tinta-sign-timestamp-down-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("signed.pdf");
    let save = out.to_string_lossy().into_owned();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("form.pdf", None, FIXTURE.to_vec()).expect("opens");
        app.save_override = Some(save);
        app
    });
    h.run_steps(6);
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(h.state_mut().execute("sign.certify_invisible"));
    h.run_steps(2);
    {
        let d = h.state_mut().sign_draft.as_mut().unwrap();
        d.new_id.create = false;
        d.new_id.file = concat!(env!("CARGO_MANIFEST_DIR"), "/../sign/tests/data/ec-p256.p12").into();
        d.new_id.file_password = "test".into();
    }
    h.run_steps(1);
    h.get_by_label("Continue").click();
    h.run_steps(4);
    h.get_by_label("Continue").click();
    h.run_steps(2);
    {
        let d = h.state_mut().sign_draft.as_mut().unwrap();
        d.timestamp.on = true;
        d.timestamp.url = format!("http://127.0.0.1:{port}/");
        d.password = "test".into();
    }
    // The dialog grows by the server row; let it settle before clicking.
    h.run_steps(3);
    support::outside_menus(&h, "Sign").click();
    h.run_steps(6);
    let s = h.state();
    let error = s.sign_draft.as_ref().and_then(|d| d.error.clone()).unwrap_or_default();
    assert!(error.contains("could not be reached"), "{error:?}");
    assert!(!out.exists(), "nothing is written");
    assert!(!s.session.get(s.views[0].id).unwrap().signatures.iter().any(|x| x.signed));
    assert!(!s.sign_timestamp.on, "a failed signing doesn't change the remembered choice");
    let _ = std::fs::remove_dir_all(&dir);
}
