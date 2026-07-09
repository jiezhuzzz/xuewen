use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier {
    Doi(String),
    Arxiv(String),
    None,
}

/// Author list stored as a JSON array in a nullable TEXT column.
/// NULL ⇄ empty; unparseable stored JSON decodes to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Authors(pub Vec<String>);

impl sqlx::Type<sqlx::Sqlite> for Authors {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Sqlite>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for Authors {
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let raw = <Option<&str> as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        // Garbage JSON degrades to empty silently: this runs per-row on every list
        // fetch (no paper id in scope, and a warn here would spam on each poll).
        Ok(Authors(
            raw.and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
        ))
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for Authors {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        if self.0.is_empty() {
            return Ok(sqlx::encode::IsNull::Yes);
        }
        let json = serde_json::to_string(&self.0)?;
        <String as sqlx::Encode<sqlx::Sqlite>>::encode(json, buf)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PaperStatus {
    Resolved,
    NeedsReview,
}

/// The metadata block shared by resolution output and the stored record.
/// Column names match the `papers` table; flattened into `Paper` for sqlx/serde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct PaperMeta {
    pub title: Option<String>,
    #[sqlx(rename = "abstract")]
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Authors,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: PaperStatus,
}

/// A stored bibliographic record. Column names match the papers table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Paper {
    pub id: String,
    pub content_hash: String,
    pub rel_path: String,
    pub cite_key: Option<String>,
    pub added_at: String,
    pub deleted_at: Option<String>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub meta: PaperMeta,
}

/// A named group of related papers. Column names match the `projects` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
    pub created_at: String,
}

/// A project plus its membership count, for list views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct ProjectSummary {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub project: Project,
    pub paper_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_match_schema() {
        assert_eq!(
            serde_json::to_string(&PaperStatus::Resolved).unwrap(),
            "\"resolved\""
        );
        assert_eq!(
            serde_json::to_string(&PaperStatus::NeedsReview).unwrap(),
            "\"needs_review\""
        );
    }

    #[test]
    fn identifier_equality() {
        assert_eq!(
            Identifier::Doi("10.1/x".into()),
            Identifier::Doi("10.1/x".into())
        );
        assert_ne!(Identifier::Doi("10.1/x".into()), Identifier::None);
    }
}
