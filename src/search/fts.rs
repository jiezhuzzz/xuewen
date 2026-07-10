use anyhow::Result;
use std::path::Path;
use std::sync::Mutex;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, STORED, STRING, TEXT};
use tantivy::snippet::SnippetGenerator;
use tantivy::{doc, Index, IndexReader, IndexWriter, TantivyDocument, Term};

/// Which paper fields a query runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSel {
    pub title: bool,
    pub authors: bool,
    pub abstract_text: bool,
    pub body: bool,
}

impl FieldSel {
    pub fn all() -> Self {
        Self { title: true, authors: true, abstract_text: true, body: true }
    }

    /// Parse a `fields=title,body` CSV. Absent, empty, or all-unknown input
    /// falls back to every field (unknown values are ignored, never an error).
    pub fn parse(csv: Option<&str>) -> Self {
        let mut sel = Self { title: false, authors: false, abstract_text: false, body: false };
        for part in csv.unwrap_or("").split(',').map(str::trim) {
            match part {
                "title" => sel.title = true,
                "authors" => sel.authors = true,
                "abstract" => sel.abstract_text = true,
                "body" => sel.body = true,
                _ => {}
            }
        }
        if sel.any() {
            sel
        } else {
            Self::all()
        }
    }

    pub fn any(&self) -> bool {
        self.title || self.authors || self.abstract_text || self.body
    }

    /// Authors is the only selected field — semantic search is meaningless.
    pub fn authors_only(&self) -> bool {
        self.authors && !self.title && !self.abstract_text && !self.body
    }
}

/// One paper as a Tantivy document (all fields stored for snippets).
#[derive(Debug, Clone)]
pub struct PaperDoc {
    pub id: String,
    pub title: String,
    pub authors: String,
    pub venue: String,
    pub abstract_text: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct FtsHit {
    pub paper_id: String,
    pub score: f32,
    /// Which field the snippet came from: title|authors|abstract|body.
    pub field: String,
    /// HTML-safe: escaped text with <mark> highlights only.
    pub snippet_html: String,
}

struct FtsFields {
    id: Field,
    title: Field,
    authors: Field,
    venue: Field,
    abstract_text: Field,
    body: Field,
}

pub struct FtsIndex {
    index: Index,
    /// Lazy: read-only users (CLI search while `serve` runs) must not take
    /// Tantivy's single-writer lock.
    writer: Mutex<Option<IndexWriter>>,
    reader: IndexReader,
    f: FtsFields,
}

impl FtsIndex {
    /// Open (or create) the index at `dir`. On corruption the directory is
    /// wiped and recreated — it is derived data. Returns `(index, created)`;
    /// when `created` the caller must clear all FTS stamps so the sweep
    /// re-indexes everything.
    pub fn open(dir: &Path) -> Result<(Self, bool)> {
        std::fs::create_dir_all(dir)?;
        let fresh = !dir.join("meta.json").exists();
        match Self::try_open(dir) {
            Ok(idx) => Ok((idx, fresh)),
            Err(e) => {
                tracing::warn!("tantivy index at {} unusable ({e}); rebuilding", dir.display());
                std::fs::remove_dir_all(dir)?;
                std::fs::create_dir_all(dir)?;
                Ok((Self::try_open(dir)?, true))
            }
        }
    }

    fn try_open(dir: &Path) -> Result<Self> {
        let mut b = Schema::builder();
        let id = b.add_text_field("paper_id", STRING | STORED);
        let title = b.add_text_field("title", TEXT | STORED);
        let authors = b.add_text_field("authors", TEXT | STORED);
        let venue = b.add_text_field("venue", TEXT | STORED);
        let abstract_text = b.add_text_field("abstract", TEXT | STORED);
        let body = b.add_text_field("body", TEXT | STORED);
        let schema = b.build();
        let index = Index::open_or_create(MmapDirectory::open(dir)?, schema)?;
        let reader = index.reader()?;
        Ok(Self {
            index,
            writer: Mutex::new(None),
            reader,
            f: FtsFields { id, title, authors, venue, abstract_text, body },
        })
    }

    fn with_writer<T>(&self, op: impl FnOnce(&mut IndexWriter) -> Result<T>) -> Result<T> {
        let mut guard = self.writer.lock().expect("fts writer lock poisoned");
        if guard.is_none() {
            *guard = Some(self.index.writer(50_000_000)?);
        }
        let out = op(guard.as_mut().expect("writer just created"))?;
        // Make the change visible to the next search immediately (personal
        // scale: commit cost is negligible).
        self.reader.reload()?;
        Ok(out)
    }

    pub fn upsert(&self, d: &PaperDoc) -> Result<()> {
        self.with_writer(|w| {
            w.delete_term(Term::from_field_text(self.f.id, &d.id));
            w.add_document(doc!(
                self.f.id => d.id.clone(),
                self.f.title => d.title.clone(),
                self.f.authors => d.authors.clone(),
                self.f.venue => d.venue.clone(),
                self.f.abstract_text => d.abstract_text.clone(),
                self.f.body => d.body.clone(),
            ))?;
            w.commit()?;
            Ok(())
        })
    }

    pub fn delete(&self, paper_id: &str) -> Result<()> {
        self.with_writer(|w| {
            w.delete_term(Term::from_field_text(self.f.id, paper_id));
            w.commit()?;
            Ok(())
        })
    }

    pub fn search(&self, q: &str, sel: &FieldSel, limit: usize) -> Result<Vec<FtsHit>> {
        let q = q.trim();
        if q.is_empty() || !sel.any() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut fields = Vec::new();
        if sel.title { fields.push(self.f.title); }
        if sel.authors { fields.push(self.f.authors); }
        if sel.abstract_text { fields.push(self.f.abstract_text); }
        if sel.body { fields.push(self.f.body); }

        let mut parser = QueryParser::for_index(&self.index, fields);
        parser.set_field_boost(self.f.title, 3.0);
        parser.set_field_boost(self.f.authors, 2.0);
        parser.set_field_boost(self.f.abstract_text, 1.5);
        // Lenient: user input must never be a query syntax error.
        let (query, _errors) = parser.parse_query_lenient(q);

        let searcher = self.reader.searcher();
        let top = searcher.search(&query, &TopDocs::with_limit(limit))?;
        let mut out = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr)?;
            let paper_id = doc
                .get_first(self.f.id)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let (field, snippet_html) = self.best_snippet(&searcher, query.as_ref(), &doc, sel)?;
            out.push(FtsHit { paper_id, score, field, snippet_html });
        }
        Ok(out)
    }

    /// The first selected field (title > authors > abstract > body) with a
    /// highlighted fragment; falls back to the escaped title text.
    fn best_snippet(
        &self,
        searcher: &tantivy::Searcher,
        query: &dyn tantivy::query::Query,
        doc: &TantivyDocument,
        sel: &FieldSel,
    ) -> Result<(String, String)> {
        let candidates: [(&str, Field, bool); 4] = [
            ("title", self.f.title, sel.title),
            ("authors", self.f.authors, sel.authors),
            ("abstract", self.f.abstract_text, sel.abstract_text),
            ("body", self.f.body, sel.body),
        ];
        for (name, field, enabled) in candidates {
            if !enabled {
                continue;
            }
            let mut gen = SnippetGenerator::create(searcher, query, field)?;
            gen.set_max_num_chars(200);
            let snip = gen.snippet_from_doc(doc);
            if !snip.highlighted().is_empty() {
                let html = snip.to_html().replace("<b>", "<mark>").replace("</b>", "</mark>");
                return Ok((name.to_string(), html));
            }
        }
        let title = doc
            .get_first(self.f.title)
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        Ok(("title".to_string(), html_escape(title)))
    }
}

/// Minimal HTML escaping for snippet text we assemble ourselves.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, body: &str) -> PaperDoc {
        PaperDoc {
            id: id.into(),
            title: title.into(),
            authors: "Ada Lovelace ; Alan Turing".into(),
            venue: "USENIX Security".into(),
            abstract_text: "We defend binaries against automated analysis.".into(),
            body: body.into(),
        }
    }

    fn open_tmp() -> (FtsIndex, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
        (idx, dir)
    }

    #[test]
    fn parse_field_selection() {
        assert!(FieldSel::parse(None).title);
        let s = FieldSel::parse(Some("authors,body"));
        assert!(!s.title && s.authors && !s.abstract_text && s.body);
        // Unknown-only input falls back to all (never an error).
        assert!(FieldSel::parse(Some("bogus")).title);
        assert!(FieldSel::parse(Some("authors")).authors_only());
        assert!(!FieldSel::parse(Some("authors,title")).authors_only());
    }

    #[test]
    fn upsert_search_and_snippet() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "AntiFuzz: Impeding Fuzzing Audits", "fuzzing resistance techniques")).unwrap();
        idx.upsert(&doc("p2", "Unrelated Paper", "nothing to see here")).unwrap();

        let hits = idx.search("fuzzing", &FieldSel::all(), 10).unwrap();
        assert_eq!(hits[0].paper_id, "p1");
        assert!(hits[0].snippet_html.contains("<mark>"), "got: {}", hits[0].snippet_html);
        assert!(!hits.iter().any(|h| h.paper_id == "p2"));
    }

    #[test]
    fn field_selection_restricts_matching() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "A Title", "the body mentions quicksort")).unwrap();
        let sel = FieldSel { title: true, authors: false, abstract_text: false, body: false };
        assert!(idx.search("quicksort", &sel, 10).unwrap().is_empty());
        let sel = FieldSel { title: false, authors: false, abstract_text: false, body: true };
        let hits = idx.search("quicksort", &sel, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].field, "body");
    }

    #[test]
    fn title_hit_outranks_body_hit() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("in-title", "Quicksort Analysis", "some text")).unwrap();
        idx.upsert(&doc("in-body", "Sorting Survey", "quicksort quicksort quicksort")).unwrap();
        let hits = idx.search("quicksort", &FieldSel::all(), 10).unwrap();
        assert_eq!(hits[0].paper_id, "in-title");
    }

    #[test]
    fn upsert_replaces_and_delete_removes() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "Old Title", "b")).unwrap();
        idx.upsert(&doc("p1", "New Title", "b")).unwrap();
        assert!(idx.search("old", &FieldSel::all(), 10).unwrap().is_empty());
        assert_eq!(idx.search("new", &FieldSel::all(), 10).unwrap().len(), 1);
        idx.delete("p1").unwrap();
        assert!(idx.search("new", &FieldSel::all(), 10).unwrap().is_empty());
    }

    #[test]
    fn corrupt_dir_is_wiped_and_reports_created() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("meta.json"), b"not json").unwrap();
        let (_idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
    }

    #[test]
    fn escapes_html() {
        assert_eq!(html_escape("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn second_writer_on_same_dir_is_refused() {
        // Tantivy's writer lock is per-directory and enforced even within one
        // process, so this stands in for a `rebuild` run while `serve` is
        // still holding the lock on the same index dir.
        let dir = tempfile::tempdir().unwrap();
        let (idx1, _created) = FtsIndex::open(dir.path()).unwrap();
        idx1.upsert(&doc("p1", "A Title", "body")).unwrap(); // forces writer creation, lock held

        let (idx2, _created2) = FtsIndex::open(dir.path()).unwrap(); // open is lazy, succeeds
        assert!(idx2.delete("x").is_err(), "second writer on a locked dir must fail");
    }

    #[test]
    fn zero_limit_returns_empty_instead_of_panicking() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "AntiFuzz: Impeding Fuzzing Audits", "fuzzing resistance")).unwrap();
        assert!(idx.search("fuzzing", &FieldSel::all(), 0).unwrap().is_empty());
    }
}
