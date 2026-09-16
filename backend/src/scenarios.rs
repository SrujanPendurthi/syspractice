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
    Scenario {
        id: "pastebin",
        title: "Pastebin",
        prompt: "Design a service that stores a block of text and returns a link to it.",
        difficulty: Difficulty::Easy,
        functional: &[
            "Paste text and get back a short link",
            "Read a paste by its link",
            "Optional expiry and unlisted-vs-public visibility",
        ],
        non_functional: &[
            "10M pastes/day, read-heavy",
            "Pastes can be up to 10MB",
            "A link that has not expired always resolves",
        ],
        rubric: &[
            item(Level::Junior, "The text body belongs in blob storage, not in a row alongside the metadata"),
            item(Level::Junior, "Link generation and the read path — cache in front of the metadata lookup"),
            item(Level::Senior, "How expiry actually deletes: a sweeper job, a TTL on the object, or lazy on read"),
            item(Level::Senior, "Size limits enforced at upload, and what a 10MB paste does to the response path"),
            item(Level::Staff, "An unlisted link is not access control — say what the security model actually is"),
        ],
    },
    Scenario {
        id: "unique-id-generator",
        title: "Distributed Unique ID Generator",
        prompt: "Design a service that hands out unique 64-bit IDs across a fleet of machines.",
        difficulty: Difficulty::Easy,
        functional: &[
            "Every ID is unique across the whole fleet",
            "IDs are roughly sortable by creation time",
            "Callers can get an ID without a network round trip where possible",
        ],
        non_functional: &[
            "1M IDs/second at peak",
            "IDs fit in 64 bits",
            "No single machine is a bottleneck or a single point of failure",
        ],
        rubric: &[
            item(Level::Junior, "The bit layout: timestamp, machine id, sequence — and how many bits each gets"),
            item(Level::Junior, "Why a database auto-increment does not survive the move off one machine"),
            item(Level::Senior, "How a machine gets its id at boot, and what stops two machines sharing one"),
            item(Level::Senior, "Sequence exhaustion within a millisecond — block, or borrow from the next tick"),
            item(Level::Staff, "Clock skew and NTP going backwards is the failure that silently emits duplicates"),
        ],
    },
    Scenario {
        id: "uptime-monitor",
        title: "Uptime Monitor",
        prompt: "Design a service that checks whether customer websites are up and alerts when they are not.",
        difficulty: Difficulty::Easy,
        functional: &[
            "Check a URL on a schedule the customer chooses",
            "Alert by email or webhook when a check fails",
            "Show recent uptime history per site",
        ],
        non_functional: &[
            "500k monitored URLs, most checked every minute",
            "A real outage is alerted within two minutes",
            "A single failed check does not page anyone",
        ],
        rubric: &[
            item(Level::Junior, "Checks are queued work, not a loop on one box — scheduler and worker pool are separate"),
            item(Level::Junior, "Confirmation before alerting: N consecutive failures, or a second check from elsewhere"),
            item(Level::Senior, "Check results are a time series; storing every check forever is the wrong default"),
            item(Level::Senior, "Alert de-duplication and flap suppression so one outage is one page"),
            item(Level::Staff, "Distinguishing 'the site is down' from 'our checker's region is down'"),
        ],
    },
    Scenario {
        id: "feature-flags",
        title: "Feature Flag Service",
        prompt: "Design a service that lets engineers turn features on for a subset of users without a deploy.",
        difficulty: Difficulty::Easy,
        functional: &[
            "Define a flag and target it by user, percentage, or attribute",
            "Applications evaluate flags at request time",
            "Changing a flag takes effect quickly across the fleet",
        ],
        non_functional: &[
            "Flag evaluation adds under 1ms to a request",
            "50k application instances reading flags",
            "The control plane being down must not take the applications down",
        ],
        rubric: &[
            item(Level::Junior, "Evaluation happens in-process against a cached ruleset, not as a network call per flag"),
            item(Level::Junior, "How rules reach the fleet — polling a snapshot, or a push/stream channel"),
            item(Level::Senior, "Percentage rollouts must hash the user id so a user does not flip on every request"),
            item(Level::Senior, "The SDK serves the last known good ruleset when the control plane is unreachable"),
            item(Level::Staff, "Flag reads vastly outnumber writes, so the write path can be slow and strongly consistent"),
        ],
    },
    Scenario {
        id: "leaderboard",
        title: "Real-Time Leaderboard",
        prompt: "Design the leaderboard for a game with millions of concurrent players.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Show the global top 100",
            "Show a player their own rank and the players around them",
            "Scores update continuously during play",
        ],
        non_functional: &[
            "50M players, 100k score updates/second at peak",
            "Top-100 reads in under 100ms",
            "Ranks may lag by seconds, but must not be wrong by a lot",
        ],
        rubric: &[
            item(Level::Junior, "A sorted set in memory, not an ORDER BY over the whole players table on every read"),
            item(Level::Junior, "The top 100 is cached and shared; a player's own rank is the expensive query"),
            item(Level::Senior, "Sharding a global ranking is the hard part — say how ranks compose across shards"),
            item(Level::Senior, "Durability: the in-memory ranking is a derived view, so where does truth live"),
            item(Level::Staff, "Exact rank for every player is far more expensive than approximate — justify which you sell"),
        ],
    },
    Scenario {
        id: "ticket-booking",
        title: "Ticket Booking (Ticketmaster)",
        prompt: "Design seat selection and checkout for high-demand event ticketing.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Browse events and see which seats are free",
            "Hold a seat while the buyer completes payment",
            "Confirm the booking, or release the seat if payment fails",
        ],
        non_functional: &[
            "A popular on-sale draws 1M people in the first minute",
            "A seat is never sold twice",
            "Browsing stays available while an on-sale is in progress",
        ],
        rubric: &[
            item(Level::Junior, "A seat hold with an expiry, distinct from a confirmed booking"),
            item(Level::Junior, "Reads of seat availability and writes of a reservation are very different workloads"),
            item(Level::Senior, "The reservation write is the serialization point — a transaction or conditional update, not a read-then-write"),
            item(Level::Senior, "Expiring held seats reliably when the buyer walks away mid-checkout"),
            item(Level::Staff, "A queue in front of the on-sale converts a thundering herd into throughput you control"),
        ],
    },
    Scenario {
        id: "job-scheduler",
        title: "Distributed Job Scheduler",
        prompt: "Design a service that runs user-defined jobs on a schedule across a worker fleet.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Schedule a job by cron expression or one-off timestamp",
            "Run the job on some worker and record the result",
            "Retry failures with backoff",
        ],
        non_functional: &[
            "10M scheduled jobs, 50k firing per minute",
            "A job due at 09:00 starts within seconds of 09:00",
            "A job is not silently skipped when a worker dies",
        ],
        rubric: &[
            item(Level::Junior, "Separate deciding what is due from executing it — scheduler and workers are different tiers"),
            item(Level::Junior, "Workers claim jobs from a queue; the claim has a lease so a dead worker's job is retaken"),
            item(Level::Senior, "Finding due jobs without scanning all 10M — a time-bucketed index or a priority structure"),
            item(Level::Senior, "At-least-once delivery means jobs must be idempotent; say so and say who enforces it"),
            item(Level::Staff, "A backlog after an outage will fire thousands of overdue jobs at once unless it is shaped"),
        ],
    },
    Scenario {
        id: "proximity-service",
        title: "Nearby Places (Yelp)",
        prompt: "Design the service that answers 'what businesses are within 5km of me'.",
        difficulty: Difficulty::Medium,
        functional: &[
            "Return businesses within a radius of a point",
            "Filter by category and sort by distance",
            "Business details and hours are read alongside the search",
        ],
        non_functional: &[
            "200M businesses, 5k searches/second",
            "Search returns in under 200ms",
            "Business data changes rarely; searches are constant",
        ],
        rubric: &[
            item(Level::Junior, "You cannot compute distance to 200M rows per query — the index does the narrowing"),
            item(Level::Junior, "Business records are read-heavy and nearly static, so they cache well"),
            item(Level::Senior, "Geohash, quadtree, or S2 — and why the chosen cell size fits a 5km query"),
            item(Level::Senior, "A radius that spans cell boundaries must search neighbouring cells too"),
            item(Level::Staff, "Density is wildly uneven: a fixed grid gives one cell for Montana and thousands for Manhattan"),
        ],
    },
    Scenario {
        id: "file-sync",
        title: "File Sync (Dropbox)",
        prompt: "Design a service that keeps a folder in sync across a user's devices.",
        difficulty: Difficulty::Hard,
        functional: &[
            "Upload a file and have it appear on the user's other devices",
            "Sync only what changed, not the whole file",
            "Resolve the case where two devices edit the same file",
        ],
        non_functional: &[
            "100M users, 1GB average per user",
            "A small edit to a large file syncs quickly",
            "A file is never silently lost or corrupted",
        ],
        rubric: &[
            item(Level::Junior, "Files are chunked; metadata and chunk storage are separate systems"),
            item(Level::Junior, "Clients are notified of changes rather than polling for them"),
            item(Level::Senior, "Content-addressed chunks give deduplication and make an interrupted upload resumable"),
            item(Level::Senior, "A version or revision chain per file, so a conflict is detectable rather than last-write-wins"),
            item(Level::Staff, "Conflict resolution is a product decision — say what the user sees, not just what the server does"),
        ],
    },
    Scenario {
        id: "collaborative-editor",
        title: "Collaborative Document Editor",
        prompt: "Design a document editor where several people type in the same document at once.",
        difficulty: Difficulty::Hard,
        functional: &[
            "Multiple users edit one document simultaneously",
            "Everyone converges on the same final text",
            "Show who else is in the document and where their cursor is",
        ],
        non_functional: &[
            "Up to 50 concurrent editors per document",
            "A keystroke appears to collaborators in under 200ms",
            "No edit is lost, including edits made while briefly offline",
        ],
        rubric: &[
            item(Level::Junior, "A persistent connection per editing session, with one server owning a given document"),
            item(Level::Junior, "Edits are operations against a version, not whole-document saves"),
            item(Level::Senior, "OT or CRDT — name the choice and what it costs, rather than hand-waving 'merge'"),
            item(Level::Senior, "Presence and cursors are ephemeral, high-frequency data and should not go through the durable path"),
            item(Level::Staff, "Failing over a document to another server mid-session without losing unacknowledged operations"),
        ],
    },
    Scenario {
        id: "key-value-store",
        title: "Distributed Key-Value Store",
        prompt: "Design a replicated key-value store that survives node loss.",
        difficulty: Difficulty::Hard,
        functional: &[
            "get(key) and put(key, value)",
            "Data is replicated across nodes",
            "Nodes can be added and removed while serving",
        ],
        non_functional: &[
            "Tens of TB across hundreds of nodes",
            "Single-digit millisecond reads",
            "Stays available for reads during a network partition",
        ],
        rubric: &[
            item(Level::Junior, "Keys are partitioned across nodes, and a client can find the node for a key"),
            item(Level::Junior, "Every key is on more than one node, so losing a node loses no data"),
            item(Level::Senior, "Consistent hashing with virtual nodes, and what happens to keys when a node joins"),
            item(Level::Senior, "The consistency choice, stated as a trade: quorum reads and writes, or eventual with repair"),
            item(Level::Staff, "Detecting and reconciling divergent replicas — vector clocks, read repair, anti-entropy"),
        ],
    },
    Scenario {
        id: "stock-exchange",
        title: "Stock Exchange",
        prompt: "Design the order matching engine and market data feed for an exchange.",
        difficulty: Difficulty::Hard,
        functional: &[
            "Accept limit and market orders, and cancellations",
            "Match orders by price then time priority",
            "Publish trades and the order book to subscribers",
        ],
        non_functional: &[
            "500k orders/second per symbol at open",
            "Order acknowledged in microseconds, not milliseconds",
            "Every order and trade is auditable and replayable",
        ],
        rubric: &[
            item(Level::Junior, "Matching is per-symbol, so symbols partition cleanly across machines"),
            item(Level::Junior, "The order book is an in-memory structure; a database in the matching path is too slow"),
            item(Level::Senior, "Determinism: a single writer per symbol, because concurrent matching breaks price-time priority"),
            item(Level::Senior, "Durability without disk in the hot path — sequenced input log, replicated before acknowledgement"),
            item(Level::Staff, "Failover must replay the input log to reach the identical book state, not an approximate one"),
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
    fn ids_are_unique() {
        // `find` takes the first match, so a duplicated id would shadow a whole scenario
        // rather than fail anything — and the list would show two rows sharing progress.
        let mut ids: Vec<_> = SCENARIOS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two scenarios share an id");
    }

    #[test]
    fn the_rubric_never_goes_over_the_wire() {
        let json = serde_json::to_string(&SCENARIOS[0]).unwrap();
        assert!(!json.contains("rubric"), "the rubric leaked into the scenario list: {json}");
        assert!(json.contains("url-shortener"), "the rest of the scenario should still serialize");
    }
}
