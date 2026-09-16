import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { streamCritique } from './api';
import { nextStage, stageIndex, STAGES, type Level, type Stage, type Topology } from './types';

interface CritiquePanelProps {
  scenarioId: string;
  stage: Stage;
  difficulty: Level;
  onAdvance: (stage: Stage) => void;
  /** Read lazily so dragging nodes does not re-render this panel mid-stream. */
  getTopology: () => Topology;
  /** Same reason — typing in the notes pane must not re-render the panel. */
  getNotes: () => string;
  /**
   * Fires when the canvas and notes have been still long enough to be worth a critique.
   * Stable by contract: taking a changing prop instead would put this panel back into
   * every drag frame, which is the thing `memo` is here to prevent.
   */
  subscribeToIdle: (cb: () => void) => () => void;
}

function CritiquePanelInner({
  scenarioId,
  stage,
  difficulty,
  onAdvance,
  getTopology,
  getNotes,
  subscribeToIdle,
}: CritiquePanelProps) {
  const [text, setText] = useState('');
  const [thinking, setThinking] = useState('');
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [auto, setAuto] = useState(true);
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
        { scenarioId, topology: getTopology(), notes: getNotes(), stage, difficulty },
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
  }, [scenarioId, stage, difficulty, getTopology, getNotes]);

  // `run` aborts whatever is already streaming, so an edit landing mid-critique restarts
  // cleanly rather than interleaving two responses into the same panel.
  useEffect(() => (auto ? subscribeToIdle(run) : undefined), [auto, subscribeToIdle, run]);

  const index = stageIndex(stage);
  const next = nextStage(stage);

  return (
    <section className="flex h-full w-96 shrink-0 flex-col border-l border-slate-800 bg-slate-900">
      <header className="border-b border-slate-800 px-4 py-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-semibold text-slate-200">
              Stage {index + 1} of {STAGES.length}
            </h2>
            <p className="text-[11px] text-slate-400">{STAGES[index].label}</p>
          </div>
          <button
            onClick={running ? () => abort.current?.abort() : run}
            className="rounded bg-emerald-600 px-3 py-1 text-xs font-medium text-white hover:bg-emerald-500"
          >
            {running ? 'Stop' : 'Review'}
          </button>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <div className="flex gap-1">
            {STAGES.map((s, i) => (
              <span
                key={s.stage}
                title={s.label}
                className={`h-1.5 w-6 rounded-full ${i <= index ? 'bg-emerald-500' : 'bg-slate-700'}`}
              />
            ))}
          </div>
          <button
            onClick={() => next && onAdvance(next)}
            disabled={!next}
            className="ml-auto rounded px-2 py-0.5 text-[11px] text-slate-300 hover:bg-slate-800 disabled:text-slate-600 disabled:hover:bg-transparent"
          >
            {next ? 'Next stage →' : 'Last stage'}
          </button>
        </div>

        <label className="mt-2 flex items-center gap-1.5 text-[10px] text-slate-500">
          <input
            type="checkbox"
            checked={auto}
            onChange={(e) => setAuto(e.target.checked)}
            className="h-3 w-3 accent-emerald-600"
          />
          Review automatically once I stop editing
        </label>
      </header>

      <div className="flex-1 overflow-y-auto px-4 py-3">
        {error && (
          <p className="rounded border border-red-800 bg-red-950 px-3 py-2 text-xs text-red-300">
            {error}
          </p>
        )}
        {!error && !text && !running && (
          <p className="text-xs text-slate-500">
            Draw your architecture and write your assumptions. The critic takes one stage at
            a time and pushes back with a question rather than handing over the answer.
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
