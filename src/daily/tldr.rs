//! The daily feed's summary types now live in the shared `crate::summary`
//! module. Re-exported here so the daily module's `tldr::` paths and its
//! `ChatClient` name stay stable.
pub use crate::summary::{generate_summary, Summarizer as ChatClient, Summary, FULL_TEXT_CAP};
