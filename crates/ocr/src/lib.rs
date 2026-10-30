//! pdfcraft-ocr — Scan & OCR ▸ Recognize text (L4).
//!
//! Recognition runs the ocrs engine (MIT/Apache-2.0) with its pre-trained models (CC-BY-SA-4.0,
//! fetched by `cargo xtask models`; see ATTRIBUTION.toml). The caller renders a page to pixels;
//! [`Ocr::recognize`] finds the words in them, and [`text_layer`] turns words placed in user space
//! into page content: invisible text (rendering mode 3) over each word, so the page becomes a
//! searchable image (Acrobat's "Searchable Image (Exact)": the image is left untouched).
//!
//! The ocrs models read the Latin alphabet without accents (English). Tinta Office fork: other
//! languages are read with PaddleOCR's models (`paddle`), which sit beside the ocrs ones.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rten_imageproc::{BoundingRect, RotatedRect};

pub mod paddle;

pub use pdfcraft_fonts::helvetica_width;

/// The model files, as named in ATTRIBUTION.toml.
pub const DETECTION_MODEL: &str = "text-detection.onnx";
pub const RECOGNITION_MODEL: &str = "text-recognition.onnx";

/// The recognition model's alphabet, as it was trained (ocrs-models' `DEFAULT_ALPHABET`): its
/// output classes follow this order. The ocrs crate's own copy has an `E` where the training had
/// `€`, which made a scanned "€250" read as "E250".
pub const ALPHABET: &str = " 0123456789!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~€ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The languages the models read (ISO 639-1) and their names in themselves. English is read by
/// the ocrs models, the others by PaddleOCR's ([`paddle::Script::for_language`]).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("es", "Español"),
    ("ca", "Català"),
    ("gl", "Galego"),
    ("eu", "Euskara"),
    ("pt", "Português"),
    ("fr", "Français"),
    ("it", "Italiano"),
    ("de", "Deutsch"),
    ("uk", "Українська"),
    ("ru", "Русский"),
    ("be", "Беларуская"),
    ("bg", "Български"),
];

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    /// Shown to end users (release packages ship the models; a source build fetches them with
    /// `cargo xtask models`). The UI catalogs key this exact text.
    #[error("the text recognition models are not installed (reinstall PdfCraft, or set PDFCRAFT_MODELS to the folder that holds them)")]
    NoModels,
    #[error("loading {0}: {1}")]
    Load(String, String),
    #[error("text recognition failed: {0}")]
    Recognize(String),
    #[error("the image is empty")]
    EmptyImage,
}

/// Where the two model files are.
#[derive(Clone, Debug, PartialEq)]
pub struct Models {
    pub detection: PathBuf,
    pub recognition: PathBuf,
}

impl Models {
    /// A complete model pair in `dir`, preferring current ONNX files over legacy RTEN files.
    /// An incomplete update never mixes formats or hides an intact legacy pair.
    pub fn in_dir(dir: &Path) -> Option<Models> {
        [(DETECTION_MODEL, RECOGNITION_MODEL), ("text-detection.rten", "text-recognition.rten")].into_iter().find_map(|(detection, recognition)| {
            let m = Models { detection: dir.join(detection), recognition: dir.join(recognition) };
            (m.detection.is_file() && m.recognition.is_file()).then_some(m)
        })
    }

    /// Look for the models: `$PDFCRAFT_MODELS`, then where the release packages install them
    /// beside the executable ([`Models::dirs_beside_exe`]), then the source tree's `assets/models/`.
    pub fn find() -> Option<Models> {
        Self::search_dirs().iter().find_map(|d| Self::in_dir(d))
    }

    /// The directories [`Models::find`] looks in, in order.
    pub fn search_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(d) = std::env::var_os("PDFCRAFT_MODELS") {
            dirs.push(PathBuf::from(d));
        }
        if let Some(exe) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            dirs.extend(Self::dirs_beside_exe(&exe));
        }
        dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models"));
        dirs
    }

    /// Where the release packages put the models, relative to the directory holding the
    /// executable (packaging/*):
    /// - `models/` beside it: the Windows MSI and portable zip;
    /// - `../Resources/models`: the macOS app bundle (`Contents/MacOS` → `Contents/Resources`);
    /// - `../share/pdfcraft/models`: the FHS-style trees (`bin/` → `share/`) of the Linux
    ///   deb/rpm/tar.gz/AppImage/Flatpak and the FreeBSD tarball, wherever they are installed.
    pub fn dirs_beside_exe(exe_dir: &Path) -> Vec<PathBuf> {
        vec![exe_dir.join("models"), exe_dir.join("../Resources/models"), exe_dir.join("../share/pdfcraft/models")]
    }
}

/// Whether `language` can be read with the installed models: English needs only the ocrs pair,
/// the others their PaddleOCR model and dictionary beside it too.
pub fn language_available(language: &str) -> bool {
    let Some(models) = Models::find() else { return false };
    match paddle::Script::for_language(language) {
        None => true,
        Some(script) => {
            let (model, dict) = script.files();
            let dir = models.detection.parent().unwrap_or(Path::new("."));
            dir.join(model).is_file() && dir.join(dict).is_file()
        }
    }
}

/// A recognised word: its text and its box in image pixels `[left, top, right, bottom]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: [f32; 4],
}

/// A recognised line of words, in reading order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Line {
    pub words: Vec<Word>,
}

impl Line {
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// A loaded recogniser. Loading takes a moment; keep one and reuse it.
pub struct Ocr {
    engine: ocrs::OcrEngine,
    /// Where the PaddleOCR models are looked for: beside the detection model.
    dir: PathBuf,
    /// The PaddleOCR line recognisers loaded so far.
    paddle: Mutex<HashMap<paddle::Script, Arc<paddle::Paddle>>>,
}

impl Ocr {
    pub fn load(models: &Models) -> Result<Ocr, OcrError> {
        let load = |p: &Path| rten::Model::load_file(p).map_err(|e| OcrError::Load(p.display().to_string(), e.to_string()));
        let params = ocrs::OcrEngineParams {
            detection_model: Some(load(&models.detection)?),
            recognition_model: Some(load(&models.recognition)?),
            alphabet: Some(ALPHABET.into()),
            ..Default::default()
        };
        let engine = ocrs::OcrEngine::new(params).map_err(|e| OcrError::Load("ocr engine".into(), e.to_string()))?;
        let dir = models.detection.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(Ocr { engine, dir, paddle: Mutex::default() })
    }

    /// The PaddleOCR recogniser for `script`, loaded the first time it is asked for.
    fn paddle(&self, script: paddle::Script) -> Result<Arc<paddle::Paddle>, OcrError> {
        let mut loaded = self.paddle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(p) = loaded.get(&script) {
            return Ok(p.clone());
        }
        let (model, dict) = script.files();
        let (model, dict) = (self.dir.join(model), self.dir.join(dict));
        if !model.is_file() || !dict.is_file() {
            return Err(OcrError::NoModels);
        }
        let p = Arc::new(paddle::Paddle::load(&model, &dict)?);
        loaded.insert(script, p.clone());
        Ok(p)
    }

    /// Recognise the text in an image written in `language` (a code from [`LANGUAGES`]).
    pub fn recognize_language(&self, pixels: &[u8], width: u32, height: u32, language: &str) -> Result<Vec<Line>, OcrError> {
        let Some(script) = paddle::Script::for_language(language) else { return self.recognize(pixels, width, height) };
        let reader = self.paddle(script)?;
        let rgb = Self::channels(pixels, width, height)?;
        let channels = rgb.len() / (width as usize).saturating_mul(height as usize).max(1);
        let err = |e: &dyn std::fmt::Display| OcrError::Recognize(e.to_string());
        let source = ocrs::ImageSource::from_bytes(&rgb, (width, height)).map_err(|e| err(&e))?;
        let input = self.engine.prepare_input(source).map_err(|e| err(&e))?;
        let found = self.detect(&rgb, width, height)?;
        // The dots of "і" and "ї" and other marks found on their own would pull the line finder
        // off the line: leave them to the word they belong to (its line's crop holds them).
        let boxes: Vec<[f32; 4]> = found
            .iter()
            .map(|r| {
                let b = r.bounding_rect();
                [b.left(), b.top(), b.right(), b.bottom()]
            })
            .collect();
        let found: Vec<_> = found.iter().zip(&boxes).filter(|(_, b)| !boxes.iter().any(|other| mark_of(**b, *other))).map(|(r, _)| *r).collect();
        let lines: Vec<(Vec<[f32; 4]>, [f32; 4])> = self
            .engine
            .find_text_lines(&input, &found)
            .iter()
            .filter_map(|line| {
                let rects: Vec<[f32; 4]> = line
                    .iter()
                    .map(|r| {
                        let b = r.bounding_rect();
                        [b.left(), b.top(), b.right(), b.bottom()]
                    })
                    .collect();
                let bounds = rects.iter().copied().reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])?;
                Some((rects, bounds))
            })
            .collect();
        let mut out = Vec::new();
        for (rects, bounds) in lines.iter().filter(|(_, b)| !lines.iter().any(|(_, other)| mark_of(*b, *other))) {
            // Boxes far apart on one line (table cells, columns, a heading beside a picture) are
            // read apart, or the model runs their words together. Each gap is read on its own too,
            // for a word the detector missed there; empty paper reads as nothing, or as words too
            // unsure to keep.
            let mut words = Vec::new();
            let pad = (bounds[3] - bounds[1]) * 0.15;
            let mut previous_right: Option<f32> = None;
            for part in segments(rects, (bounds[3] - bounds[1]) * SEGMENT_GAP) {
                let Some(part_bounds) = part.iter().copied().reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]) else {
                    continue;
                };
                if let Some(right) = previous_right {
                    // Inset by the padding, so the crop ends where the neighbouring boxes start.
                    let gap = [right + pad, bounds[1], part_bounds[0] - pad, bounds[3]];
                    if gap[2] > gap[0] {
                        words.extend(self.read_segment(&reader, &rgb, (width, height, channels), Vec::new(), gap, language)?);
                    }
                }
                previous_right = Some(previous_right.map_or(part_bounds[2], |r| r.max(part_bounds[2])));
                // The line's top and bottom, so a segment of small letters keeps the line height.
                let part_bounds = [part_bounds[0], bounds[1], part_bounds[2], bounds[3]];
                words.extend(self.read_segment(&reader, &rgb, (width, height, channels), part, part_bounds, language)?);
            }
            if !words.is_empty() {
                out.push(Line { words });
            }
        }
        Ok(out)
    }

    /// Read one run of word boxes of a line, `bounds` around them all. Words the model is
    /// unsure of (pictures, texture and stray marks read as letters) are left out.
    fn read_segment(
        &self,
        reader: &paddle::Paddle,
        rgb: &[u8],
        (width, height, channels): (u32, u32, usize),
        rects: Vec<[f32; 4]>,
        bounds: [f32; 4],
        language: &str,
    ) -> Result<Vec<Word>, OcrError> {
        // A little room around the ink, as the model was trained with.
        let pad = (bounds[3] - bounds[1]) * 0.15;
        let crop =
            [(bounds[0] - pad).max(0.0), (bounds[1] - pad).max(0.0), (bounds[2] + pad).min(width as f32), (bounds[3] + pad).min(height as f32)];
        let (read, per_step) = reader.read_line(rgb, width, height, channels, crop)?;
        let mut spans = paddle::words(&read);
        let mut texts: Vec<String> = spans.iter().map(|s| s.text.clone()).collect();
        paddle::tidy(&mut texts, language);
        for (s, t) in spans.iter_mut().zip(texts) {
            s.text = t;
        }
        // The detected word boxes when they match the words read; else the boxes from where
        // along the line each word was read.
        let boxed: Vec<(paddle::Span, [f32; 4])> = if spans.len() == rects.len() {
            spans.into_iter().zip(rects).collect()
        } else {
            spans
                .into_iter()
                .map(|s| {
                    let rect = [crop[0] + s.steps.0 as f32 * per_step, bounds[1], (crop[0] + s.steps.1 as f32 * per_step).min(crop[2]), bounds[3]];
                    (s, rect)
                })
                .collect()
        };
        Ok(boxed.into_iter().filter(|(s, _)| s.score >= MIN_WORD_SCORE).map(|(s, rect)| Word { text: s.text, rect }).collect())
    }

    /// [`Ocr::recognize_language`] for a page that may hold its text sideways (a book
    /// photographed across, a landscape table on a portrait page): when most word boxes found
    /// are taller than wide, the image is also read turned a quarter each way, and the reading
    /// with the most text kept. Returns the lines in the turned image's pixels and the turn,
    /// whose [`Turn::to_source`] maps them back.
    pub fn recognize_upright(&self, pixels: &[u8], width: u32, height: u32, language: &str) -> Result<(Vec<Line>, Turn), OcrError> {
        let lines = self.recognize_language(pixels, width, height, language)?;
        if !self.sideways(pixels, width, height)? {
            return Ok((lines, Turn::None));
        }
        let mut best = (lines, Turn::None);
        for turn in [Turn::Clockwise, Turn::CounterClockwise] {
            let Some((turned, w, h)) = turn_image(pixels, width, height, turn) else { continue };
            let read = self.recognize_language(&turned, w, h, language)?;
            if text_amount(&read) > text_amount(&best.0) {
                best = (read, turn);
            }
        }
        Ok(best)
    }

    /// Whether most of the words detected in the image are taller than wide.
    fn sideways(&self, pixels: &[u8], width: u32, height: u32) -> Result<bool, OcrError> {
        let rgb = Self::channels(pixels, width, height)?;
        let found = self.detect(&rgb, width, height)?;
        let boxes: Vec<[f32; 4]> = found
            .iter()
            .map(|r| {
                let b = r.bounding_rect();
                [b.left(), b.top(), b.right(), b.bottom()]
            })
            .collect();
        Ok(mostly_tall(&boxes))
    }

    /// The words found in an image of 1 or 3 channels (as [`Ocr::channels`] gives). The detection
    /// model takes a fixed size and shrinks a larger image to it, which loses small print on a
    /// page photographed whole, so a larger image is also searched in [`TILE`]s, and the two sets
    /// of boxes merged ([`merge_detections`]).
    fn detect(&self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<RotatedRect>, OcrError> {
        let whole = self.detect_in(rgb, width, height)?;
        if width <= TILE.0 && height <= TILE.1 {
            return Ok(whole);
        }
        let channels = rgb.len() / (width as usize).saturating_mul(height as usize).max(1);
        let mut tiled = Vec::new();
        let (xs, ys) = (tile_starts(width, TILE.0), tile_starts(height, TILE.1));
        for (row, &y) in ys.iter().enumerate() {
            for (col, &x) in xs.iter().enumerate() {
                let (tw, th) = (TILE.0.min(width.saturating_sub(x)), TILE.1.min(height.saturating_sub(y)));
                let Some(tile) = crop(rgb, width, channels, [x, y, tw, th]) else { continue };
                // Each tile keeps the words centred in its own share of the overlaps: the other
                // half belongs to its neighbour, which sees those words further from its edge.
                let (left, right) = owned(&xs, col, TILE.0, width);
                let (top, bottom) = owned(&ys, row, TILE.1, height);
                tiled.extend(
                    self.detect_in(&tile, tw, th)?
                        .into_iter()
                        .map(|r| RotatedRect::new(r.center().translate(y as f32, x as f32), r.up_axis(), r.width(), r.height()))
                        .filter(|r| {
                            let c = r.center();
                            (left..right).contains(&c.x) && (top..bottom).contains(&c.y)
                        }),
                );
            }
        }
        Ok(merge_detections(whole, tiled))
    }

    /// The words the detection model finds in one image, shrunk to its input size if larger.
    fn detect_in(&self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<RotatedRect>, OcrError> {
        let err = |e: &dyn std::fmt::Display| OcrError::Recognize(e.to_string());
        let source = ocrs::ImageSource::from_bytes(rgb, (width, height)).map_err(|e| err(&e))?;
        let input = self.engine.prepare_input(source).map_err(|e| err(&e))?;
        self.engine.detect_words(&input).map_err(|e| err(&e))
    }

    /// The image with 1 or 3 channels, as ocrs wants.
    fn channels(pixels: &[u8], width: u32, height: u32) -> Result<std::borrow::Cow<'_, [u8]>, OcrError> {
        if width == 0 || height == 0 || pixels.is_empty() {
            return Err(OcrError::EmptyImage);
        }
        Ok(if pixels.len() == (width as usize).saturating_mul(height as usize).saturating_mul(4) {
            pixels.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect::<Vec<u8>>().into()
        } else {
            pixels.into()
        })
    }

    /// Load the models found by [`Models::find`].
    pub fn find() -> Result<Ocr, OcrError> {
        Self::load(&Models::find().ok_or(OcrError::NoModels)?)
    }

    /// Recognise the text in an RGBA (or RGB, or grey) image, `width` × `height` pixels.
    pub fn recognize(&self, pixels: &[u8], width: u32, height: u32) -> Result<Vec<Line>, OcrError> {
        let rgb = Self::channels(pixels, width, height)?;
        let err = |e: &dyn std::fmt::Display| OcrError::Recognize(e.to_string());
        let source = ocrs::ImageSource::from_bytes(&rgb, (width, height)).map_err(|e| err(&e))?;
        let input = self.engine.prepare_input(source).map_err(|e| err(&e))?;
        let found = self.detect(&rgb, width, height)?;
        let lines = self.engine.find_text_lines(&input, &found);
        let read = self.engine.recognize_text(&input, &lines).map_err(|e| err(&e))?;
        use ocrs::TextItem;
        Ok(read
            .into_iter()
            .flatten()
            .map(|line| Line {
                words: line
                    .words()
                    .map(|w| {
                        let r = w.bounding_rect();
                        Word { text: w.to_string(), rect: [r.left() as f32, r.top() as f32, r.right() as f32, r.bottom() as f32] }
                    })
                    .filter(|w| !w.text.trim().is_empty())
                    .collect(),
            })
            .filter(|l| !l.words.is_empty())
            .collect())
    }
}

/// The size (width, height) of the tiles [`Ocr::detect`] searches: twice the detection model's
/// input (600 × 800), which keeps a tile's small print legible once shrunk to it, while a word
/// of a few centimetres still fits whole in one tile. Measured on phone photos of whole pages
/// in Spanish, Catalan and Ukrainian: 40–170% more words read than the whole image alone.
const TILE: (u32, u32) = (1200, 1600);
/// How far neighbouring tiles overlap: wider than most words, so each word lies whole in one.
const TILE_OVERLAP: u32 = 400;

/// Where tiles `tile` long start along an edge `len` long: overlapping by [`TILE_OVERLAP`], the
/// last one ending at the edge.
fn tile_starts(len: u32, tile: u32) -> Vec<u32> {
    let step = tile.saturating_sub(TILE_OVERLAP).max(1);
    let last = len.saturating_sub(tile);
    let mut out: Vec<u32> = (0..).map(|i: u32| i.saturating_mul(step)).take_while(|s| *s < last).collect();
    out.push(last);
    out
}

/// The part of an edge `len` long that tile `i` (of tiles `tile` long starting at `starts`)
/// owns: from the middle of its overlap with the tile before to the middle of its overlap with
/// the tile after.
fn owned(starts: &[u32], i: usize, tile: u32, len: u32) -> (f32, f32) {
    let span = |i: usize| starts.get(i).map(|s| (*s as f32, s.saturating_add(tile).min(len) as f32));
    let here = span(i).unwrap_or((0.0, len as f32));
    let from = i.checked_sub(1).and_then(span).map_or(f32::NEG_INFINITY, |before| (here.0 + before.1) / 2.0);
    let to = i.checked_add(1).and_then(span).map_or(f32::INFINITY, |next| (next.0 + here.1) / 2.0);
    (from, to)
}

/// The `[x, y, width, height]` part of an image `width` pixels wide with `channels` bytes per
/// pixel. `None` when the part is not inside the image.
fn crop(pixels: &[u8], width: u32, channels: usize, [x, y, w, h]: [u32; 4]) -> Option<Vec<u8>> {
    if x.checked_add(w)? > width {
        return None;
    }
    let row = (width as usize).checked_mul(channels)?;
    let (x, w) = ((x as usize).checked_mul(channels)?, (w as usize).checked_mul(channels)?);
    let mut out = Vec::with_capacity(w.checked_mul(h as usize)?);
    for line in y..y.checked_add(h)? {
        let start = (line as usize).checked_mul(row)?.checked_add(x)?;
        out.extend_from_slice(pixels.get(start..start.checked_add(w)?)?);
    }
    Some(out)
}

/// Merge the word boxes found in a whole image with those found in its tiles. Tile boxes repeat
/// where tiles overlap, and a word cut by a tile edge leaves a piece: a box mostly inside a
/// larger one is dropped. A box of the whole image stands for the tile boxes inside it when
/// they are about as tall as it is (a large word the tiles cut up), and is dropped when they
/// are much shorter (small print run together by the shrunken image); found by nothing in the
/// tiles, it is kept.
fn merge_detections(whole: Vec<RotatedRect>, tiled: Vec<RotatedRect>) -> Vec<RotatedRect> {
    let rect = |r: &RotatedRect| {
        let b = r.bounding_rect();
        [b.left(), b.top(), b.right(), b.bottom()]
    };
    let mut tiled: Vec<([f32; 4], RotatedRect)> = tiled.into_iter().map(|r| (rect(&r), r)).collect();
    tiled.sort_by(|a, b| area(b.0).total_cmp(&area(a.0)));
    let mut kept: Vec<([f32; 4], RotatedRect)> = Vec::new();
    for (b, r) in tiled {
        if !kept.iter().any(|(k, _)| mostly_inside(b, *k)) {
            kept.push((b, r));
        }
    }
    let mut out = Vec::new();
    for r in whole {
        let w = rect(&r);
        let inside: Vec<usize> = kept.iter().enumerate().filter(|(_, (k, _))| mostly_inside(*k, w)).map(|(i, _)| i).collect();
        let tall = inside.iter().filter_map(|i| kept.get(*i)).all(|(k, _)| k[3] - k[1] >= (w[3] - w[1]) * 0.6);
        if inside.is_empty() {
            out.push(r);
        } else if tall {
            // Highest index first, so the others stay where they are.
            for i in inside.into_iter().rev() {
                kept.remove(i);
            }
            out.push(r);
        }
    }
    out.extend(kept.into_iter().map(|(_, r)| r));
    out
}

/// The area of a box `[left, top, right, bottom]`; 0 for an empty or inverted one.
fn area(b: [f32; 4]) -> f32 {
    let a = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    if a.is_finite() { a } else { 0.0 }
}

/// Whether at least 80% of box `a` lies inside box `b`.
fn mostly_inside(a: [f32; 4], b: [f32; 4]) -> bool {
    let overlap = area([a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]);
    let own = area(a);
    own > 0.0 && overlap >= own * 0.8
}

/// A quarter turn applied to an image before reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    None,
    /// A quarter turn clockwise: the left edge becomes the top.
    Clockwise,
    /// A quarter turn counter-clockwise: the right edge becomes the top.
    CounterClockwise,
}

impl Turn {
    /// The point of the image as given (`width` × `height` pixels) that the point (`x`, `y`)
    /// of the turned image shows.
    pub fn to_source(self, x: f32, y: f32, width: u32, height: u32) -> (f32, f32) {
        match self {
            Turn::None => (x, y),
            Turn::Clockwise => (y, height as f32 - x),
            Turn::CounterClockwise => (width as f32 - y, x),
        }
    }
}

/// `pixels` (`width` × `height`, any number of bytes per pixel) turned by `turn`, with its new
/// width and height. `None` when the buffer does not hold whole pixels of that size.
pub fn turn_image(pixels: &[u8], width: u32, height: u32, turn: Turn) -> Option<(Vec<u8>, u32, u32)> {
    let (w, h) = (width as usize, height as usize);
    let count = w.checked_mul(h).filter(|n| *n > 0)?;
    if !pixels.len().is_multiple_of(count) {
        return None;
    }
    let channels = pixels.len() / count;
    let (tw, th) = match turn {
        Turn::None => return Some((pixels.to_vec(), width, height)),
        Turn::Clockwise | Turn::CounterClockwise => (h, w),
    };
    let mut out = Vec::with_capacity(pixels.len());
    for y in 0..th {
        for x in 0..tw {
            // The source pixel whose centre this one shows.
            let (sx, sy) = match turn {
                Turn::Clockwise => (y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let at = (sy * w + sx) * channels;
            out.extend_from_slice(pixels.get(at..at + channels)?);
        }
    }
    Some((out, height, width))
}

/// Whether most of `boxes` (`[left, top, right, bottom]`) are clearly taller than wide, as the
/// words of text turned sideways are. Single letters and digits, about square, count neither way.
fn mostly_tall(boxes: &[[f32; 4]]) -> bool {
    let (mut tall, mut wide) = (0usize, 0usize);
    for b in boxes {
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        if !(w > 0.0 && h > 0.0) {
            continue;
        }
        if h > w * 1.5 {
            tall += 1;
        } else if w > h * 1.5 {
            wide += 1;
        }
    }
    tall > wide
}

/// How much text a reading holds: its characters, letters and digits only.
fn text_amount(lines: &[Line]) -> usize {
    lines.iter().flat_map(|l| &l.words).map(|w| w.text.chars().filter(|c| c.is_alphanumeric()).count()).sum()
}

/// The least mean score a word read by the PaddleOCR models needs to be kept.
const MIN_WORD_SCORE: f32 = 0.7;

/// How many line heights wide a gap between word boxes must be to read the boxes either side
/// apart. Measured on phone photos of flyers and of a textbook: inside a run of text the
/// detector leaves gaps of up to about 2 line heights (spaces, and letters or words its boxes
/// missed, which only read whole with their neighbours), while columns and table cells sit
/// 3 or more apart.
const SEGMENT_GAP: f32 = 3.0;

/// Split a line's word boxes (`[left, top, right, bottom]`) into runs, left to right, wherever
/// the gap between neighbours is wider than `min_gap`.
fn segments(rects: &[[f32; 4]], min_gap: f32) -> Vec<Vec<[f32; 4]>> {
    let mut sorted: Vec<[f32; 4]> = rects.iter().copied().filter(|r| r.iter().all(|v| v.is_finite())).collect();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut out: Vec<Vec<[f32; 4]>> = Vec::new();
    let mut right = f32::NEG_INFINITY;
    for r in sorted {
        match out.last_mut() {
            Some(run) if r[0] - right <= min_gap => run.push(r),
            _ => out.push(vec![r]),
        }
        right = right.max(r[2]);
    }
    out
}

/// Whether the line in `a` is only a mark of the larger line in `b` that detection found on its
/// own (the dot of a Ukrainian "і" or "ї", an accent): much smaller, over or under it and
/// touching it. Reading it would add a stray letter. Boxes are `[left, top, right, bottom]`.
fn mark_of(a: [f32; 4], b: [f32; 4]) -> bool {
    let (ha, hb) = (a[3] - a[1], b[3] - b[1]);
    let wa = a[2] - a[0];
    if !(ha > 0.0 && hb > 0.0 && wa > 0.0) || ha * 2.0 > hb {
        return false;
    }
    let overlap = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let gap = (a[1] - b[3]).max(b[1] - a[3]).max(0.0);
    overlap > wa * 0.5 && gap < hb * 0.5
}

/// A word placed on a page, in PDF user space: the bottom-left corner of its box, and the
/// vectors along its bottom edge (`across`, left → right as read) and its left edge (`up`).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedWord {
    pub text: String,
    pub origin: [f64; 2],
    pub across: [f64; 2],
    pub up: [f64; 2],
}

impl PlacedWord {
    /// Place a pixel-space word with `to_user`, which maps an image pixel (x right, y down) to
    /// user space.
    pub fn place(word: &Word, to_user: impl Fn(f32, f32) -> [f64; 2]) -> PlacedWord {
        let [l, t, r, b] = word.rect;
        let o = to_user(l, b);
        let br = to_user(r, b);
        let tl = to_user(l, t);
        PlacedWord { text: word.text.clone(), origin: o, across: [br[0] - o[0], br[1] - o[1]], up: [tl[0] - o[0], tl[1] - o[1]] }
    }
}

/// Helvetica's ascender and descender (em fractions): a word box spans about this much.
const BOX_EM: f64 = 0.93;
const DESCENT: f64 = 0.21;
/// Helvetica's cap height and x-height (em fractions).
const CAP_HEIGHT: f64 = 0.718;
const X_HEIGHT: f64 = 0.523;

/// How far `text`'s ink reaches above and below the baseline, in em fractions: a word box is
/// tight around the ink, so "DT" spans the cap height only, "sun" the x-height, and "gap" reaches
/// down to the descender. Text without letters or digits keeps the full ascender-descender span.
fn ink_extent(text: &str) -> (f64, f64) {
    if !text.chars().any(char::is_alphanumeric) {
        return (BOX_EM - DESCENT, -DESCENT);
    }
    let tall = text
        .chars()
        .any(|c| c.is_uppercase() || c.is_ascii_digit() || "bdfhijklt!?/\\()[]{}|'\"#$%&@".contains(c) || (c.is_alphabetic() && !c.is_ascii()));
    let low = text.chars().any(|c| "gjpqy()[]{}|_".contains(c));
    (if tall { CAP_HEIGHT } else { X_HEIGHT }, if low { -DESCENT } else { 0.0 })
}

fn num(v: f64) -> String {
    let s = format!("{:.4}", if v.abs() < 1e-9 { 0.0 } else { v });
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// A font embedded in the page for the words Helvetica can't hold (Cyrillic, and Latin letters
/// outside Western Europe): its resource name, the two-byte code of each character it has, and
/// its advance widths. Tinta Office fork.
pub struct UnicodeFont<'a> {
    pub resource: &'a str,
    pub codes: &'a std::collections::HashMap<char, u16>,
    pub width: fn(&str, f64) -> f64,
}

/// Page content that writes `words` as invisible text in `/PCHelv` (standard Helvetica,
/// WinAnsiEncoding), each word stretched to its box so selection and search highlight the
/// right place. Marked content `/OCR` so it can be told apart from the page's own text.
pub fn text_layer(words: &[PlacedWord]) -> Vec<u8> {
    text_layer_with(words, None)
}

/// [`text_layer`], writing the words WinAnsiEncoding can't hold in `unicode` when there is one
/// (otherwise they are written in Helvetica, where those characters become `?`).
pub fn text_layer_with(words: &[PlacedWord], unicode: Option<&UnicodeFont>) -> Vec<u8> {
    // `3 Tr` and the font are graphics state, which `ET` doesn't reset: without `q … Q` they
    // would stay set for every later content stream on the page, so text added after OCR (a
    // header or footer) drew invisibly (#3029).
    let mut out = b"/OCR BMC\nq\nBT\n3 Tr\n/PCHelv 1 Tf\n".to_vec();
    let mut in_unicode = false;
    for w in words {
        // The word's codes in the Unicode font, when Helvetica can't hold it and the font can.
        let codes: Option<Vec<u16>> = unicode
            .filter(|_| pdfcraft_fonts::first_non_win_ansi(&w.text).is_some())
            .and_then(|f| w.text.chars().map(|c| f.codes.get(&c).copied()).collect());
        let width = match (&codes, unicode) {
            (Some(_), Some(f)) => (f.width)(&w.text, 1.0),
            _ => helvetica_width(&w.text, 1.0),
        };
        let height = w.up[0].hypot(w.up[1]);
        if !(width > 0.0 && width.is_finite()) || !(height > 0.0 && height.is_finite()) {
            continue;
        }
        match (&codes, unicode) {
            (Some(_), Some(f)) if !in_unicode => {
                out.extend_from_slice(format!("/{} 1 Tf\n", f.resource).as_bytes());
                in_unicode = true;
            }
            (None, _) if in_unicode => {
                out.extend_from_slice(b"/PCHelv 1 Tf\n");
                in_unicode = false;
            }
            _ => {}
        }
        // The em square: the word's own ink (cap height or x-height, descenders or not) spans the
        // box, so the text is as tall as the scanned word; its width is stretched to fit the word.
        let (top, bottom) = ink_extent(&w.text);
        let em = [w.up[0] / (top - bottom), w.up[1] / (top - bottom)];
        let ax = [w.across[0] / width, w.across[1] / width];
        let o = [w.origin[0] - em[0] * bottom, w.origin[1] - em[1] * bottom];
        let m = [ax[0], ax[1], em[0], em[1], o[0], o[1]];
        let nums: Vec<String> = m.iter().map(|v| num(*v)).collect();
        out.extend_from_slice(nums.join(" ").as_bytes());
        out.extend_from_slice(b" Tm ");
        match &codes {
            Some(codes) => {
                out.push(b'<');
                for c in codes {
                    out.extend_from_slice(format!("{c:04X}").as_bytes());
                }
                out.push(b'>');
            }
            None => out.extend(pdfcraft_fonts::literal(&pdfcraft_fonts::win_ansi(&w.text))),
        }
        out.extend_from_slice(b" Tj\n");
    }
    out.extend_from_slice(b"ET\nQ\nEMC\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dot_detected_apart_from_its_line_is_not_a_line() {
        let line = [100.0, 200.0, 400.0, 230.0];
        assert!(mark_of([300.0, 190.0, 306.0, 197.0], line), "the dot of an і just above");
        assert!(!mark_of([300.0, 150.0, 306.0, 157.0], line), "too far above: its own text");
        assert!(!mark_of([100.0, 240.0, 400.0, 270.0], line), "the next line");
        assert!(!mark_of([500.0, 190.0, 506.0, 197.0], line), "beside it, not over it");
        assert!(!mark_of(line, line), "the line itself");
        assert!(!mark_of([0.0; 4], line), "an empty box");
    }

    #[test]
    fn a_line_is_read_apart_where_its_boxes_are_far_apart() {
        // Words spaced 8 px, then a 60 px gutter (a second column), given out of order.
        let (a, b, c) = ([10.0, 0.0, 50.0, 20.0], [58.0, 0.0, 90.0, 20.0], [150.0, 2.0, 200.0, 20.0]);
        assert_eq!(segments(&[c, a, b], 30.0), [vec![a, b], vec![c]]);
        assert_eq!(segments(&[a, b], 30.0), [vec![a, b]], "a space between words is no gutter");
        assert_eq!(segments(&[a, c], 100.0), [vec![a, c]], "a gap as wide as the least is kept");
        // A box inside a wider one before it is no gap.
        let wide = [0.0, 0.0, 300.0, 20.0];
        assert_eq!(segments(&[wide, c], 30.0), [vec![wide, c]]);
        assert!(segments(&[], 30.0).is_empty());
        assert!(segments(&[[f32::NAN, 0.0, 1.0, 1.0]], 30.0).is_empty(), "boxes that are not numbers are dropped");
    }

    #[test]
    fn places_words_through_the_mapping() {
        // 2 pixels per point, page 612 × 792, y flipped.
        let to_user = |x: f32, y: f32| [x as f64 / 2.0, 792.0 - y as f64 / 2.0];
        let w = PlacedWord::place(&Word { text: "Hello".into(), rect: [100.0, 200.0, 300.0, 240.0] }, to_user);
        assert_eq!(w.origin, [50.0, 672.0]);
        assert_eq!(w.across, [100.0, 0.0]);
        assert_eq!(w.up, [0.0, 20.0]);
    }

    /// The text layer is Helvetica in WinAnsiEncoding: every character the model can recognise
    /// must be one it can hold, or recognised text would be saved as `?`. A model with a wider
    /// alphabet (Chinese, for example) needs a text layer in another font first.
    #[test]
    fn the_model_alphabet_fits_the_text_layer_font() {
        assert_eq!(pdfcraft_fonts::first_undrawable(ALPHABET), None);
    }

    #[test]
    fn text_layer_is_invisible_text_fitted_to_each_box() {
        let w = PlacedWord { text: "Hi".into(), origin: [10.0, 20.0], across: [30.0, 0.0], up: [0.0, 9.3] };
        let s = String::from_utf8(text_layer(&[w])).unwrap();
        assert!(s.contains("3 Tr") && s.contains("/PCHelv 1 Tf"), "{s}");
        let width = helvetica_width("Hi", 1.0);
        // "Hi" has no descender: its box spans the cap height, from the baseline up.
        assert!(s.contains(&format!("{} 0 0 {} 10 20 Tm (Hi) Tj", num(30.0 / width), num(9.3 / CAP_HEIGHT))), "{s}");
        assert!(s.starts_with("/OCR BMC") && s.ends_with("EMC\n"));
    }

    /// #3029: the layer's invisible rendering mode and font end with the layer. Every operator
    /// that sets them sits inside a `q … Q` pair that the layer closes itself, so content drawn
    /// after it on the page starts from the graphics state it had before.
    #[test]
    fn the_layer_restores_the_graphics_state_it_changes() {
        let w = PlacedWord { text: "Hi".into(), origin: [10.0, 20.0], across: [30.0, 0.0], up: [0.0, 9.3] };
        let s = String::from_utf8(text_layer(&[w])).unwrap();
        let mut depth = 0_i32;
        for token in s.split_ascii_whitespace() {
            match token {
                "q" => depth += 1,
                "Q" => depth -= 1,
                "Tr" | "Tf" | "Tm" => assert!(depth > 0, "`{token}` outside q … Q in {s:?}"),
                _ => {}
            }
            assert!(depth >= 0, "unbalanced Q in {s:?}");
        }
        assert_eq!(depth, 0, "the layer leaves a q open: {s:?}");
    }

    /// A word box is tight around the ink, so the text's size and baseline come from the letters:
    /// capitals, lowercase without ascenders, descenders.
    #[test]
    fn the_text_is_as_tall_as_the_scanned_word() {
        assert_eq!(ink_extent("DT"), (CAP_HEIGHT, 0.0));
        assert_eq!(ink_extent("402337"), (CAP_HEIGHT, 0.0));
        assert_eq!(ink_extent("sun"), (X_HEIGHT, 0.0));
        assert_eq!(ink_extent("gap"), (X_HEIGHT, -DESCENT));
        assert_eq!(ink_extent("Gallus.Ze@camtel.cm"), (CAP_HEIGHT, 0.0));
        assert_eq!(ink_extent("Corporation"), (CAP_HEIGHT, -DESCENT));
        assert_eq!(ink_extent("--"), (BOX_EM - DESCENT, -DESCENT));
        let w = PlacedWord { text: "gap".into(), origin: [0.0, 100.0], across: [20.0, 0.0], up: [0.0, 7.3] };
        let s = String::from_utf8(text_layer(&[w])).unwrap();
        let em = 7.3 / (X_HEIGHT + DESCENT);
        assert!(s.contains(&format!(" 0 0 {} 0 {} Tm (gap) Tj", num(em), num(100.0 + em * DESCENT))), "{s}");
    }

    #[test]
    fn rotated_pages_keep_the_reading_direction() {
        // A page turned 90°: image x runs up the page.
        let to_user = |x: f32, y: f32| [y as f64, x as f64];
        let w = PlacedWord::place(&Word { text: "Up".into(), rect: [0.0, 0.0, 50.0, 10.0] }, to_user);
        assert_eq!(w.across, [0.0, 50.0]);
        assert_eq!(w.up, [-10.0, 0.0]);
    }

    /// The current ONNX pair wins when an installation also retains both legacy models.
    #[test]
    fn model_discovery_prefers_the_complete_onnx_pair() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["text-detection.onnx", "text-recognition.onnx"] {
            std::fs::write(dir.path().join(file), b"model").unwrap();
        }
        let expected = Some(Models { detection: dir.path().join("text-detection.onnx"), recognition: dir.path().join("text-recognition.onnx") });
        assert_eq!(Models::in_dir(dir.path()), expected, "a complete ONNX pair is sufficient");
        for file in ["text-detection.rten", "text-recognition.rten"] {
            std::fs::write(dir.path().join(file), b"model").unwrap();
        }
        assert_eq!(Models::in_dir(dir.path()), expected, "the complete ONNX pair takes precedence over legacy models");
    }

    /// An interrupted ONNX update still permits an intact legacy pair in the same directory.
    #[test]
    fn model_discovery_falls_back_to_legacy_when_the_onnx_pair_is_incomplete() {
        for current in ["text-detection.onnx", "text-recognition.onnx"] {
            let dir = tempfile::tempdir().unwrap();
            for file in [current, "text-detection.rten", "text-recognition.rten"] {
                std::fs::write(dir.path().join(file), b"model").unwrap();
            }
            assert_eq!(
                Models::in_dir(dir.path()),
                Some(Models { detection: dir.path().join("text-detection.rten"), recognition: dir.path().join("text-recognition.rten") }),
                "{current} alone must not hide a complete legacy pair"
            );
        }
    }

    /// Detection and recognition must come from one complete format pair, never a mixed update.
    #[test]
    fn model_discovery_refuses_mixed_format_pairs() {
        for pair in [["text-detection.onnx", "text-recognition.rten"], ["text-detection.rten", "text-recognition.onnx"]] {
            let dir = tempfile::tempdir().unwrap();
            for file in pair {
                std::fs::write(dir.path().join(file), b"model").unwrap();
            }
            assert_eq!(Models::in_dir(dir.path()), None, "{pair:?} is not a complete model pair");
        }
    }

    /// Existing source builds and PDFCRAFT_MODELS overrides retain their complete RTEN pair.
    #[test]
    fn model_discovery_keeps_legacy_only_installations() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Models::in_dir(dir.path()), None);
        std::fs::write(dir.path().join("text-detection.rten"), b"model").unwrap();
        assert_eq!(Models::in_dir(dir.path()), None, "one legacy model is incomplete");
        std::fs::write(dir.path().join("text-recognition.rten"), b"model").unwrap();
        assert_eq!(
            Models::in_dir(dir.path()),
            Some(Models { detection: dir.path().join("text-detection.rten"), recognition: dir.path().join("text-recognition.rten") })
        );
    }

    /// #103: every release package's layout puts the models where the installed app looks.
    /// Mirrors packaging/{windows/package.ps1,macos/package.sh,linux/package.sh,freebsd/package.sh}.
    #[test]
    fn finds_the_models_in_every_release_package_layout() {
        let base = std::env::temp_dir().join(format!("pdfcraft-ocr-layouts-{}", std::process::id()));
        let layouts = [
            ("windows", "PdfCraft", "PdfCraft/models"),
            ("macos", "PdfCraft.app/Contents/MacOS", "PdfCraft.app/Contents/Resources/models"),
            ("linux-deb-rpm", "usr/bin", "usr/share/pdfcraft/models"),
            ("linux-tar", "pdfcraft-1.0.0-linux-x86_64/bin", "pdfcraft-1.0.0-linux-x86_64/share/pdfcraft/models"),
            ("appimage", "PdfCraft.AppDir/usr/bin", "PdfCraft.AppDir/usr/share/pdfcraft/models"),
            ("flatpak", "app/bin", "app/share/pdfcraft/models"),
            ("freebsd", "usr/local/bin", "usr/local/share/pdfcraft/models"),
        ];
        for (name, exe_dir, models_dir) in layouts {
            let root = base.join(name);
            let (exe_dir, models_dir) = (root.join(exe_dir), root.join(models_dir));
            std::fs::create_dir_all(&exe_dir).unwrap();
            std::fs::create_dir_all(&models_dir).unwrap();
            let found = || Models::dirs_beside_exe(&exe_dir).iter().find_map(|d| Models::in_dir(d));
            assert_eq!(found(), None, "{name}: no models yet");
            for file in [DETECTION_MODEL, RECOGNITION_MODEL] {
                std::fs::write(models_dir.join(file), b"model").unwrap();
            }
            let models = found().unwrap_or_else(|| panic!("{name}: models in {} not found", models_dir.display()));
            assert_eq!(models.detection.canonicalize().unwrap(), models_dir.join(DETECTION_MODEL).canonicalize().unwrap(), "{name}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The message tells end users what to do; release builds have no cargo.
    #[test]
    fn the_alphabet_is_the_trained_one() {
        let chars: Vec<char> = ALPHABET.chars().collect();
        assert_eq!(chars.len(), 96);
        // The euro sign follows the ASCII punctuation (class 44; 0 is the CTC blank), not a second E.
        assert_eq!(chars.iter().position(|&c| c == '€'), Some(43));
        assert_eq!(chars.iter().filter(|&&c| c == 'E').count(), 1);
    }

    #[test]
    fn missing_models_message_is_for_end_users() {
        let message = OcrError::NoModels.to_string();
        assert!(!message.contains("cargo") && message.contains("PDFCRAFT_MODELS"), "{message}");
    }

    /// End to end on rendered text, when the models are installed (`cargo xtask models`).
    #[test]
    fn reads_rendered_text() {
        let Some(models) = Models::find() else {
            eprintln!("skipped: OCR models not installed");
            return;
        };
        let ocr = Ocr::load(&models).unwrap();
        let (w, h, px) = test_image::hello();
        let lines = ocr.recognize(&px, w, h).unwrap();
        let text: Vec<String> = lines.iter().map(Line::text).collect();
        assert!(text.iter().any(|t| t.contains("HELL") && t.contains("WOR")), "{text:?}");
        let word = lines.iter().flat_map(|l| &l.words).find(|w| w.text.contains("HELL")).unwrap();
        assert!(word.rect[0] >= 10.0 && word.rect[0] < 60.0, "{word:?}");
    }

    #[test]
    fn columns_on_one_line_are_read_as_separate_words() {
        let Some(models) = Models::find() else {
            eprintln!("skipped: OCR models not installed");
            return;
        };
        let ocr = Ocr::load(&models).unwrap();
        // Two columns, a gutter of eight letters between them.
        let (w, h, px) = test_image::text("HELLO        WORLD");
        let lines = match ocr.recognize_language(&px, w, h, "es") {
            Err(OcrError::NoModels) => {
                eprintln!("skipped: the Latin model is not installed");
                return;
            }
            other => other.unwrap(),
        };
        let words: Vec<&str> = lines.iter().flat_map(|l| &l.words).map(|w| w.text.as_str()).collect();
        assert!(words.iter().any(|w| w.starts_with("HELL")) && words.iter().any(|w| w.starts_with("WOR")), "{words:?}");
        assert!(!words.iter().any(|w| w.contains("HELL") && w.contains("WOR")), "the columns ran together: {words:?}");
        let hello = lines.iter().flat_map(|l| &l.words).find(|w| w.text.starts_with("HELL")).unwrap();
        assert!(hello.rect[2] < (40 + 5 * 24) as f32 + 10.0, "the first column's box ends at its own text: {hello:?}");
    }

    #[test]
    fn a_turned_image_maps_back_onto_the_image_as_given() {
        // 3 × 2 pixels, one byte each, numbered in reading order.
        let (w, h) = (3u32, 2u32);
        let px: Vec<u8> = (0..6).collect();
        for turn in [Turn::None, Turn::Clockwise, Turn::CounterClockwise] {
            let (out, tw, th) = turn_image(&px, w, h, turn).unwrap();
            assert_eq!(out.len(), px.len());
            for y in 0..th {
                for x in 0..tw {
                    // The centre of each turned pixel maps to the centre of the pixel it copies.
                    let (sx, sy) = turn.to_source(x as f32 + 0.5, y as f32 + 0.5, w, h);
                    let source = (sy - 0.5) as u32 * w + (sx - 0.5) as u32;
                    assert_eq!(u32::from(out[(y * tw + x) as usize]), source, "{turn:?} at {x},{y}");
                }
            }
        }
        let (cw, tw, th) = turn_image(&px, w, h, Turn::Clockwise).unwrap();
        assert_eq!((tw, th), (2, 3));
        assert_eq!(cw, [3, 0, 4, 1, 5, 2], "the left column becomes the top row");
        assert!(turn_image(&px[..5], w, h, Turn::Clockwise).is_none(), "not whole pixels");
        assert!(turn_image(&[], 0, 0, Turn::Clockwise).is_none(), "no pixels");
    }

    #[test]
    fn text_is_sideways_when_most_word_boxes_stand_up() {
        let (wide, tall, square) = ([0.0, 0.0, 60.0, 20.0], [0.0, 0.0, 20.0, 60.0], [0.0, 0.0, 20.0, 22.0]);
        assert!(mostly_tall(&[tall, tall, wide, square, square]));
        assert!(!mostly_tall(&[tall, wide, wide]));
        assert!(!mostly_tall(&[square, square]), "letters alone say nothing");
        assert!(!mostly_tall(&[]));
        assert!(!mostly_tall(&[[f32::NAN, 0.0, 1.0, 9.0]]));
    }

    #[test]
    fn sideways_text_is_read_turned_upright() {
        let Some(models) = Models::find() else {
            eprintln!("skipped: OCR models not installed");
            return;
        };
        let ocr = Ocr::load(&models).unwrap();
        // "HELLO WORLD" turned a quarter counter-clockwise: it reads from the bottom up.
        let (w, h, px) = test_image::hello();
        let (px, w, h) = turn_image(&px, w, h, Turn::CounterClockwise).unwrap();
        let (lines, turn) = match ocr.recognize_upright(&px, w, h, "es") {
            Err(OcrError::NoModels) => {
                eprintln!("skipped: the Latin model is not installed");
                return;
            }
            other => other.unwrap(),
        };
        assert_eq!(turn, Turn::Clockwise, "{lines:?}");
        let words: Vec<&Word> = lines.iter().flat_map(|l| &l.words).collect();
        let hello = words.iter().find(|w| w.text.starts_with("HELL")).unwrap_or_else(|| panic!("{words:?}"));
        // Mapped back, HELLO stands in the left column of the sideways image, near its bottom.
        let (a, b) = (turn.to_source(hello.rect[0], hello.rect[1], w, h), turn.to_source(hello.rect[2], hello.rect[3], w, h));
        let (left, right, top, bottom) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        assert!(left >= 20.0 && right <= 100.0, "{left}..{right}");
        assert!(top > h as f32 / 2.0 && bottom <= h as f32, "{top}..{bottom} of {h}");
    }

    #[test]
    fn tiles_cover_the_edge_and_split_it_between_them() {
        assert_eq!(tile_starts(1000, 1200), [0], "one tile for a short edge");
        assert_eq!(tile_starts(1200, 1200), [0]);
        assert_eq!(tile_starts(3000, 1200), [0, 800, 1600, 1800], "the last tile ends at the edge");
        assert_eq!(tile_starts(0, 1200), [0]);
        for len in [1201u32, 2000, 3000, 4000, 4321] {
            let starts = tile_starts(len, 1200);
            let shares: Vec<(f32, f32)> = (0..starts.len()).map(|i| owned(&starts, i, 1200, len)).collect();
            assert_eq!(shares.first().map(|s| s.0), Some(f32::NEG_INFINITY));
            assert_eq!(shares.last().map(|s| s.1), Some(f32::INFINITY));
            for (i, pair) in shares.windows(2).enumerate() {
                assert_eq!(pair[0].1, pair[1].0, "no gap and no overlap between shares at {len}");
                // Each boundary lies where both tiles see the picture.
                let (next, end) = (starts[i + 1] as f32, (starts[i] + 1200).min(len) as f32);
                assert!(pair[0].1 >= next && pair[0].1 <= end, "{len}: {pair:?}");
            }
        }
    }

    #[test]
    fn a_crop_copies_the_rows_it_covers() {
        // 4 × 3 pixels of 2 bytes, numbered.
        let px: Vec<u8> = (0..24).collect();
        assert_eq!(crop(&px, 4, 2, [1, 1, 2, 2]).unwrap(), [10, 11, 12, 13, 18, 19, 20, 21]);
        assert_eq!(crop(&px, 4, 2, [0, 0, 4, 3]).unwrap(), px);
        assert!(crop(&px, 4, 2, [3, 0, 2, 1]).is_none(), "past the right edge");
        assert!(crop(&px, 4, 2, [0, 2, 1, 2]).is_none(), "past the bottom");
        assert!(crop(&px, 4, 2, [u32::MAX, 0, 2, 1]).is_none());
    }

    #[test]
    fn tiled_and_whole_detections_merge_without_repeats() {
        let rr = |[l, t, r, b]: [f32; 4]| RotatedRect::from_rect(rten_imageproc::RectF::from_tlbr(t, l, b, r));
        let boxes = |v: Vec<RotatedRect>| {
            let mut v: Vec<[i32; 4]> = v
                .iter()
                .map(|r| {
                    let b = r.bounding_rect();
                    [b.left(), b.top(), b.right(), b.bottom()].map(|v| v.round() as i32)
                })
                .collect();
            v.sort();
            v
        };
        // A word found whole in one tile and cut in the next: the piece goes.
        let word = [100.0, 100.0, 300.0, 130.0];
        assert_eq!(boxes(merge_detections(vec![], vec![rr(word), rr([200.0, 100.0, 300.0, 130.0])])), [[100, 100, 300, 130]]);
        // The same word from two tiles is kept once.
        assert_eq!(boxes(merge_detections(vec![], vec![rr(word), rr(word)])).len(), 1);
        // A large heading the tiles cut in two is taken whole from the whole image.
        let heading = [0.0, 0.0, 1000.0, 120.0];
        let halves = vec![rr([0.0, 5.0, 520.0, 115.0]), rr([480.0, 5.0, 1000.0, 115.0])];
        assert_eq!(boxes(merge_detections(vec![rr(heading)], halves)), [[0, 0, 1000, 120]]);
        // Small print the shrunken whole image ran together into one blob stays as the tiles read it.
        let lines = vec![rr([0.0, 0.0, 400.0, 20.0]), rr([0.0, 30.0, 400.0, 50.0]), rr([0.0, 60.0, 400.0, 80.0])];
        assert_eq!(boxes(merge_detections(vec![rr([0.0, 0.0, 400.0, 80.0])], lines)).len(), 3);
        // What only the whole image found is kept.
        assert_eq!(boxes(merge_detections(vec![rr(word)], vec![])), [[100, 100, 300, 130]]);
    }

    #[test]
    fn small_print_on_a_large_image_is_found() {
        const DOT: u32 = 3;
        const INK: u8 = 110;
        const PAPER: u8 = 170;
        let Some(models) = Models::find() else {
            eprintln!("skipped: OCR models not installed");
            return;
        };
        let ocr = Ocr::load(&models).unwrap();
        // "HELLO WORLD" DOT × 7 pixels high on a page photographed whole, 3000 × 4000 pixels:
        // shrunk to the detection model's 600 × 800 at once, its letters would be a speck.
        let (sw, h, small) = test_image::dots("HELLO WORLD", DOT);
        let (big_w, big_h) = (3000u32, 4000u32);
        // Grey print on grey paper, as a photo shows it.
        let mut px = vec![PAPER; (big_w * big_h * 4) as usize];
        let (x0, y0) = (1700u32, 2300u32);
        for y in 0..h {
            let from = (y * sw * 4) as usize;
            let to = (((y0 + y) * big_w + x0) * 4) as usize;
            for (dst, src) in px[to..to + (sw * 4) as usize].iter_mut().zip(&small[from..from + (sw * 4) as usize]) {
                *dst = if *src == 0 { INK } else { PAPER };
            }
        }
        let lines = ocr.recognize(&px, big_w, big_h).unwrap();
        // Found where it is (the blocky letters themselves read only roughly).
        let words: Vec<&Word> = lines.iter().flat_map(|l| &l.words).collect();
        let found = words.iter().filter(|w| {
            w.rect[0] >= x0 as f32 - 10.0
                && w.rect[2] <= (x0 + sw) as f32 + 10.0
                && w.rect[1] >= y0 as f32 - 10.0
                && w.rect[3] <= (y0 + h) as f32 + 10.0
        });
        assert!(found.map(|w| w.text.chars().filter(|c| c.is_alphabetic()).count()).sum::<usize>() >= 4, "{words:?}");
    }

    mod test_image {
        /// "HELLO WORLD" drawn with blocky 5×7 letters, 4 pixels per dot, black on white.
        pub fn hello() -> (u32, u32, Vec<u8>) {
            text("HELLO WORLD")
        }

        /// `text` (the letters of "HELLO WORLD" and spaces) drawn as [`hello`] draws it.
        pub fn text(text: &str) -> (u32, u32, Vec<u8>) {
            dots(text, 4)
        }

        /// `text` drawn with `dot` pixels per dot of its letters.
        pub fn dots(text: &str, dot: u32) -> (u32, u32, Vec<u8>) {
            const GLYPHS: &[(char, [&str; 7])] = &[
                ('H', ["X...X", "X...X", "X...X", "XXXXX", "X...X", "X...X", "X...X"]),
                ('E', ["XXXXX", "X....", "X....", "XXXX.", "X....", "X....", "XXXXX"]),
                ('L', ["X....", "X....", "X....", "X....", "X....", "X....", "XXXXX"]),
                ('O', [".XXX.", "X...X", "X...X", "X...X", "X...X", "X...X", ".XXX."]),
                ('W', ["X...X", "X...X", "X...X", "X.X.X", "X.X.X", "XX.XX", "X...X"]),
                ('R', ["XXXX.", "X...X", "X...X", "XXXX.", "X.X..", "X..X.", "X...X"]),
                ('D', ["XXXX.", "X...X", "X...X", "X...X", "X...X", "X...X", "XXXX."]),
            ];
            let (x0, y0) = (40u32, 40u32);
            let (w, h) = (x0 * 2 + text.len() as u32 * 6 * dot, y0 * 2 + 7 * dot);
            let mut px = vec![255u8; (w * h * 4) as usize];
            for (i, c) in text.chars().enumerate() {
                let Some((_, rows)) = GLYPHS.iter().find(|g| g.0 == c) else { continue };
                for (ry, row) in rows.iter().enumerate() {
                    for (rx, b) in row.bytes().enumerate() {
                        if b != b'X' {
                            continue;
                        }
                        for dy in 0..dot {
                            for dx in 0..dot {
                                let x = x0 + (i as u32 * 6 + rx as u32) * dot + dx;
                                let y = y0 + ry as u32 * dot + dy;
                                let o = ((y * w + x) * 4) as usize;
                                px[o..o + 3].copy_from_slice(&[0, 0, 0]);
                            }
                        }
                    }
                }
            }
            (w, h, px)
        }
    }
}
