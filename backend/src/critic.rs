//! The local critique agent: builds the prompt, then proxies llama-server's token stream.
//!
//! Talks to llama.cpp's `llama-server` over its OpenAI-compatible endpoint. Nothing here
//! knows about GGUF files or model loading — that is llama-server's job.

use axum::response::sse::Event;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;

use crate::error::AppError;
use crate::scenarios::{Level, Scenario};
use crate::topology::Topology;

/// A stage of the interview. The critic grades one at a time — asking a 2.5B model to
/// cover requirements, math, structure and failure modes at once reliably drops one of
/// them, and a single-stage answer is short enough to run while the candidate is still
/// drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Requirements,
    Estimation,
    HighLevel,
    DeepDive,
    Bottlenecks,
}

impl Stage {
    /// What to grade, and what to explicitly leave alone. The "ignore" half matters as
    /// much as the "grade" half: without it the model drifts back to reviewing the whole
    /// design at every stage.
    fn focus(self) -> &'static str {
        match self {
            Stage::Requirements => "\
STAGE 1 of 5 — REQUIREMENTS. Grade only whether they have scoped the problem: which \
features are in and out, who the users are, what they have chosen NOT to build. Their \
notes are the answer here; the diagram is not. Do not comment on components or scale yet.",
            // "Check their maths" is not enough on its own: both a 2.5B and a 12B will
            // read a stated result and agree with it. They have to be told to recompute
            // from the inputs before they are allowed to judge, and told that the unit is
            // where the error usually hides.
            Stage::Estimation => "\
STAGE 2 of 5 — ESTIMATION. Grade only the arithmetic in their notes: QPS, storage, \
bandwidth, and whether those numbers follow from the stated requirements.

Before you write anything, recompute every number they claim, yourself, from their own \
assumptions. Do not accept a stated result because it looks plausible — derive it again \
and compare. State your figure next to theirs. Check the unit especially: a slip between \
GB and TB, or per-day and per-second, is the most common error here and you must catch \
it. An absent number is also a finding. Do not review the diagram's structure.",
            Stage::HighLevel => "\
STAGE 3 of 5 — HIGH-LEVEL DESIGN. Grade only the shape of the diagram: are the major \
components present, does the request path make sense end to end, is the data flow \
coherent. Stay out of the internals of any single component.",
            Stage::DeepDive => "\
STAGE 4 of 5 — DEEP DIVE. Pick the single most interesting component on their diagram \
and go deep on it alone: its data model, its interface, how it scales. Ignore every \
other part of the design.",
            Stage::Bottlenecks => "\
STAGE 5 of 5 — BOTTLENECKS AND TRADE-OFFS. Grade only what breaks: single points of \
failure, what saturates first under the stated load, and the trade-off they made without \
naming it. Assume the happy path already works.",
        }
    }
}

/// The interviewer's standing rules. Constant across stages and levels — the parts that
/// vary are appended by `system_prompt`.
const RULES: &str = "\
You are a senior engineer running a system design interview. You are reviewing a \
candidate's architecture diagram and notes, not writing the answer for them.

Rules:
- Never hand over a full solution. Surface the gap, then ask the question that exposes it.
- Ground every point in a component, connection, or note that is actually there, or in one \
  that is conspicuously missing.
- Be specific about failure modes: name the component that dies and what breaks downstream.
- If a back-of-the-envelope number contradicts their design, do the arithmetic and show it.";

/// The full system prompt for one stage at one level.
///
/// The length cap is load-bearing, not cosmetic: this runs automatically whenever the
/// canvas goes quiet, so a four-section answer per stage would never finish before the
/// next edit aborts it.
pub fn system_prompt(level: Level, stage: Stage) -> String {
    format!(
        "{RULES}\n\n{}\n\n{}\n\nAnswer in under 150 words, then ask exactly one \
question. No headings, no bullet lists longer than three items. Say nothing about any \
other stage.",
        level.calibration(),
        stage.focus(),
    )
}

/// Build the user-side prompt. Split out from the HTTP call so it is testable without a
/// running model.
pub fn build_prompt(scenario: &Scenario, topology: &Topology, notes: &str, level: Level) -> String {
    let bullets = |items: &mut dyn Iterator<Item = &str>| {
        items.map(|s| format!("  - {s}\n")).collect::<String>()
    };

    // Stages 1 and 2 are answered in prose, so an empty notes pane is itself the finding
    // and has to reach the model as such rather than as a missing section.
    let notes = if notes.trim().is_empty() {
        "(the candidate has written no notes)"
    } else {
        notes.trim()
    };

    format!(
        "SCENARIO: {}\n{}\n\nFunctional requirements:\n{}\nNon-functional requirements:\n{}\n\
         Rubric the design is graded against (do not read these out verbatim):\n{}\n\
         CANDIDATE'S NOTES:\n{notes}\n\n\
         CANDIDATE'S DIAGRAM:\n{}",
        scenario.title,
        scenario.prompt,
        bullets(&mut scenario.functional.iter().copied()),
        bullets(&mut scenario.non_functional.iter().copied()),
        bullets(&mut scenario.rubric_for(level)),
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
    system: String,
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
                { "role": "system", "content": system },
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

        let p = build_prompt(scenario, &topology, "1157 writes/sec", Level::Staff);
        assert!(p.contains("URL Shortener"));
        assert!(p.contains("Redis [cache]"), "diagram must reach the model: {p}");
        assert!(p.contains("Key generation strategy"), "rubric must reach the model");
        assert!(p.contains("1157 writes/sec"), "notes must reach the model: {p}");
    }

    #[test]
    fn a_junior_is_not_graded_on_staff_concerns() {
        let scenario = scenarios::find("url-shortener").unwrap();
        let topology = Topology::default();

        let junior = build_prompt(scenario, &topology, "", Level::Junior);
        let staff = build_prompt(scenario, &topology, "", Level::Staff);

        assert!(junior.contains("Key generation strategy"), "junior still gets the basics");
        assert!(
            !junior.contains("thundering herd"),
            "a staff-tagged rubric line leaked into a junior prompt: {junior}"
        );
        assert!(staff.contains("thundering herd"), "staff must still see it");
    }

    #[test]
    fn an_empty_notes_pane_is_stated_rather_than_left_blank() {
        let scenario = scenarios::find("url-shortener").unwrap();
        let p = build_prompt(scenario, &Topology::default(), "   ", Level::Senior);
        assert!(
            p.contains("has written no notes"),
            "an empty pane is the finding for stages 1-2, so it must be explicit: {p}"
        );
    }

    #[test]
    fn each_stage_asks_for_only_itself() {
        let p = system_prompt(Level::Senior, Stage::Estimation);
        assert!(p.contains("STAGE 2 of 5"), "the stage must be named: {p}");
        assert!(p.contains("QPS"), "estimation must ask for the numbers");
        assert!(
            !p.contains("DEEP DIVE") && !p.contains("BOTTLENECKS"),
            "one stage's prompt must not carry another's focus: {p}"
        );
        assert!(p.contains("150 words"), "the length cap is what makes auto-run affordable");
    }

    #[test]
    fn the_level_changes_who_is_in_the_room() {
        let junior = system_prompt(Level::Junior, Stage::HighLevel);
        let staff = system_prompt(Level::Staff, Stage::HighLevel);
        assert!(junior.contains("JUNIOR role"));
        assert!(staff.contains("STAFF role"));
        assert!(junior.contains("HIGH-LEVEL"), "the stage survives the level swap");
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
