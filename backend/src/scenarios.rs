use serde::{Deserialize, Serialize};

/// Seniority the design is graded at. Ordered, so a rubric line is in scope when its
/// level is at or below the one being interviewed for — `Staff` sees everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Junior,
    Senior,
    Staff,
}

impl Level {
    /// How the interviewer is told to pitch the conversation. A junior loop teaches;
    /// a staff loop assumes the fundamentals and attacks the trade-offs.
    pub fn calibration(self) -> &'static str {
        match self {
            Level::Junior => "\
You are interviewing this candidate for a JUNIOR role. They are learning. Explain the \
reasoning behind a gap rather than only naming it, and never penalise them for missing \
multi-region, consistency, or hot-spot concerns — those are not in scope at this level. \
One concrete thing to fix beats five things to worry about.",
            Level::Senior => "\
You are interviewing this candidate for a SENIOR role. Assume the fundamentals. Push on \
partitioning, failure modes, and whether they can justify a choice over its alternative.",
            Level::Staff => "\
You are interviewing this candidate for a STAFF role. Assume the design is competent and \
attack it. Look for the trade-off they made without noticing, and the failure mode that \
only appears under load or partition.",
        }
    }
}

/// One thing a strong answer covers, and the level at which it becomes fair game.
#[derive(Debug)]
pub struct RubricItem {
    pub level: Level,
    pub text: &'static str,
}

/// Terser than writing `RubricItem { .. }` twenty times.
const fn item(level: Level, text: &'static str) -> RubricItem {
    RubricItem { level, text }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: &'static str,
    pub title: &'static str,
    pub prompt: &'static str,
    pub functional: &'static [&'static str],
    pub non_functional: &'static [&'static str],
    /// What a strong answer covers. Fed to the critic, never shown in the UI —
    /// seeing it would defeat the drill, so it is skipped on the wire entirely
    /// rather than trusted not to be rendered.
    #[serde(skip)]
    pub rubric: &'static [RubricItem],
}

impl Scenario {
    /// The rubric lines in scope at `level`.
    pub fn rubric_for(&self, level: Level) -> impl Iterator<Item = &'static str> {
        self.rubric.iter().filter(move |r| r.level <= level).map(|r| r.text)
    }
}

pub fn find(id: &str) -> Option<&'static Scenario> {
    SCENARIOS.iter().find(|s| s.id == id)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "url-shortener",
        title: "URL Shortener",
        prompt: "Design a URL shortening service like bit.ly.",
        functional: &[
            "Shorten a long URL to a unique short code",
            "Redirect a short code to the original URL",
            "Optional custom aliases and expiry",
        ],
        non_functional: &[
            "100M new URLs/day, 10:1 read:write",
            "Redirect p99 under 100ms",
            "Short codes never collide or get reused",
        ],
        rubric: &[
            item(Level::Junior, "Key generation strategy (counter+base62, hash+collision check, or pre-generated key range) and why"),
            item(Level::Junior, "Read path is cache-first; redirect does not hit the primary DB"),
            item(Level::Senior, "Partitioning scheme for the URL table and how it grows"),
            item(Level::Senior, "Cache invalidation and TTL on expiring links"),
            item(Level::Staff, "What happens on cache loss — thundering herd on the DB"),
        ],
    },
    Scenario {
        id: "chat-app",
        title: "Chat App (WhatsApp)",
        prompt: "Design a real-time 1:1 and group messaging service.",
        functional: &[
            "Send/receive messages in near real time",
            "Delivery and read receipts",
            "Offline delivery and message history",
            "Group chats up to 500 members",
        ],
        non_functional: &[
            "500M daily active users",
            "Message delivery p99 under 500ms",
            "Messages are never lost or duplicated",
        ],
        rubric: &[
            item(Level::Junior, "Connection model — persistent WebSocket vs long poll, and where connections terminate"),
            item(Level::Junior, "Message queue for offline delivery and ordering guarantees"),
            item(Level::Senior, "How a sender finds the recipient's connected gateway (session/presence registry)"),
            item(Level::Senior, "Message store partitioning by conversation, not by user"),
            item(Level::Staff, "Fan-out strategy for groups: write fan-out vs read fan-out"),
        ],
    },
    Scenario {
        id: "rate-limiter",
        title: "Distributed Rate Limiter",
        prompt: "Design a rate limiter shared across a fleet of API servers.",
        functional: &[
            "Per-API-key and per-IP limits",
            "Multiple policies (per-second burst, per-hour quota)",
            "Return remaining quota to the caller",
        ],
        non_functional: &[
            "1M requests/sec across the fleet",
            "Adds under 5ms to request latency",
            "Limits hold approximately even during a partition",
        ],
        rubric: &[
            item(Level::Junior, "Algorithm choice: token bucket vs sliding window log vs sliding window counter, with the memory trade-off"),
            item(Level::Junior, "Where counters live — local vs centralized Redis — and the sync lag that implies"),
            item(Level::Senior, "Behavior when the counter store is unreachable: fail open or fail closed"),
            item(Level::Senior, "Hot-key problem when one API key dominates traffic"),
            item(Level::Staff, "Clock skew across nodes for window boundaries"),
        ],
    },
    Scenario {
        id: "video-streaming",
        title: "Video Streaming (YouTube)",
        prompt: "Design a video upload and streaming platform.",
        functional: &[
            "Upload a video and transcode to multiple bitrates",
            "Stream with adaptive bitrate playback",
            "Search and view counts",
        ],
        non_functional: &[
            "500 hours uploaded per minute",
            "Playback starts in under 2 seconds globally",
            "Storage is durable and cost-aware",
        ],
        rubric: &[
            item(Level::Junior, "Upload path bypasses app servers — pre-signed URL straight to object storage"),
            item(Level::Junior, "Transcode pipeline is async and queue-driven, with retries on failure"),
            item(Level::Senior, "Chunked storage plus CDN edge caching; origin shields"),
            item(Level::Senior, "Adaptive bitrate manifest (HLS/DASH) and where it is generated"),
            item(Level::Staff, "View counts are eventually consistent, not a synchronous DB write per play"),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junior_rubric_is_a_strict_subset_of_staff() {
        for s in SCENARIOS {
            let junior: Vec<_> = s.rubric_for(Level::Junior).collect();
            let senior: Vec<_> = s.rubric_for(Level::Senior).collect();
            let staff: Vec<_> = s.rubric_for(Level::Staff).collect();

            assert!(!junior.is_empty(), "{} has nothing to grade a junior on", s.id);
            assert!(junior.len() < staff.len(), "{} never widens past junior", s.id);
            assert!(junior.iter().all(|t| senior.contains(t)), "{} senior drops a junior line", s.id);
            assert!(senior.iter().all(|t| staff.contains(t)), "{} staff drops a senior line", s.id);
            assert_eq!(staff.len(), s.rubric.len(), "{} hides a line even at staff", s.id);
        }
    }

    #[test]
    fn the_rubric_never_goes_over_the_wire() {
        let json = serde_json::to_string(&SCENARIOS[0]).unwrap();
        assert!(!json.contains("rubric"), "the rubric leaked into the scenario list: {json}");
        assert!(json.contains("url-shortener"), "the rest of the scenario should still serialize");
    }
}
