use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xuewen::models::Identifier;
use xuewen::resolve::{Resolution, Resolver};

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
    let res = resolver.resolve(&Identifier::Doi(doi.to_string())).await;

    match res {
        Resolution::Resolved(md) => {
            assert_eq!(md.source, "crossref");
            assert_eq!(
                md.title.as_deref(),
                Some("KGAT: Knowledge Graph Attention Network for Recommendation")
            );
            assert_eq!(md.doi.as_deref(), Some(doi));
            assert_eq!(md.year, Some(2019));
        }
        Resolution::Unresolved => panic!("expected Resolved"),
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
    let res = resolver.resolve(&Identifier::Arxiv(id.to_string())).await;

    match res {
        Resolution::Resolved(md) => {
            assert_eq!(md.source, "arxiv");
            assert_eq!(md.title.as_deref(), Some("Attention Is All You Need"));
            assert_eq!(md.arxiv_id.as_deref(), Some(id)); // stamped by the resolver
        }
        Resolution::Unresolved => panic!("expected Resolved"),
    }
}

#[tokio::test]
async fn http_error_degrades_to_unresolved() {
    // A server with no stubs returns 404 for everything.
    let server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi("10.9999/nope".to_string()))
        .await;
    assert_eq!(res, Resolution::Unresolved);
}

#[tokio::test]
async fn none_identifier_is_unresolved() {
    let resolver = Resolver::new(None).unwrap();
    assert_eq!(resolver.resolve(&Identifier::None).await, Resolution::Unresolved);
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
    let res = resolver.resolve(&Identifier::Doi(doi.to_string())).await;
    assert_eq!(res, Resolution::Unresolved);
}
