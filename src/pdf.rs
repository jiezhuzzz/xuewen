use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Command;

/// Extract text from pages 1..=`last_page` using the `pdftotext` binary.
pub fn extract_text(path: &Path, last_page: u32) -> Result<String> {
    let out = Command::new("pdftotext")
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg(last_page.to_string())
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
}
