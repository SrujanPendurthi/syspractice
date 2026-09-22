//! The local critique agent: builds the prompt, then proxies llama-server's token stream.
//!
//! Talks to llama.cpp's `llama-server` over its OpenAI-compatible endpoint. Nothing here
//! knows about GGUF files or model loading — that is llama-server's job.

use axum::response::sse::Event;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;

use crate::calc;
use crate::error::AppError;
use crate::scenarios::{Level, Scenario, Track};
use crate::topology::Topology;

/// A stage of the interview. The critic grades one at a time — asking a 2.5B model to
/// cover requirements, math, structure and failure modes at once reliably drops one of
/// them, and a single-stage answer is short enough to run while the candidate is still
/// drawing.
///
/// This is the union of every track's stages, not one sequence. `Track::stages` says
/// which of them a given interview walks and in what order; nothing here implies an
/// ordering, and a stage belongs to exactly one track apart from the two shared openers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    // Shared by every track.
    Requirements,
    Estimation,
    // Classic.
    HighLevel,
    DeepDive,
    Bottlenecks,
    // Machine learning.
    DataAndFeatures,
    Modeling,
    Serving,
    EvaluationAndMonitoring,
}

impl Stage {
    /// The stage's name, for the prompt's own heading.
    fn heading(self) -> &'static str {
        match self {
            Stage::Requirements => "REQUIREMENTS",
            Stage::Estimation => "ESTIMATION",
            Stage::HighLevel => "HIGH-LEVEL DESIGN",
            Stage::DeepDive => "DEEP DIVE",
            Stage::Bottlenecks => "BOTTLENECKS AND TRADE-OFFS",
            Stage::DataAndFeatures => "DATA AND FEATURES",
            Stage::Modeling => "MODELLING",
            Stage::Serving => "SERVING",
            Stage::EvaluationAndMonitoring => "EVALUATION AND MONITORING",
        }
    }

    /// What to grade, and what to explicitly leave alone. The "ignore" half matters as
    /// much as the "grade" half: without it the model drifts back to reviewing the whole
    /// design at every stage.
    ///
    /// Takes the track because `Requirements` and `Estimation` are walked by both and mean
    /// different things in each — scoping a service is not framing a prediction task, and
    /// sizing a cache is not sizing a training set.
    ///
    /// Those two are spelled out for every track rather than given a fallback arm. A
    /// wildcard would compile, and a third track would silently inherit the
    /// distributed-systems briefing for its first two stages; this way adding one fails
    /// here until somebody writes its text. The stages that belong to a single track keep
    /// their `_`, because there is nothing for another track to get wrong.
    fn focus(self, track: Track) -> &'static str {
        match (track, self) {
            (Track::Ml, Stage::Requirements) => "\
Grade only whether they have framed the prediction problem. What exactly is being \
predicted, for whom, and at what moment? What is the label, and where does it come from — \
is it logged, inferred, or does somebody have to label it by hand? Which online metric \
does the business actually care about? Push hard on one thing: what is the non-ML \
baseline, and why is it not enough? A design that cannot say why a heuristic loses has \
not justified the model. Their notes are the answer here; the diagram is not. Say nothing \
about features, model architecture or serving.",

            (Track::Ml, Stage::Estimation) => "\
Grade only the arithmetic in their notes: how many labelled examples exist and how fast \
they arrive, the storage for features and embeddings, inference queries per second, and \
what the model costs to serve.

Before you write anything, recompute every number they claim, yourself, from their own \
assumptions. Do not accept a stated result because it looks plausible. State your figure \
next to theirs. Watch the unit: a slip between GB and TB, or per-day and per-second, is \
the most common error here and you must catch it. Watch two more that are specific to \
this: a training set counted in rows when the cost is driven by tokens or by images, and \
an inference budget quoted per request when the hardware is billed per GPU-hour. An \
absent number is also a finding. Do not review the diagram's structure.",

            (_, Stage::DataAndFeatures) => "\
Grade only the data. Where does each feature come from, how is it computed, and — the \
question that matters most — is it computed the same way at training time and at serving \
time? Name any feature that could not actually be known at prediction time; that is \
leakage and it is the single most common way one of these designs is quietly wrong. Ask \
how the training set is sampled, how class imbalance is handled, and what happens to a \
user or item seen for the first time. Ignore the model architecture entirely.",

            (_, Stage::Modeling) => "\
If their notes quote an accuracy figure, do this before anything else. Find the \
percentage of positive cases in THEIR notes. A model that always answers \"no\" — never \
flags a single case — scores 100 minus that exact percentage, subtracted as written with \
no conversion to a fraction. Use only the number they wrote; there is no other rate. Your \
first line must be exactly this, filled in: Always-no baseline: 100 - <their positive \
rate> = <result>%. Your accuracy: <their accuracy>%. Then say which is higher. If they \
give no positive rate, your first sentence asks for it. Only then grade the rest.

Grade only the model choice and how it is trained. Does the model suit the data they \
have, rather than the data they wish they had? Push on the simplest thing that could \
work first. Ask how often it is retrained and what triggers that. If the design retrieves \
and then ranks, check that the two stages have different jobs and different costs. Ignore \
serving infrastructure and monitoring.",

            (_, Stage::Serving) => "\
Grade only how a prediction reaches the caller. Is scoring done ahead of time or on the \
request path, and does the latency budget survive it? Where do features come from at \
serving time, and what happens when that lookup is slow or empty? Ask about batching, \
caching of predictions, and what is served while the model is being swapped. A design \
with no answer for the first request after a deploy has not finished. Ignore how the \
model was trained.",

            (_, Stage::EvaluationAndMonitoring) => "\
If their notes quote an accuracy figure, do this before anything else. Find the \
percentage of positive cases in THEIR notes. A model that always answers \"no\" — never \
flags a single case — scores 100 minus that exact percentage, subtracted as written with \
no conversion to a fraction. Use only the number they wrote; there is no other rate. Your \
first line must be exactly this, filled in: Always-no baseline: 100 - <their positive \
rate> = <result>%. Your accuracy: <their accuracy>%. Then say which is higher. If they \
give no positive rate, your first sentence asks for it. Only then grade the rest.

Grade only what happens after it ships. Which offline metric did they choose and why is \
it a reasonable stand-in for the online one — and what would make the two disagree? How \
is the change actually measured on real traffic? Then the failure modes specific to this \
kind of system: input drift, label drift, and the feedback loop where the model's own \
output becomes its next training set and quietly narrows what it will ever recommend. \
Assume the serving path already works.",

            (Track::Classic, Stage::Requirements) => "\
Grade only whether they have scoped the problem: which features are in and out, who the \
users are, what they have chosen NOT to build. Their notes are the answer here; the \
diagram is not. Do not comment on components or scale yet.",

            // "Check their maths" is not enough on its own: both a 2.5B and a 12B will
            // read a stated result and agree with it. They have to be told to recompute
            // from the inputs before they are allowed to judge, and told that the unit is
            // where the error usually hides.
            (Track::Classic, Stage::Estimation) => "\
Grade only the arithmetic in their notes: QPS, storage, bandwidth, and whether those \
numbers follow from the stated requirements.

Before you write anything, recompute every number they claim, yourself, from their own \
assumptions. Do not accept a stated result because it looks plausible — derive it again \
and compare. State your figure next to theirs. Check the unit especially: a slip between \
GB and TB, or per-day and per-second, is the most common error here and you must catch \
it. An absent number is also a finding. Do not review the diagram's structure.",

            (_, Stage::HighLevel) => "\
Grade only the shape of the diagram: are the major components present, does the request \
path make sense end to end, is the data flow coherent. Stay out of the internals of any \
single component.",

            (_, Stage::DeepDive) => "\
Pick the single most interesting component on their diagram and go deep on it alone: its \
data model, its interface, how it scales. Ignore every other part of the design.",

            (_, Stage::Bottlenecks) => "\
Grade only what breaks: single points of failure, what saturates first under the stated \
load, and the trade-off they made without naming it. Assume the happy path already works.",
        }
    }
}

impl Stage {
    /// `focus`, for a critique whose arithmetic code has already checked and shown.
    ///
    /// The estimation text orders the model to recompute every number, and the ML texts
    /// order it to write the baseline line. Both are right without checks and wrong with
    /// them: told to trust the checked figures, MiniCPM5-2B still redid the sums code could
    /// not check and called a correct 400M wrong, three runs out of three. So with checks,
    /// the model grades assumptions and gaps and delivers no arithmetic verdict at all.
    fn checked_focus(self, track: Track) -> &'static str {
        match (track, self) {
            (Track::Classic, Stage::Estimation) => "\
Grade only the estimation in their notes. Are the assumptions believable for this problem — \
how many users, how many requests each, how big each object is, how long it is kept? Which \
numbers that matter here have they not worked out at all: peak QPS, storage over the \
retention period, bandwidth? A missing number is a finding. Do not recompute anything and \
do not call any figure right or wrong; if a figure has no working, ask them to show it. Do \
not review the diagram's structure.",
            (Track::Ml, Stage::Estimation) => "\
Grade only the estimation in their notes. Are the assumptions believable — how many \
labelled examples exist and how fast they arrive, the storage for features and embeddings, \
inference queries per second, what the model costs to serve? Watch for a training set \
counted in rows when the cost is driven by tokens or images, and an inference budget quoted \
per request when the hardware is billed per GPU-hour. A missing number is a finding. Do not \
recompute anything and do not call any figure right or wrong; if a figure has no working, \
ask them to show it. Do not review the diagram's structure.",
            // Both open with a paragraph ordering the baseline line, which code now writes.
            (_, Stage::Modeling | Stage::EvaluationAndMonitoring) => {
                let focus = self.focus(track);
                focus.split_once("\n\n").map_or(focus, |(_, rest)| rest)
            }
            (
                _,
                Stage::Requirements
                | Stage::DataAndFeatures
                | Stage::Serving
                | Stage::HighLevel
                | Stage::DeepDive
                | Stage::Bottlenecks,
            ) => self.focus(track),
        }
    }
}

/// The interviewer's standing rules, per track. Constant across stages and levels — the
/// parts that vary are appended by `system_prompt`.
///
/// Split by track because the two rounds are run by different people. A distributed
/// systems interviewer wants the component that dies and what breaks downstream; an ML
/// interviewer wants the baseline you beat and the class balance behind the number you
/// just quoted.
/// The standing rule each track has that orders the model to do arithmetic itself. Left
/// out when code has already done it: kept, it contradicts "do not say whether any number is
/// right or wrong" from the same prompt, and a 2B model follows whichever it read last.
const ARITHMETIC_RULES: [&str; 2] = [
    "\n- If a back-of-the-envelope number contradicts the design, do the arithmetic and show it.",
    "\n- Never credit a quoted accuracy on its own. Compare it with what a model that always \
  predicts the most common class would score, and state that figure next to theirs.",
];

fn rules(track: Track) -> &'static str {
    match track {
        Track::Classic => "\
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
- If a back-of-the-envelope number contradicts the design, do the arithmetic and show it.",

        // The metric rules are this track's version of the lesson the estimation stage
        // already learned the hard way: a model will read a stated result and agree with
        // it. Measured on MiniCPM5-2B: told only to "ask what the class balance is", it was
        // handed notes saying spam was 0.5% of messages and a model scoring 95%, and in two
        // runs out of two it flagged the number as suspicious without once working out that
        // answering "not spam" every time scores 99.5%. A general rule here was not enough
        // either: moved into this list with a worked example, it went 0 for 6 — four runs
        // never considered it and two computed the figure in their reasoning, got the
        // direction muddled, and dropped it. What the estimation stage does instead is put
        // the ordered computation inside the stage's own focus, first. The Modeling and
        // Evaluation focus strings now do the same, with the formula spelled out and the
        // answer's first sentence fixed: 6/6 then led with the comparison and reached the
        // right verdict, 4/6 with the exact figure — the two misses turned 0.5% into 99.95%,
        // the GB/TB slip again, so the subtraction is now shown on a percentage with an
        // explicit warning not to convert it. This rule stays as the standing principle.
        Track::Ml => "\
You are a senior machine learning engineer running an ML system design interview. The \
candidate is sitting across from you and you are speaking to them, so address them as \
\"you\". You are reviewing their system diagram and notes, not writing the answer for them.

Rules:
- Speak TO the candidate, never about them. Never write \"the candidate\", \"they\", or \
  \"them\" — that is talking to an observer who is not in the room.
- Never hand over a full solution. Surface the gap, then ask the question that exposes it.
- Ground every point in a component, connection, or note that is actually on their board, \
  or in one that is conspicuously missing.
- Never credit a quoted accuracy on its own. Compare it with what a model that always \
  predicts the most common class would score, and state that figure next to theirs.
- Treat a missing baseline as a finding. If they have not said what a heuristic or a \
  simpler model would achieve, they have not shown the model earns its cost.
- Watch for a feature that could not be known at prediction time. Name it as leakage and \
  say which one.",
    }
}

/// The full system prompt for one stage of one track at one level.
///
/// The stage's position is computed here rather than written into `focus`, because the
/// tracks are different lengths — the classic interview is five stages and the ML one is
/// six. Baking "STAGE 2 of 5" into the text is what made a second track impossible.
///
/// The length cap is load-bearing, not cosmetic: this runs automatically whenever the
/// canvas goes quiet, so a four-section answer per stage would never finish before the
/// next edit aborts it.
pub fn system_prompt(track: Track, level: Level, stage: Stage) -> String {
    prompt_with_focus(track, level, stage, stage.focus(track), false)
}

/// `system_prompt`, for a critique that `check_arithmetic` found something to check in.
pub fn checked_system_prompt(track: Track, level: Level, stage: Stage, checks: &calc::Checks) -> String {
    if checks.is_empty() {
        // Where code could not work out a baseline, the model must not either. Left with the
        // template, it was handed "positives are 0.5%, recall 90%, precision 40%" and wrote
        // "100 - 0.005 = 99.995%. Your accuracy: 40%" — precision taken for accuracy, and
        // the fraction slip back.
        if matches!(stage, Stage::Modeling | Stage::EvaluationAndMonitoring) {
            return prompt_with_focus(track, level, stage, stage.checked_focus(track), true)
                + "\n\nDo not work out or quote any baseline or accuracy figure yourself. If they \
give an accuracy without the share of cases that are positive, ask them for that share.";
        }
        return system_prompt(track, level, stage);
    }
    prompt_with_focus(track, level, stage, stage.checked_focus(track), true) + &checks.instruction()
}

fn prompt_with_focus(track: Track, level: Level, stage: Stage, focus: &str, checked: bool) -> String {
    let stages = track.stages();
    // A stage from another track is a bug upstream — the handler validates this — but the
    // prompt still has to say something rather than panic mid-interview.
    debug_assert!(
        track.position(stage).is_some(),
        "{stage:?} is not part of {track:?} — check_stage should have refused this",
    );
    let position = track.position(stage).map(|i| i + 1).unwrap_or(1);

    format!(
        "{}\n\n{}\n\nSTAGE {position} of {} — {}. {}\n\nAnswer in under 150 words, then ask \
exactly one question. No headings, no bullet lists longer than three items. Say nothing \
about any other stage.",
        if checked {
            ARITHMETIC_RULES.iter().fold(rules(track).to_string(), |r, rule| r.replace(rule, ""))
        } else {
            rules(track).to_string()
        },
        level.calibration(),
        stages.len(),
        stage.heading(),
        focus,
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

/// Where the model runs, and how it is asked.
#[derive(Clone)]
pub struct Runner {
    pub url: String,
    /// llama-server ignores this and serves whatever it was started with, but Ollama
    /// resolves it and 404s on an unknown name. Configurable via SYSPRACTICE_MODEL.
    pub model: String,
    /// Sent as `reasoning_effort` when set; left out entirely when not, so a runner that
    /// does not know the field never sees it. `none` switches a reasoning model's thinking
    /// phase off on Ollama. Note `think: false` does *not*: Ollama's OpenAI-compatible
    /// endpoint ignores it without complaint, which is an easy way to measure nothing.
    /// `check_arithmetic` sends `none` whatever this is set to: it is a copying job.
    pub reasoning_effort: Option<String>,
}

impl Runner {
    fn endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.url)
    }

    fn body(&self, stream: bool, temperature: f64, system: String, prompt: String) -> serde_json::Value {
        let mut body = json!({
            "model": self.model,
            "stream": stream,
            "temperature": temperature,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": prompt },
            ],
        });
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }
        body
    }
}

/// `preface` goes out as the answer's first `token` frame, ahead of the model's: lines
/// written by code that the model is told the candidate has already read.
pub async fn stream_critique(
    runner: &Runner,
    system: String,
    prompt: String,
    preface: String,
) -> Result<impl Stream<Item = Result<Event, Infallible>>, AppError> {
    let url = runner.endpoint();

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&runner.body(true, 0.4, system, prompt))
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

        if !preface.is_empty() {
            yield Ok::<Event, Infallible>(Event::default().event("token").data(encode(&preface)));
        }

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
pub async fn complete(runner: &Runner, system: String, prompt: String) -> Result<String, AppError> {
    // Lower temperature than the critique's 0.4: the same two designs should not change
    // places between one grading and the next.
    send(runner, runner.body(false, 0.2, system, prompt), true).await
}

/// Post one non-streaming request and return the answer's text. `or_reasoning` accepts a
/// reasoning model's `reasoning` field when `content` is empty — right for a verdict, wrong
/// for an extraction, where chain of thought is not the JSON that was asked for.
async fn send(runner: &Runner, body: serde_json::Value, or_reasoning: bool) -> Result<String, AppError> {
    let url = runner.endpoint();

    let body: serde_json::Value = reqwest::Client::new()
        .post(&url)
        .json(&body)
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
        .or_else(|| choice["reasoning"].as_str().filter(|_| or_reasoning))
        .unwrap_or_default()
        .trim()
        .to_string();

    if text.is_empty() {
        return Err(AppError::Runner(format!("{url} (empty answer)")));
    }
    Ok(text)
}

// --- checked arithmetic --------------------------------------------------------

/// The extraction's instructions. It copies and never computes: asked to expand `100M` to
/// digits, MiniCPM5-2B wrote `1000000000`, and asked for `e6` it read `500B` as `500e3`.
/// Copying the notes' own text is the one thing it does reliably, so suffixes and the
/// bytes-or-billions question are left to `calc`.
const EXTRACT: &str = r#"You copy the arithmetic out of a candidate's interview notes as JSON. You do not check it, judge it, or fix it.

Answer with one JSON object and nothing else:
{"calculations": [{"expression": "...", "claimed": "...", "unit": "..."}], "positive_rate": "...", "accuracy": "..."}

calculations: one entry for every place the notes multiply, divide, add or subtract numbers to get a result they wrote down. Skip ratios such as 100:1 and anything with no result.
- expression: the inputs they used, joined with * / + - and parentheses. Copy each number exactly as they wrote it, with its K, M, B, T, KB, MB, GB, TB or bytes suffix; do not convert or expand it. Drop words that are not numbers. Include every factor they multiplied by, even one they only wrote in words: "a year" is 365 days, "per second" from a daily figure is / 86400.
- claimed: the result exactly as they wrote it, copied character for character with its unit, such as "7GB" or "230/s". Use "" if they wrote no result.
- unit: "bytes" if the result is a size in bytes, otherwise "count".

positive_rate: the percentage of positive cases (spam, fraud, clicks, etc.) exactly as written, with its % sign, such as "3%". Use "" if none is stated.
accuracy: the model accuracy exactly as written, with its % sign. Use "" if none is stated.

If the notes contain none of these, answer {"calculations": [], "positive_rate": "", "accuracy": ""}."#;

/// Long enough for a cold model load on a warm runner, short enough that a hung one costs
/// the critique twenty seconds rather than the whole request.
const EXTRACT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Which stages grade a number the model cannot compute on its own. The rest skip the
/// extraction and pay no latency for it.
fn wanted(stage: Stage) -> Option<calc::Wanted> {
    match stage {
        Stage::Estimation => Some(calc::Wanted::Calculations),
        Stage::Modeling | Stage::EvaluationAndMonitoring => Some(calc::Wanted::Baseline),
        _ => None,
    }
}

/// Copy the candidate's arithmetic out of their notes and redo it in code.
///
/// Never fails: a runner that is down, slow, or answers nonsense yields no checks and the
/// critique goes out exactly as it did before. A runner that is down fails again straight
/// after in `stream_critique`, which is where the quota refund lives.
///
/// Reasoning is forced off whatever the deployment chose. This is a copying job; a thinking
/// phase would add minutes to a call whose whole point is to let the critique skip one.
///
/// The flag says whether the runner answered the extraction. If it did, GPU time was spent,
/// and the caller must not refund the quota when the critique's own call then fails.
pub async fn check_arithmetic(runner: &Runner, stage: Stage, notes: &str) -> (calc::Checks, bool) {
    let none = (calc::Checks::default(), false);
    let Some(wanted) = wanted(stage) else { return none };
    // Free to rule out, and a model call is not: no digit is no sum, and a baseline needs
    // two percentages.
    let possible = match wanted {
        calc::Wanted::Calculations => notes.bytes().any(|b| b.is_ascii_digit()),
        calc::Wanted::Baseline => notes.matches('%').count() >= 2,
    };
    if !possible {
        return none;
    }
    // Sums written out with an `=` need no model, and do not depend on one copying digits
    // it has been measured to rewrite. The extraction is for the ones written in prose.
    //
    // Notes that write `=` at all are left to the scan entirely. A sum it declined — bits, a
    // unit of time, a name like `H100` — is declined on purpose, and handed to the extraction
    // it comes back with the words stripped: `10 Mbps * 3600 = 4.5 GB` as `10 * 3600`.
    if wanted == calc::Wanted::Calculations && notes.contains('=') {
        return (calc::scan(notes), false);
    }

    let runner = Runner { reasoning_effort: Some("none".into()), ..runner.clone() };
    let mut body = runner.body(false, 0.0, EXTRACT.into(), notes.into());
    body["max_tokens"] = json!(500);
    // Ollama and llama-server both constrain decoding to valid JSON with this; a runner that
    // ignores it still usually answers in JSON, and `from_extraction` copes when it does not.
    body["response_format"] = json!({ "type": "json_object" });

    match tokio::time::timeout(EXTRACT_TIMEOUT, send(&runner, body, false)).await {
        Ok(Ok(raw)) => (calc::Checks::from_extraction(&raw, notes, wanted), true),
        _ => none,
    }
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
        let p = system_prompt(Track::Classic, Level::Senior, Stage::Estimation);
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
            let p = system_prompt(Track::Classic, Level::Junior, stage);
            assert!(p.contains("Speak TO the candidate"), "{stage:?} lost the voice rule");
        }
    }

    #[test]
    fn the_level_changes_who_is_in_the_room() {
        let junior = system_prompt(Track::Classic, Level::Junior, Stage::HighLevel);
        let staff = system_prompt(Track::Classic, Level::Staff, Stage::HighLevel);
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


    #[test]
    fn the_stage_number_is_counted_not_written() {
        // It used to be baked into each focus string as "STAGE 2 of 5", which is the line
        // that made a second track impossible — the ML interview is six stages long.
        let classic = system_prompt(Track::Classic, Level::Senior, Stage::Estimation);
        assert!(classic.contains("STAGE 2 of 5 — ESTIMATION"), "{classic}");

        let ml = system_prompt(Track::Ml, Level::Senior, Stage::EvaluationAndMonitoring);
        assert!(ml.contains("STAGE 6 of 6 — EVALUATION AND MONITORING"), "{ml}");

        for track in Track::ALL {
            for (i, stage) in track.stages().iter().enumerate() {
                let p = system_prompt(*track, Level::Senior, *stage);
                let expected = format!("STAGE {} of {}", i + 1, track.stages().len());
                assert!(p.contains(&expected), "{track:?} {stage:?} wanted {expected}: {p}");
            }
        }
    }

    #[test]
    fn a_shared_stage_asks_for_different_things_in_each_track() {
        // Requirements and Estimation are walked by both tracks. If they read the same in
        // both, the ML interview is a systems interview with different problem text.
        let classic = system_prompt(Track::Classic, Level::Senior, Stage::Requirements);
        let ml = system_prompt(Track::Ml, Level::Senior, Stage::Requirements);
        assert_ne!(classic, ml, "Requirements is identical in both tracks");
        assert!(ml.contains("label"), "ML requirements must ask what is being predicted");
        assert!(ml.contains("baseline"), "ML requirements must demand the non-ML baseline");
        assert!(!classic.contains("baseline"), "a classic round should not ask for one");

        // The estimation lesson has to survive in both: a model will agree with a stated
        // number unless it is told to recompute it first.
        for track in Track::ALL {
            let p = system_prompt(*track, Level::Senior, Stage::Estimation);
            assert!(p.contains("recompute"), "{track:?} estimation lost the recompute order");
            assert!(p.contains("GB and TB"), "{track:?} estimation lost the unit warning");
        }
    }

    #[test]
    fn no_stage_leaks_another_stages_focus() {
        for track in Track::ALL {
            for stage in track.stages() {
                let p = system_prompt(*track, Level::Senior, *stage);
                for other in track.stages() {
                    if other == stage {
                        continue;
                    }
                    assert!(
                        !p.contains(other.heading()),
                        "{track:?} {stage:?} names {other:?}: {p}",
                    );
                }
            }
        }
    }

    #[test]
    fn the_ml_interviewer_will_not_take_a_metric_on_trust() {
        // The same failure as the estimation stage's: a model reads "95% accuracy" and
        // congratulates them, without asking what answering "no" every time would score.
        let p = system_prompt(Track::Ml, Level::Senior, Stage::Modeling);
        assert!(
            p.contains("always predicts the most common class"),
            "the ML rules no longer order the trivial-baseline computation: {p}",
        );
        assert!(
            p.contains("state that figure next to theirs"),
            "the rule must make the model show its number, not just gesture at the problem",
        );
        // The ordered computation lives in the stages that grade a metric, where the
        // model actually acts on it — a standing rule alone went 0 for 6.
        for stage in [Stage::Modeling, Stage::EvaluationAndMonitoring] {
            let p = system_prompt(Track::Ml, Level::Senior, stage);
            assert!(p.contains("100 minus that exact percentage"), "{stage:?} lost the formula");
            assert!(p.contains("no conversion to a fraction"), "{stage:?} lost the unit warning");
            // No worked numbers: with reasoning off the model copied the example's figure
            // instead of the user's in 3 of 6 runs.
            assert!(!p.contains("2% are positive"), "{stage:?} has a numeric example again");
            assert!(p.contains("Always-no baseline: 100 -"), "{stage:?} lost the template line");
        }
        assert!(p.contains("baseline"), "the ML rules lost the baseline rule");
        assert!(p.contains("leakage"), "the ML rules lost the leakage rule");
        assert!(p.contains("machine learning engineer"), "wrong interviewer in the room");

        let classic = system_prompt(Track::Classic, Level::Senior, Stage::DeepDive);
        assert!(!classic.contains("most common class"), "ML rules leaked into a classic round");
    }


    #[test]
    fn reasoning_effort_is_only_sent_when_configured() {
        let mut runner = Runner {
            url: "http://runner".into(),
            model: "m".into(),
            reasoning_effort: None,
        };
        let body = runner.body(true, 0.4, "s".into(), "p".into());
        // Absent, not null: a runner that does not know the field must never see it.
        assert!(body.get("reasoning_effort").is_none(), "sent unasked: {body}");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");

        runner.reasoning_effort = Some("none".into());
        let body = runner.body(false, 0.2, "s".into(), "p".into());
        assert_eq!(body["reasoning_effort"], "none");
        assert_eq!(body["stream"], false);
        assert_eq!(runner.endpoint(), "http://runner/v1/chat/completions");
    }

    #[test]
    fn with_checks_the_model_gives_no_arithmetic_verdict_of_its_own() {
        let calcs = calc::Checks::from_extraction(
            r#"{"calculations": [{"expression": "100M * 365 * 500B", "claimed": "18GB", "unit": "bytes"}]}"#,
            "100M * 365 * 500B = 18GB",
            calc::Wanted::Calculations,
        );
        let baseline = calc::Checks::from_extraction(
            r#"{"positive_rate": "0.5%", "accuracy": "95%"}"#,
            "Spam is 0.5% of messages. Model is 95% accurate.",
            calc::Wanted::Baseline,
        );
        assert!(!calcs.is_empty() && !baseline.is_empty());

        for track in [Track::Classic, Track::Ml] {
            let p = checked_system_prompt(track, Level::Senior, Stage::Estimation, &calcs);
            assert!(!p.contains("recompute every number"), "{track:?}: {p}");
            assert!(p.contains("Do not recompute anything") && p.contains("already been shown a check"));
            // Without checks nothing changes.
            let plain = checked_system_prompt(track, Level::Senior, Stage::Estimation, &calc::Checks::default());
            assert_eq!(plain, system_prompt(track, Level::Senior, Stage::Estimation));
        }
        // The standing rules that order arithmetic go too — and each is really in its track's
        // rules, or removing it by text would be silently removing nothing.
        for (track, rule) in [(Track::Classic, ARITHMETIC_RULES[0]), (Track::Ml, ARITHMETIC_RULES[1])] {
            assert!(rules(track).contains(rule), "{track:?}'s rule text drifted from ARITHMETIC_RULES");
            let stage = if track == Track::Ml { Stage::Modeling } else { Stage::Estimation };
            let checks = if track == Track::Ml { &baseline } else { &calcs };
            let p = checked_system_prompt(track, Level::Senior, stage, checks);
            assert!(!p.contains("do the arithmetic and show it") && !p.contains("always predicts the most common"), "{p}");
            assert!(system_prompt(track, Level::Senior, stage).contains(rule.trim_start()));
        }
        for stage in [Stage::Modeling, Stage::EvaluationAndMonitoring] {
            let unchecked = checked_system_prompt(Track::Ml, Level::Senior, stage, &calc::Checks::default());
            assert!(!unchecked.contains("Always-no baseline: 100 -") && unchecked.contains("Do not work out"), "{unchecked}");
            let p = checked_system_prompt(Track::Ml, Level::Senior, stage, &baseline);
            assert!(!p.contains("Always-no baseline: 100 -"), "{stage:?} still orders the template: {p}");
            assert!(p.contains("Grade only") && p.contains("That is settled"));
        }
    }
}
