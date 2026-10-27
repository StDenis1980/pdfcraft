//! Tinta Office fork: Recognize Text reads Spanish, Catalan and Ukrainian with the PaddleOCR
//! models (skipped when they are not installed: `cargo xtask models`). Synthetic pages only.

use pdfcraft_engine::ocr::{OcrSettings, engine, language_available};
use pdfcraft_engine::{Session, export};

/// A page showing `text` at 14 pt in Inter, which draws Cyrillic as well as Latin: the OCR text
/// layer's writer, made visible.
fn page_showing(text: &str) -> std::sync::Arc<Vec<u8>> {
    let s = Session::new();
    let blank = s.create_blank(595.0, 200.0, 1).unwrap();
    let mut doc = pdfcraft_cos::Document::open_with_password(blank, None).unwrap();
    let resource = pdfcraft_edit::text::register_text_font(&mut doc, 0, pdfcraft_fonts::INTER_REGULAR).unwrap();
    let codes = pdfcraft_fonts::embedding::embedded_codes(pdfcraft_fonts::INTER_REGULAR).unwrap();
    let font = pdfcraft_ocr::UnicodeFont { resource: &resource, codes: &codes, width: pdfcraft_fonts::inter_width };
    // Every word Inter-coded (a Latin one too), at its natural width, on one baseline.
    let mut x = 36.0;
    let mut words = Vec::new();
    for w in text.split(' ') {
        let width = pdfcraft_fonts::inter_width(w, 14.0);
        words.push(pdfcraft_ocr::PlacedWord { text: w.into(), origin: [x, 100.0], across: [width, 0.0], up: [0.0, 10.0] });
        x += width + 4.0;
    }
    let mut layer = pdfcraft_ocr::text_layer_with(&words, Some(&font));
    let at = layer.windows(4).position(|w| w == b"3 Tr").unwrap();
    layer[at] = b'0';
    pdfcraft_edit::stamp(&mut doc, 0, "T", layer).unwrap();
    std::sync::Arc::new(pdfcraft_cos::write_full(&doc, &pdfcraft_cos::SaveOptions::default()).unwrap())
}

/// What Recognize Text reads on a scan of [`page_showing`] `text`, rendered at 300 dpi, as offices scan.
fn read_back(text: &str, language: &str) -> Option<String> {
    if !language_available(language) {
        eprintln!("skipped: the {language} OCR model is not installed");
        return None;
    }
    let mut s = Session::new();
    let id = s.open("text.pdf", None, page_showing(text), None).unwrap();
    let png = export::Exporter::new(s.get(id).unwrap()).png(0, 300.0).unwrap();
    let scan = s.create_from_images(&[("scan.png".into(), png)]).unwrap();
    let id = s.open("scan.pdf", None, scan, None).unwrap();
    let settings = OcrSettings { language: language.into(), ..Default::default() };
    let job = s.ocr_job(id, &[], settings).unwrap();
    let pages = job.run(&engine().unwrap(), |_, _| true);
    let page = pages.first().unwrap();
    assert_eq!(page.skipped, None);
    Some(page.text())
}

#[test]
fn spanish_accents_are_read() {
    let Some(text) = read_back("José Ángel Peñaranda Muñoz, calle Mayor", "es") else { return };
    assert_eq!(text, "José Ángel Peñaranda Muñoz, calle Mayor");
}

#[test]
fn catalan_letters_are_read() {
    let Some(text) = read_back("Col·legi de València, plaça Ausiàs", "ca") else { return };
    assert_eq!(text, "Col·legi de València, plaça Ausiàs");
}

#[test]
fn ukrainian_is_read() {
    let Some(text) = read_back("Добрий день, Київ і Ґалаґан", "uk") else { return };
    assert_eq!(text, "Добрий день, Київ і Ґалаґан");
}

#[test]
fn more_languages_are_read() {
    for (language, text) in [
        ("uk", "Їжак їсть яблука, а Євгенія читає"),
        ("ru", "Съешь ещё этих мягких булок"),
        ("bg", "Щастливият юноша живее в София"),
        ("gl", "A miña irmá vive na Coruña"),
        ("eu", "Euskal Herriko unibertsitatea"),
        ("pt", "Não há ação sem coração"),
        ("fr", "Où est la gare? À côté du château"),
        ("de", "Größe und Übung für Straßen"),
    ] {
        let Some(read) = read_back(text, language) else { return };
        assert_eq!(read, text, "{language}");
    }
}
