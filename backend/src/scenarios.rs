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

/// How hard the problem itself is. Unrelated to `Level`, which is the seniority you have
/// chosen to be interviewed at — you can take an Easy problem at Staff level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
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
    pub difficulty: Difficulty,
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
        difficulty: Difficulty::Easy,
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
        difficulty: Difficulty::Medium,
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
        difficulty: Difficulty::Medium,
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
        difficulty: Difficulty::Hard,
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
    Scenario {
        id: "news-feed",
        title: "News Feed (Twitter/Instagram)",
        prompt: "Design the home timeline for a social network.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Post text and images to followers",
            "Home timeline ranked or reverse-chronological",
            "Follow and unfollow accounts",
        ],
        non_functional: &[
            "200M daily active users",
            "Timeline loads in under 200ms",
            "A post is visible to followers within seconds",
        ],
        rubric: &[
            item(Level::Junior, "Separate write path (post) from read path (timeline build)"),
            item(Level::Junior, "Timeline is served from a cache, not assembled from the posts table per request"),
            item(Level::Senior, "Fan-out on write vs fan-out on read, and which one the follower graph forces"),
            item(Level::Senior, "Feed storage is capped and trimmed rather than unbounded per user"),
            item(Level::Staff, "Celebrity accounts break fan-out on write — the hybrid merge at read time"),
        ],
    },
    Scenario {
        id: "web-crawler",
        title: "Web Crawler",
        prompt: "Design a crawler that indexes a large portion of the public web.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Fetch pages starting from a seed set and follow links",
            "Store raw content for downstream indexing",
            "Recrawl pages as they change",
        ],
        non_functional: &[
            "1B pages per month",
            "Never overwhelm a single origin server",
            "Survive restarts without recrawling everything",
        ],
        rubric: &[
            item(Level::Junior, "Frontier queue of URLs to visit, separate from the set already seen"),
            item(Level::Junior, "Duplicate detection so the same URL is not fetched repeatedly"),
            item(Level::Senior, "Politeness: per-domain rate limiting and robots.txt, and where that state lives"),
            item(Level::Senior, "Content-level dedup — the same page under many URLs"),
            item(Level::Staff, "Crawl traps and infinite URL spaces; depth and budget limits per domain"),
        ],
    },
    Scenario {
        id: "notification-service",
        title: "Notification Service",
        prompt: "Design a service that delivers push, email, and SMS notifications.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Accept a notification request from any internal service",
            "Deliver via push, email, or SMS based on user preference",
            "Respect per-user opt-out and quiet hours",
        ],
        non_functional: &[
            "500M notifications/day with bursts at the top of the hour",
            "A notification is never delivered twice",
            "A third-party provider outage does not lose messages",
        ],
        rubric: &[
            item(Level::Junior, "Queue between request intake and delivery so senders never block"),
            item(Level::Junior, "Per-channel adapters behind one interface; preference lookup before send"),
            item(Level::Senior, "Retry with backoff, and a dead letter queue for what never succeeds"),
            item(Level::Senior, "Idempotency key so a retry does not become a second notification"),
            item(Level::Staff, "Provider failover and per-provider rate limits without reordering a user's messages"),
        ],
    },
    Scenario {
        id: "search-autocomplete",
        title: "Search Autocomplete",
        prompt: "Design the typeahead suggestions for a large search box.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Return the top suggestions for a prefix as the user types",
            "Rank by popularity",
            "Reflect trending queries within a day",
        ],
        non_functional: &[
            "Suggestions return in under 50ms at p99",
            "100k queries/sec at peak",
            "Suggestions are the same for everyone — no personalization",
        ],
        rubric: &[
            item(Level::Junior, "Prefix lookup structure (trie or sorted index), not a LIKE scan over a table"),
            item(Level::Junior, "Top-k precomputed per prefix rather than ranked at request time"),
            item(Level::Senior, "Build pipeline is offline and periodic; serving layer is read-only"),
            item(Level::Senior, "Sharding by prefix and how the tree is kept in memory"),
            item(Level::Staff, "Trending terms need a faster path than the daily rebuild — the two-tier merge"),
        ],
    },
    Scenario {
        id: "object-storage",
        title: "Object Storage (S3)",
        prompt: "Design a durable object store with an HTTP API.",
        difficulty: Difficulty::Hard,
        functional: &[
            "PUT and GET an object by bucket and key",
            "List keys in a bucket by prefix",
            "Objects range from bytes to terabytes",
        ],
        non_functional: &[
            "Eleven nines of durability",
            "Reads served globally",
            "Storage cost matters more than write latency",
        ],
        rubric: &[
            item(Level::Junior, "Metadata (key to location) is stored separately from the object bytes"),
            item(Level::Junior, "Large objects are split into chunks rather than stored as one blob"),
            item(Level::Senior, "Replication or erasure coding, and the durability-versus-cost trade-off"),
            item(Level::Senior, "The metadata index is the scaling bottleneck — how it partitions"),
            item(Level::Staff, "Background repair: detecting and rebuilding lost replicas before they compound"),
        ],
    },
    Scenario {
        id: "payment-ledger",
        title: "Payment Ledger",
        prompt: "Design the ledger behind a payments product.",
        difficulty: Difficulty::Hard,
        functional: &[
            "Move money between two accounts",
            "Report a balance for any account at any point in time",
            "Reverse or refund a prior transaction",
        ],
        non_functional: &[
            "Money is never created or destroyed",
            "10k transfers/sec",
            "Every balance change is auditable years later",
        ],
        rubric: &[
            item(Level::Junior, "Double-entry: every movement is two matching entries, and balance is derived"),
            item(Level::Junior, "Entries are append-only — never updated or deleted in place"),
            item(Level::Senior, "Idempotency so a retried transfer request does not move money twice"),
            item(Level::Senior, "Balance snapshots so reads do not replay the whole history"),
            item(Level::Staff, "Cross-shard transfers when the two accounts are not co-located"),
        ],
    },
    Scenario {
        id: "metrics-pipeline",
        title: "Metrics Pipeline",
        prompt: "Design the ingestion and query path for a metrics system.",
        difficulty: Difficulty::Hard,
        functional: &[
            "Ingest time-series points from millions of hosts",
            "Query ranges and aggregates for dashboards",
            "Alert when a series crosses a threshold",
        ],
        non_functional: &[
            "10M points/sec ingest",
            "Dashboard queries over a day of data return in under a second",
            "Retain two years at a coarser resolution",
        ],
        rubric: &[
            item(Level::Junior, "Ingest is buffered through a queue, decoupled from the write to storage"),
            item(Level::Junior, "A time-series store, not a row per point in a relational table"),
            item(Level::Senior, "Rollups and downsampling drive retention and query speed"),
            item(Level::Senior, "Partitioning by time and by series, and why time alone is not enough"),
            item(Level::Staff, "High-cardinality labels are what actually kills the index"),
        ],
    },
    Scenario {
        id: "ride-sharing",
        title: "Ride Sharing (Uber)",
        prompt: "Design the matching and tracking system for a ride-hailing service.",
        difficulty: Difficulty::Hard,
        functional: &[
            "Match a rider to a nearby available driver",
            "Track the driver's location during the trip",
            "Show the rider an ETA",
        ],
        non_functional: &[
            "1M concurrent drivers reporting location every few seconds",
            "Match within seconds of a request",
            "Two riders are never assigned the same driver",
        ],
        rubric: &[
            item(Level::Junior, "Driver locations live in a hot store keyed for proximity, not the primary DB"),
            item(Level::Junior, "Separate the high-frequency location stream from the low-frequency trip record"),
            item(Level::Senior, "Geospatial indexing — geohash or quadtree — and the cell-size trade-off"),
            item(Level::Senior, "Matching must lock a driver atomically or two riders get the same car"),
            item(Level::Staff, "Supply and demand are regional, so the system partitions by geography, not by user"),
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
