//! Tinta Office fork: the OCR tools offer every language Recognize Text reads, and say which of
//! them have their model installed.

use pdfcraft_automation::{Automation, Content, tools};
use serde_json::{Value, json};

#[test]
fn the_ocr_tools_offer_every_language() {
    let codes: Vec<&str> = pdfcraft_engine::ocr::LANGUAGES.iter().map(|l| l.0).collect();
    for name in ["ocr_recognize", "ocr_recognize_files"] {
        let tool = tools().into_iter().find(|t| t.name == name).unwrap();
        let offered: Vec<&str> =
            tool.input_schema["properties"]["language"]["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(offered, codes, "{name}");
    }
    let mut a = Automation::new();
    let Some(Content::Json(status)) = a.call("ocr_status", &json!({})).unwrap().into_iter().next() else { panic!("ocr_status") };
    let languages = status["languages"].as_array().unwrap();
    assert_eq!(languages.len(), codes.len());
    for l in languages {
        let code = l["code"].as_str().unwrap();
        assert_eq!(l["available"], Value::Bool(pdfcraft_engine::ocr::language_available(code)), "{code}");
    }
    assert!(languages.iter().any(|l| l["code"] == "uk" && l["name"] == "Українська"));
}
