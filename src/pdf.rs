use anyhow::{anyhow, Result};
use regex::Regex;
use std::path::Path;
use std::process::Command;

/// Run `pdftotext <extra-args…> <path> -` and return stdout.
fn run_pdftotext(path: &Path, extra_args: &[&str]) -> Result<String> {
    let out = Command::new("pdftotext")
        .args(extra_args)
        .arg(path)
        .arg("-") // write to stdout
        .output()
        .map_err(|e| anyhow!("failed to run pdftotext (is poppler-utils installed?): {e}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "pdftotext failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Extract text from pages 1..=`last_page` using the `pdftotext` binary.
pub fn extract_text(path: &Path, last_page: u32) -> Result<String> {
    run_pdftotext(path, &["-f", "1", "-l", &last_page.to_string()])
}

/// Extract text from the whole document (no page limit), pages separated by
/// form feeds (`\f`), using the `pdftotext` binary.
pub fn extract_text_all(path: &Path) -> Result<String> {
    run_pdftotext(path, &[])
}

/// Repair pdftotext's small-caps artifact for a paper's own name: a word
/// rendered in small caps (e.g. "RTCᴏɴ") extracts as an uppercase pair
/// ("RTC ON"), which then pollutes anything derived from the text (LLM
/// summaries parrot "RTC ON"). For every distinctive token of the resolved
/// title (≥4 chars, ≥2 uppercase), rejoin two-piece uppercase splits of that
/// token — both pieces ≥2 chars so ordinary words are never glued together.
pub fn repair_smallcaps(text: &str, title: &str) -> String {
    let mut out = text.to_string();
    for token in title.split(|c: char| !c.is_ascii_alphanumeric()) {
        if token.len() < 4 || token.chars().filter(|c| c.is_ascii_uppercase()).count() < 2 {
            continue;
        }
        let upper = token.to_ascii_uppercase();
        for i in 2..=upper.len() - 2 {
            let split = format!("{} {}", &upper[..i], &upper[i..]);
            // \b on both ends: only whole-word pairs, never inside words.
            let re = Regex::new(&format!(r"\b{}\b", regex::escape(&split)))
                .expect("escaped literal is a valid regex");
            out = re.replace_all(&out, token).into_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use printpdf::{BuiltinFont, Mm, PdfDocument};
    use std::fs::File;
    use std::io::BufWriter;

    fn write_pdf(path: &Path, line: &str) {
        let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(page1)
            .get_layer(layer1)
            .use_text(line, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(File::create(path).unwrap()))
            .unwrap();
    }

    fn write_two_page_pdf(path: &Path, line1: &str, line2: &str) {
        use printpdf::{BuiltinFont, Mm, PdfDocument};
        use std::io::BufWriter;
        let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(page1)
            .get_layer(layer1)
            .use_text(line1, 12.0, Mm(15.0), Mm(280.0), &font);
        let (page2, layer2) = doc.add_page(Mm(210.0), Mm(297.0), "L2");
        doc.get_page(page2)
            .get_layer(layer2)
            .use_text(line2, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(std::fs::File::create(path).unwrap()))
            .unwrap();
    }

    #[test]
    fn extracts_known_text() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("sample.pdf");
        write_pdf(&pdf, "Attention Is All You Need");
        let text = extract_text(&pdf, 1).unwrap();
        assert!(
            text.contains("Attention Is All You Need"),
            "extracted text was: {text:?}"
        );
    }

    #[test]
    fn extract_text_all_returns_every_page_with_separators() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("two.pdf");
        write_two_page_pdf(&pdf, "First Page Words", "Second Page Words");
        let text = extract_text_all(&pdf).unwrap();
        assert!(text.contains("First Page Words"));
        assert!(text.contains("Second Page Words"));
        assert!(text.contains('\u{0c}'), "pdftotext page separator expected");
    }

    #[test]
    fn repairs_smallcaps_splits_of_title_tokens() {
        let title = "RTCON: Context-Adaptive Function-Level Fuzzing for RTOS Kernels";
        let text = "we present RTC ON , a fuzzer. RTC ON employs classification.";
        assert_eq!(
            super::repair_smallcaps(text, title),
            "we present RTCON , a fuzzer. RTCON employs classification."
        );
    }

    #[test]
    fn repair_smallcaps_leaves_unrelated_text_alone() {
        let title = "RTCON: Context-Adaptive Fuzzing";
        let text = "THE CAT sat on RTOS mats"; // no split of any title token
        assert_eq!(super::repair_smallcaps(text, title), text);
    }

    #[test]
    fn repair_smallcaps_requires_two_chars_per_piece() {
        // "B ERT"/"BER T" (1-char piece) must NOT be rejoined; "BE RT" is.
        let title = "BERT: Pre-training of Deep Bidirectional Transformers";
        assert_eq!(
            super::repair_smallcaps("uses BE RT daily", title),
            "uses BERT daily"
        );
        assert_eq!(
            super::repair_smallcaps("plan B ERT now", title),
            "plan B ERT now"
        );
    }

    #[test]
    fn repair_smallcaps_with_empty_title_is_identity() {
        assert_eq!(super::repair_smallcaps("any text", ""), "any text");
    }
}
