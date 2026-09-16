use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: &'static str,
    pub title: &'static str,
    pub prompt: &'static str,
    pub functional: &'static [&'static str],
    pub non_functional: &'static [&'static str],
    /// What a strong answer covers. Fed to the critic, never shown in the UI —
    /// seeing it would defeat the drill.
    pub rubric: &'static [&'static str],
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
            "Key generation strategy (counter+base62, hash+collision check, or pre-generated key range) and why",
            "Read path is cache-first; redirect does not hit the primary DB",
            "Partitioning scheme for the URL table and how it grows",
            "Cache invalidation and TTL on expiring links",
            "What happens on cache loss — thundering herd on the DB",
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
            "Connection model — persistent WebSocket vs long poll, and where connections terminate",
            "How a sender finds the recipient's connected gateway (session/presence registry)",
            "Message queue for offline delivery and ordering guarantees",
            "Fan-out strategy for groups: write fan-out vs read fan-out",
            "Message store partitioning by conversation, not by user",
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
            "Algorithm choice: token bucket vs sliding window log vs sliding window counter, with the memory trade-off",
            "Where counters live — local vs centralized Redis — and the sync lag that implies",
            "Behavior when the counter store is unreachable: fail open or fail closed",
            "Hot-key problem when one API key dominates traffic",
            "Clock skew across nodes for window boundaries",
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
            "Upload path bypasses app servers — pre-signed URL straight to object storage",
            "Transcode pipeline is async and queue-driven, with retries on failure",
            "Chunked storage plus CDN edge caching; origin shields",
            "Adaptive bitrate manifest (HLS/DASH) and where it is generated",
            "View counts are eventually consistent, not a synchronous DB write per play",
        ],
    },
];
