//! The local critique agent: builds the prompt, then proxies llama-server's token stream.
//!
//! Talks to llama.cpp's `llama-server` over its OpenAI-compatible endpoint. Nothing here
//! knows about GGUF files or model loading — that is llama-server's job.

use axum::response::sse::Event;
use futures::{Stream, StreamExt};
use serde_json::json;
use std::convert::Infallible;

use crate::error::AppError;
use crate::scenarios::Scenario;
use crate::topology::Topology;

const SYSTEM_PROMPT: &str = "\
You are a staff engineer running a system design interview. You are reviewing a candidate's \
architecture diagram, not writing the answer for them.

Rules:
- Never hand over a full solution. Surface the gap, then ask the question that exposes it.
- Ground every point in a component or connection that is actually on their diagram, or in one \
  that is conspicuously missing.
- Be specific about failure modes: name the component that dies and what breaks downstream.
- If a back-of-the-envelope number contradicts their design, do the arithmetic and show it.

Reply in exactly these sections:
## What holds up
## Single points of failure
## Gaps against the requirements
## Questions I'd push on next";

/// Build the user-side prompt. Split out from the HTTP call so it is testable without a
/// running model.
pub fn build_prompt(scenario: &Scenario, topology: &Topology) -> String {
    let bullets = |items: &[&str]| {
        items
            .iter()
            .map(|s| format!("  - {s}\n"))
            .collect::<String>()
    };

    format!(
        "SCENARIO: {}\n{}\n\nFunctional requirements:\n{}\nNon-functional requirements:\n{}\n\
         Rubric the design is graded against (do not read these out verbatim):\n{}\n\
         CANDIDATE'S DIAGRAM:\n{}",
        scenario.title,
        scenario.prompt,
        bullets(scenario.functional),
        bullets(scenario.non_functional),
        bullets(scenario.rubric),
        topology.describe(),
    )
}

/// One piece of a streamed response.
#[derive(Debug, PartialEq)]
pub enum Delta {
    /// Text belonging to the answer itself.
    Content(String),
    /// Chain-of-thought from a reasoning model. Not part of the critique, but worth
    /// surfacing — a reasoning model can spend a minute here emitting no content at all,
    /// and a silent panel is indistinguishable from a hung one.
    Reasoning(String),
}

/// Extract one piece from an OpenAI-style streaming chunk. Returns `None` for the
/// role-only opening chunk and the final empty delta.
fn parse_delta(data: &str) -> Option<Delta> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let delta = &v["choices"][0]["delta"];

    // Content wins: some runners populate both on the same frame.
    if let Some(t) = delta["content"].as_str().filter(|t| !t.is_empty()) {
        return Some(Delta::Content(t.to_string()));
    }
    if let Some(t) = delta["reasoning"].as_str().filter(|t| !t.is_empty()) {
        return Some(Delta::Reasoning(t.to_string()));
    }
    None
}

pub async fn stream_critique(
    runner_url: &str,
    model: &str,
    prompt: String,
) -> Result<impl Stream<Item = Result<Event, Infallible>>, AppError> {
    let url = format!("{runner_url}/v1/chat/completions");

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&json!({
            // llama-server ignores this and serves whatever it was started with, but Ollama
            // resolves it and 404s on an unknown name. Configurable via SYSPRACTICE_MODEL.
            "model": model,
            "stream": true,
            "temperature": 0.4,
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user",   "content": prompt },
            ],
        }))
        .send()
        .await
        .map_err(|_| AppError::Runner(url.clone()))?
        .error_for_status()
        .map_err(|e| AppError::Runner(format!("{url} ({e})")))?;

    fn encode(t: &str) -> String {
        serde_json::to_string(t).expect("a string always serializes")
    }

    Ok(async_stream::stream! {
        let mut bytes = resp.bytes_stream();
        let mut buf = String::new();

        while let Some(chunk) = bytes.next().await {
            let Ok(chunk) = chunk else { break };
            buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(idx) = buf.find('\n') {
                let line: String = buf.drain(..=idx).collect();
                let Some(data) = line.trim().strip_prefix("data: ") else { continue };
                if data == "[DONE]" {
                    return;
                }
                // JSON-encoded so a token containing a newline stays on one SSE data line.
                let event = match parse_delta(data) {
                    Some(Delta::Content(t)) => Event::default().event("token").data(encode(&t)),
                    Some(Delta::Reasoning(t)) => Event::default().event("thinking").data(encode(&t)),
                    None => continue,
                };
                yield Ok::<Event, Infallible>(event);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios;
    use crate::topology::{Node, NodeKind};

    #[test]
    fn prompt_carries_the_rubric_and_the_diagram() {
        let scenario = scenarios::find("url-shortener").unwrap();
        let topology = Topology {
            nodes: vec![Node {
                id: "n1".into(),
                kind: NodeKind::Cache,
                label: "Redis".into(),
                x: 0.0,
                y: 0.0,
            }],
            edges: vec![],
        };

        let p = build_prompt(scenario, &topology);
        assert!(p.contains("URL Shortener"));
        assert!(p.contains("Redis [cache]"), "diagram must reach the model: {p}");
        assert!(p.contains("Key generation strategy"), "rubric must reach the model");
    }

    #[test]
    fn parses_streaming_deltas() {
        assert_eq!(
            parse_delta(r#"{"choices":[{"delta":{"content":"Redis"}}]}"#),
            Some(Delta::Content("Redis".into()))
        );
        // Opening chunk carries only the role.
        assert_eq!(parse_delta(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#), None);
        // Final chunk has an empty delta.
        assert_eq!(parse_delta(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#), None);
        // Malformed chunks must be skipped, not panic the stream.
        assert_eq!(parse_delta("not json"), None);
    }

    #[test]
    fn reasoning_frames_are_not_dropped() {
        // Verbatim frame shape from Ollama + gemma4:12b, which sends an empty `content`
        // alongside a populated `reasoning` for the whole thinking phase. Reading only
        // `content` throws away every one of them and the panel looks hung.
        let frame = r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"","reasoning":"The"},"finish_reason":null}]}"#;
        assert_eq!(parse_delta(frame), Some(Delta::Reasoning("The".into())));
    }

    #[test]
    fn content_wins_when_a_frame_carries_both() {
        let frame = r#"{"choices":[{"delta":{"content":"answer","reasoning":"thought"}}]}"#;
        assert_eq!(parse_delta(frame), Some(Delta::Content("answer".into())));
    }
}
