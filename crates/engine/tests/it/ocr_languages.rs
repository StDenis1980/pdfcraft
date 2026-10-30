//! Tinta Office fork: Recognize Text reads Spanish, Catalan and Ukrainian with the PaddleOCR
//! models (skipped when they are not installed: `cargo xtask models`). Synthetic pages only.

use pdfcraft_engine::ocr::{OcrSettings, engine, language_available};
use pdfcraft_engine::{Session, export};

/// A page showing `text` at 14 pt in Inter, which draws Cyrillic as well as Latin: the OCR text
/// layer's writer, made visible.
fn page_showing(text: &str) -> std::sync::Arc<Vec<u8>> {
    page_showing_lines(&[text], 200.0)
}

/// A page `height` points tall showing `lines` at 14 pt in Inter, 30 points apart from the top.
fn page_showing_lines(lines: &[&str], height: f64) -> std::sync::Arc<Vec<u8>> {
    let s = Session::new();
    let blank = s.create_blank(595.0, height, 1).unwrap();
    let mut doc = pdfcraft_cos::Document::open_with_password(blank, None).unwrap();
    let resource = pdfcraft_edit::text::register_text_font(&mut doc, 0, pdfcraft_fonts::INTER_REGULAR).unwrap();
    let codes = pdfcraft_fonts::embedding::embedded_codes(pdfcraft_fonts::INTER_REGULAR).unwrap();
    let font = pdfcraft_ocr::UnicodeFont { resource: &resource, codes: &codes, width: pdfcraft_fonts::inter_width };
    // Every word Inter-coded (a Latin one too), at its natural width, on its line's baseline.
    let mut words = Vec::new();
    for (k, text) in lines.iter().enumerate() {
        let y = if lines.len() == 1 { 100.0 } else { height - 50.0 - 30.0 * k as f64 };
        let mut x = 36.0;
        for w in text.split(' ') {
            let width = pdfcraft_fonts::inter_width(w, 14.0);
            words.push(pdfcraft_ocr::PlacedWord { text: w.into(), origin: [x, y], across: [width, 0.0], up: [0.0, 10.0] });
            x += width + 4.0;
        }
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

#[test]
fn a_sheet_photographed_at_an_angle_is_read_straightened() {
    if !language_available("es") {
        eprintln!("skipped: the es OCR model is not installed");
        return;
    }
    let lines = [
        "El contrato tiene una duración de doce meses",
        "desde la fecha de inicio del suministro",
        "y se renovará cada año salvo aviso previo",
        "con quince días de antelación por escrito",
    ];
    let mut s = Session::new();
    let id = s.open("text.pdf", None, page_showing_lines(&lines, 220.0), None).unwrap();
    let png = export::Exporter::new(s.get(id).unwrap()).png(0, 200.0).unwrap();
    let sheet = image::load_from_memory(&png).unwrap().to_rgb8();
    let (sw, sh) = (sheet.width() as f64, sheet.height() as f64);
    // The sheet lying on a dark table, photographed turned about 25° and in perspective.
    let (w, h) = (sheet.width() * 3 / 2, sheet.height() * 3);
    let quad = [
        [0.22 * w as f64, 0.10 * h as f64],
        [0.95 * w as f64, 0.38 * h as f64],
        [0.80 * w as f64, 0.86 * h as f64],
        [0.05 * w as f64, 0.56 * h as f64],
    ];
    let to_sheet = pdfcraft_geom::perspective::Homography::from_points(quad, [[0.0, 0.0], [sw, 0.0], [sw, sh], [0.0, sh]]).unwrap();
    let photo = image::RgbImage::from_fn(w, h, |x, y| match to_sheet.apply([x as f64 + 0.5, y as f64 + 0.5]) {
        Some([u, v]) if u >= 0.0 && v >= 0.0 && u < sw && v < sh => *sheet.get_pixel(u as u32, v as u32),
        _ => image::Rgb([70, 60, 50]),
    });
    let mut encoded = Vec::new();
    image::DynamicImage::ImageRgb8(photo).write_to(&mut std::io::Cursor::new(&mut encoded), image::ImageFormat::Png).unwrap();
    let id = s.open("photo.pdf", None, s.create_from_images(&[("photo.png".into(), encoded)]).unwrap(), None).unwrap();
    let job = s.ocr_job(id, &[], OcrSettings { language: "es".into(), ..Default::default() }).unwrap();
    let pages = job.run(&engine().unwrap(), |_, _| true);
    let page = pages.first().unwrap();
    let read = page.text();
    let expected: Vec<&str> = lines.iter().flat_map(|l| l.split(' ')).filter(|w| w.chars().count() >= 4).collect();
    let found = expected.iter().filter(|w| read.contains(**w)).count();
    assert!(found * 10 >= expected.len() * 8, "{found} of {}: {read}", expected.len());
    // The words lie on the sheet where the photo shows it: the first line's first word near
    // its top-left corner, the last line's last word near its bottom-right one.
    let info = &s.get(id).unwrap().info.pages[0];
    let at = |text: &str| {
        let word = page.words.iter().find(|p| p.text == text).unwrap_or_else(|| panic!("{text} not read: {read}"));
        let [x, y] = info.user_to_view(word.origin[0] as f32, word.origin[1] as f32);
        let k = w as f32 / info.width;
        [x * k, y * k]
    };
    let near = |p: [f32; 2], q: [f64; 2]| ((p[0] as f64 - q[0]).hypot(p[1] as f64 - q[1])) < 0.25 * sw;
    let corner = |u: f64, v: f64| {
        let from_sheet = pdfcraft_geom::perspective::Homography::from_points([[0.0, 0.0], [sw, 0.0], [sw, sh], [0.0, sh]], quad).unwrap();
        from_sheet.apply([u * sw, v * sh]).unwrap()
    };
    assert!(near(at("contrato"), corner(0.12, 0.25)), "{:?} {:?}", at("contrato"), corner(0.12, 0.25));
    assert!(near(at("escrito"), corner(0.6, 0.62)), "{:?} {:?}", at("escrito"), corner(0.6, 0.62));
}
