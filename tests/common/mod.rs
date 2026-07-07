use printpdf::{BuiltinFont, Mm, PdfDocument};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// Write a one-page PDF whose lines are `lines`, top-to-bottom.
/// pdftotext reliably extracts built-in Helvetica text.
pub fn write_test_pdf(path: &Path, lines: &[&str]) {
    let (doc, page1, layer1) =
        PdfDocument::new("test", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let layer = doc.get_page(page1).get_layer(layer1);
    let mut y = 280.0;
    for line in lines {
        layer.use_text(*line, 12.0, Mm(15.0), Mm(y), &font);
        y -= 8.0;
    }
    doc.save(&mut BufWriter::new(File::create(path).unwrap()))
        .unwrap();
}
