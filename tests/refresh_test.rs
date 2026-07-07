mod common;

use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::db;
use xuewen::models::Paper;
use xuewen::refresh::{self, RefreshTarget};
use xuewen::resolve::Resolver;

const CROSSREF_FIXTURE: &str = include_str!("fixtures/crossref_kgat.json");

/// A minimal stored paper for seeding; callers set the fields they care about.
fn seed_paper(id: &str, hash: &str, rel_path: &str, status: &str) -> Paper {
    Paper {
        id: id.into(),
        content_hash: hash.into(),
        rel_path: rel_path.into(),
        title: None,
        abstract_text: None,
        authors: None,
        venue: None,
        year: None,
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        cite_key: None,
        url: None,
        source: None,
        status: status.into(),
        added_at: "2026-07-07T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn needs_review_reresolves_and_refiles() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let hash = "deadbeefhash";
    let unsorted = library.join(format!("_unsorted/{hash}.pdf"));
    std::fs::create_dir_all(unsorted.parent().unwrap()).unwrap();

    // The stored PDF carries the DOI so re-resolution can identify it.
    let doi = "10.1145/3292500.3330701";
    common::write_test_pdf(
        &unsorted,
        &["Some Header", &format!("https://doi.org/{doi}")],
    );

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p = seed_paper(
        "01890000-0000-7000-8000-0000000000a1",
        hash,
        &format!("_unsorted/{hash}.pdf"),
        "needs_review",
    );
    db::insert_paper(&pool, &p).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 1);
    assert_eq!(summary.refiled, 1);

    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    assert_eq!(got.status, "resolved");
    assert_eq!(got.cite_key.as_deref(), Some("wang2019kgat"));
    assert_eq!(got.rel_path, "wang2019kgat.pdf");
    assert!(library.join("wang2019kgat.pdf").exists());
    assert!(!unsorted.exists());
}

#[tokio::test]
async fn resolved_paper_refiles_without_reresolving() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "abc123hash";
    let old = library.join(format!("{hash}.pdf"));
    // Content is irrelevant — a resolved paper is not re-resolved under the default target.
    common::write_test_pdf(&old, &["Whatever"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000b2",
        hash,
        &format!("{hash}.pdf"),
        "resolved",
    );
    p.title = Some("Deep Residual Learning for Image Recognition".into());
    p.authors = Some(r#"["Kaiming He"]"#.into());
    p.year = Some(2016);
    p.source = Some("crossref".into());
    db::insert_paper(&pool, &p).await.unwrap();

    // Unreachable resolver: a resolved paper must NOT be re-resolved, so no HTTP happens.
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap();

    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 0);
    assert_eq!(summary.refiled, 1);

    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    // Metadata unchanged; only the location moved to the cite-key path.
    assert_eq!(got.status, "resolved");
    assert_eq!(
        got.title.as_deref(),
        Some("Deep Residual Learning for Image Recognition")
    );
    assert_eq!(got.year, Some(2016));
    assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
    assert_eq!(got.rel_path, "he2016deep.pdf");
    assert!(library.join("he2016deep.pdf").exists());
    assert!(!old.exists());
}

#[tokio::test]
async fn all_does_not_downgrade_resolved_on_failed_reresolve() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "keepmehash";
    // A resolved paper at its cite-key path; the PDF has NO identifier, so a
    // re-resolve attempt yields Unresolved (title search hits an empty mock → 404).
    let f = library.join("he2016deep.pdf");
    common::write_test_pdf(&f, &["No identifier in this text"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000f6",
        hash,
        "he2016deep.pdf",
        "resolved",
    );
    p.title = Some("Deep Residual Learning for Image Recognition".into());
    p.authors = Some(r#"["Kaiming He"]"#.into());
    p.year = Some(2016);
    p.cite_key = Some("he2016deep".into());
    p.source = Some("crossref".into());
    db::insert_paper(&pool, &p).await.unwrap();

    // Reachable mock with no mounts → every lookup 404 → Unresolved.
    let server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::All)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 0); // downgrade prevented → not counted

    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    // Metadata preserved; still resolved; still at its cite-key path.
    assert_eq!(got.status, "resolved");
    assert_eq!(
        got.title.as_deref(),
        Some("Deep Residual Learning for Image Recognition")
    );
    assert_eq!(got.authors.as_deref(), Some(r#"["Kaiming He"]"#));
    assert_eq!(got.year, Some(2016));
    assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
    assert_eq!(got.rel_path, "he2016deep.pdf");
    assert!(f.exists());
}

#[tokio::test]
async fn refresh_by_id_prefix_targets_one() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // P1: targeted; its PDF carries a DOI so re-resolution succeeds.
    let h1 = "hash0001";
    let doi = "10.1145/3292500.3330701";
    let f1 = library.join(format!("{h1}.pdf"));
    common::write_test_pdf(&f1, &["Header", &format!("https://doi.org/{doi}")]);
    // P2: not targeted; must be untouched.
    let h2 = "hash0002";
    let f2 = library.join(format!("{h2}.pdf"));
    common::write_test_pdf(&f2, &["Other"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p1 = seed_paper(
        "01890000-0000-7000-8000-0000000000a1",
        h1,
        &format!("{h1}.pdf"),
        "needs_review",
    );
    db::insert_paper(&pool, &p1).await.unwrap();
    let mut p2 = seed_paper(
        "01890000-0000-7000-8000-0000000000b2",
        h2,
        &format!("{h2}.pdf"),
        "resolved",
    );
    p2.title = Some("Some Resolved Paper".into());
    db::insert_paper(&pool, &p2).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    // A prefix unique to P1 (P2's id ends ...0000b2).
    let summary = refresh::run(
        &pool,
        &library,
        &resolver,
        None,
        RefreshTarget::One("01890000-0000-7000-8000-0000000000a".into()),
    )
    .await
    .unwrap();
    assert_eq!(summary.processed, 1);

    let got1 = db::get_by_id(&pool, &p1.id).await.unwrap().unwrap();
    assert_eq!(got1.rel_path, "wang2019kgat.pdf");
    assert_eq!(got1.status, "resolved");

    // P2 completely untouched.
    let got2 = db::get_by_id(&pool, &p2.id).await.unwrap().unwrap();
    assert_eq!(got2.rel_path, format!("{h2}.pdf"));
    assert_eq!(got2.cite_key, None);
    assert!(f2.exists());
}

#[tokio::test]
async fn all_reresolves_resolved_paper() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "stalehash1";
    let doi = "10.1145/3292500.3330701";
    // The paper currently lives at a stale cite-key path; put the real PDF there.
    let f = library.join("old2000stale.pdf");
    common::write_test_pdf(&f, &["Header", &format!("https://doi.org/{doi}")]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000d4",
        hash,
        "old2000stale.pdf",
        "resolved",
    );
    p.title = Some("Old Stale Title".into());
    p.authors = Some(r#"["Old Author"]"#.into());
    p.year = Some(2000);
    p.cite_key = Some("old2000stale".into());
    db::insert_paper(&pool, &p).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::All)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 1);

    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    // Stale metadata replaced by the freshly-resolved record, and re-filed.
    assert_eq!(
        got.title.as_deref(),
        Some("KGAT: Knowledge Graph Attention Network for Recommendation")
    );
    assert_eq!(got.year, Some(2019));
    assert_eq!(got.rel_path, "wang2019kgat.pdf");
    assert!(library.join("wang2019kgat.pdf").exists());
    assert!(!f.exists());
}

#[tokio::test]
async fn refiles_two_same_base_papers_with_distinct_keys() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // Two already-resolved papers with identical author/year/title → the same
    // cite-key base, both still at their hash paths. One refresh pass must give
    // them distinct keys (the second suffixed), proving the sequential
    // commit-then-query disambiguation excludes only self and sees prior writes.
    let h1 = "samebaseA";
    let h2 = "samebaseB";
    let f1 = library.join(format!("{h1}.pdf"));
    let f2 = library.join(format!("{h2}.pdf"));
    common::write_test_pdf(&f1, &["A"]);
    common::write_test_pdf(&f2, &["B"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    let mut p1 = seed_paper(
        "01890000-0000-7000-8000-00000000aa01",
        h1,
        &format!("{h1}.pdf"),
        "resolved",
    );
    p1.title = Some("Deep Residual Learning for Image Recognition".into());
    p1.authors = Some(r#"["Kaiming He"]"#.into());
    p1.year = Some(2016);
    p1.added_at = "2026-07-07T00:00:00Z".into();
    db::insert_paper(&pool, &p1).await.unwrap();

    let mut p2 = seed_paper(
        "01890000-0000-7000-8000-00000000bb02",
        h2,
        &format!("{h2}.pdf"),
        "resolved",
    );
    p2.title = Some("Deep Residual Learning for Image Recognition".into());
    p2.authors = Some(r#"["Kaiming He"]"#.into());
    p2.year = Some(2016);
    p2.added_at = "2026-07-07T00:00:01Z".into(); // ordered after p1
    db::insert_paper(&pool, &p2).await.unwrap();

    // Resolved papers under the default target are re-filed but not re-resolved,
    // so the (unreachable) resolver is never called.
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap();
    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.refiled, 2);

    // First paper keeps the plain base; the second is disambiguated with a suffix.
    let got1 = db::get_by_id(&pool, &p1.id).await.unwrap().unwrap();
    let got2 = db::get_by_id(&pool, &p2.id).await.unwrap().unwrap();
    assert_eq!(got1.cite_key.as_deref(), Some("he2016deep"));
    assert_eq!(got1.rel_path, "he2016deep.pdf");
    assert_eq!(got2.cite_key.as_deref(), Some("he2016deepa"));
    assert_eq!(got2.rel_path, "he2016deepa.pdf");
    assert!(library.join("he2016deep.pdf").exists());
    assert!(library.join("he2016deepa.pdf").exists());
}
