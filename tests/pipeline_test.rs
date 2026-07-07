mod common;

use xuewen::db;
use xuewen::pipeline::{ingest_file, Libraries, Outcome};

#[tokio::test]
async fn ingests_pdf_and_dedups() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // A PDF whose header carries a title and a DOI.
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &[
            "Attention Is All You Need",
            "https://doi.org/10.1145/3292500.3330701",
        ],
    );

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    // First ingest: stored, filed, moved.
    let out = ingest_file(&pool, &dirs, &pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };

    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.title.as_deref(), Some("Attention Is All You Need"));
    assert_eq!(paper.doi.as_deref(), Some("10.1145/3292500.3330701"));
    assert_eq!(paper.status, "needs_review");

    // File was copied into the library and the original moved to _processed.
    assert!(library.join(format!("{}.pdf", paper.content_hash)).exists());
    assert!(!pdf_path.exists());
    assert!(processed.join("paper.pdf").exists());

    // Re-ingest identical content (from processed copy) -> Duplicate.
    let again = processed.join("paper.pdf");
    let out2 = ingest_file(&pool, &dirs, &again).await.unwrap();
    assert_eq!(out2, Outcome::Duplicate);
}
