//! Reading scanned text (OCR), on this computer only: pages without a text
//! layer get the same [`TextRun`]s as digital ones, so labels, search and
//! copy work on scans.
//!
//! The engine is `ocrs` (pure Rust, models run by `rten`). Its two models
//! (about 12 MB) are not built in: they are fetched once into the data
//! folder (see [`MODELS`]), checked against pinned SHA-256 sums, and used
//! offline from then on. This crate never touches the network itself.

use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use pdfr_core::detect::{DETECT_SCALE, PageGray};
use pdfr_core::geom::Rect;
use pdfr_core::text::TextRun;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// One model file: where it is published, its size and SHA-256 sum.
pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// The models, pinned to the files the ocrs 0.14 tools download.
pub const MODELS: [ModelFile; 2] = [
    ModelFile {
        name: "text-detection.onnx",
        url: "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.onnx",
        bytes: 2_499_479,
        sha256: "a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe",
    },
    ModelFile {
        name: "text-recognition.onnx",
        url: "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.onnx",
        bytes: 9_713_177,
        sha256: "86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5",
    },
];

/// Total download size of the models, in bytes.
pub fn download_size() -> u64 {
    MODELS.iter().map(|m| m.bytes).sum()
}

/// The folder the models live in, inside the app's data folder.
pub fn model_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("ocr")
}

/// Whether both models are in `dir` (with the right sizes).
pub fn models_present(dir: &Path) -> bool {
    MODELS
        .iter()
        .all(|m| std::fs::metadata(dir.join(m.name)).is_ok_and(|md| md.len() == m.bytes))
}

/// Whether `bytes` are exactly the pinned model file.
pub fn verify(m: &ModelFile, bytes: &[u8]) -> bool {
    let sum = Sha256::digest(bytes);
    let hex: String = sum.iter().map(|b| format!("{b:02x}")).collect();
    bytes.len() as u64 == m.bytes && hex == m.sha256
}

/// Stores a downloaded model in `dir` after checking it: written to a
/// temporary file first, so a half-written model is never used.
pub fn install(dir: &Path, m: &ModelFile, bytes: &[u8]) -> Result<(), String> {
    if !verify(m, bytes) {
        return Err(format!("{} did not match its checksum", m.name));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.part", m.name));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(m.name)).map_err(|e| e.to_string())
}

/// A loaded OCR engine. Loading takes a moment; keep one per thread.
pub struct Reader {
    engine: OcrEngine,
}

impl Reader {
    /// Loads the models from `dir`, checking them first.
    pub fn load(dir: &Path) -> Result<Reader, String> {
        let load = |m: &ModelFile| -> Result<rten::Model, String> {
            let bytes = std::fs::read(dir.join(m.name)).map_err(|e| e.to_string())?;
            if !verify(m, &bytes) {
                return Err(format!("{} is damaged", m.name));
            }
            rten::Model::load(bytes).map_err(|e| e.to_string())
        };
        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(load(&MODELS[0])?),
            recognition_model: Some(load(&MODELS[1])?),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        Ok(Reader { engine })
    }

    /// The text on a grey page render at [`DETECT_SCALE`], as phrases in
    /// page points.
    pub fn read(&self, page: &PageGray) -> Vec<TextRun> {
        let Ok(src) = ImageSource::from_bytes(&page.px, (page.w as u32, page.h as u32)) else {
            return vec![];
        };
        let Ok(input) = self.engine.prepare_input(src) else {
            return vec![];
        };
        let Ok(words) = self.engine.detect_words(&input) else {
            return vec![];
        };
        let lines = self.engine.find_text_lines(&input, &words);
        let Ok(texts) = self.engine.recognize_text(&input, &lines) else {
            return vec![];
        };
        let mut out = Vec::new();
        for line in texts.iter().flatten() {
            let chars: Vec<(char, Rect)> = line
                .chars()
                .iter()
                .map(|c| {
                    let r = c.rect;
                    let s = DETECT_SCALE;
                    let rect = Rect::new(
                        r.left() as f32 / s,
                        r.top() as f32 / s,
                        r.right() as f32 / s,
                        r.bottom() as f32 / s,
                    );
                    (c.char, rect)
                })
                .collect();
            out.extend(phrases(&chars));
        }
        out
    }
}

/// Splits a recognised line into phrases at wide gaps (a label and the
/// next column's text are separate phrases, as in a text layer), dropping
/// phrases without a letter or digit.
fn phrases(chars: &[(char, Rect)]) -> Vec<TextRun> {
    let words = words(chars);
    let Some(first) = words.first() else {
        return vec![];
    };
    let height = words.iter().map(|w| w.1.height()).fold(0.0, f32::max);
    let mut groups: Vec<Vec<&(String, Rect, Vec<Rect>)>> = vec![vec![first]];
    for pair in words.windows(2) {
        if pair[1].1.x0 - pair[0].1.x1 > 1.8 * height {
            groups.push(vec![]);
        }
        groups.last_mut().unwrap().push(&pair[1]);
    }
    groups
        .into_iter()
        .filter_map(|g| run(&g, height))
        .filter(|r| r.text.chars().any(char::is_alphanumeric))
        .collect()
}

/// Words of a line: text, box, and each character's box.
fn words(chars: &[(char, Rect)]) -> Vec<(String, Rect, Vec<Rect>)> {
    let mut out: Vec<(String, Rect, Vec<Rect>)> = vec![];
    let mut cur: Option<(String, Rect, Vec<Rect>)> = None;
    for &(c, r) in chars {
        if c.is_whitespace() {
            out.extend(cur.take());
            continue;
        }
        match &mut cur {
            Some((s, b, rs)) => {
                s.push(c);
                *b = union(b, &r);
                rs.push(r);
            }
            None => cur = Some((c.to_string(), r, vec![r])),
        }
    }
    out.extend(cur);
    out
}

fn union(a: &Rect, b: &Rect) -> Rect {
    Rect::new(
        a.x0.min(b.x0),
        a.y0.min(b.y0),
        a.x1.max(b.x1),
        a.y1.max(b.y1),
    )
}

/// One phrase from its words, with a character position for every
/// character (a space spans the gap between words).
fn run(words: &[&(String, Rect, Vec<Rect>)], height: f32) -> Option<TextRun> {
    // Lone marks at the ends ("[", "|") are box edges and rules, not text.
    let mark = |w: &&&(String, Rect, Vec<Rect>)| !w.0.chars().any(char::is_alphanumeric);
    let start = words.iter().position(|w| !mark(&w))?;
    let end = words.iter().rposition(|w| !mark(&w))?;
    let words = &words[start..=end];
    let mut rect = words.first()?.1;
    let (mut text, mut xs, mut ws) = (String::new(), vec![], vec![]);
    for (i, (s, b, rs)) in words.iter().map(|w| (&w.0, &w.1, &w.2)).enumerate() {
        if i > 0 {
            text.push(' ');
            xs.push(rect.x1);
            ws.push((b.x0 - rect.x1).max(0.0));
        }
        rect = union(&rect, b);
        for (c, r) in s.chars().zip(rs) {
            text.push(c);
            xs.push(r.x0);
            ws.push(r.width().max(0.1));
        }
    }
    Some(TextRun {
        rect,
        text,
        size: (height * 0.8).max(1.0),
        xs,
        ws,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str, x0: f32, step: f32) -> Vec<(char, Rect)> {
        s.chars()
            .enumerate()
            .map(|(i, c)| {
                let x = x0 + i as f32 * step;
                (c, Rect::new(x, 10.0, x + step * 0.9, 20.0))
            })
            .collect()
    }

    #[test]
    fn line_splits_at_wide_gaps() {
        let mut c = chars("First name", 10.0, 5.0);
        c.push((' ', Rect::new(60.0, 10.0, 200.0, 20.0)));
        c.extend(chars("Date", 200.0, 5.0));
        let runs = phrases(&c);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "First name");
        assert_eq!(runs[0].xs.len(), runs[0].text.chars().count());
        assert_eq!(runs[1].text, "Date");
        assert!((runs[1].rect.x0 - 200.0).abs() < 0.01);
    }

    #[test]
    fn punctuation_only_is_dropped() {
        assert!(phrases(&chars("|..", 0.0, 4.0)).is_empty());
    }

    #[test]
    fn checksum_guards_models() {
        assert!(!verify(&MODELS[0], b"not a model"));
    }
}
