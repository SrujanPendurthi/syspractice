import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { QuotaExceeded, streamCritique, Unauthorized } from './api';
import { nextStage, stageIndex, STAGES, type Level, type Stage, type Topology } from './types';

interface CritiquePanelProps {
  scenarioId: string;
  stage: Stage;
  level: Level;
  onAdvance: (stage: Stage) => void;
  /** Read lazily so dragging nodes does not re-render this panel mid-stream. */
  getTopology: () => Topology;
  notes: string;
  onNotesChange: (notes: string) => void;
  /**
   * Fires when the canvas and notes have been still long enough to be worth a critique.
   * Stable by contract: taking a changing prop instead would put this panel back into
   * every drag frame, which is the thing `memo` is here to prevent.
   */
  subscribeToIdle: (cb: () => void) => () => void;
  /** Critiques cost GPU time, so they are the one thing that needs an account. */
  signedIn: boolean;
  onSignIn: () => void;
}

function CritiquePanelInner({
  scenarioId,
  stage,
  level,
  onAdvance,
  getTopology,
  notes,
  onNotesChange,
  subscribeToIdle,
  signedIn,
  onSignIn,
}: CritiquePanelProps) {
  const [text, setText] = useState('');
  const [running, setRunning] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  /** Signed out, or out of critiques for the hour. Not an error — a state with an action. */
  const [limit, setLimit] = useState<string | null>(null);
  const [auto, setAuto] = useState(true);
  const abort = useRef<AbortController | null>(null);
  const startedAt = useRef(0);

  // Abort an in-flight stream when the panel goes away or the scenario changes.
  useEffect(() => () => abort.current?.abort(), []);

  // The reasoning phase is most of a critique, so the wait needs a number against it.
  useEffect(() => {
    if (!running) return;
    const t = setInterval(
      () => setElapsed(Math.round((Date.now() - startedAt.current) / 1000)),
      1000,
    );
    return () => clearInterval(t);
  }, [running]);

  const run = useCallback(async () => {
    // Auto-review fires on a timer, so this guard is what stops a signed-out visitor
    // generating a 401 every twenty seconds while they draw.
    if (!signedIn) return;
    abort.current?.abort();
    const controller = new AbortController();
    abort.current = controller;

    setText('');
    setError(null);
    setLimit(null);
    setRunning(true);
    startedAt.current = Date.now();
    setElapsed(0);

    try {
      await streamCritique(
        { scenarioId, topology: getTopology(), notes, stage, level },
        // `thinking` frames are deliberately dropped rather than ignored upstream: their
        // arrival is what proves the model is working, but their content is chain of
        // thought, which reads as mid-sentence noise. The elapsed counter says the same
        // thing in four characters.
        (kind, t) => {
          if (kind === 'token') setText((prev) => prev + t);
        },
        controller.signal,
      );
    } catch (e) {
      if (controller.signal.aborted) {
        // Nothing to report: the user edited, advanced a stage, or pressed Stop.
      } else if (e instanceof Unauthorized || e instanceof QuotaExceeded) {
        // Both are normal answers rather than faults, and both have a specific thing the
        // reader can do about them, so they get their own line rather than a red box.
        setError(null);
        setLimit(e.message);
      } else {
        setError(e instanceof Error ? e.message : String(e));
      }
    } finally {
      if (!controller.signal.aborted) setRunning(false);
    }
  }, [scenarioId, stage, level, getTopology, notes, signedIn]);

  // Stopping by hand has to clear `running` itself. The `finally` in `run` deliberately
  // does not: on the auto-restart path the replacement run has already set it back to
  // true, and resetting there would flicker. Nothing else was clearing it, so Stop left
  // the panel wedged with the elapsed counter climbing against a dead request.
  const stop = useCallback(() => {
    abort.current?.abort();
    setRunning(false);
  }, []);

  // A critique belongs to the stage and scenario it was written against. Without this a
  // stage-5 answer sits under a "Stage 1 of 5" header after New, and a stream that was
  // in flight when the stage changed keeps appending under the new stage's name.
  useEffect(() => {
    abort.current?.abort();
    setRunning(false);
    setText('');
    setError(null);
    setLimit(null);
  }, [stage, scenarioId]);

  // `run` aborts whatever is already streaming, so an edit landing mid-critique restarts
  // cleanly rather than interleaving two responses into the same panel.
  useEffect(() => (auto ? subscribeToIdle(run) : undefined), [auto, subscribeToIdle, run]);

  const index = stageIndex(stage);
  const next = nextStage(stage);

  return (
    <section className="flex w-full shrink-0 flex-col border-t border-line bg-surface lg:h-full lg:w-[28rem] lg:border-t-0 lg:border-l">
      {/* Stages 1 and 2 are graded on these notes, so they sit next to the critique that
          grades them rather than across the window in the sidebar. */}
      <div className="border-b border-line px-4 py-3">
        <h2 className="mb-2 text-[12px] font-medium text-ink-dim">Notes</h2>
        <textarea
          value={notes}
          onChange={(e) => onNotesChange(e.target.value)}
          rows={7}
          className="w-full resize-y rounded border border-line bg-page px-2 py-1.5 font-mono text-[12px] leading-relaxed text-ink"
        />
        <p className="mt-1.5 text-[11px] leading-relaxed text-ink-dim">
          Requirements and back-of-the-envelope maths. The first two stages are graded on
          this, not on the diagram.
        </p>
      </div>

      <header className="border-b border-line px-4 py-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-[14px] font-semibold">
              Stage {index + 1} of {STAGES.length}
            </h2>
            <p className="text-[12px] text-ink-dim">{STAGES[index].label}</p>
          </div>
          <button
            onClick={running ? stop : signedIn ? run : onSignIn}
            className="rounded bg-easy-ink px-3 py-1.5 text-[12px] font-medium text-white hover:brightness-110"
          >
            {running ? 'Stop' : signedIn ? 'Review' : 'Sign in to review'}
          </button>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <div className="flex gap-1">
            {STAGES.map((s, i) => (
              <span
                key={s.stage}
                title={s.label}
                className={`h-1.5 w-6 rounded-full ${i <= index ? 'bg-easy' : 'bg-line'}`}
              />
            ))}
          </div>
          <button
            onClick={() => next && onAdvance(next)}
            disabled={!next}
            className="ml-auto rounded px-2 py-0.5 text-[12px] text-ink-dim hover:bg-line/60 hover:text-ink disabled:text-ink-faint/60 disabled:hover:bg-transparent"
          >
            {next ? 'Next stage' : 'Last stage'}
          </button>
        </div>

        <label className="mt-2.5 flex items-center gap-1.5 text-[11px] text-ink-dim">
          <input
            type="checkbox"
            checked={auto}
            onChange={(e) => setAuto(e.target.checked)}
            className="h-3 w-3 accent-easy"
          />
          Review automatically once I stop editing
        </label>
      </header>

      <div className="flex-1 overflow-y-auto px-4 py-3">
        {error && (
          <p className="rounded border border-hard/30 bg-hard/5 px-3 py-2 text-[13px] text-hard-ink">
            {error}
          </p>
        )}
        {limit && (
          <p className="rounded border border-line bg-page px-3 py-2 text-[13px] leading-relaxed text-ink-dim">
            {limit}{' '}
            {!signedIn && (
              <button onClick={onSignIn} className="text-easy-ink underline underline-offset-2">
                Sign in
              </button>
            )}
          </p>
        )}
        {!error && !limit && !text && !running && (
          <p className="text-[13px] leading-relaxed text-ink-dim">
            Draw your architecture and write down your assumptions. The critic works one
            stage at a time and pushes back with a question rather than handing over the
            answer.{' '}
            {!signedIn && 'Reviewing needs an account; drawing does not.'}
          </p>
        )}
        {/* A reasoning model can think for a minute before emitting any critique text, so
            silence here is indistinguishable from a hang. The counter is the proof. */}
        {running && !text && (
          <p className="flex items-center gap-2 text-[13px] text-ink-dim">
            <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-easy" />
            Thinking… {elapsed}s
          </p>
        )}

        <pre className="whitespace-pre-wrap font-sans text-[13px] leading-[1.65] text-ink">
          {text}
          {running && <span className="animate-pulse">▍</span>}
        </pre>
      </div>
    </section>
  );
}

export const CritiquePanel = memo(CritiquePanelInner);
