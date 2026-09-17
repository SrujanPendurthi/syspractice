import { useCallback, useEffect, useState } from 'react';
import { cancelMatch, createMatch, joinMatch, listMatches, listScenarios, Unauthorized } from './api';
import { ago } from './time';
import type { Level, Match, Scenario } from './types';
import { LEVELS } from './types';
import { useAuth } from './useAuth';

interface VersusProps {
  onOpen: (matchId: number) => void;
  onBack: () => void;
  onSignIn: () => void;
}

/** The clock, in minutes. Under five nobody finishes; over an hour it is not a race. */
const LENGTHS = [10, 20, 30];

/**
 * The lobby. Open challenges anyone can take, and your own matches.
 *
 * Readable signed out on purpose: this is the page that has to make the feature look
 * worth making an account for, and a login wall in front of an empty room is how a
 * two-player feature never gets its second player.
 */
export function Versus({ onOpen, onBack, onSignIn }: VersusProps) {
  const { user } = useAuth();
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [open, setOpen] = useState<Match[]>([]);
  const [mine, setMine] = useState<Match[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const { open, mine } = await listMatches();
      setOpen(open);
      setMine(mine);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    listScenarios().then(setScenarios).catch(() => undefined);
  }, []);

  // Keyed on the user: signing in has to pull in your own matches rather than leave the
  // lobby showing only what an anonymous visitor could see.
  useEffect(() => {
    void refresh();
  }, [refresh, user]);

  const title = (id: string) => scenarios.find((s) => s.id === id)?.title ?? id;

  const act = async (run: () => Promise<unknown>) => {
    setError(null);
    try {
      await run();
      await refresh();
    } catch (e) {
      setError((e as Error).message);
      if (e instanceof Unauthorized) onSignIn();
    }
  };

  return (
    <div className="min-h-full bg-page">
      <header className="border-b border-line">
        <div className="mx-auto flex max-w-3xl items-baseline gap-3 px-6 py-4">
          <button onClick={onBack} className="font-mono text-[15px] font-medium tracking-tight">
            syspractice
          </button>
          <span className="text-[13px] text-ink-dim">Head to head</span>
          {user && <span className="ml-auto text-[13px] text-ink-dim">{user.displayName}</span>}
        </div>
      </header>

      <main className="mx-auto max-w-3xl px-6 py-8">
        <h1 className="text-[20px] font-semibold">Race someone</h1>
        <p className="mt-1.5 max-w-prose text-[14px] leading-relaxed text-ink-dim">
          Same problem, same rubric, same clock, separate boards. Neither of you sees the
          other's design until the time is up — then one model call grades both and says
          which covered more of the rubric.
        </p>

        {error && (
          <p className="mt-4 rounded border border-hard/40 bg-hard/5 px-3 py-2 text-[13px] text-hard-ink">
            {error}
          </p>
        )}

        {user ? (
          <Challenge scenarios={scenarios} onCreate={(id) => onOpen(id)} onError={setError} />
        ) : (
          <p className="mt-6 text-[14px] text-ink-dim">
            <button onClick={onSignIn} className="text-easy-ink underline underline-offset-2">
              Sign in
            </button>{' '}
            to post a challenge or take one.
          </p>
        )}

        <Group title="Open challenges" empty="Nobody is waiting. Post one and it shows up here.">
          {open.map((m) => (
            <Row key={m.id} match={m} title={title}>
              {user && m.hostId === user.id ? (
                <button
                  onClick={() => void act(() => cancelMatch(m.id))}
                  className="text-[13px] text-ink-dim hover:text-ink"
                >
                  Cancel
                </button>
              ) : (
                <button
                  onClick={() =>
                    user
                      ? void act(async () => {
                          await joinMatch(m.id);
                          onOpen(m.id);
                        })
                      : onSignIn()
                  }
                  className="rounded border border-line px-2.5 py-1 text-[13px] hover:border-ink-faint"
                >
                  Take it
                </button>
              )}
            </Row>
          ))}
        </Group>

        {user && (
          <Group title="Your matches" empty="Nothing yet.">
            {mine.map((m) => (
              <Row key={m.id} match={m} title={title}>
                <button
                  onClick={() => onOpen(m.id)}
                  className="text-[13px] text-easy-ink hover:underline"
                >
                  {status(m, user.id)}
                </button>
              </Row>
            ))}
          </Group>
        )}
      </main>
    </div>
  );
}

/** What this match wants from you, said as the thing you would click. */
function status(m: Match, me: number): string {
  if (m.cancelled) return 'cancelled';
  if (m.verdict) {
    if (m.winner === 'draw') return 'draw';
    return (m.winner === 'host') === (m.hostId === me) ? 'you won' : 'you lost';
  }
  if (!m.startedAt) return 'waiting for a taker';
  const mine = m.hostId === me ? m.hostSubmitted : m.guestSubmitted;
  return mine ? 'waiting on them' : 'your turn';
}

function Challenge({
  scenarios,
  onCreate,
  onError,
}: {
  scenarios: Scenario[];
  onCreate: (id: number) => void;
  onError: (message: string) => void;
}) {
  const [scenarioId, setScenarioId] = useState('');
  const [level, setLevel] = useState<Level>('senior');
  const [minutes, setMinutes] = useState(20);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const { id } = await createMatch(scenarioId, level, minutes);
      onCreate(id);
    } catch (err) {
      onError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="mt-6 flex flex-wrap items-center gap-2 rounded border border-line p-4">
      <select
        value={scenarioId}
        onChange={(e) => setScenarioId(e.target.value)}
        required
        className="rounded border border-line bg-page px-2 py-1 text-[13px]"
        aria-label="Problem"
      >
        <option value="">Pick a problem…</option>
        {scenarios.map((s) => (
          <option key={s.id} value={s.id}>
            {s.title}
          </option>
        ))}
      </select>

      <select
        value={level}
        onChange={(e) => setLevel(e.target.value as Level)}
        className="rounded border border-line bg-page px-2 py-1 text-[13px]"
        aria-label="Level"
      >
        {LEVELS.map((l) => (
          <option key={l.level} value={l.level}>
            {l.label}
          </option>
        ))}
      </select>

      <select
        value={minutes}
        onChange={(e) => setMinutes(Number(e.target.value))}
        className="rounded border border-line bg-page px-2 py-1 text-[13px]"
        aria-label="Length"
      >
        {LENGTHS.map((n) => (
          <option key={n} value={n}>
            {n} minutes
          </option>
        ))}
      </select>

      <button
        type="submit"
        disabled={busy}
        className="ml-auto rounded bg-ink px-3 py-1.5 text-[13px] text-page disabled:opacity-50"
      >
        {busy ? 'Posting…' : 'Post challenge'}
      </button>
    </form>
  );
}

function Group({
  title,
  empty,
  children,
}: {
  title: string;
  empty: string;
  children: React.ReactNode[];
}) {
  return (
    <section className="mt-8">
      <h2 className="text-[15px] font-semibold">{title}</h2>
      {children.length === 0 ? (
        <p className="mt-2 text-[14px] text-ink-dim">{empty}</p>
      ) : (
        <ul className="mt-2 border-t border-line">{children}</ul>
      )}
    </section>
  );
}

function Row({
  match,
  title,
  children,
}: {
  match: Match;
  title: (id: string) => string;
  children: React.ReactNode;
}) {
  return (
    <li className="flex items-center gap-3 border-b border-line py-3">
      <div className="min-w-0 flex-1">
        <p className="text-[14px] text-ink">{title(match.scenarioId)}</p>
        <p className="mt-0.5 text-[12px] capitalize text-ink-dim">
          {match.host} · {match.level} · {match.minutes} min · {ago(match.createdAt)}
        </p>
      </div>
      {children}
    </li>
  );
}
