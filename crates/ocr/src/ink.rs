//! Marks printed in light ink across the text, such as a "DUPLICADO" or "COPIA" stamp on a
//! contract: the detector takes such a mark's letters and the dark lines under them for one
//! tall block, and those lines go unread. [`has_light_mark`] tells when a page shows one, and
//! [`without_light_ink`] gives the page with only its dark ink, to read again.

/// How many times the page's middle word box a box must be tall to be a mark, not a word.
const MARK_HEIGHT: f32 = 3.0;
/// How many times as wide as tall a mark's box is at least: a word of big letters.
const MARK_WIDTH: f32 = 3.0;
/// How many word boxes of the text's own height a mark's box crosses at least.
const MARK_CROSSES: usize = 2;
/// Below this share of the paper's lightness a pixel is dark ink.
const DARK: f32 = 0.4;
/// From this share of the paper's lightness up a pixel is paper.
const PAPER: f32 = 0.92;
/// How many times a mark's light ink pixels must outnumber its dark ones: dark text has light
/// pixels only along its edges.
const LIGHT_OVER_DARK: usize = 2;
/// The side of the squares the paper's lightness is measured in, in pixels: big enough to hold
/// paper between the letters, small enough to follow a shadow.
const CELL: usize = 32;

/// The lightness (0–255) of the pixel starting at `i` in an image of `channels` bytes a pixel.
fn luma(pixels: &[u8], i: usize, channels: usize) -> f32 {
    match (channels, pixels.get(i..i.saturating_add(channels))) {
        (3, Some([r, g, b])) => 0.299 * f32::from(*r) + 0.587 * f32::from(*g) + 0.114 * f32::from(*b),
        (_, Some([v, ..])) => f32::from(*v),
        _ => 0.0,
    }
}

/// The paper's lightness around each [`CELL`]: the lightest pixel of the cell and its eight
/// neighbours, so a cell filled by a letter still finds paper beside it. Row by row, `(cells
/// across, cells down)` with it.
fn paper(pixels: &[u8], width: usize, height: usize, channels: usize) -> (Vec<f32>, usize, usize) {
    let (across, down) = (width.div_ceil(CELL), height.div_ceil(CELL));
    let mut lightest = vec![0.0f32; across.saturating_mul(down)];
    for y in 0..height {
        for x in 0..width {
            let v = luma(pixels, (y * width + x) * channels, channels);
            if let Some(cell) = lightest.get_mut((y / CELL) * across + x / CELL) {
                *cell = cell.max(v);
            }
        }
    }
    let mut around = vec![0.0f32; lightest.len()];
    for cy in 0..down {
        for cx in 0..across {
            let mut most = 0.0f32;
            for ny in cy.saturating_sub(1)..=(cy + 1).min(down - 1) {
                for nx in cx.saturating_sub(1)..=(cx + 1).min(across - 1) {
                    most = most.max(lightest.get(ny * across + nx).copied().unwrap_or(0.0));
                }
            }
            if let Some(a) = around.get_mut(cy * across + cx) {
                *a = most;
            }
        }
    }
    (around, across, down)
}

/// Whether the page `pixels` (`channels` bytes a pixel), whose detected word boxes are `boxes`
/// (`[left, top, right, bottom]`), shows a mark in light ink: a box [`MARK_HEIGHT`] times as
/// tall as the middle box, [`MARK_WIDTH`] times as wide as tall, across [`MARK_CROSSES`] word
/// boxes or more, and whose ink is mostly light. A heading in dark ink, or apart from the text,
/// is not one.
pub(crate) fn has_light_mark(pixels: &[u8], width: u32, height: u32, channels: usize, boxes: &[[f32; 4]]) -> bool {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || pixels.len() < w.saturating_mul(h).saturating_mul(channels) {
        return false;
    }
    let mut heights: Vec<f32> = boxes.iter().map(|b| b[3] - b[1]).filter(|v| v.is_finite() && *v > 0.0).collect();
    heights.sort_by(f32::total_cmp);
    let Some(middle) = heights.get(heights.len() / 2).copied() else { return false };
    // A stamp is a run of big letters lying over the lines of text: wide, and crossing word
    // boxes of the text's own height. A big heading or a picture stands apart.
    let crosses = |b: &[f32; 4], o: &[f32; 4]| o[3] - o[1] < MARK_HEIGHT * middle && o[0] < b[2] && b[0] < o[2] && o[1] < b[3] && b[1] < o[3];
    let tall: Vec<&[f32; 4]> = boxes
        .iter()
        .filter(|b| {
            let (w, h) = (b[2] - b[0], b[3] - b[1]);
            h >= MARK_HEIGHT * middle && w >= MARK_WIDTH * h && boxes.iter().filter(|o| crosses(b, o)).count() >= MARK_CROSSES
        })
        .collect();
    if tall.is_empty() {
        return false;
    }
    let (paper, across, _) = paper(pixels, w, h, channels);
    tall.iter().any(|b| {
        let clamp = |v: f32, most: usize| if v.is_finite() { (v.max(0.0) as usize).min(most) } else { 0 };
        let (left, top, right, bottom) = (clamp(b[0], w), clamp(b[1], h), clamp(b[2], w), clamp(b[3], h));
        let (mut dark, mut light) = (0usize, 0usize);
        for y in top..bottom {
            for x in left..right {
                let p = paper.get((y / CELL) * across + x / CELL).copied().unwrap_or(0.0);
                if p <= 0.0 {
                    continue;
                }
                let share = luma(pixels, (y * w + x) * channels, channels) / p;
                if share < DARK {
                    dark += 1;
                } else if share < PAPER {
                    light += 1;
                }
            }
        }
        light > 0 && light >= dark.saturating_mul(LIGHT_OVER_DARK)
    })
}

/// The page `pixels` (`channels` bytes a pixel) with every pixel lighter than dark ink turned
/// to paper white: light marks, tints and pale pictures gone, the dark text left. `None` for an
/// image whose size does not match.
pub(crate) fn without_light_ink(pixels: &[u8], width: u32, height: u32, channels: usize) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    let size = w.checked_mul(h)?.checked_mul(channels)?;
    if size == 0 || pixels.len() != size {
        return None;
    }
    let (paper, across, _) = paper(pixels, w, h, channels);
    let mut out = pixels.to_vec();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * channels;
            let p = paper.get((y / CELL) * across + x / CELL).copied().unwrap_or(0.0);
            if p > 0.0
                && luma(pixels, i, channels) / p >= DARK
                && let Some(px) = out.get_mut(i..i.saturating_add(channels))
            {
                px.fill(255);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white page `w` × `h`, one byte a pixel, with `rects` filled in their shade.
    fn page(w: usize, h: usize, rects: &[([usize; 4], u8)]) -> Vec<u8> {
        let mut p = vec![255u8; w * h];
        for &([l, t, r, b], v) in rects {
            for y in t..b {
                for x in l..r {
                    p[y * w + x] = v;
                }
            }
        }
        p
    }

    #[test]
    fn a_tall_box_of_light_ink_is_a_mark_and_dark_ink_is_not() {
        // Words 20 px tall, in thin dark strokes, and a box 100 px tall.
        let words: Vec<[f32; 4]> = (0..6).map(|k| [10.0, 10.0 + 30.0 * k as f32, 200.0, 30.0 + 30.0 * k as f32]).collect();
        let tall = [20.0, 40.0, 320.0, 140.0];
        let boxes: Vec<[f32; 4]> = words.iter().copied().chain([tall]).collect();
        let strokes: Vec<([usize; 4], u8)> =
            (0..6).flat_map(|k| (0..20).map(move |s| ([12 + 9 * s, 12 + 30 * k, 15 + 9 * s, 28 + 30 * k], 0u8))).collect();
        // Light grey letters across the tall box.
        let mut marked = strokes.clone();
        marked.extend((0..5).map(|k| ([30 + 55 * k, 50, 70 + 55 * k, 130], 180u8)));
        assert!(has_light_mark(&page(320, 200, &marked), 320, 200, 1, &boxes));
        // The same box in dark ink: a heading.
        let mut heading = strokes.clone();
        heading.extend((0..5).map(|k| ([30 + 55 * k, 50, 70 + 55 * k, 130], 10u8)));
        assert!(!has_light_mark(&page(320, 200, &heading), 320, 200, 1, &boxes));
        // Light letters apart from the text (a pale heading, a picture): no word crossed.
        let apart: Vec<[f32; 4]> = words.iter().map(|w| [w[0], w[1] + 200.0, w[2], w[3] + 200.0]).chain([tall]).collect();
        assert!(!has_light_mark(&page(320, 400, &marked), 320, 400, 1, &apart));
        // Narrow: a picture or a column of big letters, not a stamp's word.
        let narrow: Vec<[f32; 4]> = words.iter().copied().chain([[20.0, 40.0, 200.0, 140.0]]).collect();
        assert!(!has_light_mark(&page(320, 200, &marked), 320, 200, 1, &narrow));
        // No box tall enough.
        assert!(!has_light_mark(&page(320, 200, &marked), 320, 200, 1, &words));
        assert!(!has_light_mark(&[], 0, 0, 1, &boxes));
        assert!(!has_light_mark(&page(320, 200, &marked), 320, 200, 1, &[[f32::NAN, 0.0, f32::INFINITY, 1e9]]));
    }

    #[test]
    fn light_ink_turns_to_paper_and_dark_ink_stays() {
        let shaded = page(64, 64, &[([0, 0, 64, 64], 200), ([10, 10, 20, 20], 20), ([30, 30, 50, 50], 140)]);
        let out = without_light_ink(&shaded, 64, 64, 1).unwrap();
        assert_eq!(out[15 * 64 + 15], 20, "dark ink kept");
        assert_eq!(out[40 * 64 + 40], 255, "light ink gone");
        assert_eq!(out[5 * 64 + 5], 255, "paper in a shadow white");
        assert_eq!(without_light_ink(&shaded, 65, 64, 1), None);
        assert_eq!(without_light_ink(&[], 0, 0, 3), None);
        let rgb: Vec<u8> = shaded.iter().flat_map(|v| [*v, *v, *v]).collect();
        let out = without_light_ink(&rgb, 64, 64, 3).unwrap();
        assert_eq!(&out[(15 * 64 + 15) * 3..][..3], [20, 20, 20]);
        assert_eq!(&out[(40 * 64 + 40) * 3..][..3], [255, 255, 255]);
    }
}
