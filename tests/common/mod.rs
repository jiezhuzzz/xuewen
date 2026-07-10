use printpdf::{BuiltinFont, Mm, PdfDocument};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use xuewen::db;
use xuewen::models::{Authors, Paper, PaperMeta, PaperStatus};

/// Write a one-page PDF whose lines are `lines`, top-to-bottom.
/// pdftotext reliably extracts built-in Helvetica text.
pub fn write_test_pdf(path: &Path, lines: &[&str]) {
    let (doc, page1, layer1) = PdfDocument::new("test", Mm(210.0), Mm(297.0), "Layer 1");
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

/// A migrated pool plus a library root, seeded with one active, resolved
/// paper `id`. `rel_path` points at a file that doesn't exist, so any
/// extraction attempt (e.g. chat's `paper_text`) fails and falls back to
/// metadata alone. The backing temp dir is leaked so both stay valid for the
/// caller's duration (mirrors `web_test.rs`'s `temp_pool`/`paper` seeding).
pub async fn pool_and_root_with_paper(id: &str) -> (sqlx::SqlitePool, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let library_root = dir.path().join("library");
    std::fs::create_dir_all(&library_root).unwrap();

    let paper = Paper {
        id: id.into(),
        content_hash: id.into(),
        rel_path: "missing.pdf".into(),
        cite_key: Some(id.into()),
        added_at: "2026-07-07T00:00:00Z".into(),
        deleted_at: None,
        meta: PaperMeta {
            title: Some(format!("Paper {id}")),
            abstract_text: Some("An abstract.".into()),
            authors: Authors(vec!["Ada Lovelace".into()]),
            venue: Some("KDD".into()),
            year: Some(2020),
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: Some("crossref".into()),
            status: PaperStatus::Resolved,
        },
    };
    db::insert_paper(&pool, &paper).await.unwrap();

    std::mem::forget(dir);
    (pool, library_root)
}
