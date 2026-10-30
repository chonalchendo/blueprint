//! What the index's reads return: a row of `list`, a hit of `search`, and a record as `Index::records` reads it.

use serde::Serialize;
use serde_json::{Map, Value};

/// One row of `list`. A field it skips is optional in TypeScript, not null.
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(optional_fields)]
pub struct Row {
    /// The path id.
    pub id: String,
    /// The kind.
    pub kind: String,
    /// The title; records have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The status; a record's is its `state`, where its kind has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub under: Option<String>,
    /// A design's kind, from its path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_kind: Option<String>,
    /// A design's scope, from its path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// A delta's milestone; none for an editorial delta.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    /// A delta's cause.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    /// The delta a delta chains on (34, design change 4), so a session finds a chain's tip from the list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chains: Option<String>,
}

/// One row of `search`.
#[derive(Debug, Serialize, ts_rs::TS)]
pub struct Hit {
    /// The path id.
    pub id: String,
    /// The kind.
    pub kind: String,
    /// The title.
    pub title: Option<String>,
    /// BM25, higher is better.
    pub rank: f64,
    /// Matching text from the best column, the terms marked `«` and `»`.
    pub snippet: String,
}

/// One record as `Index::records` reads it.
#[derive(Debug)]
pub struct Record {
    /// The path id.
    pub id: String,
    /// The header, as `show` prints it.
    pub header: Map<String, Value>,
    /// The body: what the record's appends wrote.
    pub body: String,
}
