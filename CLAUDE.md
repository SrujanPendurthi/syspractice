# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

System design interview trainer. Draw an architecture on a canvas, get a streamed critique from a model graded against a scenario rubric. Originally local-first and single-user; now a hosted, multi-user service with accounts, because discussion and 1v1s cannot work any other way. `prd.md` is the scope source of truth.

## Stack (decided — no open choices)

- **Backend:** Rust + **Axum 0.8**, in `backend/`. Binds loopback `127.0.0.1:8787`.
- **DB:** **SQLite via sqlx 0.8** (`runtime-tokio`, `sqlite`, `migrate`). File defaults to `backend/syspractice.db`.
- **Frontend:** React 19 + Vite + TypeScript + Tailwind v4, canvas by **@xyflow/react (React Flow) 12**, in `frontend/`.
- **AI:** any runner speaking OpenAI-compatible `/v1/chat/completions`. No FFI — plain HTTP. `./dev` uses **Ollama** (`:11434`) when present, **llama.cpp `llama-server`** (`:8080`) when `LLAMA_MODEL` is set.

Env overrides: `SYSPRACTICE_DB`, `SYSPRACTICE_ADDR`, `SYSPRACTICE_MODEL`, `LLAMA_SERVER_URL`, `LLAMA_MODEL`, `LLAMA_PORT`, `SYSPRACTICE_DIST`, `SYSPRACTICE_SECURE_COOKIES`, `SYSPRACTICE_TRUST_PROXY`, `SYSPRACTICE_CRITIQUE_QUOTA`.

**The two runners differ on one field.** llama-server ignores the request's `"model"` and serves whatever it was launched with; Ollama _resolves_ it and returns `404 model 'local' not found` for anything unknown. Hence `SYSPRACTICE_MODEL` (default `"local"`, correct only for llama-server). On the Ollama path `./dev` walks a preference list (MiniCPM5-2B, then qwen3:8b, then gemma4:12b) and only falls back to whatever `ollama list` prints first.

## The one architectural contract

Everything routes through the **serialized canvas topology** — a JSON graph of nodes and labeled directed edges.

```
React Flow state ──toTopology()──▶ Topology JSON ──▶ Rust structs ──▶ prompt text
                                        │
                                        └──▶ SQLite attempts.topology
```

Five things are **hand-mirrored** across the language boundary: `Topology`/`NodeKind`
(`topology.rs` ↔ `types.ts`), `Stage` (`critic.rs` ↔ `types.ts`), and `Level`,
`Difficulty` plus `Track` (`scenarios.rs` ↔ `types.ts`). `Stage` is mirrored twice over —
the union of variants, and `STAGES_BY_TRACK`, which has to match `Track::stages`. Track
_labels_ are deliberately not mirrored: they come from `/api/tracks`.

`backend/src/topology.rs` and `frontend/src/types.ts` are **hand-mirrored**. Renaming a field or adding a `NodeKind` means editing both in the same commit — plus `NODE_KINDS` in `types.ts` for the palette entry. `topology.rs`'s round-trip test asserts the camelCase wire format, so a Rust-side rename fails the build; a TS-side rename fails silently. If this drifts more than once, switch to `ts-rs` and derive.

`Topology::describe()` is the only thing the model sees of the diagram — coordinates are deliberately dropped, and edges are rendered by node _label_, not id.

## Layout

```
backend/src/
  main.rs        wiring only: env, pool, listener
  routes.rs      HTTP layer + AppState. Handlers stay thin.
  db.rs          persistence + derived progress. No HTTP types cross this boundary.
  auth.rs        argon2, session tokens, the AuthUser/MaybeUser extractors
  concepts.rs    the teaching material. Static, structured, tagged by scenario
  error.rs       AppError, and its IntoResponse
  critic.rs      prompt construction + llama-server SSE proxy
  topology.rs    the shared domain contract
  scenarios.rs   static scenario + rubric data
frontend/src/
  App.tsx        SettingsProvider + AuthProvider + router, and the one <AppBar>
  AppBar.tsx     the constant shell: brand, sections, account, appearance menu
  useTheme.tsx   theme / contrast / motion / text-size, and where they persist
  useAuth.tsx    the signed-in user, loaded once from /api/auth/me
  AuthPage.tsx   one form, sign-in/register toggle
  Learn.tsx      concept index and article, /learn and /learn/<id>
  Segmented.tsx  the one exclusive-choice control, used by the bar and the problem list
  Discuss.tsx    forum index and thread, /discuss and /discuss/<postId>
  Versus.tsx     the 1v1 lobby, /versus
  MatchRoom.tsx  resolves /m/<matchId> to its scenario, then hands off to Workspace
  MatchPanel.tsx the clock, the opponent's status, Submit, and the verdict
  time.ts        the one place a SQLite timestamp is parsed
  ProblemList.tsx  the problem table, difficulty chips, solved/attempted markers
  Workspace.tsx  owns canvas state, sidebar, session save/load for one scenario
  Canvas.tsx     React Flow only
  CritiquePanel.tsx  notes pane, stage header, streams and renders the critique
  useRoute.ts    pushState + popstate. Two routes did not justify a router dep
  api.ts         fetch wrappers + the SSE reader
  types.ts       topology contract + canvas conversion
```

## Commands

```
./dev                                    # everything: model runner + backend + frontend
SYSPRACTICE_MODEL=qwen3:8b ./dev         # pick a specific ollama model
LLAMA_MODEL=/path/model.gguf ./dev       # use llama.cpp instead of ollama
```

`./dev` runs `npm install` if `node_modules` is missing, holds the frontend until the
backend answers `/api/scenarios`, and tears down every child process group on exit.
Env: `LLAMA_MODEL`, `LLAMA_PORT`, `SYSPRACTICE_ADDR`, `SYSPRACTICE_DB`.

Containers — the backend serving the built frontend, Ollama, and a one-shot model pull:

```
docker compose up --build -d             # http://localhost:8787
docker compose logs -f app
docker compose down                      # keeps the database and the model
docker compose down -v                   # deletes both
LLAMA_SERVER_URL=http://host.docker.internal:11434 docker compose up -d --no-deps app   # host GPU (Mac)
```

Individual pieces:

```
# backend (from backend/)
cargo build
cargo test                       # single test: cargo test round_trips -- --nocapture
cargo run                        # http://127.0.0.1:8787

# frontend (from frontend/)
npm run dev                      # http://localhost:5173, proxies /api to :8787
npm run build                    # tsc -b && vite build
npm run typecheck

# model runner — critique returns 502 until this is up
llama-server -m <model.gguf> --port 8080
```

## Things that will bite

- **The critique endpoint is a POST**, so `EventSource` is unusable. `api.ts` reads the body and parses SSE frames by hand. Each frame's data is a **JSON-encoded string** so tokens containing newlines survive — don't "simplify" that to raw text.
- **Reasoning models emit `delta.reasoning` with `delta.content` empty.** Measured on gemma4:12b: 686 reasoning frames to 456 content frames on one critique. Reading only `content` throws all of them away and the panel looks hung for the entire thinking phase. `parse_delta` returns `Delta::Reasoning`, the backend tags it as SSE event `thinking` (vs `token`), and the panel treats their _arrival_ as the liveness signal while rendering only an elapsed-seconds counter — the content itself is chain of thought and reads as mid-sentence noise. Frames carrying both fields count as content.
- **Ollama caps every model at a 4096-token context** regardless of what it was trained for (MiniCPM5-2B: 131k, gemma4:12b: 262k), and `/v1/chat/completions` ignores `options.num_ctx` — verified, the request changes nothing. Only `OLLAMA_CONTEXT_LENGTH` on the server does, so `./dev` exports it before `ollama serve` and warns when it reuses a daemon it did not start. It matters because overflow truncates from the front, dropping the system prompt, and the critique silently stops following the four-section format. `ollama ps` prints the real number.
- **A model will rubber-stamp a stated number.** Told to "check their maths", both MiniCPM5-2B and gemma4:12b read `100M * 365 * 500B = 18GB` and replied that it checks out — it is 18.25 TB, a 1000x unit slip. Not a capacity problem; the 12B failed identically. `Stage::Estimation` now orders the model to recompute from the inputs _before_ judging, and names the GB/TB and per-day/per-second slips as the ones to look for. Weaken that wording and the estimation stage silently goes back to agreeing with whatever it is shown.
- **The ML interviewer will praise an accuracy figure that is worse than doing nothing, unless it is ordered to do the arithmetic first.** Handed notes saying spam was 0.5% of messages and a model scoring 95%, MiniCPM5-2B flagged the number as suspicious and never once worked out that answering "not spam" every time scores 99.5% — 0 for 8 across two wordings of a general rule. The fix is the estimation stage's: the Modeling and Evaluation focus strings open with the computation, give the formula (100 minus the positive rate, subtracted as written, not converted to a fraction), and fix the answer's first line as a filled-in template: `Always-no baseline: 100 - <rate> = <result>%. Your accuracy: <x>%.` **There is no numeric worked example, on purpose:** with reasoning off, a "2% gives 98%" example was copied into the answer in 3 of 6 runs in place of the user's own rate. Measured with that template and reasoning off: MiniCPM5-2B gave the right verdict 12/12 but the exact figure only 3/12 (it converts 0.5% to 0.005 regardless); `gemma4:12b` got 6/6 exact. **Measure prompt changes on a runner with a real context window:** a host Ollama not started by `./dev` sits at 4096 tokens, and a reasoning model overflows that and loses the system prompt from the front, so every number measured there is suspect.
- **The default model's reasoning has no length limit, and the 4096-token cap was quietly acting as one.** With a real 16k context on the GPU, 8 of 9 MiniCPM5-2B critiques were still reasoning at five minutes; the fast timings recorded earlier were almost certainly taken on a 4096 daemon that cut the reasoning short. `SYSPRACTICE_REASONING_EFFORT=none` turns reasoning off (sent as `reasoning_effort`; Ollama's OpenAI endpoint silently *ignores* `think: false`, which is an easy way to measure nothing) and brings a critique to 5–30s even on the container's CPU — but it breaks arithmetic: the estimation stage caught the 18GB-that-is-18TB slip in 1 of 6 runs and said "your storage math checks out" in another, the exact rubber-stamp this file warns about. Even `gemma4:12b` with reasoning off multiplies `36.5B × 500` three zeros short. Two prompt fixes for that — visible working on a template line, and a powers-of-ten method — were measured, did no better (1/6, then 0/6), and were reverted. The knob therefore defaults to off. Long multiplication is not something these models do reliably without reasoning, and no wording tried changed that.
- **An abandoned critique really does stop the model.** Measured on the container: Ollama at ~1200% CPU mid-critique, 0% within two seconds of the client going away. Dropping the SSE stream drops the upstream request and Ollama cancels it, so abort-on-edit frees the runner rather than queueing hidden work behind the next critique.
- **The interview is staged, one stage per critique.** `Stage::focus(track)` in `critic.rs` says what to grade _and_ what to ignore; without the ignore half the model drifts back to reviewing the whole design every time. The ~150-word cap is load-bearing rather than cosmetic — critiques auto-run when the canvas goes quiet, and a four-section answer never finishes before the next edit aborts it. Measured on MiniCPM: 1m38s unstaged, 36-46s per stage — most likely on a 4096-token daemon that was cutting the reasoning short; see the reasoning entry below.
- **Difficulty filters the rubric in Rust, not in the prompt.** `Scenario::rubric_for(level)` drops out-of-scope lines before they are ever sent. A 2B will not reliably ignore a rubric line it can see. `rubric` is `#[serde(skip)]` so it never reaches the browser either.
- **`getTopology` and `subscribeToIdle` live in `Workspace.tsx` and must stay referentially stable.** Both are callbacks over refs precisely so `CritiquePanel` (memoized) keeps its props unchanged through a drag — one reads the canvas lazily, the other hands out idle notifications without a changing `revision` prop. Break either and every drag frame re-renders a panel that is mid-stream. The debounce effect's cleanup is also what gives abort-on-edit for free.
- **There is no CORS layer, and there must not need to be.** In production the backend serves `frontend/dist` itself (`ServeDir` with a `ServeFile` fallback in `main.rs`), so the app and the API are one origin; in development Vite proxies `/api`. That `ServeFile` fallback is also the SPA rewrite `useRoute` depends on for paths like `/s/url-shortener`. Moving the frontend to a separate host reintroduces both problems _and_ breaks the session cookie, which `api.ts` sends as `credentials: 'same-origin'`.
- **"Session" means a logged-in browser. A go at a scenario is an "attempt".** The table was called `sessions` until accounts arrived; it is `attempts` now (migration `0003`), and `auth_sessions` is the auth one. Same lesson as `difficulty` vs `level` — one word, two meanings, renamed while it was still cheap.
- **`update_attempt` reads before it writes, on purpose.** `db::attempt_scenario` fetches the attempt's own `scenario_id` so `check_stage` validates against the problem the attempt is actually against, rather than one the caller names — a check the caller supplies the input to is a check the caller can pass. It is one indexed lookup on a path that was already writing, and it is the reason a trivial save does two queries. Deleting it looks like an easy optimisation and silently reopens the hole.
- **Ownership is enforced in the SQL, never in a handler.** Every user-owned query in `db.rs` carries its own `WHERE user_id = ?`. `get_attempt` had no check at all when there was one user, which would have been a direct object reference the day there were two. A miss returns `NotFound` rather than `Forbidden` on purpose: whether that id exists is not the asker's business. `db::tests::one_user_cannot_touch_anothers_attempt` asserts the dangerous case is not a 200.
- **Inference is metered, so the idle timer is a cost.** `IDLE_MS` in `Workspace.tsx` was 8s when the model ran on the user's own laptop; it is 20s now, and `/api/critique` refuses past `SYSPRACTICE_CRITIQUE_QUOTA` (default 60/user/hour) with a 429. Anonymous visitors can browse and draw but not critique, which is what makes the quota enforceable at all.
- **The quota is claimed in one statement, not counted and then written.** `db::reserve_critique` evaluates the count _inside_ the `INSERT`; read-then-write let twelve concurrent requests all see the same under-limit count, measured at 5 through on a limit of 2. If the runner then refuses, `release_critique` gives the slot back — no GPU time was spent, so charging would be charging for our own outage. Once the stream starts the charge stands, because closing the tab does not refund the seconds.
- **The API is a nested router with its own `fallback`.** Otherwise an unknown `/api/...` path falls through to the SPA fallback and answers 200 with `index.html`, which `api.ts` parses as JSON and reports as an opaque `SyntaxError` — a renamed route would fail that way instead of 404ing. Handlers take `AppJson<T>` rather than `Json<T>` for the same reason: axum's own rejection is plain text, so a missing field reached the user as "Unprocessable Entity".
- **Cookies are `Secure` by default and `./dev` turns that off.** Safari will not store a `Secure` cookie from `http://localhost`, so with it on every local sign-in silently fails. `SYSPRACTICE_SECURE_COOKIES=0` is a development-only escape hatch; the backend prints a warning when it is set.
- **Registration is an account-existence oracle, and rate limiting is the only mitigation here.** It has to say "that email is taken" to be usable, which hands back exactly what login spends `dummy_verify` and `BadCredentials` to deny. `register_throttle` caps it at 5 per client address per hour — counted on success too, because the thing being limited is how often one address may _ask_. Confirmation email is the real fix and needs infrastructure this does not have. The same throttle is what stops an unauthenticated ~460ms argon2 per request being a free CPU sink.
- **`X-Forwarded-For` is ignored unless `SYSPRACTICE_TRUST_PROXY=1`.** It is caller-supplied: believing it with no proxy in front lets one client vary a header and evade every rate limit. Believing the socket address behind a proxy throttles the whole internet as one client. Both are wrong in opposite directions, so it is a deployment flag rather than a default.
- **`notes` is capped at 8,000 characters (`MAX_NOTES` in `routes.rs`, `maxLength` on the textarea).** It is pasted verbatim into the critique prompt, and the quota counts requests rather than tokens — uncapped, one request spent 1.5MB of inference for one unit of quota. It is also what keeps the prompt inside the context window.
- **`memo(CritiquePanel)` needs _every_ prop stable, not just the two with comments on them.** `onAdvance` was a plain arrow redefined each render, which defeated the shallow compare completely — the memo did nothing at all from the day the panel was extracted until a test pass found it. `persist` and `advance` are `useCallback` now. Anything new passed to this panel has to be stable through a drag or the optimization silently stops existing again.
- **The syllabus is `scenarios.rs`, not an invention.** Every concept in `concepts.rs` exists because rubric lines keep asking for it, and each names the scenarios that exercise it. Tests enforce both directions: a concept pointing at a scenario id that does not exist would silently never appear anywhere, and a Hard scenario with nothing to read is a dead end for someone who did not understand their critique. That second test is what found there was no conflict-resolution concept while `file-sync` and `collaborative-editor` both needed one.
- **The match view never carries an attempt id, and that is the whole exercise.** `db::MatchView` has no `host_attempt_id`/`guest_attempt_id` field and neither does the TS mirror — an opponent holding the id of your board can go and read it, and the format only works if neither of you sees the other's design until the clock stops. Only _whether_ each side has submitted crosses the wire. `db::tests::the_match_view_never_carries_either_board` asserts on the serialized JSON rather than on the struct, because the struct is the thing somebody would helpfully add a field to.
- **`graded_at` is stamped before the model runs, not after.** Both players open the result at the same moment — the moment the clock stops — so without the claim the same match is graded twice, billed twice, and can answer the same question two different ways. `claim_grading` is a conditional UPDATE whose `rows_affected` is the lock. `release_grading` puts it back when the runner never answered, or a single outage leaves the match stamped graded with no verdict in it, unreadable and unretryable. Verified: with no runner up, the 502 leaves `graded_at` NULL and `critique_log` empty.
- **A walkover costs no inference.** If one side or neither submitted, the clock already decided it and there is nothing to compare — `grade_if_ready` returns before reserving quota. That verdict is also written in the third person, because one row is stored and _both_ players read it: "your opponent did not submit" is read by the opponent as if they had won.
- **Which player is Design A is decided by match id parity, not by role.** LLM pairwise judging has a position bias, so fixing the host in the first slot hands the advantage to whoever posted the challenge, every time. Parity is deterministic, so a regrade lays the same two designs out the same way rather than flipping the result by reshuffling. The judge is never told who is host and who is guest — `critic::tests::the_judge_sees_two_designs_and_neither_players_name` asserts the words do not appear in the prompt.
- **There is no SSE in the match, deliberately.** The plan assumed a live channel for the countdown; a deadline is a timestamp, and the client counts down against it with no server involved. What actually has to arrive is "your opponent submitted", which is one row every five seconds. An SSE fan-out needs a subscriber registry and a reconnect story — revisit it when there is presence to show, not for a clock.
- **`minutes || ' minutes'`, with no leading `+`.** SQLite takes the sign from the number, and `+-1 minutes` evaluates the whole expression to NULL — a NULL deadline makes every comparison false, so the match becomes unfinishable rather than expired and nobody ever gets a verdict. Exactly the trap `store_auth_session` hit with days. It is also what lets a test build an expired match with `minutes = -1` instead of sleeping through one.
- **Submitting a board is final, and the guard is in the UPDATE.** Without the `IS NULL` check you could keep editing while watching your opponent's clock run down. The handler pre-checks the same thing purely so a double-clicked Submit does not leave a duplicate attempt behind — the UPDATE stays the authority, because the deadline can pass between the two.
- **Auto-critique is off during a match** (`Workspace.tsx`, the `IDLE_MS` effect returns early on `matchId`). It would be a paid hint on a clock, and the format's whole premise is that the only feedback is the verdict.
- **Nothing in the discussion is ever deleted, only `deleted_at`-stamped.** A moderator taking down spam should leave the row that proves who posted it, and a hard `DELETE` of a post takes its whole comment thread with it through `ON DELETE CASCADE`. The cost is that every read has to carry `deleted_at IS NULL` — and that a foreign key no longer proves a post is _visible_, which is why `create_comment` and `set_post_vote` insert conditionally on a `WHERE EXISTS` rather than trusting the reference.
- **A post's score is summed from `post_votes` on read; there is no counter column.** Same reason progress is derived: a counter needs a second write on every vote and is wrong forever the one time that write is lost. One user to one ballot comes from the `(post_id, user_id)` primary key and the upsert's `DO UPDATE` — without that clause, voting twice inserts twice and the sum counts both. `db::tests::a_second_vote_replaces_the_first_rather_than_adding_to_it` is the guard.
- **`users.is_admin` is set by hand in SQL.** There is no admin UI and no endpoint that grants it, because an endpoint that hands out moderator rights is a much larger thing to get right than the one `OR ? = 1` that reads it. Moderation is a soft delete after the fact; the rate limits are what keep the queue small enough to work through by hand — 5 posts and 30 comments per account per hour, keyed by user id rather than address because an account costs a registration, which is itself limited.
- **SQLite writes `datetime('now')` as UTC with no zone marker, and `new Date` reads that as local time.** Every timestamp would be hours out — which looks like a clock bug rather than a parsing one, and on a match countdown it is the difference between eight minutes left and having finished five hours ago. `time.ts` is the only place that parse happens; use `parseSqlTime`/`ago` rather than writing another one.
- **Axum's extractor rejections are plain text, all of them.** `AppJson` fixed the body; a path id that would not parse still came back as `Invalid URL: Cannot parse \`abc\` to a \`i64\``with a`text/plain`type, which`api.ts`cannot read, so the user saw the bare status line.`AppPath`and`AppQuery`cover the rest and the API router has a`method_not_allowed_fallback`. Any new handler uses those, not the bare `Path`/`Query`.
- **The discussion's scenario filter lives in the query string, and `useRoute` does not track it.** The workspace links to `/discuss?scenario=<id>` in a new tab, so the filter has to survive a cold load — `Discuss` reads `window.location.search` itself at mount. Changing the dropdown calls `history.replaceState` directly rather than `navigate()`, because routing through `useRoute` would set its path to one carrying a query string and stop `/discuss` matching at all.
- **Post and comment bodies are rendered as text, never as markup.** `whitespace-pre-wrap` and nothing else. A markdown renderer here buys a dependency and an HTML injection surface over user input at the same time — the same reasoning that keeps concept content structured rather than parsed, except that here the input is not ours.
- **Concept content is structured data, not markdown.** The shape is uniform — sections, probes, scenarios — so a parser would buy a dependency and an HTML injection surface and nothing else. `/api/concepts` returns summaries only; bodies come from `/api/concepts/{id}`.
- **Links out of the workspace are `<a target="_blank">`, never `navigate()`.** Routing away unmounts `Workspace` and takes the diagram, notes and stage with it. That is why signing in is an overlay and why "Read first" opens a tab — the whole point of reading a concept mid-interview is to have it beside the canvas.
- **Password hashing runs in `spawn_blocking`.** argon2 is deliberately tens of milliseconds of CPU. On an async worker that is a stalled runtime under any concurrency. Login also runs `auth::dummy_verify` when the email matched nothing, so "no such account" and "wrong password" cost the same wall-clock time as well as returning the same string.
- **`difficulty` and `level` are different axes and neither is a rename of the other.** `Scenario::difficulty` is Easy/Medium/Hard, how hard the problem is. `Level` is Junior/Senior/Staff, who you are being interviewed as, and it filters the rubric via `Scenario::rubric_for`. You can take an Easy problem at Staff level. The critique request's field is `level`; it was called `difficulty` until the problem list needed the other meaning.
- **Progress is derived, not stored.** `db::list_progress` is a `GROUP BY` over `attempts`, and solved means some attempt reached **that problem's own last stage** — looked up per scenario from `db::finish_lines`, not compared against a literal. It was `stage = 'bottlenecks'`, which was true while there was one interview and silently wrong the day there were two. There is no progress table and no migration for it. That is also why `Workspace` persists on stage advance rather than only on Save: without it the list stays empty for anyone who never presses the button. Solved measures that the interview was walked end to end, not that the design was good.
- **A track owns its stage sequence, and `Stage` is the union of every track's, not an ordering.** Classic walks five stages, ML walks six. Nothing may assume five — not the pips, not the heading, not the prompt. `"STAGE n of 5"` used to be written into each `focus` string and that literal is exactly what made a second interview impossible; it is computed from `track.stages()` now. `Requirements` and `Estimation` are the only shared stages, and `focus` spells them out per track rather than using a fallback arm, so a third track fails to compile until somebody writes its text instead of silently inheriting the distributed-systems briefing.
- **`solved` must not depend on a handler having been careful.** `check_stage` in `routes.rs` refuses a stage from another track on create, update and critique — but the SQL matches each attempt against _its own_ scenario's finish line anyway, so a row written around the handler still cannot tick a problem. The lookup matches `json_each`'s key rather than building a `$."<id>"` path: a path built by concatenation throws on an id ending in a backslash, which would 500 that user's progress on every page load forever. Pre-`c240aef` rows can hold any `scenario_id`, so that was not hypothetical.
- **The node palette is shared across tracks, not filtered.** All fifteen kinds show on every problem: a classic design may legitimately want a stream processor, and hiding components per track is a filter that buys nothing. Fifteen categorical accents is already past what anyone holds apart at a glance — the accent is a hint, the label is what identifies a node. Group by family before finding a sixteenth colour.
- **In the containers the database is a file in the `data` volume, not a service.** SQLite has no server to run; `/data/syspractice.db` persists because the volume does, and `docker compose down -v` deletes it with no confirmation. The image creates `/data` owned by the non-root `app` user _before_ declaring the volume — a fresh named volume copies that ownership, and a root-owned one makes the first migration fail. Run exactly one `app` container. The login, registration, post and comment throttles are in-process counters, so each replica counts separately and a client whose requests land on two gets up to **double** every one of those limits — five sign-in attempts per fifteen minutes becomes ten. The critique quota is claimed in SQL and stays exact. (`compose.yaml` publishes a fixed host port, so `--scale app=2` fails on the port before any of this can happen — which is the right outcome.)
- **The model runner is never published to the host.** `compose.yaml` gives `ollama` no `ports:` because it has no authentication — anything that can reach it can spend the GPU and bypass the critique quota entirely. Only `app` reaches it, over the compose network. `app` itself is published on `127.0.0.1` only.
- **Docker on a Mac cannot use the GPU, and for the ML track that makes the containerized runner unusable.** The containerized Ollama runs on the CPU: one classic critique measured 122s, against 41s through Ollama running natively. The ML Modeling stage is worse — longer instructions and a reasoning model that thinks at length — and four of six measured runs were still reasoning when a 10-minute cap cut them off, with no answer at all. Auto-critique fires after 20s of quiet and is aborted by the next edit, so at 122s most never finish. `LLAMA_SERVER_URL=http://host.docker.internal:11434 docker compose up -d --no-deps app` points the app at the host's Ollama instead; `--no-deps` keeps the idle container runner from starting, and the override only lasts until the next plain `up` unless it goes in a `.env` file. The model pull is not a dependency of `app` on purpose: browsing, drawing and saving work without a runner, and critique returns 502 until the pull finishes.
- **`app` runs with `init: true` because the backend installs no signal handler.** It is PID 1 in its container, and PID 1 with no handler ignores SIGTERM, so every `down` or restart waited out the stop timeout and ended in a SIGKILL — cutting a streaming critique off rather than closing it. SQLite in WAL mode survives that, so nothing was lost, but the fix is one line. The alternative is `with_graceful_shutdown` in `main.rs`.
- **`backend/build.rs` exists for one reason: a migration added on its own does not trigger a rebuild.** `sqlx::migrate!` embeds the migration files, but cargo only rebuilds when a file it already tracks changes, and a *new* file is not one of those. Without `build.rs`, a commit that only adds `migrations/0007_*.sql` rebuilds to the old binary with the old schema — silently in Docker, where the target dir is a cache mount. Do not delete it as dead code.
- **`SYSPRACTICE_SECURE_COOKIES` defaults to 0 in `compose.yaml`**, as in `./dev`, because the stack is served over plain `http://localhost`. Anything behind TLS must set it to 1.
- **sqlx runtime-checked queries** (`query_as`, not `query!`). No `DATABASE_URL` needed at build time, but typos are caught by tests, not the compiler.
- **Colour has two jobs and they are kept apart.** Violet (`--color-accent`) is _interaction_: links, focus, the active nav item, primary buttons, the row hover edge. The teal/amber/rose triad is _assessment_: difficulty, outcome, status. Greyscale is structure. They used to be one set — the "Sign in" button and the word "Easy" were the same teal — so the palette said "this is an easy thing" and "this is the thing to click" with one colour. A new interactive element takes the accent; a new status takes the triad; nothing takes both. `NODE_KINDS`' API Gateway accent was moved off violet to indigo for the same reason.
- **The difficulty triad exists twice in `index.css` on purpose, and the second set flips direction per theme.** `--color-easy/medium/hard` are LeetCode's vivid values, for shapes — ring arcs, dots, fills. `--color-*-ink` are the same hues moved until they read as _text on this theme's page_: **darker** on white (the vivid set runs 1.9:1 to 2.9:1 there and fails as type — LeetCode ships that flaw, we do not) and **lighter** on near-black. That inversion is the whole reason the two sets cannot be collapsed into one colour reused everywhere.
- **Four independent appearance axes, not a list of themes.** Theme (system/light/dark), contrast (system/normal/high), motion (system/full/reduced) and text size (normal/large) are orthogonal — someone can want high contrast in dark mode, or larger text in light. Each defaults to `system` and follows its media query live. High contrast is a real token set in `index.css`, not a filter over the normal one.
- **The pre-paint script in `index.html` is duplicated logic, deliberately.** It reads the same `syspractice.*` localStorage keys that `useTheme.tsx` owns and sets the same `data-` attributes before React exists. Without it a dark-mode reader gets a full white page for as long as the bundle takes to boot, which is worst for exactly the person who chose dark to avoid being flashed. If the keys or the attribute names change, both have to change. It stays inline and dependency-free: anything it waits on puts the flash back.
- **The type scale is in `rem` so the large-text setting can work.** `html[data-text='large']` moves one root font-size and the whole scale follows. A hard-coded `text-[13px]` will not follow it — there were 152 of those before the scale existed, and they are why nothing could be scaled. Use `text-xs/sm/base/lg/xl/2xl`, or `text-read` for the concept articles, which is the one place with long-form prose and gets more leading than the UI scale.
- **`<AppBar>` is rendered once, by `App.tsx`.** A page must not add its own header. The bar's slots are fixed — brand, sections, account, appearance — and the most a page may do is drop a slot: the workspace removes the section links because it is a full-screen tool, and nothing else about the bar changes. `localStorage` can throw outright in a locked-down browser, so every read and write in `useTheme.tsx` is wrapped; a setting that will not save beats a page that will not render.
- **React Flow's chrome is themed in `index.css`, not through props.** The controls, edge strokes, edge label backgrounds, handles and the dot grid all come from the library's own stylesheet and know nothing about the tokens, so they are overridden by class. `<Background>` takes no `color` prop any more — it inherits `--color-grid`, which is what makes the grid survive a theme switch.
- **`nodeTypes` lives outside the component** in `Canvas.tsx`. Moving it inside remounts every node on every drag frame.
- `:memory:` SQLite pools are capped at 1 connection in `db::connect` — each pooled connection would otherwise get its own private database.
- `./dev` runs `set -m` so each child leads its own process group and the trap can kill grandchildren (`cargo run` and `npm run dev` are both wrappers). That also means children sit outside the terminal's foreground group, so their stdin is detached — otherwise vite reads stdin, takes `SIGTTIN`, and suspends.

## Code style

- **Rust:** `thiserror` for typed errors (`AppError`), `anyhow` at the binary edge. No `unwrap`/`expect` outside tests; the two `expect`s on `serde_json::to_string(&Topology)` are infallible-by-construction and commented as such.
- **TS:** functional components, explicit props interfaces, `strict` + `verbatimModuleSyntax`. Tailwind classes inline, no CSS files beyond `index.css`.

## Not built yet (PRD scope, deliberately deferred)

Back-of-the-envelope calculator, flashcards, and inline (non-`window.prompt`) label
editing. The critique loop was scaffolded end to end first because it is the piece that
constrains every other decision.

Everything else planned has shipped: accounts, concepts, discussion, 1v1, and the machine
learning track. `prd.md` predates the track split and does not mention it.

1v1 ships as a race with no spectating: you cannot watch a match you are not in, and you
cannot see your opponent's board even after the verdict — only the verdict text, which
names the rubric lines each design covered and missed. Showing the two boards side by side
afterwards is the obvious next addition and needs a read-only canvas view, which does not
exist yet.

The discussion tab ships flat — comments do not nest, votes are on posts only, the index is
newest-first with no ranking, and there is no report queue. Threading and a hot ranking are
the two most likely first additions; a report queue is not worth a table until somebody is
actually reading it.

The ML track ships with ten problems against the classic track's twenty-four, and its
concepts cover its Hard problems but not every rubric line — `the_hardest_scenarios_have_something_to_read`
only asserts that _a_ concept names a scenario, not that it answers it.

## Parent repo note

`../CLAUDE.md` mandates `graphify query` for codebase questions. There is no `graphify-out/` here yet — run `graphify .` to generate it, or ignore the rule until then.
