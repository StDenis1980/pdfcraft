//! Tinta Office fork: `doc_create` from an Office file runs the suite app's own converter (here a
//! stand-in named by `TINTA_TEXTO_CLI`) on a copy of it, and opens the PDF it writes.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A one-page PDF from `pdfcraft-cli` itself (a blank page saved through a script).
fn blank_pdf(dir: &Path) -> PathBuf {
    let script = dir.join("blank.json");
    let pdf = dir.join("blank.pdf");
    let steps = serde_json::json!([
        { "tool": "doc_create", "args": { "from": "blank", "width": 200, "height": 300 } },
        { "tool": "doc_save", "args": { "doc": 1, "path": pdf.to_str().unwrap() } },
    ]);
    std::fs::write(&script, steps.to_string()).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_pdfcraft-cli")).arg("run").arg("--script").arg(&script).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    pdf
}

fn sandbox(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-cli-tinta-office-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Create a PDF from `dir/Carta.docx` and save it as `dir/out.pdf`, with the Texto converter at
/// `tool`.
fn create_from_letter(dir: &Path, tool: &Path) -> Output {
    std::fs::write(dir.join("Carta.docx"), b"PK\x03\x04 not really a document").unwrap();
    let script = dir.join("steps.json");
    let steps = serde_json::json!([
        { "tool": "doc_create", "args": { "from": "office", "path": "Carta.docx" } },
        { "tool": "doc_save", "args": { "doc": 1, "path": "out.pdf" } },
    ]);
    std::fs::write(&script, steps.to_string()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_pdfcraft-cli"))
        .env("TINTA_TEXTO_CLI", tool)
        .arg("run")
        .arg("--script")
        .arg(&script)
        .arg("--root")
        .arg(dir)
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn a_word_file_becomes_the_pdf_tinta_texto_writes() {
    let dir = sandbox("convert");
    let pdf = blank_pdf(&dir);
    let tool = dir.join("wordcraft-cli");
    // Called as `convert <copy>.docx <out>.pdf`, it writes the blank page as the result.
    std::fs::write(&tool, format!("#!/bin/sh\n[ \"$1\" = convert ] && case \"$2\" in *.docx) cp '{}' \"$3\";; esac\n", pdf.display())).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = create_from_letter(&dir, &tool);
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("Carta.pdf"), "named after the file: {stdout}");
    let out = std::fs::read(dir.join("out.pdf")).unwrap();
    let info = pdfcraft_render::inspect(std::sync::Arc::new(out), None).unwrap();
    assert_eq!(info.pages.len(), 1);
    assert_eq!((info.pages[0].width.round(), info.pages[0].height.round()), (200.0, 300.0), "the converter's page");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_tinta_texto_the_error_says_to_install_it() {
    let dir = sandbox("missing");
    let o = create_from_letter(&dir, &dir.join("no-such-wordcraft-cli"));
    assert!(!o.status.success());
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("Tinta Texto") && stderr.contains("not installed"), "{stderr}");
    assert!(!dir.join("out.pdf").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in for `wordcraft-cli` in `dir` that writes `pdf` for any .docx it is asked to convert.
fn fake_texto(dir: &Path, pdf: &Path) -> PathBuf {
    let tool = dir.join("wordcraft-cli");
    std::fs::write(&tool, format!("#!/bin/sh\n[ \"$1\" = convert ] && case \"$2\" in *.docx) cp '{}' \"$3\";; esac\n", pdf.display())).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    tool
}

/// Run `steps` under `--root dir`, with the Texto converter at `tool`.
fn run_steps(dir: &Path, tool: &Path, steps: serde_json::Value) -> Output {
    let script = dir.join("steps.json");
    std::fs::write(&script, steps.to_string()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_pdfcraft-cli"))
        .env("TINTA_TEXTO_CLI", tool)
        .arg("run")
        .arg("--script")
        .arg(&script)
        .arg("--root")
        .arg(dir)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn page_count(path: &Path) -> usize {
    pdfcraft_render::inspect(std::sync::Arc::new(std::fs::read(path).unwrap()), None).unwrap().pages.len()
}

#[test]
fn combine_takes_a_word_file_between_pdfs() {
    let dir = sandbox("combine");
    let pdf = blank_pdf(&dir);
    let tool = fake_texto(&dir, &pdf);
    std::fs::write(dir.join("Carta.docx"), b"PK\x03\x04 not really a document").unwrap();
    let o = run_steps(
        &dir,
        &tool,
        serde_json::json!([
            { "tool": "doc_combine", "args": { "paths": ["blank.pdf", "Carta.docx", "blank.pdf"], "out": "all.pdf" } },
        ]),
    );
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(page_count(&dir.join("all.pdf")), 3, "the letter's page between the two PDFs");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn batch_convert_makes_a_pdf_of_a_word_file() {
    let dir = sandbox("batch");
    let pdf = blank_pdf(&dir);
    let tool = fake_texto(&dir, &pdf);
    std::fs::create_dir_all(dir.join("in")).unwrap();
    std::fs::write(dir.join("in/Carta.docx"), b"PK\x03\x04 not really a document").unwrap();
    let o = run_steps(
        &dir,
        &tool,
        serde_json::json!([
            { "tool": "doc_batch", "args": { "paths": ["in"], "out_dir": "out", "steps": ["convert"] } },
        ]),
    );
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("\"completed\":1") || stdout.contains("\"completed\": 1"), "{stdout}");
    assert_eq!(page_count(&dir.join("out/Carta.pdf")), 1, "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}
