//! Tinta Office fork: the reading pipeline behind the `ocrs` engine.
//!
//! The ocrs models read the Latin alphabet without accents (English). Other languages are read
//! with PaddleOCR's line models (`paddle`), which sit beside the ocrs ones: the ocrs detector
//! finds the words (also in tiles of a large photo), the lines are read in level pieces, apart
//! at columns and gutters, words too unsure to keep are left out, and a word list (`lexicon`)
//! puts right letters the models misread. [`Ocr::recognize_upright`] also reads a page turned
//! a quarter or a half, for text that runs sideways or upside down.
//!
//! Kept in its own file so upstream's changes to `ocrs.rs` and `lib.rs` merge cleanly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rten_imageproc::{BoundingRect, RotatedRect};

use crate::{Line, Models, Ocr, OcrError, Word, ink, lexicon, paddle};

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

/// What the fork's reading adds to a loaded [`Ocr`]: where the PaddleOCR models and word
/// lists are looked for, and those loaded so far.
#[derive(Default)]
pub(crate) struct Extras {
    /// Beside the detection model (empty: none, as for models loaded from bytes).
    pub(crate) dir: PathBuf,
    /// The PaddleOCR line recognisers loaded so far.
    paddle: Mutex<HashMap<paddle::Script, Arc<paddle::Paddle>>>,
    /// The word lists loaded so far, by language (`None`: not installed).
    lexicons: Mutex<HashMap<String, Option<Arc<lexicon::Lexicon>>>>,
}

impl Extras {
    pub(crate) fn beside(models: &Models) -> Extras {
        Extras { dir: models.detection.parent().map(Path::to_path_buf).unwrap_or_default(), ..Extras::default() }
    }
}

impl Ocr {
    /// The word list for `language` found beside the models ([`load_lexicon`]), loaded once.
    fn lexicon(&self, language: &str) -> Option<Arc<lexicon::Lexicon>> {
        let mut loaded = self.tinta.lexicons.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loaded.entry(language.to_owned()).or_insert_with(|| load_lexicon(&self.tinta.dir, language).map(Arc::new)).clone()
    }

    /// The PaddleOCR recogniser for `script`, loaded the first time it is asked for.
    fn paddle(&self, script: paddle::Script) -> Result<Arc<paddle::Paddle>, OcrError> {
        let mut loaded = self.tinta.paddle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(p) = loaded.get(&script) {
            return Ok(p.clone());
        }
        let (model, dict) = script.files();
        let (model, dict) = (self.tinta.dir.join(model), self.tinta.dir.join(dict));
        if !model.is_file() || !dict.is_file() {
            return Err(OcrError::NoModels);
        }
        let p = Arc::new(paddle::Paddle::load(&model, &dict)?);
        loaded.insert(script, p.clone());
        Ok(p)
    }

    /// Recognise the text in an image written in `language` (a code from [`LANGUAGES`]).
    pub fn recognize_language(&self, pixels: &[u8], width: u32, height: u32, language: &str) -> Result<Vec<Line>, OcrError> {
        self.read_language(pixels, width, height, language, true)
    }

    /// [`Ocr::recognize_language`]; with `again`, a page that shows a mark in light ink across
    /// its text ([`ink::has_light_mark`]) is also read with only its dark ink, and the reading
    /// with more text kept.
    fn read_language(&self, pixels: &[u8], width: u32, height: u32, language: &str, again: bool) -> Result<Vec<Line>, OcrError> {
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
        let marked = again && ink::has_light_mark(&rgb, width, height, channels, &boxes);
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
        let lines: Vec<&(Vec<[f32; 4]>, [f32; 4])> = lines.iter().filter(|(_, b)| !lines.iter().any(|(_, other)| mark_of(*b, *other))).collect();
        // Columns of text side by side are read apart and one after the other, as a reader
        // reads them, not line by line across the page.
        let blocks = columns(&lines.iter().map(|(rects, _)| rects.as_slice()).collect::<Vec<_>>());
        // Each line read, with where it goes: the top of its block of columns (its own top
        // outside one), its column, its own top.
        let mut out: Vec<((f32, usize, f32), Line)> = Vec::new();
        for (rects, bounds) in lines {
            let middle = (bounds[1] + bounds[3]) / 2.0;
            let block = blocks.iter().find(|b| middle >= b.top && middle <= b.bottom);
            let cuts = block.map_or(&[][..], |b| b.cuts.as_slice());
            let words = self.read_line(&reader, &rgb, (width, height, channels), rects, cuts, language)?;
            let Some(block) = block else {
                if !words.is_empty() {
                    out.push(((bounds[1], 0, bounds[1]), Line { words }));
                }
                continue;
            };
            let mut by_column: Vec<Vec<Word>> = vec![Vec::new(); cuts.len().saturating_add(1)];
            for w in words {
                let x = (w.rect[0] + w.rect[2]) / 2.0;
                if let Some(column) = by_column.get_mut(cuts.iter().filter(|c| x > **c).count()) {
                    column.push(w);
                }
            }
            for (column, words) in by_column.into_iter().enumerate().filter(|(_, w)| !w.is_empty()) {
                out.push(((block.top, column, bounds[1]), Line { words }));
            }
        }
        if !blocks.is_empty() {
            out.sort_by(|(a, _), (b, _)| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));
        }
        let lines: Vec<Line> = out.into_iter().map(|(_, line)| line).collect();
        if let Some(dark) = marked.then(|| ink::without_light_ink(&rgb, width, height, channels)).flatten() {
            let again = self.read_language(&dark, width, height, language, false)?;
            if text_amount(&again) > text_amount(&lines) {
                return Ok(again);
            }
        }
        Ok(lines)
    }

    /// Read the word boxes (`[left, top, right, bottom]`) of one line the detector found.
    fn read_line(
        &self,
        reader: &paddle::Paddle,
        rgb: &[u8],
        dims: (u32, u32, usize),
        rects: &[[f32; 4]],
        cuts: &[f32],
        language: &str,
    ) -> Result<Vec<Word>, OcrError> {
        // Boxes far apart on one line (table cells, columns, a heading beside a picture) are
        // read apart, or the model runs their words together. Each gap is read on its own too,
        // for a word the detector missed there; empty paper reads as nothing, or as words too
        // unsure to keep.
        //
        // A line photographed at a slant climbs across its box, which then holds parts of the
        // lines above and below: it is read in pieces short enough to stay level.
        let band = Band::fit(rects);
        let mut words = Vec::new();
        let pad = band.height * 0.15;
        let mut previous: Option<[f32; 4]> = None;
        for part in segments(rects, band.height * SEGMENT_GAP, cuts) {
            for piece in band.pieces(part) {
                let Some(piece_bounds) = band.around(&piece) else { continue };
                if let Some(before) = previous.filter(|b| piece_bounds[0] - b[2] > band.height * SEGMENT_GAP) {
                    // Inset by the padding, so the crop ends where the neighbouring boxes start.
                    let (left, right) = (before[2] + pad, piece_bounds[0] - pad);
                    let gap = [left, before[1].min(piece_bounds[1]), right, before[3].max(piece_bounds[3])];
                    if gap[2] > gap[0] {
                        words.extend(self.read_segment(reader, rgb, dims, Vec::new(), gap, language)?);
                    }
                }
                previous = Some(previous.map_or(piece_bounds, |b| if piece_bounds[2] > b[2] { piece_bounds } else { b }));
                words.extend(self.read_segment(reader, rgb, dims, piece, piece_bounds, language)?);
            }
        }
        Ok(words)
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
        if let Some(lex) = self.lexicon(language) {
            for t in texts.iter_mut() {
                if let Some(fixed) = lexicon::correct(t, &lex, language) {
                    *t = fixed;
                }
            }
        }
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
        let scores: Vec<f32> = boxed.iter().map(|(s, _)| s.score).collect();
        Ok(boxed
            .into_iter()
            .zip(kept(&scores))
            .filter(|(_, keep)| *keep)
            .map(|((s, rect), _)| {
                let confidence = s.score * 100.0;
                Word::new(s.text, rect, "ocrs").with_confidence(confidence)
            })
            .collect())
    }

    /// [`Ocr::recognize_language`] for a page that may hold its text sideways (a book
    /// photographed across, a landscape table on a portrait page): when most word boxes found
    /// are taller than wide, the image is also read turned a quarter each way, and the reading
    /// with the most text kept. Returns the lines in the turned image's pixels and the turn,
    /// whose [`Turn::to_source`] maps them back.
    pub fn recognize_upright(&self, pixels: &[u8], width: u32, height: u32, language: &str) -> Result<(Vec<Line>, Turn), OcrError> {
        let lines = self.recognize_language(pixels, width, height, language)?;
        let boxes = self.boxes(pixels, width, height)?;
        let words = lines.iter().map(|l| l.words.len()).sum::<usize>();
        let turns: &[Turn] = if mostly_tall(&boxes) {
            &[Turn::Clockwise, Turn::CounterClockwise]
        } else if words.saturating_mul(UPSIDE_DOWN_WORDS) < boxes.len() {
            &[Turn::Half]
        } else {
            &[]
        };
        let mut best = (lines, Turn::None);
        for turn in turns {
            let Some((turned, w, h)) = turn_image(pixels, width, height, *turn) else { continue };
            let read = self.recognize_language(&turned, w, h, language)?;
            if text_amount(&read) > text_amount(&best.0) {
                best = (read, *turn);
            }
        }
        Ok(best)
    }

    /// Whether most of the words detected in the image are taller than wide.
    /// The word boxes (`[left, top, right, bottom]`) the detector finds in an image.
    fn boxes(&self, pixels: &[u8], width: u32, height: u32) -> Result<Vec<[f32; 4]>, OcrError> {
        let rgb = Self::channels(pixels, width, height)?;
        let found = self.detect(&rgb, width, height)?;
        Ok(found
            .iter()
            .map(|r| {
                let b = r.bounding_rect();
                [b.left(), b.top(), b.right(), b.bottom()]
            })
            .collect())
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
        if pixels.is_empty() {
            return Err(OcrError::EmptyImage);
        }
        // The sides come from documents: capped before anything is sized by them.
        crate::ocrs::checked_len(width, height)?;
        Ok(if pixels.len() == (width as usize).saturating_mul(height as usize).saturating_mul(4) {
            pixels.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect::<Vec<u8>>().into()
        } else {
            pixels.into()
        })
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
                        Word::new(w.to_string(), [r.left() as f32, r.top() as f32, r.right() as f32, r.bottom() as f32], "ocrs")
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
/// of a few centimetres still fits whole in one tile. Measured on 25 phone photos of whole
/// pages in Spanish, Catalan and Ukrainian: 19% more words read than the whole image alone, and
/// up to 6× more on a page of dense small print.
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
/// larger one is dropped. A box of the whole image stands for the tile boxes it overlaps when
/// they all lie inside it and are about as tall as it is (a large word the tiles cut up), and
/// is dropped otherwise (small print run together by the shrunken image, or a line whose words
/// the tiles found reaching past it, which would be read twice); found by nothing in the tiles,
/// it is kept.
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
        let touching: Vec<usize> = kept.iter().enumerate().filter(|(_, (k, _))| overlaps(*k, w)).map(|(i, _)| i).collect();
        if touching.is_empty() {
            out.push(r);
        } else if touching.iter().filter_map(|i| kept.get(*i)).all(|(k, _)| mostly_inside(*k, w) && k[3] - k[1] >= (w[3] - w[1]) * 0.6) {
            // Highest index first, so the others stay where they are.
            for i in touching.into_iter().rev() {
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

/// Whether boxes `a` and `b` share more than a sliver: a tenth of the smaller one.
fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    let overlap = area([a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]);
    overlap > 0.0 && overlap > area(a).min(area(b)) * 0.1
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
    /// Half a turn: the bottom edge becomes the top.
    Half,
}

impl Turn {
    /// The point of the image as given (`width` × `height` pixels) that the point (`x`, `y`)
    /// of the turned image shows.
    pub fn to_source(self, x: f32, y: f32, width: u32, height: u32) -> (f32, f32) {
        match self {
            Turn::None => (x, y),
            Turn::Clockwise => (y, height as f32 - x),
            Turn::CounterClockwise => (width as f32 - y, x),
            Turn::Half => (width as f32 - x, height as f32 - y),
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
        Turn::Half => (w, h),
    };
    let mut out = Vec::with_capacity(pixels.len());
    for y in 0..th {
        for x in 0..tw {
            // The source pixel whose centre this one shows.
            let (sx, sy) = match turn {
                Turn::Clockwise => (y, h - 1 - x),
                Turn::Half => (w - 1 - x, h - 1 - y),
                _ => (w - 1 - y, x),
            };
            let at = (sy * w + sx) * channels;
            out.extend_from_slice(pixels.get(at..at + channels)?);
        }
    }
    Some(match turn {
        Turn::Half => (out, width, height),
        _ => (out, height, width),
    })
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
pub fn text_amount(lines: &[Line]) -> usize {
    lines.iter().flat_map(|l| &l.words).map(|w| w.text.chars().filter(|c| c.is_alphanumeric()).count()).sum()
}

/// Languages with a word list that corrects what the models misread, and the Hunspell
/// dictionary's file name (beside the models; `cargo xtask models` fetches it).
const LEXICONS: &[(&str, &str)] = &[("uk", "uk_UA"), ("es", "es_ES")];
/// A page whose words the reader keeps fewer than one in this many of the boxes the detector
/// found is also read upside down: upside-down letters are found as text, but read as words too
/// unsure to keep. Measured on phone photos: upright pages keep one word in 1–4 boxes (a flyer
/// full of pictures the most), the same pages upside down one in 30 or fewer.
const UPSIDE_DOWN_WORDS: usize = 10;
/// The largest dictionary file read: a Hunspell dictionary is a few megabytes.
const MAX_LEXICON_BYTES: u64 = 64 << 20;

/// The word list in `dir` that puts right the letters the models misread in `language`: the
/// Hunspell dictionary `<name>.aff` + `<name>.dic` named in [`LEXICONS`]. `None` when the
/// language has none, or its files are missing, too large or empty.
fn load_lexicon(dir: &Path, language: &str) -> Option<lexicon::Lexicon> {
    let name = LEXICONS.iter().find(|(l, _)| *l == language).map(|(_, n)| *n)?;
    let read = |ext: &str| {
        let path = dir.join(format!("{name}.{ext}"));
        let size = std::fs::metadata(&path).ok()?.len();
        (size <= MAX_LEXICON_BYTES).then(|| std::fs::read_to_string(&path).ok()).flatten()
    };
    let lex = lexicon::Lexicon::parse(&read("aff")?, &read("dic")?);
    (!lex.is_empty()).then_some(lex)
}

/// The least mean score a word read by the PaddleOCR models needs to be kept.
const MIN_WORD_SCORE: f32 = 0.7;
/// The least a word needs between two words sure enough to keep: a short word in running text
/// ("і", "a", "y") is read from a letter or two, so its mean swings more than a long word's,
/// while junk read from a picture or a stain rarely sits between sure words.
const MIN_WORD_SCORE_IN_TEXT: f32 = 0.4;

/// Which words of a run, scored `scores` in reading order, to keep: those sure enough, and
/// those less sure between two that are.
fn kept(scores: &[f32]) -> Vec<bool> {
    let sure = |i: Option<usize>| i.and_then(|i| scores.get(i)).is_some_and(|s| *s >= MIN_WORD_SCORE);
    (0..scores.len())
        .map(|i| {
            let s = scores.get(i).copied().unwrap_or(0.0);
            s >= MIN_WORD_SCORE || (s >= MIN_WORD_SCORE_IN_TEXT && sure(i.checked_sub(1)) && sure(i.checked_add(1)))
        })
        .collect()
}

/// How many line heights wide a gap between word boxes must be to read the boxes either side
/// apart. Measured on phone photos of flyers and of a textbook: inside a run of text the
/// detector leaves gaps of up to about 2 line heights (spaces, and letters or words its boxes
/// missed, which only read whole with their neighbours), while columns and table cells sit
/// 3 or more apart.
const SEGMENT_GAP: f32 = 3.0;

/// Split a line's word boxes (`[left, top, right, bottom]`) into runs, left to right, wherever
/// the gap between neighbours is wider than `min_gap`, and at every x of `cuts` (the gutters
/// [`columns`] found) between two boxes' centres.
fn segments(rects: &[[f32; 4]], min_gap: f32, cuts: &[f32]) -> Vec<Vec<[f32; 4]>> {
    let mut sorted: Vec<[f32; 4]> = rects.iter().copied().filter(|r| r.iter().all(|v| v.is_finite())).collect();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut out: Vec<Vec<[f32; 4]>> = Vec::new();
    let mut right = f32::NEG_INFINITY;
    let mut centre = f32::NEG_INFINITY;
    for r in sorted {
        let mid = (r[0] + r[2]) / 2.0;
        match out.last_mut() {
            Some(run) if r[0] - right <= min_gap && !cuts.iter().any(|c| centre < *c && *c <= mid) => run.push(r),
            _ => out.push(vec![r]),
        }
        right = right.max(r[2]);
        centre = mid;
    }
    out
}

/// How wide a gap must be, in line heights, to be a gutter between columns of text when it
/// sits at the same place on line after line. Spaces between words are about a third of a
/// line height; text set in two columns side by side (a document in Russian and Ukrainian, or
/// Spanish and Catalan) leaves a line height or more.
const GUTTER_GAP: f32 = 1.0;
/// How many lines in a row must leave the gap open: a few lines of prose can line up their
/// spaces by chance.
const GUTTER_LINES: usize = 5;
/// How many words a line usually holds on each side of a gutter: fewer, the columns are table
/// cells, which read row by row.
const GUTTER_WORDS: usize = 3;
/// The most places across the page [`columns`] tries for a gutter, so a huge image costs no
/// more than a page.
const GUTTER_STEPS: usize = 4096;

/// Columns of text side by side, from `top` to `bottom` of the image, split at the x of each
/// of `cuts`, left to right.
#[derive(Clone, Debug, PartialEq)]
struct Columns {
    top: f32,
    bottom: f32,
    cuts: Vec<f32>,
}

/// One row of text across the image: the word boxes of the detector lines at its height,
/// left to right, and the height of its middle box.
struct Row {
    boxes: Vec<[f32; 4]>,
    top: f32,
    bottom: f32,
    height: f32,
}

/// Where one row stands at an x: beside its text, across a word or a narrow space, or in a
/// gap as wide as a gutter with so many words on its left and its right.
enum At {
    Outside,
    Across,
    Gap(usize, usize),
}

impl Row {
    fn at(&self, x: f32) -> At {
        let (Some(first), Some(last)) = (self.boxes.first(), self.boxes.iter().map(|b| b[2]).reduce(f32::max)) else { return At::Outside };
        if x < first[0] || x > last {
            return At::Outside;
        }
        let left = self.boxes.iter().filter(|b| b[2] <= x).count();
        let gap_left = self.boxes.iter().filter(|b| b[2] <= x).map(|b| b[2]).reduce(f32::max);
        let gap_right = self.boxes.iter().filter(|b| b[0] >= x).map(|b| b[0]).reduce(f32::min);
        match (gap_left, gap_right) {
            (Some(l), Some(r)) if self.boxes.iter().all(|b| b[2] <= x || b[0] >= x) && r - l >= GUTTER_GAP * self.height => {
                At::Gap(left, self.boxes.len().saturating_sub(left))
            }
            _ => At::Across,
        }
    }
}

/// Gather the detector lines' boxes into rows: a line whose middle lies within the row being
/// built joins it (a line of the next column, found apart), the others start a new row.
fn rows(lines: &[&[[f32; 4]]]) -> Vec<Row> {
    let mut lines: Vec<Vec<[f32; 4]>> = lines
        .iter()
        .map(|l| l.iter().copied().filter(|r| r.iter().all(|v| v.is_finite()) && r[3] > r[1]).collect::<Vec<_>>())
        .filter(|l| !l.is_empty())
        .collect();
    let middle = |l: &[[f32; 4]]| {
        let (top, bottom) = l.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(t, b), r| (t.min(r[1]), b.max(r[3])));
        (top + bottom) / 2.0
    };
    lines.sort_by(|a, b| middle(a).total_cmp(&middle(b)));
    let mut out: Vec<Row> = Vec::new();
    for line in lines {
        let mid = middle(&line);
        match out.last_mut() {
            Some(row) if mid >= row.top && mid <= row.bottom => row.boxes.extend(line),
            _ => out.push(Row { boxes: line, top: 0.0, bottom: 0.0, height: 0.0 }),
        }
        if let Some(row) = out.last_mut() {
            row.boxes.sort_by(|a, b| a[0].total_cmp(&b[0]));
            row.top = row.boxes.iter().map(|b| b[1]).fold(f32::INFINITY, f32::min);
            row.bottom = row.boxes.iter().map(|b| b[3]).fold(f32::NEG_INFINITY, f32::max);
            let mut heights: Vec<f32> = row.boxes.iter().map(|b| b[3] - b[1]).collect();
            heights.sort_by(f32::total_cmp);
            row.height = heights.get(heights.len() / 2).copied().unwrap_or(0.0);
        }
    }
    out
}

/// The middle of `values`, or 0 for none.
fn median(mut values: Vec<usize>) -> usize {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

/// Find columns of text side by side among a page's detected lines (each its word boxes,
/// `[left, top, right, bottom]`): an x that falls in a gap a gutter wide on
/// [`GUTTER_LINES`] or more rows in a row, with [`GUTTER_WORDS`] words or more on each side,
/// and that no row in between crosses with a word. Rows beside the gap (a short line of one
/// column) neither count nor break it.
fn columns(lines: &[&[[f32; 4]]]) -> Vec<Columns> {
    let rows = rows(lines);
    let (Some(left), Some(right)) = (
        rows.iter().flat_map(|r| r.boxes.iter().map(|b| b[0])).reduce(f32::min),
        rows.iter().flat_map(|r| r.boxes.iter().map(|b| b[2])).reduce(f32::max),
    ) else {
        return Vec::new();
    };
    let height = {
        let mut heights: Vec<f32> = rows.iter().map(|r| r.height).filter(|h| *h > 0.0).collect();
        heights.sort_by(f32::total_cmp);
        heights.get(heights.len() / 2).copied().unwrap_or(0.0)
    };
    if height <= 0.0 || right <= left {
        return Vec::new();
    }
    let step = (height / 4.0).max((right - left) / GUTTER_STEPS as f32);
    // Each x and the run of rows (first, last) it is a gutter of.
    let mut found: Vec<(f32, usize, usize)> = Vec::new();
    let mut close = |x: f32, run: &mut Vec<(usize, usize, usize)>| {
        let words_left = median(run.iter().map(|r| r.1).collect());
        let words_right = median(run.iter().map(|r| r.2).collect());
        if let (Some(first), Some(last)) = (run.first(), run.last())
            && run.len() >= GUTTER_LINES
            && words_left >= GUTTER_WORDS
            && words_right >= GUTTER_WORDS
        {
            found.push((x, first.0, last.0));
        }
        run.clear();
    };
    for k in 0..=GUTTER_STEPS {
        let x = left + step * k as f32;
        if x > right {
            break;
        }
        let mut run: Vec<(usize, usize, usize)> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            match row.at(x) {
                At::Outside => {}
                At::Across => close(x, &mut run),
                At::Gap(l, r) => run.push((i, l, r)),
            }
        }
        close(x, &mut run);
    }
    // Neighbouring xs of one gutter become its middle; gutters whose rows overlap one block.
    let mut gutters: Vec<(f32, f32, usize, usize, usize)> = Vec::new(); // (first x, last x, first row, last row, xs)
    found.sort_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2)).then(a.0.total_cmp(&b.0)));
    for (x, first, last) in found {
        match gutters.last_mut() {
            Some(g) if g.2 == first && g.3 == last && x - g.1 <= step * 1.5 => {
                g.1 = x;
                g.4 += 1;
            }
            _ => gutters.push((x, x, first, last, 1)),
        }
    }
    let mut spans: Vec<(usize, usize, f32)> = gutters.iter().map(|g| (g.2, g.3, (g.0 + g.1) / 2.0)).collect();
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.total_cmp(&b.2)));
    let mut blocks: Vec<(usize, usize, Vec<f32>)> = Vec::new();
    for (first, last, x) in spans {
        match blocks.last_mut() {
            Some(b) if first <= b.1 => {
                b.1 = b.1.max(last);
                // A gutter found again over other rows, a little aside, is the same gutter.
                if !b.2.iter().any(|c| (c - x).abs() <= height * GUTTER_GAP) {
                    b.2.push(x);
                }
            }
            _ => blocks.push((first, last, vec![x])),
        }
    }
    blocks
        .into_iter()
        .filter_map(|(first, last, mut cuts)| {
            cuts.sort_by(f32::total_cmp);
            Some(Columns { top: rows.get(first)?.top, bottom: rows.get(last)?.bottom, cuts })
        })
        .collect()
}

/// The band a line of text runs along: the straight line through its word boxes' centres
/// (`y = slope × x + offset`, in pixels) and the height of its tallest box.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    slope: f32,
    offset: f32,
    height: f32,
}

/// The steepest slope [`Band::fit`] believes: about 11°. Steeper, the boxes are not one line.
const MAX_SLOPE: f32 = 0.2;
/// How far a piece of a slanted line may climb across its length, in line heights, before
/// [`Band::pieces`] cuts it: half a line, so the piece's box holds only half a line of its
/// neighbours' letters at its ends.
const MAX_CLIMB: f32 = 0.5;

impl Band {
    /// The band through the boxes (`[left, top, right, bottom]`) of a line: level when they are
    /// too few or too close together to tell a slope.
    fn fit(rects: &[[f32; 4]]) -> Band {
        let boxes: Vec<[f32; 4]> = rects.iter().copied().filter(|r| r.iter().all(|v| v.is_finite()) && r[2] >= r[0] && r[3] >= r[1]).collect();
        let height = boxes.iter().map(|r| r[3] - r[1]).fold(0.0, f32::max);
        let centres: Vec<(f32, f32)> = boxes.iter().map(|r| ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0)).collect();
        let n = centres.len().max(1) as f32;
        let (mx, my) = centres.iter().fold((0.0, 0.0), |(x, y), c| (x + c.0 / n, y + c.1 / n));
        let (cov, var) = centres.iter().fold((0.0, 0.0), |(c, v), p| (c + (p.0 - mx) * (p.1 - my), v + (p.0 - mx) * (p.0 - mx)));
        // Centres closer together than a line height (one word, or a short stack) say nothing.
        let slope = if centres.len() > 1 && var > (height * height).max(1.0) { (cov / var).clamp(-MAX_SLOPE, MAX_SLOPE) } else { 0.0 };
        let slope = if slope.is_finite() { slope } else { 0.0 };
        // A line that climbs less than [`MAX_CLIMB`] across its length is level: its centres
        // differ by its letters (capitals, descenders), and it is read in its own box, as tall
        // as all its boxes together.
        let span = centres.iter().map(|c| c.0).fold(f32::NEG_INFINITY, f32::max) - centres.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
        let climb = slope.abs() * span;
        if !climb.is_finite() || climb <= MAX_CLIMB * height {
            let top = boxes.iter().map(|r| r[1]).fold(f32::INFINITY, f32::min);
            let bottom = boxes.iter().map(|r| r[3]).fold(f32::NEG_INFINITY, f32::max);
            return if top <= bottom {
                Band { slope: 0.0, offset: (top + bottom) / 2.0, height: bottom - top }
            } else {
                Band { slope: 0.0, offset: 0.0, height: 0.0 }
            };
        }
        Band { slope, offset: my - slope * mx, height }
    }

    /// The boxes of one run of a line, left to right, cut into pieces each climbing at most
    /// [`MAX_CLIMB`] line heights along the band. A level line stays one piece.
    fn pieces(&self, run: Vec<[f32; 4]>) -> Vec<Vec<[f32; 4]>> {
        let longest = if self.slope == 0.0 { f32::INFINITY } else { MAX_CLIMB * self.height / self.slope.abs() };
        let mut out: Vec<Vec<[f32; 4]>> = Vec::new();
        let mut start = f32::NEG_INFINITY;
        for r in run {
            match out.last_mut() {
                Some(piece) if r[2] - start <= longest => piece.push(r),
                _ => {
                    start = r[0];
                    out.push(vec![r]);
                }
            }
        }
        out
    }

    /// The box to read a piece of the line in: its word boxes, and the band's full height
    /// over the piece's length, so a piece of small letters keeps the line's height.
    fn around(&self, piece: &[[f32; 4]]) -> Option<[f32; 4]> {
        let b = piece.iter().copied().reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])?;
        let (y0, y1) = (self.slope * b[0] + self.offset, self.slope * b[2] + self.offset);
        let half = self.height / 2.0;
        let out = [b[0], b[1].min(y0.min(y1) - half), b[2], b[3].max(y0.max(y1) + half)];
        out.iter().all(|v| v.is_finite()).then_some(out)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::test_image;

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

    /// Rows 30 px apart from `top`, each the 20 px tall word boxes of `words` (left, width).
    fn rows_of(top: f32, count: usize, words: &[(f32, f32)]) -> Vec<Vec<[f32; 4]>> {
        (0..count).map(|k| words.iter().map(|&(x, w)| [x, top + 30.0 * k as f32, x + w, top + 30.0 * k as f32 + 20.0]).collect()).collect()
    }

    #[test]
    fn columns_of_text_side_by_side_are_found() {
        // Four words of a column, a gutter of 2 line heights (40 px), four of the next.
        let two = [(0.0, 60.0), (70.0, 50.0), (130.0, 80.0), (220.0, 60.0), (320.0, 70.0), (400.0, 40.0), (450.0, 90.0), (550.0, 50.0)];
        let heading = rows_of(0.0, 1, &[(0.0, 600.0)]);
        let body = rows_of(40.0, 8, &two);
        let lines: Vec<&[[f32; 4]]> = heading.iter().chain(&body).map(Vec::as_slice).collect();
        let found = columns(&lines);
        assert_eq!(found.len(), 1, "{found:?}");
        let [cut] = found[0].cuts[..] else { panic!("{found:?}") };
        assert!((282.0..=318.0).contains(&cut), "{cut}");
        assert_eq!((found[0].top, found[0].bottom), (40.0, 40.0 + 30.0 * 7.0 + 20.0), "the heading across both is not in the block");
        // The same columns found by the detector as lines of their own.
        let apart: Vec<Vec<[f32; 4]>> = body.iter().flat_map(|row| [row[..4].to_vec(), row[4..].to_vec()]).collect();
        let lines: Vec<&[[f32; 4]]> = apart.iter().map(Vec::as_slice).collect();
        assert_eq!(columns(&lines), found);
        // A short last line of the left column alone neither counts nor breaks the gutter.
        let mut short = body.clone();
        short.insert(4, rows_of(40.0 + 30.0 * 4.0 - 5.0, 1, &[(0.0, 60.0)]).remove(0));
        let lines: Vec<&[[f32; 4]]> = short.iter().map(Vec::as_slice).collect();
        assert_eq!(columns(&lines).len(), 1);
    }

    #[test]
    fn text_that_only_lines_up_by_chance_has_no_columns() {
        let two = [(0.0, 60.0), (70.0, 50.0), (130.0, 80.0), (220.0, 60.0), (320.0, 70.0), (400.0, 40.0), (450.0, 90.0), (550.0, 50.0)];
        let few = rows_of(0.0, 4, &two);
        let lines: Vec<&[[f32; 4]]> = few.iter().map(Vec::as_slice).collect();
        assert!(columns(&lines).is_empty(), "four lines are not enough");
        // A table: a word or two each side of the gap.
        let table = rows_of(0.0, 10, &[(0.0, 120.0), (320.0, 60.0)]);
        let lines: Vec<&[[f32; 4]]> = table.iter().map(Vec::as_slice).collect();
        assert!(columns(&lines).is_empty(), "table cells read row by row");
        // Prose: spaces of 10 px, never a line height.
        let prose = rows_of(0.0, 10, &[(0.0, 60.0), (70.0, 50.0), (130.0, 80.0), (220.0, 60.0), (290.0, 70.0), (370.0, 40.0)]);
        let lines: Vec<&[[f32; 4]]> = prose.iter().map(Vec::as_slice).collect();
        assert!(columns(&lines).is_empty());
        // A line across the gap halfway breaks it into runs too short.
        let mut crossed = rows_of(0.0, 9, &two);
        crossed[4] = vec![[0.0, 120.0, 600.0, 140.0]];
        let lines: Vec<&[[f32; 4]]> = crossed.iter().map(Vec::as_slice).collect();
        assert!(columns(&lines).is_empty());
        assert!(columns(&[]).is_empty());
        assert!(columns(&[&[[f32::NAN, 0.0, f32::INFINITY, 1.0]]]).is_empty());
    }

    #[test]
    fn a_line_is_cut_at_a_gutter() {
        let (a, b, c) = ([10.0, 0.0, 50.0, 20.0], [58.0, 0.0, 90.0, 20.0], [110.0, 2.0, 200.0, 20.0]);
        assert_eq!(segments(&[a, b, c], 60.0, &[]), [vec![a, b, c]]);
        assert_eq!(segments(&[a, b, c], 60.0, &[100.0]), [vec![a, b], vec![c]]);
        assert_eq!(segments(&[a, b, c], 60.0, &[300.0]), [vec![a, b, c]], "a gutter beside the line cuts nothing");
    }

    #[test]
    fn a_line_is_read_apart_where_its_boxes_are_far_apart() {
        // Words spaced 8 px, then a 60 px gutter (a second column), given out of order.
        let (a, b, c) = ([10.0, 0.0, 50.0, 20.0], [58.0, 0.0, 90.0, 20.0], [150.0, 2.0, 200.0, 20.0]);
        assert_eq!(segments(&[c, a, b], 30.0, &[]), [vec![a, b], vec![c]]);
        assert_eq!(segments(&[a, b], 30.0, &[]), [vec![a, b]], "a space between words is no gutter");
        assert_eq!(segments(&[a, c], 100.0, &[]), [vec![a, c]], "a gap as wide as the least is kept");
        // A box inside a wider one before it is no gap.
        let wide = [0.0, 0.0, 300.0, 20.0];
        assert_eq!(segments(&[wide, c], 30.0, &[]), [vec![wide, c]]);
        assert!(segments(&[], 30.0, &[]).is_empty());
        assert!(segments(&[[f32::NAN, 0.0, 1.0, 1.0]], 30.0, &[]).is_empty(), "boxes that are not numbers are dropped");
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
    fn a_half_turn_maps_back_onto_the_image_as_given() {
        // 3 × 2 pixels, one byte each: 0 1 2 / 3 4 5.
        let px: Vec<u8> = (0..6).collect();
        let (turned, w, h) = turn_image(&px, 3, 2, Turn::Half).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(turned, [5, 4, 3, 2, 1, 0]);
        assert_eq!(Turn::Half.to_source(0.0, 0.0, 3, 2), (3.0, 2.0));
        assert_eq!(Turn::Half.to_source(1.0, 0.5, 3, 2), (2.0, 1.5));
        assert_eq!(turn_image(&turned, 3, 2, Turn::Half).unwrap().0, px, "twice is the image as given");
    }

    #[test]
    fn upside_down_text_is_read_turned_upright() {
        let Some(models) = Models::find() else {
            eprintln!("skipped: OCR models not installed");
            return;
        };
        let ocr = Ocr::load(&models).unwrap();
        let (w, h, px) = test_image::hello();
        let (px, w, h) = turn_image(&px, w, h, Turn::Half).unwrap();
        let (lines, turn) = match ocr.recognize_upright(&px, w, h, "es") {
            Err(OcrError::NoModels) => {
                eprintln!("skipped: the Latin model is not installed");
                return;
            }
            other => other.unwrap(),
        };
        assert_eq!(turn, Turn::Half, "{lines:?}");
        let words: Vec<&Word> = lines.iter().flat_map(|l| &l.words).collect();
        let hello = words.iter().find(|w| w.text.starts_with("HELL")).unwrap_or_else(|| panic!("{words:?}"));
        // Mapped back, HELLO lies right of the middle of the upside-down image: its words run
        // from right to left there.
        let (a, b) = (turn.to_source(hello.rect[0], hello.rect[1], w, h), turn.to_source(hello.rect[2], hello.rect[3], w, h));
        assert!(a.0.min(b.0) > w as f32 / 2.0, "{a:?} {b:?} of {w}");
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
    fn the_ukrainian_word_list_is_found_beside_the_models() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_lexicon(dir.path(), "uk").is_none(), "not installed");
        std::fs::write(dir.path().join("uk_UA.aff"), "SET UTF-8\n").unwrap();
        std::fs::write(dir.path().join("uk_UA.dic"), "1\nвідповідно\n").unwrap();
        let lex = load_lexicon(dir.path(), "uk").unwrap();
        assert_eq!(lexicon::correct("видповидно", &lex, "uk").as_deref(), Some("відповідно"));
        assert!(load_lexicon(dir.path(), "ru").is_none(), "Russian has no word list");
        std::fs::write(dir.path().join("uk_UA.dic"), "0\n").unwrap();
        assert!(load_lexicon(dir.path(), "uk").is_none(), "an empty list is no list");
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
    fn an_unsure_short_word_between_sure_ones_is_kept() {
        assert_eq!(kept(&[0.99, 0.83, 0.52, 0.97]), [true, true, true, true]);
        // Alone, or at the edge of the run, it is not.
        assert_eq!(kept(&[0.52]), [false]);
        assert_eq!(kept(&[0.52, 0.97]), [false, true]);
        assert_eq!(kept(&[0.99, 0.52, 0.5, 0.97]), [true, false, false, true]);
        // Junk stays out whatever its neighbours.
        assert_eq!(kept(&[0.99, 0.2, 0.97]), [true, false, true]);
        assert_eq!(kept(&[]), Vec::<bool>::new());
    }

    #[test]
    fn a_whole_image_box_across_tile_boxes_is_not_read_twice() {
        let rr = |[l, t, r, b]: [f32; 4]| RotatedRect::from_rect(rten_imageproc::RectF::from_tlbr(t, l, b, r));
        // The shrunken whole image found a line the tiles found as two words, one of which
        // reaches past it: keeping the line too would read those words twice.
        let words = vec![rr([100.0, 100.0, 300.0, 120.0]), rr([310.0, 100.0, 620.0, 121.0])];
        let merged = merge_detections(vec![rr([100.0, 98.0, 480.0, 122.0])], words);
        let mut lefts: Vec<f32> = merged.iter().map(|r| r.bounding_rect().left()).collect();
        lefts.sort_by(f32::total_cmp);
        assert_eq!(lefts, [100.0, 310.0]);
        assert!(merged.iter().all(|r| r.bounding_rect().width() < 350.0));
        // A sliver of overlap with a neighbour does not stop a word only the whole image found.
        let merged = merge_detections(vec![rr([100.0, 200.0, 300.0, 230.0])], vec![rr([295.0, 200.0, 500.0, 230.0])]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn a_slanted_line_is_read_in_level_pieces() {
        // Ten words 20 high climbing 5 pixels per 100 across: over the line, 45 pixels, more
        // than two lines' height.
        let words: Vec<[f32; 4]> = (0..10).map(|i| i as f32 * 100.0).map(|x| [x, 300.0 + x * 0.05, x + 80.0, 320.0 + x * 0.05]).collect();
        let band = Band::fit(&words);
        assert!((band.slope - 0.05).abs() < 1e-3, "{band:?}");
        assert_eq!(band.height, 20.0);
        let pieces = band.pieces(words.clone());
        assert_eq!(pieces.iter().map(Vec::len).sum::<usize>(), 10);
        assert!(pieces.len() >= 4, "{pieces:?}");
        for piece in &pieces {
            let b = band.around(piece).unwrap();
            assert!(b[3] - b[1] <= 20.0 * (1.0 + MAX_CLIMB) + 1.0, "{b:?}");
            // Every word lies inside the box its piece is read in.
            assert!(piece.iter().all(|w| w[1] >= b[1] && w[3] <= b[3]));
        }
        // A level line stays one piece, as tall as its words.
        let level: Vec<[f32; 4]> = (0..10).map(|i| i as f32 * 100.0).map(|x| [x, 300.0, x + 80.0, 320.0]).collect();
        let band = Band::fit(&level);
        assert_eq!(band.slope, 0.0);
        let pieces = band.pieces(level);
        assert_eq!(pieces.len(), 1);
        assert_eq!(band.around(&pieces[0]), Some([0.0, 300.0, 980.0, 320.0]));
        // One word, or none, or junk, is a level band.
        assert_eq!(Band::fit(&[[0.0, 0.0, 50.0, 10.0]]).slope, 0.0);
        assert_eq!(Band::fit(&[]).height, 0.0);
        assert_eq!(Band::fit(&[[f32::NAN, 0.0, 1.0, 1.0], [0.0, 0.0, f32::INFINITY, 1.0]]), Band { slope: 0.0, offset: 0.0, height: 0.0 });
        // Boxes stacked steeper than any line are not taken for one.
        assert_eq!(Band::fit(&[[0.0, 0.0, 50.0, 10.0], [100.0, 200.0, 150.0, 210.0]]).slope, MAX_SLOPE);
    }

    #[test]
    fn a_slanted_line_is_read_without_its_neighbours() {
        let Some(models) = Models::find() else {
            eprintln!("skipped: OCR models not installed");
            return;
        };
        let ocr = Ocr::load(&models).unwrap();
        let Ok(reader) = ocr.paddle(paddle::Script::Latin) else {
            eprintln!("skipped: Latin recognition model not installed");
            return;
        };
        // Three lines of "HELLO WORLD" four times over, their letters 28 pixels high and 12
        // apart, photographed at a slant: across the line it climbs past the line below.
        const SLANT: f32 = 0.06;
        const GAP: u32 = 40;
        let text = ["HELLO WORLD"; 4].join(" ");
        let (lw, lh, line) = test_image::text(&text);
        let (w, h) = (lw, lh + 2 * GAP + (lw as f32 * SLANT) as u32);
        let mut px = vec![255u8; (w * h * 3) as usize];
        for row in 0..3u32 {
            for y in 0..lh {
                for x in 0..lw {
                    if line[((y * lw + x) * 4) as usize] == 0 {
                        let o = (((y + row * GAP + (x as f32 * SLANT) as u32) * w + x) * 3) as usize;
                        px[o..o + 3].copy_from_slice(&[0, 0, 0]);
                    }
                }
            }
        }
        // The middle line's word boxes, as the detector finds them: 40 pixels in, 24 a letter.
        let mut boxes = Vec::new();
        let mut start = 0;
        for word in text.split(' ') {
            let (x0, x1) = ((40 + start * 24) as f32, (40 + (start + word.len()) * 24 - 4) as f32);
            boxes.push([x0, 40.0 + GAP as f32 + x0 * SLANT, x1, 68.0 + GAP as f32 + x1 * SLANT]);
            start += word.len() + 1;
        }
        let words = ocr.read_line(&reader, &px, (w, h, 3), &boxes, &[], "es").unwrap();
        let read: Vec<String> = words.iter().map(|w| w.text.to_uppercase()).collect();
        // The blocky "D" reads as "O" at times; the neighbours' letters run into a level crop.
        let right = read.iter().filter(|t| t.starts_with("HELL") || t.starts_with("WOR")).count();
        assert!(right >= 7, "{read:?}");
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
}
