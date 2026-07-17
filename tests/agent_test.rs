mod common;

use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use xuewen::agent::{AgentEvent, AgentService, TurnMessage, TurnPaper, TurnRequest};
use xuewen::config::{AgentBackendConfig, AgentConfig};

fn stub_cfg(timeout_secs: u64) -> AgentConfig {
    AgentConfig {
        claude_code: Some(AgentBackendConfig::default()),
        codex: Some(AgentBackendConfig {
            model: Some("gpt-5.2-codex".into()),
        }),
        timeout_secs,
        runner: Some(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/stub_runner.mjs"
        ))),
        ..AgentConfig::default()
    }
}

fn turn(question: &str) -> TurnRequest {
    TurnRequest {
        backend: "claude_code".into(),
        model: None,
        workspace: std::env::temp_dir().to_string_lossy().into_owned(),
        has_repo: false,
        paper: TurnPaper {
            title: Some("T".into()),
            authors: vec![],
            venue: None,
            year: None,
            cite_key: None,
        },
        transcript: vec![TurnMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
        question: question.into(),
    }
}

async fn collect(svc: &Arc<AgentService>, req: TurnRequest) -> Vec<AgentEvent> {
    let s = svc.run_turn(req);
    futures_util::pin_mut!(s);
    let mut out = Vec::new();
    while let Some(ev) = s.next().await {
        out.push(ev);
    }
    out
}

#[tokio::test]
async fn run_turn_streams_tool_deltas_done() {
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    let evs = collect(&svc, turn("what is this?")).await;
    assert!(
        matches!(&evs[0], AgentEvent::Tool { name, detail } if name == "Read" && detail == "paper.txt")
    );
    assert!(matches!(&evs[1], AgentEvent::Delta { text } if text == "Hel"));
    assert!(matches!(&evs[2], AgentEvent::Delta { text } if text == "lo from claude_code"));
    assert!(matches!(evs.last().unwrap(), AgentEvent::Done));
}

#[tokio::test]
async fn run_turn_surfaces_runner_errors() {
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    let evs = collect(&svc, turn("please fail")).await;
    assert!(matches!(evs.last().unwrap(), AgentEvent::Error { message } if message == "boom"));
}

#[tokio::test]
async fn run_turn_times_out_hung_runner() {
    let svc = AgentService::from_config(&stub_cfg(1)).unwrap();
    let evs = collect(&svc, turn("hang forever")).await;
    assert!(
        matches!(evs.last().unwrap(), AgentEvent::Error { message } if message.contains("timed out"))
    );
}

#[tokio::test]
async fn run_turn_reports_silent_death_with_stderr() {
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    let evs = collect(&svc, turn("die now")).await;
    assert!(
        matches!(evs.last().unwrap(), AgentEvent::Error { message } if message.contains("stub exploded"))
    );
}

#[tokio::test]
async fn from_config_gates_on_backends() {
    assert!(AgentService::from_config(&AgentConfig::default()).is_none());
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    assert_eq!(svc.backends.len(), 2);
    assert_eq!(
        svc.backend("codex").unwrap().model.as_deref(),
        Some("gpt-5.2-codex")
    );
    assert!(svc.backend("nope").is_none());
}
