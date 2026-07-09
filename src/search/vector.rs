use anyhow::{bail, Result};
use serde_json::json;

const UPSERT_BATCH: usize = 64;

/// One chunk's embedding, ready for Qdrant. Chunk text stays in SQLite.
#[derive(Debug, Clone)]
pub struct ChunkPoint {
    pub paper_id: String,
    pub seq: i64,
    pub page: Option<i64>,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct VecHit {
    pub paper_id: String,
    pub seq: i64,
    pub page: Option<i64>,
    pub score: f32,
}

/// Restrict semantic search by chunk kind (seq 0 = title+abstract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqFilter {
    All,
    OnlySummary,
    OnlyBody,
}

/// Deterministic point id: UUIDv5 of "paper_id:seq" — re-upserts overwrite.
pub fn point_id(paper_id: &str, seq: i64) -> String {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, format!("{paper_id}:{seq}").as_bytes())
        .to_string()
}

/// Qdrant over its REST API (the official crate would pull in the whole
/// tonic/prost gRPC stack for four calls).
pub struct QdrantStore {
    http: reqwest::Client,
    base_url: String,
    collection: String,
    dims: usize,
}

impl QdrantStore {
    pub fn new(base_url: &str, collection: &str, dims: usize) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            collection: collection.to_string(),
            dims,
        })
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/collections/{}{suffix}", self.base_url, self.collection)
    }

    /// Create the collection if missing; verify vector size if present.
    pub async fn ensure_collection(&self) -> Result<()> {
        let resp = self.http.get(self.url("")).send().await?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await?;
            let size = body["result"]["config"]["params"]["vectors"]["size"]
                .as_u64()
                .unwrap_or(0) as usize;
            if size != self.dims {
                bail!(
                    "qdrant collection '{}' has vector size {size} but config dims = {} — \
                     run: xuewen index rebuild --vectors-only",
                    self.collection,
                    self.dims
                );
            }
            return Ok(());
        }
        if resp.status().as_u16() != 404 {
            bail!("qdrant GET collection: {}", resp.status());
        }
        let resp = self
            .http
            .put(self.url(""))
            .json(&json!({"vectors": {"size": self.dims, "distance": "Cosine"}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant create collection: {}", resp.status());
        }
        Ok(())
    }

    /// Drop and recreate the collection (vector rebuild after a dims change).
    pub async fn recreate_collection(&self) -> Result<()> {
        let resp = self.http.delete(self.url("")).send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            bail!("qdrant delete collection: {}", resp.status());
        }
        let resp = self
            .http
            .put(self.url(""))
            .json(&json!({"vectors": {"size": self.dims, "distance": "Cosine"}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant create collection: {}", resp.status());
        }
        Ok(())
    }

    pub async fn upsert(&self, points: &[ChunkPoint]) -> Result<()> {
        for batch in points.chunks(UPSERT_BATCH) {
            let body = json!({
                "points": batch.iter().map(|p| json!({
                    "id": point_id(&p.paper_id, p.seq),
                    "vector": p.vector,
                    "payload": {"paper_id": p.paper_id, "seq": p.seq, "page": p.page},
                })).collect::<Vec<_>>()
            });
            let resp = self
                .http
                .put(format!("{}?wait=true", self.url("/points")))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                bail!("qdrant upsert: {}", resp.status());
            }
        }
        Ok(())
    }

    pub async fn search(
        &self,
        vector: &[f32],
        limit: usize,
        filter: SeqFilter,
    ) -> Result<Vec<VecHit>> {
        let mut body = json!({"vector": vector, "limit": limit, "with_payload": true});
        match filter {
            SeqFilter::All => {}
            SeqFilter::OnlySummary => {
                body["filter"] = json!({"must": [{"key": "seq", "match": {"value": 0}}]});
            }
            SeqFilter::OnlyBody => {
                body["filter"] = json!({"must": [{"key": "seq", "range": {"gte": 1}}]});
            }
        }
        let resp = self
            .http
            .post(self.url("/points/search"))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant search: {}", resp.status());
        }
        let body: serde_json::Value = resp.json().await?;
        let hits = body["result"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| {
                        Some(VecHit {
                            paper_id: h["payload"]["paper_id"].as_str()?.to_string(),
                            seq: h["payload"]["seq"].as_i64()?,
                            page: h["payload"]["page"].as_i64(),
                            score: h["score"].as_f64()? as f32,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(hits)
    }

    pub async fn delete_paper(&self, paper_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}?wait=true", self.url("/points/delete")))
            .json(&json!({"filter": {"must": [{"key": "paper_id", "match": {"value": paper_id}}]}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant delete: {}", resp.status());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn store(server: &MockServer) -> QdrantStore {
        QdrantStore::new(&server.uri(), "xuewen", 4).unwrap()
    }

    #[test]
    fn point_ids_are_deterministic_uuids() {
        let a = point_id("p1", 0);
        assert_eq!(a, point_id("p1", 0));
        assert_ne!(a, point_id("p1", 1));
        assert_ne!(a, point_id("p2", 0));
        assert!(uuid::Uuid::parse_str(&a).is_ok());
    }

    #[tokio::test]
    async fn ensure_creates_missing_collection() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen"))
            .and(body_partial_json(json!({"vectors": {"size": 4, "distance": "Cosine"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn ensure_rejects_dims_mismatch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 8, "distance": "Cosine"}}}}
            })))
            .mount(&server)
            .await;
        let err = store(&server).ensure_collection().await.unwrap_err().to_string();
        assert!(err.contains("rebuild --vectors-only"), "got: {err}");
    }

    #[tokio::test]
    async fn upsert_sends_points_with_payload() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .and(body_partial_json(json!({"points": [{"payload": {"paper_id": "p1", "seq": 0}}]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1)
            .mount(&server)
            .await;
        let pts = vec![ChunkPoint { paper_id: "p1".into(), seq: 0, page: None, vector: vec![0.1; 4] }];
        store(&server).upsert(&pts).await.unwrap();
    }

    #[tokio::test]
    async fn search_parses_hits_and_applies_seq_filter() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/search"))
            .and(body_partial_json(json!({"filter": {"must": [{"key": "seq", "range": {"gte": 1}}]}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [
                    {"id": "x", "score": 0.9, "payload": {"paper_id": "p1", "seq": 3, "page": 7}},
                    {"id": "y", "score": 0.5, "payload": {"paper_id": "p2", "seq": 1, "page": 2}}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let hits = store(&server).search(&[0.1; 4], 10, SeqFilter::OnlyBody).await.unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].paper_id, "p1");
        assert_eq!(hits[0].seq, 3);
        assert_eq!(hits[0].page, Some(7));
        assert!(hits[0].score > hits[1].score);
    }

    #[tokio::test]
    async fn delete_paper_filters_on_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/delete"))
            .and(body_partial_json(json!({"filter": {"must": [{"key": "paper_id", "match": {"value": "p1"}}]}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).delete_paper("p1").await.unwrap();
    }
}
