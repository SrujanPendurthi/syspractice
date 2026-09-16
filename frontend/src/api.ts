import type { Level, Scenario, SessionSummary, Stage, Topology } from './types';

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error ?? res.statusText);
  }
  return res.json() as Promise<T>;
}

export const listScenarios = () => fetch('/api/scenarios').then(json<Scenario[]>);

export const listSessions = () => fetch('/api/sessions').then(json<SessionSummary[]>);

export const saveSession = (
  scenarioId: string,
  topology: Topology,
  notes: string,
  stage: Stage,
) =>
  fetch('/api/sessions', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ scenarioId, topology, notes, stage }),
  }).then(json<{ id: number }>);

export const updateSession = (id: number, topology: Topology, notes: string, stage: Stage) =>
  fetch(`/api/sessions/${id}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ topology, notes, stage }),
  }).then(json<{ id: number }>);

export const loadSession = (id: number) =>
  fetch(`/api/sessions/${id}`).then(
    json<{ id: number; scenarioId: string; topology: Topology; notes: string; stage: Stage }>,
  );

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
    difficulty: Level;
  },
  onEvent: (kind: CritiqueEvent, text: string) => void,
  signal: AbortSignal,
): Promise<void> {
  const res = await fetch('/api/critique', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(req),
    signal,
  });

  if (!res.ok || !res.body) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error ?? res.statusText);
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
