//! The teaching material.
//!
//! The syllabus is not invented here — it is `scenarios.rs`. Every concept below exists
//! because rubric lines across the twelve-plus scenarios keep asking for it, and each one
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
