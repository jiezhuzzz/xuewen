use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xuewen::models::Identifier;
use xuewen::resolve::Resolver;

const ARXIV_FIXTURE: &str = include_str!("fixtures/arxiv_attention.xml");
const CROSSREF_FIXTURE: &str = include_str!("fixtures/crossref_kgat.json");

#[tokio::test]
async fn resolves_doi_via_crossref() {
    let server = MockServer::start().await;
    let doi = "10.1145/3292500.3330701";
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi(doi.to_string()), None)
        .await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "crossref");
            assert_eq!(
                md.title.as_deref(),
                Some("KGAT: Knowledge Graph Attention Network for Recommendation")
            );
            assert_eq!(md.doi.as_deref(), Some(doi));
            assert_eq!(md.year, Some(2019));
        }
        None => panic!("expected Resolved"),
    }
}

#[tokio::test]
async fn resolves_arxiv_via_api() {
    let server = MockServer::start().await;
    let id = "1706.03762";
    Mock::given(method("GET"))
        .and(path("/api/query"))
        .and(query_param("id_list", id))
        .respond_with(ResponseTemplate::new(200).set_body_string(ARXIV_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Arxiv(id.to_string()), None)
        .await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "arxiv");
            assert_eq!(md.title.as_deref(), Some("Attention Is All You Need"));
            assert_eq!(md.arxiv_id.as_deref(), Some(id)); // stamped by the resolver
        }
        None => panic!("expected Resolved"),
    }
}

#[tokio::test]
async fn http_error_degrades_to_unresolved() {
    // A server with no stubs returns 404 for everything.
    let server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi("10.9999/nope".to_string()), None)
        .await;
    assert_eq!(res, None);
}

#[tokio::test]
async fn none_identifier_is_unresolved() {
    let resolver = Resolver::new(None).unwrap();
    assert_eq!(resolver.resolve(&Identifier::None, None).await, None);
}

#[tokio::test]
async fn parse_error_degrades_to_unresolved() {
    // Server returns 200 but a malformed body: fetch succeeds, parse fails -> Unresolved.
    let server = MockServer::start().await;
    let doi = "10.1234/malformed";
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string("{ not valid json"))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi(doi.to_string()), None)
        .await;
    assert_eq!(res, None);
}

const DBLP_FIXTURE: &str = include_str!("fixtures/dblp_kgat.json");
const CROSSREF_SEARCH_FIXTURE: &str = include_str!("fixtures/crossref_search_kgat.json");

const KGAT_TITLE: &str = "KGAT: Knowledge Graph Attention Network for Recommendation";

#[tokio::test]
async fn resolves_title_via_dblp() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver.resolve(&Identifier::None, Some(KGAT_TITLE)).await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "dblp");
            assert_eq!(md.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
            assert_eq!(md.venue.as_deref(), Some("KDD"));
            assert_eq!(md.year, Some(2019));
        }
        None => panic!("expected Resolved via DBLP"),
    }
}

#[tokio::test]
async fn falls_back_to_crossref_search_when_dblp_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("query.bibliographic", KGAT_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver.resolve(&Identifier::None, Some(KGAT_TITLE)).await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "crossref");
            assert_eq!(md.doi.as_deref(), Some("10.1145/3292500.3330701"));
        }
        None => panic!("expected Resolved via Crossref fallback"),
    }
}

#[tokio::test]
async fn dblp_error_falls_back_to_crossref() {
    let server = MockServer::start().await;
    // DBLP returns a server error...
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    // ...Crossref bibliographic search then succeeds.
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("query.bibliographic", KGAT_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver.resolve(&Identifier::None, Some(KGAT_TITLE)).await;

    match res {
        Some(md) => assert_eq!(md.source, "crossref"),
        None => panic!("expected Crossref fallback after DBLP 500"),
    }
}

#[tokio::test]
async fn low_similarity_title_is_unresolved() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    // Crossref search has no stub -> 404 -> None. So overall Unresolved.

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver
        .resolve(
            &Identifier::None,
            Some("An Entirely Unrelated Paper Title About Frogs"),
        )
        .await;
    assert_eq!(res, None);
}

#[tokio::test]
async fn resolves_after_transient_429() {
    let server = MockServer::start().await;
    let doi = "10.1145/3292500.3330701";
    // First request is rate-limited, the retry succeeds.
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi(doi.to_string()), None)
        .await;

    assert!(res.is_some());
}

#[tokio::test]
async fn search_candidates_merges_both_sources_ungated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

    // A truncated query that the 0.85 gate would reject still yields the hit.
    let cands = resolver.search_candidates("KGAT: Knowledge Graph").await;
    assert_eq!(cands.len(), 1, "same DOI from both sources dedups to one");
    assert_eq!(cands[0].source, "dblp"); // DBLP queried first, wins the dedup
    assert_eq!(cands[0].doi.as_deref(), Some("10.1145/3292500.3330701"));

    // Empty query short-circuits without network.
    assert!(resolver.search_candidates("  ").await.is_empty());
}

#[tokio::test]
async fn search_candidates_degrades_when_one_source_fails() {
    let server = MockServer::start().await;
    // DBLP is down...
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    // ...Crossref search still answers.
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

    let cands = resolver.search_candidates("KGAT Knowledge Graph").await;
    assert_eq!(cands.len(), 1, "surviving source still yields candidates");
    assert_eq!(cands[0].source, "crossref");
}
