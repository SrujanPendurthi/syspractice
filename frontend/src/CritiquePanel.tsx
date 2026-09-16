import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { streamCritique } from './api';
import type { Topology } from './types';

interface CritiquePanelProps {
  scenarioId: string;
  /** Read lazily so dragging nodes does not re-render this panel mid-stream. */
  getTopology: () => Topology;
}

function CritiquePanelInner({ scenarioId, getTopology }: CritiquePanelProps) {
  const [text, setText] = useState('');
  const [thinking, setThinking] = useState('');
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abort = useRef<AbortController | null>(null);

  // Abort an in-flight stream when the panel goes away or the scenario changes.
  useEffect(() => () => abort.current?.abort(), []);

  const run = useCallback(async () => {
    abort.current?.abort();
    const controller = new AbortController();
    abort.current = controller;

    setText('');
    setThinking('');
    setError(null);
    setRunning(true);

    try {
      await streamCritique(
        scenarioId,
        getTopology(),
        (kind, t) => {
          if (kind === 'token') setText((prev) => prev + t);
          // Only the tail is kept — this is a liveness signal, not a transcript.
          else setThinking((prev) => (prev + t).slice(-400));
        },
        controller.signal,
      );
    } catch (e) {
      if (!controller.signal.aborted) setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (!controller.signal.aborted) setRunning(false);
    }
  }, [scenarioId, getTopology]);

  return (
    <section className="flex h-full w-96 shrink-0 flex-col border-l border-slate-800 bg-slate-900">
      <header className="flex items-center justify-between border-b border-slate-800 px-4 py-3">
        <h2 className="text-sm font-semibold text-slate-200">Critique</h2>
        <button
          onClick={running ? () => abort.current?.abort() : run}
          className="rounded bg-emerald-600 px-3 py-1 text-xs font-medium text-white hover:bg-emerald-500"
        >
          {running ? 'Stop' : 'Review my design'}
        </button>
      </header>

      <div className="flex-1 overflow-y-auto px-4 py-3">
        {error && (
          <p className="rounded border border-red-800 bg-red-950 px-3 py-2 text-xs text-red-300">
            {error}
          </p>
        )}
        {!error && !text && (
          <p className="text-xs text-slate-500">
            Draw your architecture, then ask for a review. The critic pushes back with
            questions rather than handing over the answer.
          </p>
        )}
        {/* A reasoning model can think for a minute before emitting any critique text.
            Showing the tail of that proves it is working rather than hung. */}
        {running && thinking && !text && (
          <div className="mb-3 rounded border border-slate-800 bg-slate-950 px-3 py-2">
            <p className="mb-1 text-[10px] uppercase tracking-wide text-slate-500">Thinking</p>
            <p className="line-clamp-6 text-[11px] italic leading-relaxed text-slate-600">
              {thinking}
            </p>
          </div>
        )}

        <pre className="whitespace-pre-wrap font-sans text-xs leading-relaxed text-slate-300">
          {text}
          {running && <span className="animate-pulse">▍</span>}
        </pre>
      </div>
    </section>
  );
}

export const CritiquePanel = memo(CritiquePanelInner);
