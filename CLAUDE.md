# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Local-first system design interview trainer. Draw an architecture on a canvas, get a streamed critique from a locally-hosted model graded against a scenario rubric. `prd.md` is the scope source of truth.

## Stack (decided — no open choices)

* **Backend:** Rust + **Axum 0.8**, in `backend/`. Binds loopback `127.0.0.1:8787`.
* **DB:** **SQLite via sqlx 0.8** (`runtime-tokio`, `sqlite`, `migrate`). File defaults to `backend/syspractice.db`.
* **Frontend:** React 19 + Vite + TypeScript + Tailwind v4, canvas by **@xyflow/react (React Flow) 12**, in `frontend/`.
* **AI:** any runner speaking OpenAI-compatible `/v1/chat/completions`. No FFI — plain HTTP. `./dev` uses **Ollama** (`:11434`) when present, **llama.cpp `llama-server`** (`:8080`) when `LLAMA_MODEL` is set.

Env overrides: `SYSPRACTICE_DB`, `SYSPRACTICE_ADDR`, `SYSPRACTICE_MODEL`, `LLAMA_SERVER_URL`, `LLAMA_MODEL`, `LLAMA_PORT`.

**The two runners differ on one field.** llama-server ignores the request's `"model"` and serves whatever it was launched with; Ollama *resolves* it and returns `404 model 'local' not found` for anything unknown. Hence `SYSPRACTICE_MODEL` (default `"local"`, correct only for llama-server). On the Ollama path `./dev` walks a preference list (MiniCPM5-2B, then qwen3:8b, then gemma4:12b) and only falls back to whatever `ollama list` prints first.

## The one architectural contract

Everything routes through the **serialized canvas topology** — a JSON graph of nodes and labeled directed edges.

```
React Flow state ──toTopology()──▶ Topology JSON ──▶ Rust structs ──▶ prompt text
                                        │
                                        └──▶ SQLite sessions.topology
```

Four things are **hand-mirrored** across the language boundary: `Topology`/`NodeKind`
(`topology.rs` ↔ `types.ts`), `Stage` (`critic.rs` ↔ `types.ts`), and `Level` plus
`Difficulty` (`scenarios.rs` ↔ `types.ts`).

`backend/src/topology.rs` and `frontend/src/types.ts` are **hand-mirrored**. Renaming a field or adding a `NodeKind` means editing both in the same commit — plus `NODE_KINDS` in `types.ts` for the palette entry. `topology.rs`'s round-trip test asserts the camelCase wire format, so a Rust-side rename fails the build; a TS-side rename fails silently. If this drifts more than once, switch to `ts-rs` and derive.

`Topology::describe()` is the only thing the model sees of the diagram — coordinates are deliberately dropped, and edges are rendered by node *label*, not id.

## Layout

```
backend/src/
  main.rs        wiring only: env, pool, listener
  routes.rs      HTTP layer + AppState. Handlers stay thin.
  db.rs          persistence + derived progress. No HTTP types cross this boundary.
  error.rs       AppError, and its IntoResponse
  critic.rs      prompt construction + llama-server SSE proxy
  topology.rs    the shared domain contract
  scenarios.rs   static scenario + rubric data
frontend/src/
  App.tsx        router only: /s/<id> is Workspace, everything else ProblemList
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

* **The critique endpoint is a POST**, so `EventSource` is unusable. `api.ts` reads the body and parses SSE frames by hand. Each frame's data is a **JSON-encoded string** so tokens containing newlines survive — don't "simplify" that to raw text.
* **Reasoning models emit `delta.reasoning` with `delta.content` empty.** Measured on gemma4:12b: 686 reasoning frames to 456 content frames on one critique. Reading only `content` throws all of them away and the panel looks hung for the entire thinking phase. `parse_delta` returns `Delta::Reasoning`, the backend tags it as SSE event `thinking` (vs `token`), and the panel treats their *arrival* as the liveness signal while rendering only an elapsed-seconds counter — the content itself is chain of thought and reads as mid-sentence noise. Frames carrying both fields count as content.
* **Ollama caps every model at a 4096-token context** regardless of what it was trained for (MiniCPM5-2B: 131k, gemma4:12b: 262k), and `/v1/chat/completions` ignores `options.num_ctx` — verified, the request changes nothing. Only `OLLAMA_CONTEXT_LENGTH` on the server does, so `./dev` exports it before `ollama serve` and warns when it reuses a daemon it did not start. It matters because overflow truncates from the front, dropping the system prompt, and the critique silently stops following the four-section format. `ollama ps` prints the real number.
* **A model will rubber-stamp a stated number.** Told to "check their maths", both MiniCPM5-2B and gemma4:12b read `100M * 365 * 500B = 18GB` and replied that it checks out — it is 18.25 TB, a 1000x unit slip. Not a capacity problem; the 12B failed identically. `Stage::Estimation` now orders the model to recompute from the inputs *before* judging, and names the GB/TB and per-day/per-second slips as the ones to look for. Weaken that wording and the estimation stage silently goes back to agreeing with whatever it is shown.
* **The interview is staged, one stage per critique.** `Stage::focus()` in `critic.rs` says what to grade *and* what to ignore; without the ignore half the model drifts back to reviewing the whole design every time. The ~150-word cap is load-bearing rather than cosmetic — critiques auto-run when the canvas goes quiet, and a four-section answer never finishes before the next edit aborts it. Measured on MiniCPM: 1m38s unstaged, 36-46s per stage.
* **Difficulty filters the rubric in Rust, not in the prompt.** `Scenario::rubric_for(level)` drops out-of-scope lines before they are ever sent. A 2B will not reliably ignore a rubric line it can see. `rubric` is `#[serde(skip)]` so it never reaches the browser either.
* **`getTopology` and `subscribeToIdle` live in `Workspace.tsx` and must stay referentially stable.** Both are callbacks over refs precisely so `CritiquePanel` (memoized) keeps its props unchanged through a drag — one reads the canvas lazily, the other hands out idle notifications without a changing `revision` prop. Break either and every drag frame re-renders a panel that is mid-stream. The debounce effect's cleanup is also what gives abort-on-edit for free.
* **Vite proxies `/api`**, so the backend has no CORS layer. Serving the frontend from anywhere other than the dev server means adding one — *and* an SPA fallback, since `useRoute` relies on the dev server rewriting unknown paths like `/s/url-shortener` to `index.html`.
* **`difficulty` and `level` are different axes and neither is a rename of the other.** `Scenario::difficulty` is Easy/Medium/Hard, how hard the problem is. `Level` is Junior/Senior/Staff, who you are being interviewed as, and it filters the rubric via `Scenario::rubric_for`. You can take an Easy problem at Staff level. The critique request's field is `level`; it was called `difficulty` until the problem list needed the other meaning.
* **Progress is derived, not stored.** `db::list_progress` is a `GROUP BY` over `sessions` — solved means some session for that scenario reached the `bottlenecks` stage. There is no progress table and no migration for it. That is also why `Workspace` persists on stage advance rather than only on Save: without it the list stays empty for anyone who never presses the button. Solved measures that the interview was walked end to end, not that the design was good.
* **sqlx runtime-checked queries** (`query_as`, not `query!`). No `DATABASE_URL` needed at build time, but typos are caught by tests, not the compiler.
* **The difficulty triad exists twice in `index.css` on purpose.** `--color-easy/medium/hard` are LeetCode's vivid values, for shapes — ring arcs, dots, fills. `--color-*-ink` are the same hues darkened, for text. The vivid set runs 1.9:1 to 2.9:1 on white and fails as type; LeetCode ships that flaw, we do not. Collapsing the pairs "because they are the same colour" reintroduces it.
* **`nodeTypes` lives outside the component** in `Canvas.tsx`. Moving it inside remounts every node on every drag frame.
* `:memory:` SQLite pools are capped at 1 connection in `db::connect` — each pooled connection would otherwise get its own private database.
* `./dev` runs `set -m` so each child leads its own process group and the trap can kill grandchildren (`cargo run` and `npm run dev` are both wrappers). That also means children sit outside the terminal's foreground group, so their stdin is detached — otherwise vite reads stdin, takes `SIGTTIN`, and suspends.

## Code style

* **Rust:** `thiserror` for typed errors (`AppError`), `anyhow` at the binary edge. No `unwrap`/`expect` outside tests; the two `expect`s on `serde_json::to_string(&Topology)` are infallible-by-construction and commented as such.
* **TS:** functional components, explicit props interfaces, `strict` + `verbatimModuleSyntax`. Tailwind classes inline, no CSS files beyond `index.css`.

## Not built yet (PRD scope, deliberately deferred)

Back-of-the-envelope calculator, flashcards, and inline (non-`window.prompt`) label editing. The critique loop was scaffolded end to end first because it is the piece that constrains every other decision.

## Parent repo note

`../CLAUDE.md` mandates `graphify query` for codebase questions. There is no `graphify-out/` here yet — run `graphify .` to generate it, or ignore the rule until then.
