import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { loadMatch, loadMatchBoards, submitMatch, QuotaExceeded, Unauthorized } from './api';
import { Canvas } from './Canvas';
import { countdown, parseSqlTime } from './time';
import { fromTopology, type Board, type Match, type MatchBoards, type Topology } from './types';
import { useAuth } from './useAuth';

interface MatchPanelProps {
  matchId: number;
  /** Stable by construction in `Workspace` — see the memo note below. */
  getTopology: () => Topology;
  notes: string;
  onNotesChange: (notes: string) => void;
  onBack: () => void;
}

/** How often the match row is re-read while the clock is running. */
const POLL_MS = 5000;

/**
 * The right-hand panel during a race: a clock, what your opponent has done, one Submit,
 * and the verdict when it lands.
 *
 * Memoized for the same reason `CritiquePanel` is. There is no idle timer in match mode,
 * but a drag still re-renders `Workspace` on every frame, and this panel owns a one-second
 * interval and a poll — re-mounting its effects sixty times a second is not free.
 *
 * ponytail: polling, not SSE. The plan assumed a live channel for the countdown, but a
 * deadline is a timestamp — the client can count down against it with no server at all,
 * and what actually has to arrive is "your opponent submitted", which is one row every
 * five seconds. An SSE fan-out needs a subscriber registry and a story for reconnects;
 * revisit when there is presence to show, not before.
 */
export const MatchPanel = memo(function MatchPanel({
  matchId,
  getTopology,
  notes,
  onNotesChange,
  onBack,
}: MatchPanelProps) {
  const { user, clear } = useAuth();
  const [match, setMatch] = useState<Match | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [comparing, setComparing] = useState(false);
  const compareButton = useRef<HTMLButtonElement>(null);
  const closeCompare = useCallback(() => {
    setComparing(false);
    compareButton.current?.focus();
  }, []);

  // A ref rather than reading `match`, so `refresh` keeps one identity for the life of the
  // match. Depending on the state it sets would give every successful poll a new callback,
  // which tears down and rebuilds the interval below on every tick.
  const loaded = useRef(false);

  const refresh = useCallback(async () => {
    try {
      setMatch(await loadMatch(matchId));
      loaded.current = true;
    } catch (e) {
      if (e instanceof Unauthorized) clear();
      // A failed poll is not worth showing once the match is on screen: the next one is
      // five seconds away, and an error flickering over a running clock reads as the
      // match itself having broken.
      else if (!loaded.current) setError((e as Error).message);
    }
  }, [matchId, clear]);

  useEffect(() => {
    loaded.current = false;
    void refresh();
  }, [refresh]);

  const settled = !!match?.verdict || !!match?.cancelled;

  // One second for the clock, five for the server. Separated because the countdown is
  // arithmetic on a timestamp we already have — it does not need the network. Stopped once
  // settled: nothing shows the clock then, and each tick would re-render the boards.
  useEffect(() => {
    if (settled) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [settled]);
  useEffect(() => {
    if (settled) return;
    const t = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(t);
  }, [settled, refresh]);

  // The verdict is written when a participant reads a finished match, so the poll that
  // notices the deadline has passed is also the thing that triggers grading.
  const msLeft = match?.deadline ? parseSqlTime(match.deadline) - now : 0;

  const iAmHost = !!user && match?.hostId === user.id;
  const mine = iAmHost ? match?.hostSubmitted : match?.guestSubmitted;
  const theirs = iAmHost ? match?.guestSubmitted : match?.hostSubmitted;
  const opponent = (iAmHost ? match?.guest : match?.host) ?? 'nobody yet';

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      setMatch(await submitMatch(matchId, getTopology(), notes));
    } catch (e) {
      if (e instanceof Unauthorized) clear();
      setError(
        e instanceof QuotaExceeded
          ? `${(e as Error).message}. Your board is saved — open this match again once the hour rolls over.`
          : (e as Error).message,
      );
      // Submitting is what triggers grading, and grading can fail on its own (the runner,
      // the quota) after the board is safely in. Re-reading separates "your submission
      // failed" from "the verdict did not run", which look identical from the throw.
      void refresh();
    } finally {
      setBusy(false);
    }
  };

  if (error && !match) {
    return (
      <Panel>
        <p className="text-sm text-hard-ink">{error}</p>
      </Panel>
    );
  }
  if (!match) {
    return (
      <Panel>
        <p className="text-sm text-ink-dim">Loading match…</p>
      </Panel>
    );
  }

  const outcome =
    match.winner === 'draw'
      ? { text: 'Draw', tone: 'text-medium-ink' }
      : (match.winner === 'host') === iAmHost
        ? { text: 'You won', tone: 'text-easy-ink' }
        : { text: 'You lost', tone: 'text-hard-ink' };

  return (
    <Panel>
      <div className="border-b border-line px-4 py-3">
        <button onClick={onBack} className="text-sm text-ink-dim hover:text-ink">
          All matches
        </button>
        <h2 className="mt-1 text-base font-semibold">
          vs {opponent}
          <span className="ml-2 font-normal text-sm capitalize text-ink-dim">
            {match.level}
          </span>
        </h2>
      </div>

      {!match.verdict && !match.cancelled && (
        <div className="border-b border-line px-4 py-3">
          {match.startedAt === null ? (
            <p className="text-sm text-ink-dim">
              Waiting for someone to take this challenge. The clock does not start until
              they do — you can draw while you wait, and submit the moment it does.
            </p>
          ) : (
            <>
              <p
                className={`font-mono text-2xl leading-none ${
                  msLeft <= 60000 ? 'text-hard-ink' : 'text-ink'
                }`}
              >
                {countdown(msLeft)}
              </p>
              <p className="mt-1 text-xs text-ink-dim">
                {msLeft <= 0 ? 'time is up' : `of ${match.minutes} minutes`}
              </p>
            </>
          )}

          <dl className="mt-3 space-y-1 text-sm">
            <Row label="You" done={!!mine} />
            <Row label={opponent} done={!!theirs} />
          </dl>
        </div>
      )}

      <div className="flex-1 overflow-y-auto px-4 py-3">
        {match.verdict ? (
          <>
            <p className={`text-lg font-semibold ${outcome.tone}`}>{outcome.text}</p>
            {/* Plain text, like every other body of model output in this app. */}
            <p className="mt-2 whitespace-pre-wrap text-sm leading-[1.7] text-ink">
              {match.verdict}
            </p>
            <button
              ref={compareButton}
              onClick={() => setComparing(true)}
              className="mt-4 w-full rounded bg-accent px-3 py-2 text-sm font-medium text-accent-ink hover:brightness-110"
            >
              Compare boards
            </button>
            {comparing && (
              <CompareBoards matchId={matchId} opponent={opponent} onClose={closeCompare} />
            )}
          </>
        ) : match.cancelled ? (
          <p className="text-sm text-ink-dim">This challenge was cancelled.</p>
        ) : mine ? (
          <p className="text-sm text-ink-dim">
            Your board is locked in. The verdict is written once you have both submitted or
            the clock runs out, whichever comes first.
          </p>
        ) : (
          <>
            <label className="text-sm font-medium text-ink-dim" htmlFor="match-notes">
              Notes
            </label>
            <textarea
              id="match-notes"
              value={notes}
              onChange={(e) => onNotesChange(e.target.value)}
              maxLength={8000}
              rows={8}
              placeholder="Requirements, the numbers, the trade-off you are making."
              className="mt-1 w-full resize-y rounded border border-line bg-page p-2 text-sm leading-relaxed outline-none focus:border-ink-faint"
            />
            <p className="mt-2 text-xs leading-relaxed text-ink-dim">
              Both designs are graded against the same rubric at {match.level} level. The
              judge is not told who drew which.
            </p>
          </>
        )}

        {error && <p className="mt-3 text-sm text-hard-ink">{error}</p>}
      </div>

      {!match.verdict && !match.cancelled && !mine && match.startedAt !== null && (
        <div className="border-t border-line px-4 py-3">
          <button
            onClick={() => void submit()}
            disabled={busy || msLeft <= 0}
            className="w-full rounded bg-accent px-3 py-2 text-sm font-medium text-accent-ink disabled:opacity-50"
          >
            {busy ? 'Submitting…' : msLeft <= 0 ? 'Time is up' : 'Submit design'}
          </button>
          <p className="mt-1.5 text-xs text-ink-dim">
            Submitting is final — you cannot edit afterwards.
          </p>
        </div>
      )}
    </Panel>
  );
});

/**
 * Both designs side by side, once the verdict is in — stacked on a narrow screen.
 *
 * Fetched from its own endpoint rather than riding on the match: the match view is polled
 * all race long and must never have a shape that could carry a board. The server answers
 * only once there is a verdict, so there is no "too early" state to draw here.
 *
 * Absolute, like the sign-in overlay, so it covers the workspace and leaves the app bar.
 */
function CompareBoards({ matchId, opponent, onClose }: { matchId: number; opponent: string; onClose: () => void }) {
  const { clear } = useAuth();
  const [boards, setBoards] = useState<MatchBoards | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadMatchBoards(matchId)
      .then(setBoards)
      .catch((e) => {
        if (e instanceof Unauthorized) clear();
        setError((e as Error).message);
      });
  }, [matchId, clear]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Compare boards"
      // On the dialog, not the window: a window listener also reached the appearance menu's
      // own Escape handler and closed that too.
      onKeyDown={(e) => {
        if (e.key !== 'Escape') return;
        e.stopPropagation();
        onClose();
      }}
      className="absolute inset-0 z-30 flex flex-col bg-page text-ink"
    >
      <div className="flex items-center justify-between border-b border-line px-4 py-2">
        <h2 className="text-base font-semibold">Compare boards</h2>
        <button autoFocus onClick={onClose} className="text-sm text-accent hover:underline">
          Back to the verdict
        </button>
      </div>
      {error ? (
        <p className="p-4 text-sm text-hard-ink">{error}</p>
      ) : !boards ? (
        <p className="p-4 text-sm text-ink-dim">Loading boards…</p>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-1 grid-rows-2 divide-y divide-line lg:grid-cols-2 lg:grid-rows-1 lg:divide-x lg:divide-y-0">
          <BoardView title="Yours" board={boards.yours} />
          <BoardView title={`${opponent}'s`} board={boards.opponents} />
        </div>
      )}
    </div>
  );
}

function BoardView({ title, board }: { title: string; board: Board | null }) {
  // The panel re-renders every second for its clock. Without the memo each tick hands
  // React Flow a fresh copy of the board to reconcile.
  const graph = useMemo(() => (board ? fromTopology(board.topology) : null), [board]);
  return (
    <section className="flex min-h-0 flex-col">
      <h3 className="border-b border-line px-4 py-1.5 text-sm font-medium text-ink-dim">{title}</h3>
      {graph ? (
        <>
          <div className="min-h-0 flex-1">
            <Canvas readOnly nodes={graph.nodes} edges={graph.edges} />
          </div>
          {board?.notes && (
            <p className="max-h-28 overflow-y-auto border-t border-line px-4 py-2 text-sm whitespace-pre-wrap text-ink-dim">
              {board.notes}
            </p>
          )}
        </>
      ) : (
        <p className="p-4 text-sm text-ink-dim">Did not submit.</p>
      )}
    </section>
  );
}

function Panel({ children }: { children: React.ReactNode }) {
  return (
    <aside className="flex w-full shrink-0 flex-col border-t border-line bg-surface lg:h-full lg:w-80 lg:border-t-0 lg:border-l">
      {children}
    </aside>
  );
}

function Row({ label, done }: { label: string; done: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="truncate text-ink-dim">{label}</dt>
      <dd className={done ? 'text-easy-ink' : 'text-ink-faint'}>
        {done ? 'submitted' : 'still drawing'}
      </dd>
    </div>
  );
}
