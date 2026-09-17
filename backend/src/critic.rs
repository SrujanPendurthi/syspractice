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
You are a senior engineer running a system design interview. The candidate is sitting \
across from you and you are speaking to them, so address them as \"you\". You are \
reviewing their architecture diagram and notes, not writing the answer for them.

Rules:
- Speak TO the candidate, never about them. Never write \"the candidate\", \"they\", or \
  \"them\" — that is talking to an observer who is not in the room.
- Never hand over a full solution. Surface the gap, then ask the question that exposes it.
- Ground every point in a component, connection, or note that is actually on their board, \
  or in one that is conspicuously missing.
- Be specific about failure modes: name the component that dies and what breaks downstream.
- If a back-of-the-envelope number contradicts the design, do the arithmetic and show it.";

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

// --- head-to-head grading ------------------------------------------------------

/// The judge's standing rules.
///
/// Three biases are named explicitly because a model exhibits all three unprompted, and
/// the first one is the reason the caller swaps which player is shown as A:
///
/// * position — the first option presented wins more than it should;
/// * verbosity — the bigger diagram reads as the better one;
/// * conflict avoidance — asked to choose, it will hedge to a draw given any excuse.
const JUDGE_RULES: &str = "\
You are judging a head-to-head system design exercise. Two candidates were given the same \
problem, the same requirements, the same rubric and the same amount of time. Decide which \
design better satisfies the rubric.

Rules:
- Judge against the rubric lines below and nothing else. The design that covers more of \
  them wins.
- More components is not better. A component the rubric never asks for is a failure mode \
  they added, not a feature.
- The order the designs appear in is not a signal, and neither is their size. Design A has \
  no advantage over Design B for being first.
- Name the specific rubric lines each design covered and each one it missed. Do not \
  praise in general terms.
- Address both candidates as \"Design A\" and \"Design B\" — you do not know who they are.

Answer in under 250 words. Your final line must be exactly one of:
VERDICT: A
VERDICT: B
VERDICT: DRAW
Use DRAW only when neither design covers a rubric line the other missed. \"Both were \
good\" is not a draw.";

/// The judge's system prompt for one level.
///
/// The level matters here for the same reason it matters to the critic: a junior match
/// graded on multi-region failover is graded on lines neither player was ever shown.
/// `rubric_for` has already filtered the list — this stops the model reaching past it.
pub fn judge_prompt(level: Level) -> String {
    let scope = match level {
        Level::Junior => "Both candidates are being judged at JUNIOR level. Do not credit \
or penalise anything to do with multi-region, consistency models, or hot-spot handling — \
those are out of scope and neither design was asked for them.",
        Level::Senior => "Both candidates are being judged at SENIOR level. Partitioning, \
failure modes, and a justified choice over its alternative are all fair game.",
        Level::Staff => "Both candidates are being judged at STAFF level. Assume both \
designs are competent, and separate them on trade-offs and on what breaks under load or \
partition.",
    };
    format!("{JUDGE_RULES}\n\n{scope}")
}

/// Which side of a match is shown to the judge as "Design A".
///
/// Decided by match id parity rather than by role, so the host is not permanently in the
/// slot that position bias favours. Deterministic, so a regrade lays the two designs out
/// the same way and cannot flip the result by reshuffling.
pub fn host_is_design_a(match_id: i64) -> bool {
    match_id % 2 == 0
}

/// Build the judge's prompt. Split from the HTTP call for the same reason
/// `build_prompt` is: it is the part worth testing without a model running.
pub fn build_verdict_prompt(
    scenario: &Scenario,
    level: Level,
    a: (&Topology, &str),
    b: (&Topology, &str),
) -> String {
    let side = |label: &str, (topology, notes): (&Topology, &str)| {
        let notes = if notes.trim().is_empty() {
            "(no notes written)"
        } else {
            notes.trim()
        };
        format!("DESIGN {label}\nNotes:\n{notes}\n\nDiagram:\n{}\n", topology.describe())
    };

    format!(
        "SCENARIO: {}\n{}\n\nFunctional requirements:\n{}\nNon-functional requirements:\n{}\n\
         Rubric both designs are graded against:\n{}\n\n{}\n{}",
        scenario.title,
        scenario.prompt,
        scenario.functional.iter().map(|s| format!("  - {s}\n")).collect::<String>(),
        scenario.non_functional.iter().map(|s| format!("  - {s}\n")).collect::<String>(),
        scenario.rubric_for(level).map(|s| format!("  - {s}\n")).collect::<String>(),
        side("A", a),
        side("B", b),
    )
}

/// Which design the judge picked, read off the verdict text.
///
/// Scans from the end: the instruction puts the line last, but a model that adds a
/// sign-off after it should not cost the match its result. An unreadable verdict is a
/// draw rather than an error — the text is still worth showing, and inventing a winner
/// from an answer that did not name one is worse than declining to.
pub fn parse_verdict(text: &str) -> Winner {
    for line in text.lines().rev() {
        let line = line.trim().trim_start_matches(['*', '#', ' ']);
        let Some(rest) = line.strip_prefix("VERDICT:") else { continue };
        return match rest.trim().trim_end_matches(['.', '*']).to_ascii_uppercase().as_str() {
            "A" => Winner::A,
            "B" => Winner::B,
            _ => Winner::Draw,
        };
    }
    Winner::Draw
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winner {
    A,
    B,
    Draw,
}

/// One non-streaming completion.
///
/// The critique endpoint streams because someone is watching it arrive. A verdict is
/// written once, stored, and read by both players — streaming it would mean two model
/// calls producing two different answers to the same question.
pub async fn complete(
    runner_url: &str,
    model: &str,
    system: String,
    prompt: String,
) -> Result<String, AppError> {
    let url = format!("{runner_url}/v1/chat/completions");

    let body: serde_json::Value = reqwest::Client::new()
        .post(&url)
        .json(&json!({
            "model": model,
            "stream": false,
            // Lower than the critique's 0.4: the same two designs should not change places
            // between one grading and the next.
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": prompt },
            ],
        }))
        .send()
        .await
        .map_err(|_| AppError::Runner(url.clone()))?
        .error_for_status()
        .map_err(|e| AppError::Runner(format!("{url} ({e})")))?
        .json()
        .await
        .map_err(|e| AppError::Runner(format!("{url} (unreadable answer: {e})")))?;

    // Some runners answer in `content`, a reasoning model may leave it empty and put the
    // answer in `reasoning`. An empty verdict is a runner failure, not a draw.
    let choice = &body["choices"][0]["message"];
    let text = choice["content"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| choice["reasoning"].as_str())
        .unwrap_or_default()
        .trim()
        .to_string();

    if text.is_empty() {
        return Err(AppError::Runner(format!("{url} (empty answer)")));
    }
    Ok(text)
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
    fn the_critic_is_told_to_address_the_candidate_directly() {
        // Without this the model drifts into reporting on the candidate to a third party:
        // "The candidate has provided no notes. What would you like them to implement?"
        for stage in [Stage::Requirements, Stage::Estimation, Stage::Bottlenecks] {
            let p = system_prompt(Level::Junior, stage);
            assert!(p.contains("Speak TO the candidate"), "{stage:?} lost the voice rule");
        }
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

    #[test]
    fn the_verdict_line_is_read_from_the_end_and_survives_decoration() {
        assert_eq!(parse_verdict("Design A partitioned.\nVERDICT: A"), Winner::A);
        assert_eq!(parse_verdict("VERDICT: B"), Winner::B);
        assert_eq!(parse_verdict("VERDICT: DRAW"), Winner::Draw);

        // Models decorate. None of these should cost a match its result.
        assert_eq!(parse_verdict("**VERDICT: B**"), Winner::B);
        assert_eq!(parse_verdict("verdict: a"), Winner::Draw, "the label is literal");
        assert_eq!(parse_verdict("VERDICT: a."), Winner::A);
        assert_eq!(parse_verdict("VERDICT: A\n\nGood luck to both of you!"), Winner::A);

        // Two lines, the last one wins — a model that restates its answer has restated it,
        // not changed its mind halfway up the page.
        assert_eq!(parse_verdict("VERDICT: A\nOn reflection:\nVERDICT: B"), Winner::B);

        // An unreadable verdict is a draw, not an invented winner. The text is still
        // worth showing; guessing which design it meant is not.
        assert_eq!(parse_verdict("Both were strong designs."), Winner::Draw);
        assert_eq!(parse_verdict("VERDICT: C"), Winner::Draw);
        assert_eq!(parse_verdict(""), Winner::Draw);
    }

    #[test]
    fn which_player_is_design_a_depends_on_the_match_not_the_role() {
        // Position bias is real: whichever design is shown first wins more than it should.
        // Fixing the host in slot A would hand that advantage to whoever created the
        // challenge, every time.
        assert!(host_is_design_a(2));
        assert!(!host_is_design_a(3));
        // Deterministic, so a regrade lays the same two designs out the same way rather
        // than flipping the result by reshuffling them.
        assert_eq!(host_is_design_a(7), host_is_design_a(7));
    }

    #[test]
    fn the_judge_sees_two_designs_and_neither_players_name() {
        let scenario = scenarios::find("url-shortener").unwrap();
        let mine = Topology {
            nodes: vec![Node { id: "n1".into(), kind: NodeKind::Cache, label: "Redis".into(), x: 0.0, y: 0.0 }],
            edges: vec![],
        };
        let theirs = Topology {
            nodes: vec![Node { id: "n2".into(), kind: NodeKind::Database, label: "Postgres".into(), x: 0.0, y: 0.0 }],
            edges: vec![],
        };

        let p = build_verdict_prompt(scenario, Level::Senior, (&mine, "1157 writes/sec"), (&theirs, ""));
        assert!(p.contains("DESIGN A") && p.contains("DESIGN B"));
        assert!(p.contains("Redis [cache]") && p.contains("Postgres [database]"), "both diagrams must reach it: {p}");
        assert!(p.contains("1157 writes/sec"), "notes must reach it");
        assert!(p.contains("(no notes written)"), "an empty pane is a finding, not a gap: {p}");
        assert!(p.contains("Key generation strategy"), "the rubric is what they are separated on");
        // The judge must not be told who is who — "host" and "guest" are roles, and one of
        // them is the person who picked the problem.
        assert!(!p.contains("host") && !p.contains("guest"), "a role label leaked to the judge: {p}");
    }

    #[test]
    fn a_junior_match_is_not_decided_on_staff_concerns() {
        let scenario = scenarios::find("url-shortener").unwrap();
        let t = Topology::default();
        let junior = build_verdict_prompt(scenario, Level::Junior, (&t, ""), (&t, ""));
        assert!(
            !junior.contains("thundering herd"),
            "a staff rubric line reached a junior match: {junior}"
        );
        assert!(judge_prompt(Level::Junior).contains("JUNIOR"));
        assert!(judge_prompt(Level::Staff).contains("STAFF"));
        // Hedging to a draw is the failure mode when a model is asked to choose.
        assert!(judge_prompt(Level::Senior).contains("VERDICT: DRAW"));
        assert!(judge_prompt(Level::Senior).contains("More components is not better"));
    }

}
