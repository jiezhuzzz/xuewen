//! End-to-end: import a PDF -> background sweep -> find it by a body phrase.

use printpdf::{BuiltinFont, Mm, PdfDocument};
use std::io::BufWriter;
use std::path::Path;

use xuewen::pipeline::{IngestCtx, Libraries};
use xuewen::resolve::Resolver;
use xuewen::search::{fts, indexer, vector, SearchRequest, SearchService};

/// Two text lines: a title-looking first line, then the body phrase. If the
/// ingest heuristics take the first line as the title, the search phrase
/// still only exists in the body — so the snippet's field must be "body".
fn write_pdf(path: &Path, title_line: &str, body_line: &str) {
    let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let layer = doc.get_page(page1).get_layer(layer1);
    layer.use_text(title_line, 14.0, Mm(15.0), Mm(280.0), &font);
    layer.use_text(body_line, 11.0, Mm(15.0), Mm(250.0), &font);
    doc.save(&mut BufWriter::new(std::fs::File::create(path).unwrap()))
        .unwrap();
}

// Upstreams refuse instantly -> the paper lands as needs_review, offline.
fn offline_resolver() -> Resolver {
    Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string())
}

#[tokio::test]
async fn imported_pdf_becomes_keyword_searchable_by_body_text() {
    let dir = tempfile::tempdir().unwrap();
    let library_root = dir.path().join("library");
    let inbox = dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = xuewen::db::connect(&url).await.unwrap();

    // 1. Ingest a PDF whose body holds a distinctive phrase.
    let pdf = inbox.join("paper.pdf");
    write_pdf(
        &pdf,
        "A Study of Distributed Authorization",
        "we evaluate the zanzibar consistency protocol",
    );
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs: Libraries {
            library_root: library_root.clone(),
            processed_dir: inbox.join("_processed"),
        },
        resolver: offline_resolver(),
        grobid: None,
    };
    ctx.ingest_file(&pdf).await.unwrap();

    // 2. One indexer sweep (keyword tier only; no embedder configured).
    let idx_dir = dir.path().join("search-index");
    let (fts_idx, _) = fts::FtsIndex::open(&idx_dir).unwrap();
    let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
    let svc = SearchService::open_with(pool, fts_idx, vectors, None);
    let summary = indexer::sweep(&svc, &library_root).await.unwrap();
    assert_eq!(summary.indexed, 1);
    assert_eq!(summary.failed, 0);

    // 3. A body phrase finds the paper, with an evidence snippet.
    let out = svc
        .search(&SearchRequest {
            q: "zanzibar".into(),
            fields: fts::FieldSel::all(),
            keyword: true,
            semantic: true,
            status: None,
            project: None,
        })
        .await
        .unwrap();
    assert!(!out.semantic.available, "no embedder configured");
    assert_eq!(out.results.len(), 1);
    let (paper, m) = &out.results[0];
    assert!(paper.rel_path.ends_with(".pdf"));
    assert_eq!(m.field, "body");
    assert!(m.snippet.contains("<mark>zanzibar</mark>"), "got: {}", m.snippet);

    // 4. Status agrees everything is indexed.
    let st = svc.status().await.unwrap();
    assert_eq!(st.fts.pending, 0);
    assert_eq!(st.fts.failed, 0);
}
