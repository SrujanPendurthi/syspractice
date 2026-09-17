//! The teaching material.
//!
//! The syllabus is not invented here — it is `scenarios.rs`. Every concept below exists
//! because rubric lines across the scenario catalogue keep asking for it, and each one
//! names the scenarios that exercise it. If a concept has no scenarios, it is not worth
//! teaching in an interview trainer; if a rubric theme has no concept, that is the gap.
//!
//! Content is structured data rather than markdown on purpose: the shape is uniform, so a
//! parser would buy nothing but a dependency and an HTML injection surface.

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub heading: &'static str,
    pub body: &'static str,
}

const fn s(heading: &'static str, body: &'static str) -> Section {
    Section { heading, body }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Concept {
    pub id: &'static str,
    pub title: &'static str,
    /// One line, shown in the list. Says what the idea *is*, not that it is important.
    pub summary: &'static str,
    pub sections: &'static [Section],
    /// What an interviewer actually asks once you have named the technique. These are the
    /// follow-ups the critic is told to push on, written as questions to rehearse.
    pub probes: &'static [&'static str],
    /// Scenario ids where this shows up in the rubric. Powers "read this before that".
    pub scenarios: &'static [&'static str],
}

pub fn find(id: &str) -> Option<&'static Concept> {
    CONCEPTS.iter().find(|c| c.id == id)
}

/// Concepts relevant to a scenario, in syllabus order.
pub fn for_scenario(scenario_id: &str) -> impl Iterator<Item = &'static Concept> + '_ {
    CONCEPTS.iter().filter(move |c| c.scenarios.contains(&scenario_id))
}

pub static CONCEPTS: &[Concept] = &[
    Concept {
        id: "estimation",
        title: "Back-of-the-envelope estimation",
        summary: "Turning a product description into numbers that constrain the design.",
        sections: &[
            s(
                "What it is",
                "Working out, from stated assumptions, roughly how much traffic and how much \
data the system handles. Not precision — the point is the order of magnitude, because \
that is what decides whether the answer is one box or a fleet.",
            ),
            s(
                "How to do it without getting lost",
                "Write the assumptions down before you compute anything: users, actions per \
user per day, bytes per action, retention. Then derive. Convert to per-second by dividing \
by 86,400 — round it to 100,000 and say you are rounding. Peak is usually taken as two to \
three times average, and you should say which multiple you chose.",
            ),
            s(
                "The mistake that costs the most",
                "Unit slips. A thousand-fold error between GB and TB, or between per-day and \
per-second, changes the architecture and nothing in the sentence looks wrong. \
100M writes/day × 500 bytes is 50 GB/day, which is 18 TB/year — not 18 GB, and the \
difference is whether this fits on a laptop. Recompute anything you are about to build on.",
            ),
            s(
                "What the numbers are for",
                "A figure you do not use is a figure you did not need. Storage per year picks \
the storage tier and the partitioning scheme. Reads per second picks the cache. \
Read:write ratio decides whether you optimise the read path or the write path. State the \
consequence out loud — an estimate with no decision attached reads as ritual.",
            ),
        ],
        probes: &[
            "What is your read-to-write ratio, and which path did you therefore optimise?",
            "What peak multiple did you assume over average, and why that one?",
            "How much storage after a year, and does that change where it lives?",
        ],
        scenarios: &["url-shortener", "news-feed", "metrics-pipeline", "object-storage"],
    },
    Concept {
        id: "caching",
        title: "Caching and invalidation",
        summary: "Keeping a second copy of hot data closer to the reader, and the cost of it.",
        sections: &[
            s(
                "What it is",
                "A fast store in front of a slow one, holding the small fraction of data that \
gets most of the reads. Most read-heavy designs are mostly a caching story, which is why \
'where is your cache and what is in it' is close to a default interview question.",
            ),
            s(
                "The strategies, and when each fits",
                "Cache-aside is the default: the application reads the cache, misses, reads \
the database, writes the cache. Read-through hides that behind the cache itself. \
Write-through writes both on every write and keeps them consistent at the cost of write \
latency. Write-behind batches writes to the database and risks losing them. Pick one and \
be able to say what happens on a miss.",
            ),
            s(
                "Invalidation",
                "The hard half. A TTL is the cheap answer and means you serve stale data for \
up to the TTL — fine for a feed, wrong for a permissions check. Explicit invalidation on \
write is exact and needs every writer to know about the cache. Versioned keys sidestep \
deletion entirely by changing the key when the data changes.",
            ),
            s(
                "What happens when it is empty",
                "This is the question that separates a rehearsed answer from a real one. A \
cold cache sends every read to the database at once — a thundering herd that can be worse \
than no cache at all, because the system was scaled on the assumption the cache absorbs \
that load. Request coalescing, staggered TTLs and warming are the usual answers.",
            ),
        ],
        probes: &[
            "What is your hit rate assumption, and what happens to the database if it is wrong?",
            "The cache restarts empty at peak — what does the database see?",
            "How does a write invalidate, and what does a reader see in between?",
        ],
        scenarios: &["url-shortener", "news-feed", "proximity-service", "search-autocomplete", "feature-flags"],
    },
    Concept {
        id: "partitioning",
        title: "Partitioning and sharding",
        summary: "Splitting data across machines once it stops fitting on one, and choosing the key.",
        sections: &[
            s(
                "What it is",
                "Dividing a dataset so each machine holds a slice. Vertical partitioning \
splits by table or column; horizontal partitioning — sharding — splits rows by a key. \
Almost every 'how does this scale' answer ends here.",
            ),
            s(
                "The key is the whole decision",
                "The partition key determines which queries stay on one machine and which \
have to fan out to all of them. Messages partitioned by conversation keep a chat's history \
together; partitioned by user they do not, and every conversation read hits two shards. \
Pick the key from the dominant read, then check what it costs the other reads.",
            ),
            s(
                "Hot spots",
                "Range partitioning by something monotonic — a timestamp, an auto-increment \
id — puts all current writes on the newest shard while the rest idle. Hashing spreads \
writes but destroys range scans. Real skew is worse than either: a celebrity account or a \
dense city gets a shard of its own whether you planned for it or not.",
            ),
            s(
                "Rebalancing",
                "Adding a machine means moving data. Modulo-based assignment moves nearly \
everything; consistent hashing moves roughly one shard's worth. Say what happens to \
in-flight reads during the move, because 'we add a node' is where a plausible design \
usually stops being one.",
            ),
        ],
        probes: &[
            "What is your partition key, and which query does it make expensive?",
            "One customer is a hundred times bigger than the rest — what happens?",
            "You add a node on Monday. What moves, and who notices?",
        ],
        scenarios: &["chat-app", "url-shortener", "key-value-store", "metrics-pipeline", "payment-ledger", "object-storage"],
    },
    Concept {
        id: "consistent-hashing",
        title: "Consistent hashing",
        summary: "Assigning keys to nodes so that adding one moves a small slice, not everything.",
        sections: &[
            s(
                "The problem it solves",
                "`hash(key) % n` is the obvious assignment and it is catastrophic to change: \
going from 10 nodes to 11 changes the destination of roughly 90% of keys. For a cache that \
is a total flush; for a data store it is a full reshuffle while serving.",
            ),
            s(
                "How it works",
                "Hash both keys and nodes onto the same ring of values. A key belongs to the \
first node clockwise from it. Adding a node captures only the keys between it and its \
predecessor — about 1/n of the total — and removing one hands its range to its neighbour. \
Nothing else moves.",
            ),
            s(
                "Virtual nodes",
                "With one point per node the ring is lumpy: by luck one node owns a third of \
it. Giving each physical node a few hundred points spreads ownership evenly and lets a \
bigger machine take a bigger share by simply getting more points. This is the part people \
omit, and it is the part that makes the technique work in practice.",
            ),
            s(
                "What it does not solve",
                "It balances keys, not load. One key read a million times a second is still \
one node's problem, and consistent hashing has nothing to say about it. That is a hot-key \
problem, answered by replication of that key or a cache in front.",
            ),
        ],
        probes: &[
            "Why not just hash modulo the node count?",
            "How many virtual nodes, and what is the effect of too few?",
            "One key is a hundred times hotter than any other — does this help at all?",
        ],
        scenarios: &["key-value-store", "chat-app", "object-storage"],
    },
    Concept {
        id: "replication",
        title: "Replication and consistency",
        summary: "Keeping copies on more than one machine, and what readers see while they diverge.",
        sections: &[
            s(
                "Why copies",
                "Durability, so losing a machine does not lose data, and read throughput, so \
reads spread across replicas. These are different goals and they pull in different \
directions: more replicas means more durability and more write cost.",
            ),
            s(
                "Synchronous or asynchronous",
                "Synchronous replication acknowledges the write only once replicas have it — \
no data loss on failover, slower writes, and unavailable if a replica is unreachable. \
Asynchronous acknowledges immediately and can lose the tail on failover. Most real systems \
run semi-synchronous: one synchronous replica, the rest catching up.",
            ),
            s(
                "Quorums",
                "With N replicas, requiring W to acknowledge a write and R to answer a read, \
R + W > N guarantees a read sees the latest write, because the sets must overlap. It lets \
you slide between fast writes and fast reads without changing the storage engine. Say which \
you chose and which operation you made slower.",
            ),
            s(
                "What a reader actually sees",
                "'Eventually consistent' is not an answer on its own — the question is what \
is visible in the meantime. Reading your own writes, monotonic reads, and seeing a reply \
before the message it answers are the failures users notice. Name the anomaly your design \
permits and why it is acceptable here.",
            ),
        ],
        probes: &[
            "A user writes, then immediately reads from another replica. What do they see?",
            "The primary dies with writes not yet replicated. What was lost, and who finds out?",
            "What are your R and W, and what did that cost?",
        ],
        scenarios: &["key-value-store", "payment-ledger", "object-storage", "chat-app"],
    },
    Concept {
        id: "fan-out",
        title: "Fan-out on write versus on read",
        summary: "Whether a feed is assembled when something is posted or when someone looks.",
        sections: &[
            s(
                "The two shapes",
                "Fan-out on write pushes each post into every follower's precomputed feed at \
publish time: reads are a single cheap lookup, writes are expensive in proportion to the \
follower count. Fan-out on read assembles the feed by querying the people you follow when \
you open the app: writes are trivial, reads are expensive and repeated.",
            ),
            s(
                "Which one the graph forces",
                "It depends on the ratio of reads to writes and on the shape of the follower \
graph. A social feed is read far more than written, which argues for fan-out on write. A \
group chat of 500 is not, which is why the same question gets the opposite answer there.",
            ),
            s(
                "Celebrities break it",
                "One account with ten million followers turns a single post into ten million \
writes. The standard answer is hybrid: fan out on write for ordinary accounts, leave \
celebrity posts out of it, and merge them in at read time from a short list of the \
high-fan-out accounts the reader follows. Naming this unprompted is most of the question.",
            ),
            s(
                "Feeds are bounded",
                "A precomputed feed is capped and trimmed — a few hundred entries, not \
everything since signup. Unbounded per-user feed storage multiplied by the user count is \
usually the largest number in the design, and nobody scrolls that far.",
            ),
        ],
        probes: &[
            "An account with ten million followers posts. Walk me through what happens.",
            "How long is a stored feed, and what happens when someone scrolls past the end?",
            "A user follows someone new — does their feed backfill?",
        ],
        scenarios: &["news-feed", "chat-app", "notification-service"],
    },
    Concept {
        id: "queues",
        title: "Queues and asynchronous work",
        summary: "Moving work off the request path so a slow step cannot take the front door down.",
        sections: &[
            s(
                "What it buys",
                "Three things worth naming separately: latency, because the caller gets an \
answer before the work is done; isolation, because a failing consumer does not fail the \
producer; and absorption, because a queue takes a burst that the workers behind it could \
not have taken directly.",
            ),
            s(
                "What it costs",
                "Everything downstream becomes eventually consistent, and the user has to be \
told — 'your upload is processing' is a product decision forced by an architectural one. \
You also inherit ordering questions, duplicate delivery, and a new thing that can back up \
silently.",
            ),
            s(
                "Backpressure and the backlog",
                "A queue that grows without bound converts a latency problem into an outage \
several hours later. Decide what happens when it fills: shed load, reject at the edge, or \
scale consumers. And say what happens after an outage — a scheduler with a two-hour backlog \
will fire every overdue job at once unless the drain is deliberately shaped.",
            ),
            s(
                "Delivery guarantees",
                "At-most-once loses messages, at-least-once duplicates them, exactly-once is \
mostly at-least-once plus idempotent consumers. Pick at-least-once and make the consumer \
idempotent; claiming exactly-once without saying how is the answer that gets probed.",
            ),
        ],
        probes: &[
            "The consumers stop for two hours. What does recovery look like?",
            "Where is the backpressure, and what does the user see when it engages?",
            "Does this queue need ordering? Per what key?",
        ],
        scenarios: &["notification-service", "job-scheduler", "web-crawler", "metrics-pipeline", "uptime-monitor"],
    },
    Concept {
        id: "idempotency",
        title: "Idempotency",
        summary: "Making a repeated operation harmless, because retries are not optional.",
        sections: &[
            s(
                "Why it is unavoidable",
                "Any network call can time out after the work was done and before the answer \
arrived. The caller cannot distinguish that from a call that never landed, so it retries. \
Every at-least-once queue, every client retry, and every failover creates duplicates — the \
only choice is whether the second copy is harmless.",
            ),
            s(
                "How to get it",
                "Some operations are naturally idempotent: setting a value, deleting by id. \
Others are not: incrementing a balance, sending an email, charging a card. For those, the \
caller supplies an idempotency key and the server records it with the result, so the second \
request returns the first answer instead of doing the work again.",
            ),
            s(
                "Where the key lives",
                "The record of used keys must be written in the same transaction as the \
effect, or a crash between the two reintroduces exactly the problem. If the effect is \
external — a payment, an email — you need an outbox: commit the intent with the data, and \
have a separate process perform it and mark it done.",
            ),
            s(
                "Say who enforces it",
                "'The consumer is idempotent' is a claim, not a design. Name the key, name \
where the dedup record is stored, and say how long it is kept — because keeping every key \
forever is its own storage problem.",
            ),
        ],
        probes: &[
            "The same message is delivered twice. Walk me through what changes.",
            "What is the idempotency key here, and who generates it?",
            "You crash between doing the work and recording that you did it. Then what?",
        ],
        scenarios: &["payment-ledger", "job-scheduler", "notification-service", "chat-app"],
    },
    Concept {
        id: "rate-limiting",
        title: "Rate limiting",
        summary: "Capping how often a caller may act, and where the counter lives.",
        sections: &[
            s(
                "The algorithms",
                "Fixed window is a counter per interval — cheapest, and lets through twice \
the limit across a boundary. Sliding window log stores a timestamp per request: exact, and \
memory grows with traffic. Sliding window counter interpolates between two fixed windows \
and is the usual compromise. Token bucket allows a burst up to the bucket size and then \
settles to the refill rate, which is what most APIs actually want.",
            ),
            s(
                "Where the counter lives",
                "In-process is fast and wrong across a fleet — ten servers with a local limit \
of 100 is a limit of 1000. A shared store is correct and adds a network hop to every \
request. The common answer is a shared store with local batching, and an explicit statement \
of how much overshoot that permits.",
            ),
            s(
                "When the counter is unreachable",
                "Fail open and an outage of the limiter becomes an outage of everything \
behind it. Fail closed and a limiter blip is a total outage. There is no right answer, \
only a stated one — and it usually differs between a public API and an internal one.",
            ),
            s(
                "Tell the caller",
                "Return the limit, the remaining quota and when it resets. A 429 with no \
information leaves the client guessing, and a guessing client retries immediately, which is \
the behaviour the limit exists to stop.",
            ),
        ],
        probes: &[
            "Two requests either side of a window boundary — how many got through?",
            "The counter store is down. Fail open or closed, and why that one here?",
            "Where do you enforce it: edge, gateway, or service?",
        ],
        scenarios: &["rate-limiter", "web-crawler", "notification-service"],
    },
    Concept {
        id: "geospatial",
        title: "Geospatial indexing",
        summary: "Answering 'what is near me' without measuring the distance to everything.",
        sections: &[
            s(
                "Why an index at all",
                "Computing distance from a point to two hundred million rows per query is not \
a design. The index exists to narrow the candidate set to a few hundred before any distance \
maths happens.",
            ),
            s(
                "The techniques",
                "Geohash interleaves latitude and longitude bits into a string, so a shared \
prefix means physical proximity and a plain string-prefix index does the narrowing. \
Quadtrees subdivide space recursively and adapt to density. S2 projects onto a sphere and \
avoids the distortion the other two inherit near the poles. All three turn two dimensions \
into one that an ordinary index can handle.",
            ),
            s(
                "Cell size is the trade",
                "Cells too large and you scan far too many candidates; too small and a single \
query touches many cells. Worse, a radius rarely sits inside one cell — a correct query has \
to search the neighbours too, and forgetting that produces a search that silently misses \
everything just over the line.",
            ),
            s(
                "Density is not uniform",
                "A fixed grid gives one cell for Montana and thousands for Manhattan. Either \
adapt the subdivision to density, as a quadtree does, or accept that some cells will be \
enormous and plan for the scan. This is the follow-up that distinguishes having read about \
geohashing from having used it.",
            ),
        ],
        probes: &[
            "Your 5km radius crosses a cell boundary. What does the query do?",
            "How do you pick the cell size, and what breaks at each extreme?",
            "Downtown versus rural — same cell size?",
        ],
        scenarios: &["ride-sharing", "proximity-service"],
    },
    Concept {
        id: "time-series",
        title: "Time series and aggregation",
        summary: "Storing measurements that arrive constantly and are read in ranges.",
        sections: &[
            s(
                "Why it is its own problem",
                "A row per data point in a relational table is the wrong default: writes are \
relentless and append-only, reads are almost always a range over time, and old data loses \
value at a predictable rate. Nothing about that matches a general-purpose row store.",
            ),
            s(
                "Roll-ups and retention",
                "Keep raw points at full resolution for a short window, then aggregate into \
coarser buckets as they age — a minute of one-second samples becomes one one-minute sample. \
Retention is part of the design, not an operational afterthought: 'store every check \
forever' is the wrong answer for almost every system that produces measurements.",
            ),
            s(
                "Cardinality is what kills you",
                "The number of distinct series is the product of every label's distinct \
values. Adding a user id or a request id as a label multiplies that by millions, and the \
index — not the data — is what falls over. This is the most common real failure in metrics \
systems and it never shows up in a load test with three test users.",
            ),
            s(
                "Late and out-of-order data",
                "Points arrive after their bucket has been aggregated, because a client was \
offline or a queue backed up. Decide whether to drop them, hold buckets open for a grace \
period, or recompute — and say which, because silently dropping them makes the graphs quietly wrong.",
            ),
        ],
        probes: &[
            "What is your cardinality, and which label could multiply it?",
            "A point arrives ten minutes late. Where does it go?",
            "What does data from a year ago look like, and what did it cost to keep?",
        ],
        scenarios: &["metrics-pipeline", "uptime-monitor", "leaderboard"],
    },
    Concept {
        id: "conflict-resolution",
        title: "Conflict resolution and versioning",
        summary: "What happens when two people change the same thing while apart.",
        sections: &[
            s(
                "Why conflicts are inevitable",
                "Any system that lets clients work offline, or that accepts writes in more \
than one place, will eventually take two changes to the same object that neither knew about \
the other. You do not get to avoid this — you only get to choose whether the resolution is \
designed or accidental.",
            ),
            s(
                "Detecting one at all",
                "Last-write-wins by wall-clock timestamp is the default and it is not \
conflict resolution, it is silent data loss with clock skew deciding the winner. Detection \
needs causality: a version number per object that a writer must present, a revision chain, \
or vector clocks that record what each replica had seen. Only once you can *detect* a \
conflict do you get to decide anything.",
            ),
            s(
                "Merging text as it is typed",
                "Operational transformation rewrites each incoming operation against the ones \
already applied, which converges but needs a central server to fix the order. CRDTs define \
data types whose merges commute, so any order converges and no server has to arbitrate — at \
the cost of metadata that grows with the edit history. Name the one you chose and what it \
costs; 'we merge the changes' is the answer that gets probed.",
            ),
            s(
                "Repair, and the part that is not technical",
                "Divergent replicas are reconciled by read repair on access and by \
anti-entropy sweeps in the background. But for a file or a document, resolution is a product \
decision before it is an engineering one: say what the user sees. Keeping both copies with a \
name like 'conflicted copy' is a real, defensible answer, and it beats a merge nobody asked \
for.",
            ),
        ],
        probes: &[
            "Two devices edit offline and both come back. What does the user see?",
            "How do you tell a conflict from a simple overwrite?",
            "Why not last-write-wins on timestamp?",
        ],
        scenarios: &["file-sync", "collaborative-editor", "key-value-store"],
    },
    Concept {
        id: "failure-modes",
        title: "Failure modes",
        summary: "The questions that turn a diagram that works into one that keeps working.",
        sections: &[
            s(
                "Single points of failure",
                "Walk your own diagram and ask, box by box, what happens when this one is \
gone. The answer is often that the whole thing stops, and that a box drawn once is a box \
nobody thought about replicating. Doing this out loud, unprompted, is the single highest-value \
habit in a system design interview.",
            ),
            s(
                "Thundering herds",
                "Anything that makes many clients act at once: a cache restarting empty, a \
TTL that expires for everyone simultaneously, every client retrying on the same schedule \
after an outage. Jittered TTLs, exponential backoff with jitter, and request coalescing are \
the standard answers, and the jitter is the part that gets left out.",
            ),
            s(
                "Retries make outages worse",
                "A struggling service that starts timing out receives every request two or \
three times over, which is precisely when it can least afford it. Circuit breakers exist to \
stop that feedback loop; retry budgets cap the amplification. Saying 'we retry' without \
either is describing the mechanism by which a slowdown becomes an outage.",
            ),
            s(
                "Degrade rather than fail",
                "Decide, per dependency, what the system does without it. A recommendations \
service being down should cost you recommendations, not the page. This is where a senior \
answer separates itself: not 'everything is replicated' but 'here is what we serve when this \
specific thing is unavailable'.",
            ),
        ],
        probes: &[
            "Point at any box and tell me what happens when it disappears.",
            "The cache comes back empty during peak. Walk me through the next sixty seconds.",
            "Which dependencies can be down while the product still works?",
        ],
        scenarios: &["url-shortener", "rate-limiter", "video-streaming", "news-feed", "key-value-store", "stock-exchange"],
    },
    // --- machine learning ----------------------------------------------------
    Concept {
        id: "training-serving-skew",
        title: "Training-serving skew",
        summary: "A feature computed one way in training and another way at serving time.",
        sections: &[
            s(
                "The shape of the bug",
                "Your model trained on a feature called purchases_last_7d, computed by a nightly batch job over the warehouse. At serving time the same feature is read from a streaming counter that resets on deploy and counts a slightly different set of events. The model is now being asked about a number it has never seen. Nothing errors. Offline metrics are unchanged, because offline you recompute the batch version. The model is simply worse in production than it was in the notebook, and no single test fails.",
            ),
            s(
                "Why it is the default outcome",
                "Training reads history; serving reads now. History is a table you can join against at leisure, and now is a lookup you have eighty milliseconds for. Those two constraints push you toward two different implementations of the same idea, written by different people at different times, and the moment there are two implementations they begin to drift. The bug is not that someone was careless — it is that the system had two places to encode one definition.",
            ),
            s(
                "Point-in-time correctness",
                "The subtler version is time travel. When you build a training row for a transaction at 10:03, every feature must be as of 10:03 — not as of when the job ran. A naive join against a current aggregates table leaks the future into the past: the model learns from a 'transactions in the last hour' count that already includes the transaction it is being asked to score. Offline results look extraordinary. Production results do not.",
            ),
            s(
                "What actually fixes it",
                "One definition, computed once, read by both paths — which is what a feature store is for. Failing that: log the exact feature vector that was served, and train on those logged vectors rather than recomputing from the warehouse. That turns the question from 'are these two implementations the same?' into 'is this the row we served?', which is a question you can answer.",
            ),
        ],
        probes: &[
            "Point at one feature on your diagram and walk me through how it is computed in training, then how it is computed at serving time.",
            "Your model uses a count over the last hour. When you build a training row for an event at 10:03, what window does that count cover?",
            "How would you find out that a feature had silently changed definition three weeks ago?",
        ],
        scenarios: &["feed-ranking", "fraud-detection", "realtime-personalisation", "ad-click-prediction"],
    },
    Concept {
        id: "feature-stores",
        title: "Feature stores",
        summary: "One definition of a feature, served to training and to inference alike.",
        sections: &[
            s(
                "What it is for",
                "A feature store exists to make one claim true: the number the model trained on and the number the model is served are produced by the same code. It does that by owning the definition, materialising it into an offline table for training and an online key-value store for serving, and refusing to let anyone compute it a third way. Everything else it does — discovery, sharing, versioning — is secondary to that one guarantee.",
            ),
            s(
                "The two halves have different jobs",
                "The offline half is a warehouse table optimised for large point-in-time joins: give me every feature as of this timestamp, for ten million rows. The online half is a single-digit-millisecond lookup by entity key, for one row. They hold the same values and almost nothing else in common, which is why the two are usually different systems with a pipeline between them, and why the lag of that pipeline is a number worth knowing.",
            ),
            s(
                "When you do not need one",
                "If every feature is computed from the request itself, there is no skew to prevent and a feature store is infrastructure you are running for nothing. The cost only starts to pay when features are aggregates over history, shared between models, or computed by a pipeline that is not the serving path. Reaching for one on a design that has three request-derived features is a signal you are describing a stack rather than solving a problem.",
            ),
            s(
                "Freshness is the design decision",
                "Every feature has a staleness budget and they are not the same. A user's lifetime purchase count can be a day old. Their count of items viewed in this session cannot be a minute old. Sorting features by how stale they are allowed to be tells you which need a streaming path and which can ride a nightly batch — and that split, not the store itself, is the part an interviewer is listening for.",
            ),
        ],
        probes: &[
            "Which of your features can be a day stale, and which cannot survive being a minute old?",
            "What happens to a prediction when the online feature lookup times out?",
            "How does a feature get from the streaming pipeline into the table you train on?",
        ],
        scenarios: &["product-recommender", "fraud-detection", "realtime-personalisation", "feed-ranking"],
    },
    Concept {
        id: "offline-vs-online-evaluation",
        title: "Offline and online evaluation",
        summary: "Why the metric that improved is not the metric anyone cared about.",
        sections: &[
            s(
                "Two different questions",
                "Offline evaluation asks: on data I already have, does this model rank better than that one? Online evaluation asks: when real people are shown this, does the thing we want to happen happen more often? They are related by assumption, not by arithmetic. Every offline metric is a proxy, and the entire craft is knowing how far you can trust yours before you have to go and find out.",
            ),
            s(
                "The gap has a cause",
                "Offline data was collected under the old model. It contains the items the old model chose to show, judged by people who were shown them in the old model's order. A new model that would surface something better gets no credit offline, because nobody ever clicked a thing they never saw. This is why offline NDCG can climb for a quarter while the online metric sits flat: you are getting better at reproducing the logs, which is not the job.",
            ),
            s(
                "Choosing the offline metric",
                "Pick the one whose failure mode you can live with. Accuracy is meaningless under class imbalance. AUC measures ranking and says nothing about whether the numbers are calibrated, which matters the moment the output is multiplied by money. NDCG cares about order near the top, which is right for search and wrong for a queue that a human works through exhaustively. Say which one, and say why it is the right proxy for this problem.",
            ),
            s(
                "The online half",
                "An A/B test is the arbiter, and it has its own traps: an effect too small for your traffic to resolve, a metric that moves because a bug shipped alongside, a novelty bump that decays over two weeks. Decide the metric, the minimum effect worth shipping, and the duration before you start. A test you stop when it looks good is not a test.",
            ),
        ],
        probes: &[
            "Which offline metric are you optimising, and what would make it disagree with your online metric?",
            "Your offline number improved by 4% and the A/B test is flat. Where do you look first?",
            "How long does your experiment run, and who decided that before it started?",
        ],
        scenarios: &[
            "search-ranking",
            "feed-ranking",
            "ad-click-prediction",
            "product-recommender",
            "content-moderation",
        ],
    },
    Concept {
        id: "embeddings-and-vector-search",
        title: "Embeddings and vector search",
        summary: "Turning things into vectors so that near means similar, and finding the near ones fast.",
        sections: &[
            s(
                "What an embedding buys",
                "An embedding maps an item into a vector where distance means something — that two documents are about the same subject, that two products get bought by the same people. Once you have that, 'find me similar things' becomes a geometry problem instead of a keyword problem, which is what lets you retrieve for a query whose exact words appear nowhere in the corpus. What you lose is exactness: the space encodes whatever the training objective rewarded, and nothing else.",
            ),
            s(
                "Approximate is the whole point",
                "Exact nearest-neighbour search over ten million vectors is a full scan, and the cost is the same for every query. Approximate indexes — HNSW, IVF, product quantisation — trade a recall guarantee for orders of magnitude of speed. The number that matters is recall at your k: what fraction of the true top ten does the index actually return? An ANN index is a tunable, and shipping one without knowing its recall is shipping an unknown amount of silent loss.",
            ),
            s(
                "Rebuilding is the operational problem",
                "Embeddings come from a model. Change the model and every vector in the index is in a different space and meaningless against the old ones — there is no incremental migration, only a full re-embed and a swap. Meanwhile new items arrive continuously and must be added to an index that some structures do not cheaply support inserting into. Say how a new item becomes searchable, and say what happens on the day the embedding model changes.",
            ),
            s(
                "Where the filter goes",
                "Real queries are never just similarity. They are similarity plus in-stock, plus in this region, plus this user is allowed to see it. Filtering after retrieval can empty your result set; filtering before it can defeat the index. Which way round you do it, and what happens when the filter is very selective, is usually the most interesting question about a vector search design.",
            ),
        ],
        probes: &[
            "What is your index's recall at ten, and how do you know?",
            "You retrain the embedding model. Walk me through the rollout.",
            "A user can only see documents their team owns. Where does that filter apply?",
        ],
        scenarios: &["rag-over-docs", "search-ranking", "product-recommender", "realtime-personalisation"],
    },
    Concept {
        id: "retrieval-and-ranking",
        title: "Retrieval then ranking",
        summary: "Two stages with different costs, because you cannot afford the good model on everything.",
        sections: &[
            s(
                "Why it is always two stages",
                "You have ten million candidates and eighty milliseconds. The model that ranks well enough to matter costs a millisecond per item. The arithmetic does not work, and no amount of hardware makes it work. So retrieval cuts ten million to a few hundred using something cheap and recall-oriented, and ranking spends the real budget on those few hundred. Any design that scores the whole corpus per request has not met the latency requirement yet.",
            ),
            s(
                "The two stages are optimised for different things",
                "Retrieval is judged on recall: did the good answer survive into the candidate set? Its precision barely matters, because ranking will sort it out. Ranking is judged on the order of the top few, and its recall is irrelevant because it never sees what retrieval dropped. Conflating the two — tuning retrieval on NDCG, or worrying about a ranker's recall — is a common and telling mistake.",
            ),
            s(
                "Retrieval's ceiling is the system's ceiling",
                "Nothing ranking does can recover an item retrieval dropped. That makes retrieval recall the hard cap on the whole system's quality, and it is the number to measure first when the end-to-end result disappoints. It is also why retrieval is usually several sources unioned together — an ANN index, a keyword index, a freshness feed, a hand-curated list — rather than one clever method.",
            ),
            s(
                "Where a third stage appears",
                "Large systems often add a cheap pre-ranker between the two, cutting thousands to hundreds so the expensive model sees fewer. And a final re-ranking pass over the top handful, for diversity or business rules, is common and belongs after scoring rather than inside it. Mixing business rules into the model's objective is how a score stops meaning anything.",
            ),
        ],
        probes: &[
            "How many candidates does retrieval return, and what does that number cost you?",
            "What is your retrieval recall, and what is the ceiling it puts on the whole system?",
            "Where do the business rules apply — inside the model or after it?",
        ],
        scenarios: &["search-ranking", "feed-ranking", "product-recommender", "rag-over-docs"],
    },
    Concept {
        id: "drift-and-feedback-loops",
        title: "Drift and feedback loops",
        summary: "The world moves, and worse, your own model moves it.",
        sections: &[
            s(
                "Three things that drift",
                "The inputs change — a new device, a new country, a new referral source, and suddenly a quarter of your traffic looks unlike anything in training. The relationship changes — the same inputs now imply a different answer, which is what happens when an adversary adapts. And the labels change, because the humans or the process producing them changed. They need different detection and different responses, and a design that says 'we monitor for drift' without saying which kind has not said anything.",
            ),
            s(
                "Detecting it without labels",
                "The honest problem is that you usually cannot measure accuracy in production, because the label arrives days later or never. So you watch what you can: the distribution of each input feature against training, the distribution of the model's own output scores, and the rate at which predictions fall near the decision threshold. A sudden shift in the score distribution is the earliest signal you get, and it costs nothing to log.",
            ),
            s(
                "The loop is the dangerous one",
                "A recommender shows what it predicts you will click. You click it. That becomes training data. Tomorrow it is more sure. What it never showed you, it never learns about, and the catalogue it draws from silently contracts around what was already popular. The same shape appears in spam — blocked messages are never reported, so the training set loses exactly the examples the model is best at catching, and its measured performance improves while the real thing rots.",
            ),
            s(
                "What breaks the loop",
                "Deliberate exploration: reserve some fraction of traffic for items the model is unsure about, and accept the cost as the price of still having a usable training set next year. Log what you showed and not only what was clicked, so you can tell 'not chosen' from 'never offered'. And hold out a small unbiased slice — a random ordering shown to a tiny share of traffic — which is the only data that tells you what your model is doing to the world rather than what the world is doing back.",
            ),
        ],
        probes: &[
            "You cannot see labels for two weeks. What tells you today that the model is degrading?",
            "What in this design ever shows a user something the model did not rank highly?",
            "Your measured precision is rising and complaints are rising too. Explain that.",
        ],
        scenarios: &[
            "spam-classifier",
            "content-moderation",
            "feed-ranking",
            "product-recommender",
            "realtime-personalisation",
            "fraud-detection",
        ],
    },
    Concept {
        id: "retrieval-augmented-generation",
        title: "Retrieval-augmented generation",
        summary: "Giving a language model the passages it needs, and knowing when it did not get them.",
        sections: &[
            s(
                "What RAG actually is",
                "Retrieve passages relevant to the question, put them in the prompt, ask the model to answer from them. That is the whole mechanism. It exists because a model's weights are fixed at training time and your documents are not, and because a cited passage is checkable in a way a model's recollection is not. It is a retrieval system with a generator on the end, and most of what goes wrong with it goes wrong in the retrieval half.",
            ),
            s(
                "Chunking is a real decision",
                "You index pieces of documents, and the piece size is a trade-off nobody can avoid. Too small and a chunk loses the context that made it meaningful — a paragraph of pronouns whose subject was two paragraphs up. Too large and the embedding averages several topics into a vector that is near nothing, while the chunk eats context window. Overlapping windows and splitting on real structure, such as headings, both beat splitting every five hundred characters, and saying so is the difference between having thought about it and not.",
            ),
            s(
                "Evaluate the halves separately",
                "When an answer is wrong, there are two very different causes: the right passage was never retrieved, or it was retrieved and the model answered badly anyway. These need different fixes, and one end-to-end quality number tells you nothing about which you have. Measure retrieval by whether the passage containing the answer made the top k, and generation by whether the answer follows from what was supplied. Teams that only measure the end result spend months tuning prompts when their recall at five was sixty percent.",
            ),
            s(
                "Permissions and refusal",
                "Two failure modes carry real consequences. Access control belongs in the retrieval query, as a filter applied before anything reaches the prompt — instructing a model not to reveal what you have already handed it is not a security boundary. And the system needs a way to decline: a retrieval score floor under which it says it does not know, because an answer confidently assembled from three irrelevant passages is worse than no answer and much harder to catch.",
            ),
        ],
        probes: &[
            "The answer was wrong. How do you tell whether retrieval or generation failed?",
            "Where is the permission check, relative to the prompt?",
            "What does this system do when nothing relevant is retrieved?",
        ],
        scenarios: &["rag-over-docs", "llm-serving", "search-ranking"],
    },
    Concept {
        id: "serving-large-models",
        title: "Serving large models",
        summary: "What inference costs when the hardware is rented by the hour and the answer arrives one token at a time.",
        sections: &[
            s(
                "Two phases, two bottlenecks",
                "Generating an answer is not one operation. Prefill reads the whole prompt at once and is compute-bound — it is a big matrix multiply and the GPU is busy. Decode then emits one token at a time, each pass reading the entire model's weights to produce a single token, which makes it memory-bandwidth-bound and leaves the compute mostly idle. Time to first token is a prefill problem. Tokens per second is a decode problem. Optimising one does nothing for the other, and a design that quotes a single latency number has not noticed there are two.",
            ),
            s(
                "Batching is not the batching you know",
                "Ordinary request batching waits for a few requests, runs them together, returns them together. That is wrong here, because requests finish at wildly different lengths and the short ones would wait on the long ones. Continuous batching instead admits new requests into the running batch every step and evicts finished ones, keeping the GPU full without coupling one user's latency to another's output length. The word is the same; the mechanism is not, and the difference is most of the throughput.",
            ),
            s(
                "The KV cache is the capacity limit",
                "Every token generated must attend to every token before it, so the model keeps their intermediate keys and values in GPU memory. That cache grows with sequence length and with every concurrent request, and it is usually what runs out first — long before compute does. Concurrency is therefore a memory question: how many sequences of what length fit in what is left after the weights. This is the number that decides how many requests one GPU serves, and a capacity estimate that reasons from FLOPs instead is answering a question nobody asked.",
            ),
            s(
                "Spending less per token",
                "Quantisation stores weights at lower precision, which shrinks both the memory the weights occupy and the bandwidth each decode step must read — the most direct lever there is, paid for in some quality that has to be measured rather than assumed. Routing is the other: send the easy majority to a small model and escalate only what needs the large one, which works exactly as well as your ability to tell which is which in advance. Both are real; neither is free, and saying which you would try first and how you would know it worked is the answer.",
            ),
            s(
                "Scaling something that takes minutes to start",
                "Loading tens of gigabytes of weights onto a fresh GPU is minutes, not seconds, and the instances may not be available when you ask. Autoscaling on current load therefore arrives late for every burst. What is left is holding warm capacity you are paying for, queueing with an admission policy and an honest error when the queue is too long, and degrading deliberately — a shorter output cap, a smaller model — rather than timing out. A design that says it will scale up on demand has not priced the demand.",
            ),
        ],
        probes: &[
            "Your p99 time-to-first-token regressed but tokens-per-second is unchanged. Which half of the system do you look at?",
            "How many concurrent requests fit on one GPU, and what did you compute that from?",
            "Traffic triples in ninety seconds and a new instance takes four minutes to load. What does the user see?",
        ],
        scenarios: &["llm-serving", "rag-over-docs", "realtime-personalisation"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = CONCEPTS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two concepts share an id");
    }

    #[test]
    fn every_concept_points_at_scenarios_that_exist() {
        // A typo here is silent: the concept simply never appears on any problem, and the
        // link between the syllabus and the drills quietly stops existing.
        for c in CONCEPTS {
            assert!(!c.scenarios.is_empty(), "{} teaches nothing anyone practises", c.id);
            for id in c.scenarios {
                assert!(scenarios::find(id).is_some(), "{} points at missing scenario {id}", c.id);
            }
        }
    }

    #[test]
    fn every_concept_is_complete() {
        for c in CONCEPTS {
            assert!(c.sections.len() >= 3, "{} is too thin to teach from", c.id);
            assert!(!c.probes.is_empty(), "{} has no follow-ups to rehearse", c.id);
            for s in c.sections {
                assert!(s.body.len() > 120, "{} / {} is a stub", c.id, s.heading);
            }
        }
    }

    #[test]
    fn the_hardest_scenarios_have_something_to_read() {
        // Note the floor: this asserts a Hard scenario is *named* by some concept, not that
        // the concept answers its rubric. `llm-serving` passed this on a retrieval article
        // while nothing taught prefill, batching or the KV cache. A test cannot check that;
        // the reason this one earns its place is that it fails loudly for a whole missing
        // area, which is the failure worth automating.
        // The point of the section is that a critique you did not understand has somewhere
        // to send you. A Hard problem with no concepts attached is a dead end.
        for s in scenarios::SCENARIOS {
            if matches!(s.difficulty, scenarios::Difficulty::Hard) {
                assert!(
                    for_scenario(s.id).next().is_some(),
                    "{} is Hard and has nothing to read first",
                    s.id
                );
            }
        }
    }
}
