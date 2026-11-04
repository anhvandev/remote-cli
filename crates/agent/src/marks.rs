//! Mark Detector: finds Marks (accessibility tree, OCR when there is none), draws them on Observation images
//! and resolves a text target to one Mark.

use image::{Rgba, RgbaImage};
use rcli_protocol::agent::{Error, Mark};

use crate::{
    coords,
    platform::{Frame, Platform, UiElement},
};

/// Marks on `frame` of `display`, in image space, numbered from `first`.
pub fn detect(p: &dyn Platform, display: u32, frame: &Frame, first: u32) -> anyhow::Result<Vec<Mark>> {
    let elements = match p.accessibility(display)? {
        Some(tree) => tree,
        None => p.recognize_text(frame)?,
    };
    Ok(elements
        .into_iter()
        .filter(|e| e.width > 0 && e.height > 0)
        .zip(first..)
        .map(|(e, number)| to_mark(frame, display, number, e))
        .collect())
}

fn to_mark(frame: &Frame, display: u32, number: u32, e: UiElement) -> Mark {
    let (x, y) = coords::to_image(frame.width, frame.height, e.x, e.y);
    let (x2, y2) = coords::to_image(frame.width, frame.height, e.x + e.width, e.y + e.height);
    Mark { number, text: e.text, role: e.role, display, x, y, width: (x2 - x).max(1), height: (y2 - y).max(1) }
}

/// The one Mark whose text is `text` (case-insensitive); if none is equal, the one that contains it.
pub fn by_text<'a>(marks: &'a [Mark], text: &str) -> Result<&'a Mark, Error> {
    let err = |code: &str, message: String| Error { code: code.into(), message };
    let want = text.trim().to_lowercase();
    let equal: Vec<&Mark> = marks.iter().filter(|m| m.text.trim().to_lowercase() == want).collect();
    let found = if equal.is_empty() { marks.iter().filter(|m| m.text.to_lowercase().contains(&want)).collect() } else { equal };
    match found[..] {
        [m] => Ok(m),
        [] => Err(err("text_not_found", format!("no Mark with text {text:?} on screen"))),
        _ => Err(err(
            "ambiguous_text",
            format!(
                "text {text:?} is ambiguous: {}; target one by Mark number",
                found.iter().map(|m| format!("Mark {} {} {:?}", m.number, m.role, m.text)).collect::<Vec<_>>().join(", ")
            ),
        )),
    }
}

/// Centre of a Mark, in image space.
pub fn center(m: &Mark) -> (u32, u32) {
    (m.x + m.width / 2, m.y + m.height / 2)
}

const BOX: Rgba<u8> = Rgba([255, 0, 160, 255]);
const LABEL_TEXT: Rgba<u8> = Rgba([255, 255, 255, 255]);
/// 3x5 digit glyphs, one row per 3 bits.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];
const PX: u32 = 2;

/// Outlines each Mark and writes its number in a label at its top-left corner.
pub fn annotate(img: &mut RgbaImage, marks: &[Mark]) {
    let (w, h) = img.dimensions();
    let mut put = |x: u32, y: u32, c| {
        if x < w && y < h {
            img.put_pixel(x, y, c)
        }
    };
    for m in marks {
        for i in 0..=m.width {
            put(m.x + i, m.y, BOX);
            put(m.x + i, m.y + m.height, BOX);
        }
        for i in 0..=m.height {
            put(m.x, m.y + i, BOX);
            put(m.x + m.width, m.y + i, BOX);
        }
        let digits = m.number.to_string();
        let (lw, lh) = (digits.len() as u32 * 4 * PX + PX, 7 * PX);
        for x in 0..lw {
            for y in 0..lh {
                put(m.x + x, m.y + y, BOX);
            }
        }
        for (i, d) in digits.bytes().enumerate() {
            for (row, bits) in DIGITS[(d - b'0') as usize].iter().enumerate() {
                for col in 0..3 {
                    if bits & (4 >> col) != 0 {
                        let (gx, gy) = (m.x + PX + (i as u32 * 4 + col) * PX, m.y + PX + row as u32 * PX);
                        (0..PX * PX).for_each(|k| put(gx + k % PX, gy + k / PX, LABEL_TEXT));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(number: u32, text: &str) -> Mark {
        Mark { number, text: text.into(), role: "button".into(), ..Default::default() }
    }

    #[test]
    fn text_matches_exactly_before_it_matches_part_of_a_mark() {
        let marks = [mark(1, "Save"), mark(2, "Save as…"), mark(3, "Cancel")];
        assert_eq!(by_text(&marks, "save").unwrap().number, 1);
        assert_eq!(by_text(&marks, "as…").unwrap().number, 2);
        assert!(by_text(&marks, "Sav").unwrap_err().message.contains("Mark 1 button \"Save\", Mark 2"));
        assert!(by_text(&marks, "Delete").unwrap_err().message.contains("no Mark"));
    }

    #[test]
    fn annotation_outlines_the_mark_and_labels_it_with_its_number() {
        let mut img = RgbaImage::new(100, 100);
        annotate(&mut img, &[Mark { number: 7, x: 10, y: 10, width: 40, height: 20, ..Default::default() }]);
        assert_eq!(img.get_pixel(40, 30), &BOX, "bottom edge");
        assert_eq!(img.get_pixel(50, 20), &BOX, "right edge");
        assert_eq!(img.get_pixel(12, 12), &LABEL_TEXT, "top-left of the 7");
        assert_eq!(img.get_pixel(12, 14), &BOX, "label background left of the 7's stem");
        assert_eq!(img.get_pixel(30, 25), &Rgba([0, 0, 0, 0]), "inside stays as it was");
    }
}
