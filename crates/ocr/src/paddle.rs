//! Tinta Office fork: line recognition with PaddleOCR's PP-OCRv5 models (Apache-2.0), for the
//! languages the ocrs models can't read: accented Latin (Spanish, Catalan, Galician, Basque,
//! French, German, Italian, Portuguese) and Cyrillic (Ukrainian, Russian, Belarusian,
//! Bulgarian). ocrs still finds the words and lines; this reads each line.
//!
//! A model takes a line image 48 pixels high, `(v / 255 - 0.5) / 0.5` per channel in BGR order,
//! and returns per time step (one every 8 pixels) the probability of each class: 0 is the CTC
//! blank, 1..=n are the dictionary's lines and n + 1 is the space.

use std::path::Path;

use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

use crate::OcrError;

/// The model's input height.
const HEIGHT: usize = 48;
/// The widest line image read in one go (a line of about 400 characters at 48 pixels high).
const MAX_WIDTH: usize = 4096;
/// A dictionary longer than this is not a recognition model's.
const MAX_CLASSES: usize = 100_000;

/// The script a language is written in, and so the model that reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Script {
    /// Latin with accents.
    Latin,
    /// East Slavic Cyrillic (and Bulgarian).
    Cyrillic,
}

impl Script {
    /// The script `language` (ISO 639-1) is read with, or `None` for the ocrs model (English).
    pub fn for_language(language: &str) -> Option<Script> {
        match language {
            "es" | "ca" | "gl" | "eu" | "fr" | "de" | "it" | "pt" => Some(Script::Latin),
            "uk" | "ru" | "be" | "bg" => Some(Script::Cyrillic),
            _ => None,
        }
    }

    /// The model and dictionary file names, as in ATTRIBUTION.toml.
    pub fn files(self) -> (&'static str, &'static str) {
        match self {
            Script::Latin => ("latin-rec.onnx", "latin-dict.txt"),
            Script::Cyrillic => ("eslav-rec.onnx", "eslav-dict.txt"),
        }
    }
}

/// A loaded line recogniser.
pub struct Paddle {
    model: rten::Model,
    /// Class index → text: the blank, the dictionary's lines, the space.
    classes: Vec<String>,
}

/// One recognised character, the time step it was read at and the model's score for it (its
/// probability, 0–1).
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub text: String,
    pub step: usize,
    pub score: f32,
}

/// Class index → text for a dictionary (one entry per line).
pub fn classes(dict: &str) -> Result<Vec<String>, OcrError> {
    let mut classes = vec![String::new()];
    classes.extend(dict.lines().map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned()));
    classes.push(" ".into());
    if classes.len() > MAX_CLASSES || classes.len() < 3 {
        return Err(OcrError::Load("dictionary".into(), format!("{} entries", classes.len())));
    }
    Ok(classes)
}

/// Greedy CTC decoding of `[steps, classes]` scores: the best class per step, repeats merged and
/// blanks dropped.
pub fn decode(scores: &[f32], steps: usize, classes: &[String]) -> Vec<Read> {
    let n = classes.len();
    let mut out = Vec::new();
    let mut last = 0;
    for step in 0..steps {
        let Some(row) = scores.get(step.saturating_mul(n)..step.saturating_add(1).saturating_mul(n)) else { break };
        // NaN scores never win.
        let (best, score) = row.iter().enumerate().fold((0, f32::NEG_INFINITY), |b, (i, v)| if *v > b.1 { (i, *v) } else { b });
        if best != 0
            && best != last
            && let Some(text) = classes.get(best)
        {
            out.push(Read { text: text.clone(), step, score: score.clamp(0.0, 1.0) });
        }
        last = best;
    }
    out
}

/// A word of a decoded line: its text, the time steps it spans (end exclusive) and the mean
/// score of its characters.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub steps: (usize, usize),
    pub score: f32,
}

/// Split a decoded line at its spaces.
pub fn words(read: &[Read]) -> Vec<Span> {
    // Each word with the sum and count of its characters' scores.
    let mut out: Vec<(Span, f32, usize)> = Vec::new();
    let mut current: Option<(Span, f32, usize)> = None;
    for r in read {
        if r.text.trim().is_empty() {
            out.extend(current.take());
            continue;
        }
        match current.as_mut() {
            Some((s, sum, n)) => {
                s.text.push_str(&r.text);
                s.steps.1 = r.step.saturating_add(1);
                *sum += r.score;
                *n = n.saturating_add(1);
            }
            None => current = Some((Span { text: r.text.clone(), steps: (r.step, r.step.saturating_add(1)), score: 0.0 }, r.score, 1)),
        }
    }
    out.extend(current);
    out.into_iter().map(|(s, sum, n)| Span { score: sum / n.max(1) as f32, ..s }).collect()
}

/// Latin letters a Cyrillic model also knows that look the same as Cyrillic ones, and the
/// Cyrillic letter each stands for. `і`/`І` only in the languages that have them.
fn cyrillic_twin(c: char, language: &str) -> Option<char> {
    Some(match c {
        'a' => 'а',
        'c' => 'с',
        'e' => 'е',
        'o' => 'о',
        'p' => 'р',
        'x' => 'х',
        'y' => 'у',
        'A' => 'А',
        'B' => 'В',
        'C' => 'С',
        'E' => 'Е',
        'H' => 'Н',
        'K' => 'К',
        'M' => 'М',
        'O' => 'О',
        'P' => 'Р',
        'T' => 'Т',
        'X' => 'Х',
        'i' if matches!(language, "uk" | "be") => 'і',
        'I' if matches!(language, "uk" | "be") => 'І',
        _ => return None,
    })
}

fn cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// Fix what the models predictably misread in `language`, for the words of one line:
/// - in a line of mostly Cyrillic text, a word whose only Latin letters have Cyrillic twins
///   (Ukrainian "і" read as Latin "i") is written in Cyrillic, so it is found by a search;
/// - in Catalan, the middle dot of "l·l" read as a hyphen or a full stop.
pub fn tidy(words: &mut [String], language: &str) {
    if Script::for_language(language) == Some(Script::Cyrillic) {
        let count = |f: fn(&char) -> bool| words.iter().flat_map(|w| w.chars()).filter(f).count();
        if count(|c| cyrillic(*c)) > count(char::is_ascii_alphabetic) {
            for w in words.iter_mut() {
                if w.chars().filter(char::is_ascii_alphabetic).all(|c| cyrillic_twin(c, language).is_some()) {
                    *w = w.chars().map(|c| cyrillic_twin(c, language).unwrap_or(c)).collect();
                }
            }
        }
    }
    if language == "ca" {
        for w in words.iter_mut() {
            let chars: Vec<char> = w.chars().collect();
            let mut out = String::with_capacity(w.len());
            for (i, c) in chars.iter().enumerate() {
                let between_ls = |a: Option<&char>, b: Option<&char>| matches!((a, b), (Some('l'), Some('l')) | (Some('L'), Some('L')));
                let geminate = matches!(c, '-' | '.' | '•' | '∙') && i > 0 && between_ls(chars.get(i - 1), chars.get(i + 1));
                out.push(if geminate { '·' } else { *c });
            }
            *w = out;
        }
    }
}

impl Paddle {
    pub fn load(model: &Path, dict: &Path) -> Result<Paddle, OcrError> {
        let text = std::fs::read_to_string(dict).map_err(|e| OcrError::Load(dict.display().to_string(), e.to_string()))?;
        let classes = classes(&text)?;
        let model = rten::Model::load_file(model).map_err(|e| OcrError::Load(model.display().to_string(), e.to_string()))?;
        Ok(Paddle { model, classes })
    }

    /// Read the line in `rect` (`[left, top, right, bottom]`, pixels) of an image with
    /// `channels` bytes per pixel. Returns the characters and how many image pixels across one
    /// time step is.
    pub fn read_line(&self, pixels: &[u8], width: u32, height: u32, channels: usize, rect: [f32; 4]) -> Result<(Vec<Read>, f32), OcrError> {
        let (w, h) = (width as usize, height as usize);
        let clamp = |v: f32, max: usize| if v.is_finite() { (v.max(0.0) as usize).min(max) } else { 0 };
        let (x0, x1) = (clamp(rect[0], w), clamp(rect[2], w));
        let (y0, y1) = (clamp(rect[1], h), clamp(rect[3], h));
        let (cw, ch) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if cw == 0 || ch == 0 || channels == 0 {
            return Ok((Vec::new(), 1.0));
        }
        let out_w = (cw.saturating_mul(HEIGHT) / ch).clamp(16, MAX_WIDTH);
        let mut input = NdTensor::<f32, 4>::zeros([1, 3, HEIGHT, out_w]);
        let sample = |x: usize, y: usize| -> [f32; 3] {
            let i = (y.saturating_mul(w).saturating_add(x)).saturating_mul(channels);
            match pixels.get(i..i.saturating_add(channels)) {
                Some([g]) | Some([g, _]) => [f32::from(*g); 3],
                Some([r, g, b, ..]) => [f32::from(*r), f32::from(*g), f32::from(*b)],
                _ => [255.0; 3],
            }
        };
        let (sx, sy) = (cw as f32 / out_w as f32, ch as f32 / HEIGHT as f32);
        for oy in 0..HEIGHT {
            // Bilinear: the four source pixels around this output pixel's centre.
            let fy = ((oy as f32 + 0.5) * sy - 0.5).max(0.0);
            let (ya, ty) = (fy as usize, fy.fract());
            let (ya, yb) = (y0.saturating_add(ya).min(y1 - 1), y0.saturating_add(ya).saturating_add(1).min(y1 - 1));
            for ox in 0..out_w {
                let fx = ((ox as f32 + 0.5) * sx - 0.5).max(0.0);
                let (xa, tx) = (fx as usize, fx.fract());
                let (xa, xb) = (x0.saturating_add(xa).min(x1 - 1), x0.saturating_add(xa).saturating_add(1).min(x1 - 1));
                let (a, b, c, d) = (sample(xa, ya), sample(xb, ya), sample(xa, yb), sample(xb, yb));
                for (k, channel) in [2, 1, 0].into_iter().enumerate() {
                    let top = a[channel] * (1.0 - tx) + b[channel] * tx;
                    let bottom = c[channel] * (1.0 - tx) + d[channel] * tx;
                    let v = top * (1.0 - ty) + bottom * ty;
                    if let Some(slot) = input.get_mut([0, k, oy, ox]) {
                        *slot = (v / 255.0 - 0.5) / 0.5;
                    }
                }
            }
        }
        let output = self.model.run_one(input.view().into(), None).map_err(|e| OcrError::Recognize(e.to_string()))?;
        let scores: NdTensor<f32, 3> = output.try_into().map_err(|e| OcrError::Recognize(format!("{e}")))?;
        let [_, steps, n] = scores.shape();
        if n != self.classes.len() {
            return Err(OcrError::Recognize(format!("the model has {n} classes, its dictionary {}", self.classes.len())));
        }
        let data = scores.to_vec();
        let read = decode(&data, steps, &self.classes);
        Ok((read, cw as f32 / steps.max(1) as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(best: &[usize], n: usize) -> Vec<f32> {
        best.iter().flat_map(|b| (0..n).map(move |i| if i == *b { 0.9 } else { 0.01 })).collect()
    }

    #[test]
    fn the_dictionary_is_framed_by_the_blank_and_the_space() {
        let c = classes("a\r\nб\nñ\n").unwrap();
        assert_eq!(c, ["", "a", "б", "ñ", " "]);
        assert!(classes("").is_err(), "no entries");
    }

    #[test]
    fn ctc_merges_repeats_and_drops_blanks() {
        let c = classes("К\nи\nї\nв").unwrap();
        // К К _ и _ и ї ї в
        let read = decode(&scores(&[1, 1, 0, 2, 0, 2, 3, 3, 4], c.len()), 9, &c);
        let text: String = read.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "Кииїв");
        assert_eq!(read.first().map(|r| r.step), Some(0));
        assert_eq!(read.last().map(|r| r.step), Some(8));
    }

    #[test]
    fn decoding_survives_short_and_odd_scores() {
        let c = classes("a").unwrap();
        assert!(decode(&[], 5, &c).is_empty(), "fewer scores than steps");
        assert!(decode(&[f32::NAN, f32::NAN, f32::NAN], 1, &c).is_empty(), "NaN never wins");
    }

    #[test]
    fn a_line_splits_into_words_at_its_spaces() {
        let c = classes("J\no\ns\né").unwrap();
        // J o s é _ space _ J o
        let read = decode(&scores(&[1, 2, 3, 4, 0, 5, 0, 1, 2], c.len()), 9, &c);
        let spans = words(&read);
        let got: Vec<_> = spans.iter().map(|s| (s.text.as_str(), s.steps)).collect();
        assert_eq!(got, [("José", (0, 4)), ("Jo", (7, 9))]);
        assert!(spans.iter().all(|s| (s.score - 0.9).abs() < 1e-5), "{spans:?}");
    }

    #[test]
    fn a_word_scores_the_mean_of_its_characters() {
        let c = classes("a\nb").unwrap();
        // a b _ a: one word, with b read at 0.3 and the rest at 0.9.
        let mut s = scores(&[1, 2, 0, 1], c.len());
        if let Some(v) = s.get_mut(c.len() + 2) {
            *v = 0.3;
        }
        let read = decode(&s, 4, &c);
        assert_eq!(read.iter().map(|r| r.score).collect::<Vec<_>>(), [0.9, 0.3, 0.9]);
        let spans = words(&read);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!(spans.iter().all(|s| (s.score - 0.7).abs() < 1e-5), "{spans:?}");
    }

    #[test]
    fn languages_pick_their_script() {
        assert_eq!(Script::for_language("uk"), Some(Script::Cyrillic));
        assert_eq!(Script::for_language("ca"), Some(Script::Latin));
        assert_eq!(Script::for_language("en"), None);
    }

    #[test]
    fn ukrainian_i_read_as_latin_is_written_in_cyrillic() {
        let mut words: Vec<String> = ["Київ", "i", "Харків"].map(String::from).to_vec();
        tidy(&mut words, "uk");
        assert_eq!(words, ["Київ", "і", "Харків"]);
        // Russian has no і; a Latin word with other letters is left alone.
        let mut words: Vec<String> = ["Москва", "i", "Windows"].map(String::from).to_vec();
        tidy(&mut words, "ru");
        assert_eq!(words, ["Москва", "i", "Windows"]);
        // A mostly Latin line is Latin.
        let mut words: Vec<String> = ["PDF", "i", "OCR", "Київ"].map(String::from).to_vec();
        tidy(&mut words, "uk");
        assert_eq!(words, ["PDF", "i", "OCR", "Київ"]);
    }

    #[test]
    fn the_catalan_middle_dot_is_restored() {
        let mut words: Vec<String> = ["Col-legi", "COL.LEGI", "al-lot", "ell-a", "-l"].map(String::from).to_vec();
        tidy(&mut words, "ca");
        assert_eq!(words, ["Col·legi", "COL·LEGI", "al·lot", "ell-a", "-l"]);
        let mut words: Vec<String> = vec!["Col-legi".into()];
        tidy(&mut words, "es");
        assert_eq!(words, ["Col-legi"], "only in Catalan");
    }
}
