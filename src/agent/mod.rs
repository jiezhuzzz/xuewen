//! Agent Ask: a per-turn Node sidecar drives the Claude Code / Codex SDKs
//! over a read-only per-paper workspace (`<library_root>/agent/<paper_id>/`
//! holding `paper.txt` and, when attached, `repo/`). One JSON request goes
//! in on stdin; JSON-lines events come back on stdout.

pub mod code;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::config::AgentConfig;
use crate::models::Paper;

#[derive(Debug, Clone)]
pub struct AgentBackend {
    pub id: String,
    pub label: String,
    pub model: Option<String>,
}

/// One event from the runner (`{"type": "..."}` JSON-lines).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Delta { text: String },
    Tool { name: String, detail: String },
    Done,
    Error { message: String },
}

#[derive(Debug, serde::Serialize)]
pub struct TurnPaper {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub cite_key: Option<String>,
}

impl TurnPaper {
    pub fn from_paper(p: &Paper) -> Self {
        Self {
            title: p.meta.title.clone(),
            authors: p.meta.authors.0.clone(),
            venue: p.meta.venue.clone(),
            year: p.meta.year,
            cite_key: p.cite_key.clone(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TurnMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TurnRequest {
    pub backend: String,
    pub model: Option<String>,
    pub workspace: String,
    #[serde(rename = "hasRepo")]
    pub has_repo: bool,
    pub paper: TurnPaper,
    pub transcript: Vec<TurnMessage>,
    pub question: String,
}

/// `<library_root>/agent/<paper_id>` — the per-paper agent workspace.
pub fn workspace_dir(library_root: &Path, paper_id: &str) -> PathBuf {
    library_root.join("agent").join(paper_id)
}

pub struct AgentService {
    pub backends: Vec<AgentBackend>,
    pub max_repo_mb: u64,
    runner: PathBuf,
    timeout: Duration,
}

impl AgentService {
    /// `None` when no backend subsection is configured — the feature is off.
    pub fn from_config(cfg: &AgentConfig) -> Option<Arc<Self>> {
        let mut backends = Vec::new();
        if let Some(b) = &cfg.claude_code {
            backends.push(AgentBackend {
                id: "claude_code".into(),
                label: "Claude Code".into(),
                model: b.model.clone(),
            });
        }
        if let Some(b) = &cfg.codex {
            backends.push(AgentBackend {
                id: "codex".into(),
                label: "Codex".into(),
                model: b.model.clone(),
            });
        }
        if backends.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            backends,
            max_repo_mb: cfg.max_repo_mb,
            runner: cfg
                .runner
                .clone()
                .unwrap_or_else(|| PathBuf::from("agent-runner/src/runner.mjs")),
            timeout: Duration::from_secs(cfg.timeout_secs),
        }))
    }

    pub fn backend(&self, id: &str) -> Option<&AgentBackend> {
        self.backends.iter().find(|b| b.id == id)
    }

    /// Startup sanity check: the feature stays enabled either way (per-turn
    /// errors carry the actionable message); this only powers a launch warning.
    pub async fn preflight(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.runner.exists() {
            problems.push(format!(
                "agent runner not found at {} — run `npm --prefix agent-runner install` or set [ai.agent].runner",
                self.runner.display()
            ));
        }
        match Command::new("node").arg("--version").output().await {
            Ok(o) if o.status.success() => {}
            _ => problems.push("`node` not found on PATH — the agent needs Node ≥ 20".into()),
        }
        problems
    }

    /// The paper's workspace, created lazily. `paper.txt` is written once;
    /// extraction failure writes a placeholder so the workspace always has it.
    pub async fn ensure_workspace(&self, library_root: &Path, paper: &Paper) -> Result<PathBuf> {
        let ws = workspace_dir(library_root, &paper.id);
        tokio::fs::create_dir_all(&ws).await?;
        let txt = ws.join("paper.txt");
        if !tokio::fs::try_exists(&txt).await? {
            let pdf = library_root.join(&paper.rel_path);
            let text = tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&pdf))
                .await?
                .unwrap_or_else(|e| {
                    tracing::warn!("agent workspace: text extraction failed: {e}");
                    "(The paper's text could not be extracted.)".to_string()
                });
            tokio::fs::write(&txt, text).await?;
        }
        Ok(ws)
    }

    /// Spawn the runner for one turn and stream its events. Dropping the
    /// stream (Stop / client disconnect) kills the child; a hung runner is
    /// killed at the turn timeout.
    pub fn run_turn(
        self: &Arc<Self>,
        req: TurnRequest,
    ) -> impl futures_util::Stream<Item = AgentEvent> {
        let runner = self.runner.clone();
        let timeout = self.timeout;
        async_stream::stream! {
            let mut child = match Command::new("node")
                .arg(&runner)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    yield AgentEvent::Error { message: format!("could not start the agent runner (is Node installed?): {e}") };
                    return;
                }
            };
            let body = match serde_json::to_vec(&req) {
                Ok(b) => b,
                Err(e) => { yield AgentEvent::Error { message: e.to_string() }; return; }
            };
            let mut stdin = child.stdin.take().expect("stdin piped");
            if let Err(e) = stdin.write_all(&body).await {
                yield AgentEvent::Error { message: format!("could not reach the agent runner: {e}") };
                return;
            }
            drop(stdin); // EOF marks the request complete

            let stdout = child.stdout.take().expect("stdout piped");
            let mut lines = BufReader::new(stdout).lines();
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                match tokio::time::timeout_at(deadline, lines.next_line()).await {
                    Err(_) => {
                        yield AgentEvent::Error { message: format!("the agent timed out after {}s", timeout.as_secs()) };
                        return; // kill_on_drop reaps the child
                    }
                    Ok(Err(e)) => {
                        yield AgentEvent::Error { message: e.to_string() };
                        return;
                    }
                    Ok(Ok(None)) => break, // stdout closed
                    Ok(Ok(Some(line))) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<AgentEvent>(&line) {
                            Ok(ev) => {
                                let terminal = matches!(ev, AgentEvent::Done | AgentEvent::Error { .. });
                                yield ev;
                                if terminal { return; }
                            }
                            Err(_) => tracing::debug!("agent runner noise: {line}"),
                        }
                    }
                }
            }
            // stdout closed without done/error: surface the stderr tail.
            let mut err = String::new();
            if let Some(mut se) = child.stderr.take() {
                use tokio::io::AsyncReadExt;
                let _ = se.read_to_string(&mut err).await;
            }
            let code = child.wait().await.ok().and_then(|s| s.code());
            let tail: String = err.chars().take(400).collect();
            yield AgentEvent::Error {
                message: format!("the agent runner exited unexpectedly (code {code:?}): {tail}"),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_is_under_library_agent() {
        assert_eq!(
            workspace_dir(Path::new("/lib"), "p1"),
            PathBuf::from("/lib/agent/p1")
        );
    }

    #[tokio::test]
    async fn ensure_workspace_keeps_existing_paper_txt() {
        let dir = tempfile::tempdir().unwrap();
        let ws = workspace_dir(dir.path(), "p1");
        tokio::fs::create_dir_all(&ws).await.unwrap();
        tokio::fs::write(ws.join("paper.txt"), "cached text")
            .await
            .unwrap();
        let svc = AgentService::from_config(&crate::config::AgentConfig {
            claude_code: Some(Default::default()),
            ..Default::default()
        })
        .unwrap();
        let paper = crate::models::Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: "p.pdf".into(),
            cite_key: Some("smith2024".into()),
            added_at: "2026-01-01".into(),
            deleted_at: None,
            starred: false,
            meta: crate::models::PaperMeta {
                title: Some("A Great Paper".into()),
                abstract_text: None,
                authors: crate::models::Authors(vec!["A. Smith".into()]),
                venue: None,
                year: Some(2024),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: crate::models::PaperStatus::Resolved,
            },
        };
        let got = svc.ensure_workspace(dir.path(), &paper).await.unwrap();
        assert_eq!(got, ws);
        assert_eq!(
            std::fs::read_to_string(ws.join("paper.txt")).unwrap(),
            "cached text"
        );
    }
}
