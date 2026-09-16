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

**The two runners differ on one field.** llama-server ignores the request's `"model"` and serves whatever it was launched with; Ollama *resolves* it and returns `404 model 'local' not found` for anything unknown. Hence `SYSPRACTICE_MODEL` (default `"local"`, correct only for llama-server). `./dev` sets it to the first entry of `ollama list` on the Ollama path.

## The one architectural contract

Everything routes through the **serialized canvas topology** — a JSON graph of nodes and labeled directed edges.

```
React Flow state ──toTopology()──▶ Topology JSON ──▶ Rust structs ──▶ prompt text
                                        │
                                        └──▶ SQLite sessions.topology
```

`backend/src/topology.rs` and `frontend/src/types.ts` are **hand-mirrored**. Renaming a field or adding a `NodeKind` means editing both in the same commit — plus `NODE_KINDS` in `types.ts` for the palette entry. `topology.rs`'s round-trip test asserts the camelCase wire format, so a Rust-side rename fails the build; a TS-side rename fails silently. If this drifts more than once, switch to `ts-rs` and derive.

`Topology::describe()` is the only thing the model sees of the diagram — coordinates are deliberately dropped, and edges are rendered by node *label*, not id.

## Layout

```
backend/src/
  main.rs        wiring only: env, pool, listener
  routes.rs      HTTP layer + AppState. Handlers stay thin.
  db.rs          persistence. No HTTP types cross this boundary.
  critic.rs      prompt construction + llama-server SSE proxy
  topology.rs    the shared domain contract
  scenarios.rs   static scenario + rubric data
frontend/src/
  App.tsx        owns canvas state, sidebar, session save/load
  Canvas.tsx     React Flow only
  CritiquePanel.tsx  streams and renders the critique
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
* **Reasoning models emit `delta.reasoning` with `delta.content` empty.** Measured on gemma4:12b: 686 reasoning frames to 456 content frames on one critique. Reading only `content` throws all of them away and the panel looks hung for the entire thinking phase. `parse_delta` returns `Delta::Reasoning`, the backend tags it as SSE event `thinking` (vs `token`), and the panel shows the tail as a liveness signal. Frames carrying both fields count as content.
* **Ollama caps every model at a 4096-token context** regardless of what it was trained for (MiniCPM5-2B: 131k, gemma4:12b: 262k), and `/v1/chat/completions` ignores `options.num_ctx` — verified, the request changes nothing. Only `OLLAMA_CONTEXT_LENGTH` on the server does, so `./dev` exports it before `ollama serve` and warns when it reuses a daemon it did not start. It matters because overflow truncates from the front, dropping the system prompt, and the critique silently stops following the four-section format. `ollama ps` prints the real number.
* **Vite proxies `/api`**, so the backend has no CORS layer. Serving the frontend from anywhere other than the dev server means adding one.
* **sqlx runtime-checked queries** (`query_as`, not `query!`). No `DATABASE_URL` needed at build time, but typos are caught by tests, not the compiler.
* **`nodeTypes` lives outside the component** in `Canvas.tsx`. Moving it inside remounts every node on every drag frame.
* `getTopology` in `App.tsx` is a stable callback over a ref precisely so `CritiquePanel` (memoized) stays out of drag renders.
* `:memory:` SQLite pools are capped at 1 connection in `db::connect` — each pooled connection would otherwise get its own private database.
* `./dev` runs `set -m` so each child leads its own process group and the trap can kill grandchildren (`cargo run` and `npm run dev` are both wrappers). That also means children sit outside the terminal's foreground group, so their stdin is detached — otherwise vite reads stdin, takes `SIGTTIN`, and suspends.

## Code style

* **Rust:** `thiserror` for typed errors (`AppError`), `anyhow` at the binary edge. No `unwrap`/`expect` outside tests; the two `expect`s on `serde_json::to_string(&Topology)` are infallible-by-construction and commented as such.
* **TS:** functional components, explicit props interfaces, `strict` + `verbatimModuleSyntax`. Tailwind classes inline, no CSS files beyond `index.css`.

## Not built yet (PRD scope, deliberately deferred)

Back-of-the-envelope calculator, flashcards, and inline (non-`window.prompt`) label editing. The critique loop was scaffolded end to end first because it is the piece that constrains every other decision.

## Parent repo note

`../CLAUDE.md` mandates `graphify query` for codebase questions. There is no `graphify-out/` here yet — run `graphify .` to generate it, or ignore the rule until then.
