import type {
  AttemptSummary,
  Concept,
  ConceptCard,
  Level,
  Match,
  MatchBoards,
  Post,
  PostComment,
  PostSummary,
  Scenario,
  ScenarioProgress,
  TrackCard,
  Stage,
  Topology,
  User,
} from './types';

/** Thrown for a 401 so callers can tell "signed out" from "broken". */
export class Unauthorized extends Error {}

/** Thrown for a 404, where a caller has something better to do than show the message. */
export class NotFound extends Error {}

/** Thrown for a 429. Inference is metered, so this is an expected answer, not a fault. */
export class QuotaExceeded extends Error {}

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    const message = body.error ?? res.statusText;
    if (res.status === 401) throw new Unauthorized(message);
    if (res.status === 404) throw new NotFound(message);
    if (res.status === 429) throw new QuotaExceeded(message);
    throw new Error(message);
  }
  return res.json() as Promise<T>;
}

/**
 * Same-origin in production, and Vite proxies `/api` in development, so the cookie rides
 * along by default. `same-origin` is stated rather than assumed because a future move to
 * a separate API host would otherwise silently sign everyone out.
 */
const send = (url: string, init?: RequestInit) =>
  fetch(url, { credentials: 'same-origin', ...init });

const post = (url: string, body?: unknown) =>
  send(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

export const listScenarios = () => send('/api/scenarios').then(json<Scenario[]>);

/** Track labels live on the server so they are not a second thing to keep mirrored. */
export const listTracks = () => send('/api/tracks').then(json<TrackCard[]>);

export const listProgress = () => send('/api/progress').then(json<ScenarioProgress[]>);

export const listAttempts = () => send('/api/attempts').then(json<AttemptSummary[]>);

export const saveAttempt = (
  scenarioId: string,
  topology: Topology,
  notes: string,
  stage: Stage,
) => post('/api/attempts', { scenarioId, topology, notes, stage }).then(json<{ id: number }>);

export const updateAttempt = (
  id: number,
  topology: Topology,
  notes: string,
  stage: Stage,
) =>
  send(`/api/attempts/${id}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ topology, notes, stage }),
  }).then(json<{ id: number }>);

export const loadAttempt = (id: number) =>
  send(`/api/attempts/${id}`).then(
    json<{ id: number; scenarioId: string; topology: Topology; notes: string; stage: Stage }>,
  );

// --- concepts ---------------------------------------------------------------

export const listConcepts = () => send('/api/concepts').then(json<ConceptCard[]>);

export const loadConcept = (id: string) =>
  send(`/api/concepts/${encodeURIComponent(id)}`).then(json<Concept>);

/** Empty when signed out, like progress — the syllabus is readable without an account. */
export const listReads = () => send('/api/reads').then(json<string[]>);

export const markRead = (id: string, read: boolean) =>
  post(`/api/concepts/${encodeURIComponent(id)}/read`, { read }).then(json<{ read: boolean }>);

// --- discussion --------------------------------------------------------------

/** Readable signed out. `scenario` narrows the index to one problem's threads. */
export const listPosts = (scenario?: string) =>
  send(`/api/posts${scenario ? `?scenario=${encodeURIComponent(scenario)}` : ''}`).then(
    json<PostSummary[]>,
  );

export const loadPost = (id: number) =>
  send(`/api/posts/${id}`).then(json<{ post: Post; comments: PostComment[] }>);

export const createPost = (title: string, body: string, scenarioId: string | null) =>
  post('/api/posts', { title, body, scenarioId }).then(json<{ id: number }>);

export const deletePost = (id: number) =>
  send(`/api/posts/${id}`, { method: 'DELETE' }).then(json<{ deleted: number }>);

export const createComment = (postId: number, body: string) =>
  post(`/api/posts/${postId}/comments`, { body }).then(json<{ id: number }>);

export const deleteComment = (id: number) =>
  send(`/api/comments/${id}`, { method: 'DELETE' }).then(json<{ deleted: number }>);

/** `value` of 0 withdraws the vote. Returns the post's recomputed score. */
export const votePost = (postId: number, value: number) =>
  post(`/api/posts/${postId}/vote`, { value }).then(json<{ score: number; myVote: number }>);

// --- head-to-head ------------------------------------------------------------

/** `open` is the lobby, `mine` is yours — empty when signed out. */
export const listMatches = () =>
  send('/api/matches').then(json<{ open: Match[]; mine: Match[] }>);

export const loadMatch = (id: number) => send(`/api/matches/${id}`).then(json<Match>);

export const createMatch = (scenarioId: string, level: Level, minutes: number) =>
  post('/api/matches', { scenarioId, level, minutes }).then(json<{ id: number }>);

export const joinMatch = (id: number) => post(`/api/matches/${id}/join`).then(json<Match>);

export const cancelMatch = (id: number) =>
  post(`/api/matches/${id}/cancel`).then(json<{ cancelled: number }>);

/** Final — the board cannot be edited afterwards. Returns the match, graded if it is ready. */
export const submitMatch = (id: number, topology: Topology, notes: string) =>
  post(`/api/matches/${id}/submit`, { topology, notes }).then(json<Match>);

export const loadMatchBoards = (id: number) =>
  send(`/api/matches/${id}/boards`).then(json<MatchBoards>);

// --- accounts ---------------------------------------------------------------

export const register = (email: string, displayName: string, password: string) =>
  post('/api/auth/register', { email, displayName, password }).then(json<User>);

export const login = (email: string, password: string) =>
  post('/api/auth/login', { email, password }).then(json<User>);

export const logout = () => post('/api/auth/logout').then(json<{ ok: boolean }>);

/** Null rather than throwing when signed out — that is the normal case, not an error. */
export const currentUser = async (): Promise<User | null> => {
  try {
    return await send('/api/auth/me').then(json<User>);
  } catch (e) {
    if (e instanceof Unauthorized) return null;
    throw e;
  }
};

/** `token` is critique text; `thinking` is a reasoning model's chain of thought. */
export type CritiqueEvent = 'token' | 'thinking';

/**
 * Stream a critique. The endpoint is a POST, so `EventSource` is out — we read the body
 * and parse SSE frames by hand. Each frame's data is a JSON-encoded string so tokens
 * containing newlines survive the transport.
 */
export async function streamCritique(
  req: {
    scenarioId: string;
    topology: Topology;
    notes: string;
    stage: Stage;
    level: Level;
  },
  onEvent: (kind: CritiqueEvent, text: string) => void,
  signal: AbortSignal,
): Promise<void> {
  const res = await send('/api/critique', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(req),
    signal,
  });

  if (!res.ok || !res.body) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    const message = body.error ?? res.statusText;
    if (res.status === 401) throw new Unauthorized(message);
    if (res.status === 429) throw new QuotaExceeded(message);
    throw new Error(message);
  }

  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = '';

  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += value;

    // SSE frames are separated by a blank line.
    let split: number;
    while ((split = buffer.indexOf('\n\n')) !== -1) {
      const frame = buffer.slice(0, split);
      buffer = buffer.slice(split + 2);

      // A frame is `event: <name>` followed by `data: <json>`. Keep-alives are bare
      // comment lines starting with ':' and carry neither.
      let kind: CritiqueEvent = 'token';
      for (const line of frame.split('\n')) {
        if (line.startsWith('event:')) kind = line.slice(6).trim() as CritiqueEvent;
        else if (line.startsWith('data:')) onEvent(kind, JSON.parse(line.slice(5).trim()) as string);
      }
    }
  }
}
