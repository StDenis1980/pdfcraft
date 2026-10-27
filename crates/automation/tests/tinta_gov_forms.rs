//! Tinta Office fork: fill, save and reopen real Spanish government forms (DGT, AEAT, Seguridad
//! Social) through the same tools as the CLI and MCP.
//!
//! The forms are public, but they are not ours to redistribute, so they are never committed: put
//! the blank PDFs in a folder and point `PDFCRAFT_GOV_FORMS_DIR` at it. Without it the test only
//! checks itself on a form it builds. A problem found here gets a small synthetic regression test
//! next to the code it fixes.

use pdfcraft_automation::{Automation, Content};
use pdfcraft_engine::{Edit, NewField, Session};
use serde_json::{Map, Value, json};

/// Spanish text with every accented letter and the Catalan middle dot, as people type it.
const SAMPLE: &str = "Peñaranda Muñoz, José Ángel · l·l ç ü";

fn tool(a: &mut Automation, name: &str, args: Value) -> Result<Value, String> {
    match a.call(name, &args).map_err(|e| format!("{name}: {e:?}"))?.into_iter().next() {
        Some(Content::Json(v)) => Ok(v),
        Some(_) => Err(format!("{name}: expected JSON")),
        None => Err(format!("{name}: no result")),
    }
}

/// Renders `page` (1-based) and checks a picture came back.
fn render(a: &mut Automation, doc: u64, page: u64) -> Result<(), String> {
    match a.call("page_render", &json!({ "doc": doc, "page": page, "dpi": 50 })).map_err(|e| format!("page_render {page}: {e:?}"))?.first() {
        Some(Content::Png { data, width, height }) if !data.is_empty() && *width > 0 && *height > 0 => Ok(()),
        _ => Err(format!("page_render {page}: no picture")),
    }
}

/// A value a field with Acrobat's `format` (from form_fields) accepts, as a person would type it.
fn formatted(format: &Value) -> String {
    match format["type"].as_str() {
        Some("date" | "time") => {
            let pattern = format["pattern"].as_str().unwrap_or("dd/mm/yyyy");
            // 15 March 2024, 09:30: every field of the pattern, longest names first.
            [
                ("yyyy", "2024"),
                ("yy", "24"),
                ("mmmm", "March"),
                ("mmm", "Mar"),
                ("mm", "03"),
                ("dd", "15"),
                ("HH", "09"),
                ("hh", "09"),
                ("MM", "30"),
                ("ss", "00"),
            ]
            .iter()
            .fold(pattern.to_owned(), |p, (from, to)| p.replace(from, to))
        }
        Some("zip") => "46500".into(),
        Some("zip4") => "46500-1234".into(),
        Some("phone") => "9612345678".into(),
        Some("ssn") => "123456789".into(),
        _ => "12".into(),
    }
}

/// The value each fillable field gets, and what reading it back should give.
fn values_for(fields: &[Value]) -> Map<String, Value> {
    let mut values = Map::new();
    for f in fields {
        if f["read_only"].as_bool() == Some(true) {
            continue;
        }
        let Some(name) = f["name"].as_str() else { continue };
        let value = match f["type"].as_str() {
            Some("text") => {
                // A formatted field gets a value its format accepts; a length limit is respected.
                let text = match f.get("format") {
                    Some(format) => formatted(format),
                    None => SAMPLE.to_owned(),
                };
                let text: String = match f["max_length"].as_u64() {
                    Some(m) => text.chars().take(usize::try_from(m).unwrap_or(usize::MAX)).collect(),
                    None => text,
                };
                json!(text)
            }
            Some("checkbox") => json!(true),
            // The first real choice: an empty option (DGT's "Provincia" starts with one) means
            // "nothing chosen", and reads back as no value.
            Some("combo" | "radio") => {
                let choice = |o: &Value| match o {
                    Value::String(s) => json!(s),
                    o => o["value"].clone(),
                };
                match f["options"].as_array().into_iter().flatten().map(choice).find(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())) {
                    Some(v) => v,
                    None => continue,
                }
            }
            _ => continue,
        };
        values.insert(name.to_owned(), value);
    }
    values
}

/// Opens `file` in `dir`, fills every field, saves a copy, reopens it and checks the values came
/// back. Returns one line for the report, or what went wrong.
fn check_form(dir: &std::path::Path, file: &str) -> Result<String, String> {
    let mut a = Automation::new().with_root(dir).map_err(|e| format!("{e:?}"))?.with_clock(|| 1_700_000_000);
    let doc = tool(&mut a, "doc_open", json!({ "path": file }))?["doc"].as_u64().ok_or("doc_open: no id")?;
    let fields = tool(&mut a, "form_fields", json!({ "doc": doc }))?["fields"].as_array().cloned().ok_or("form_fields: no list")?;
    if fields.is_empty() {
        return Err("no form fields".into());
    }
    let values = values_for(&fields);
    let filled = tool(&mut a, "form_fill", json!({ "doc": doc, "values": values }))?;
    // Spanish is in WinAnsiEncoding: no field should have to draw `?` for it. A value wider than a
    // small box is only a warning that the box shows part of it.
    let undrawable: Vec<&Value> =
        filled.get("warnings").and_then(Value::as_array).into_iter().flatten().filter(|w| !w.to_string().contains("does not fit")).collect();
    if !undrawable.is_empty() {
        return Err(format!("characters the fields can't draw: {undrawable:?}"));
    }
    render(&mut a, doc, 1)?;
    let out = format!("filled-{file}");
    tool(&mut a, "doc_save", json!({ "doc": doc, "path": out }))?;

    let reopened = tool(&mut a, "doc_open", json!({ "path": out }))?;
    let again = reopened["doc"].as_u64().ok_or("reopen: no id")?;
    let pages = reopened["pages"].as_u64().unwrap_or(0);
    let read = tool(&mut a, "form_fields", json!({ "doc": again }))?;
    let read: Vec<Value> = read["fields"].as_array().cloned().unwrap_or_default();
    let mut lost = Vec::new();
    for (name, want) in &values {
        let got = read.iter().find(|f| f["name"].as_str() == Some(name.as_str())).map(|f| &f["value"]);
        if got != Some(want) {
            lost.push(format!("{name}: wanted {want}, read {}", got.unwrap_or(&Value::Null)));
        }
    }
    if !lost.is_empty() {
        return Err(format!("{} of {} values not kept: {}", lost.len(), values.len(), lost.join("; ")));
    }
    for page in 1..=pages {
        render(&mut a, again, page)?;
    }
    Ok(format!("{file}: {} fields, {} filled, {pages} pages", fields.len(), values.len()))
}

/// The harness itself, on a form built here: a text field, a check box and a drop-down whose first
/// option is empty, like DGT's.
#[test]
fn the_harness_fills_and_reads_back_a_built_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    let bytes = s.create_blank(300.0, 400.0, 1).unwrap();
    let id = s.open("form.pdf", None, bytes, None).unwrap();
    let provincia = NewField::Combo { options: vec![String::new(), "Valencia".into()], editable: false };
    for (name, kind, y) in
        [("apellidos", NewField::Text { multiline: false }, 300.0), ("acepto", NewField::CheckBox, 250.0), ("provincia", provincia, 200.0)]
    {
        s.apply(id, Edit::AddField { page: 0, rect: [30.0, y, 250.0, y + 20.0], kind, name: Some(name.into()) }).unwrap();
    }
    std::fs::write(dir.path().join("form.pdf"), s.save_bytes(id).unwrap().as_slice()).unwrap();
    let line = check_form(dir.path(), "form.pdf").unwrap();
    assert_eq!(line, "form.pdf: 3 fields, 3 filled, 1 pages");
}

/// Every PDF in `PDFCRAFT_GOV_FORMS_DIR`. Skips when it is unset.
#[test]
fn government_forms_fill_save_and_reopen() {
    let Some(dir) = std::env::var_os("PDFCRAFT_GOV_FORMS_DIR").map(std::path::PathBuf::from) else {
        eprintln!("PDFCRAFT_GOV_FORMS_DIR is not set; skipping the government forms");
        return;
    };
    // Work on copies: the saved forms land beside them.
    let work = tempfile::tempdir().unwrap();
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().ends_with(".pdf"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no PDFs in {}", dir.display());
    let mut failures = Vec::new();
    for file in &files {
        std::fs::copy(dir.join(file), work.path().join(file)).unwrap();
        match check_form(work.path(), file) {
            Ok(line) => eprintln!("ok   {line}"),
            Err(e) => {
                eprintln!("FAIL {file}: {e}");
                failures.push(format!("{file}: {e}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} of {} forms failed:\n{}", failures.len(), files.len(), failures.join("\n"));
}
