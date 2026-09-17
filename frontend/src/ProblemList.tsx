import { useEffect, useMemo, useState } from 'react';
import { listProgress, listScenarios, listTracks } from './api';
import { Segmented } from './Segmented';
import type { Difficulty, Scenario, ScenarioProgress, Track, TrackCard } from './types';
import { useAuth } from './useAuth';

const DIFFICULTY_TEXT: Record<Difficulty, string> = {
  easy: 'text-easy-ink',
  medium: 'text-medium-ink',
  hard: 'text-hard-ink',
};

const DIFFICULTY_RING: Record<Difficulty, string> = {
  easy: 'var(--color-easy)',
  medium: 'var(--color-medium)',
  hard: 'var(--color-hard)',
};

const ORDER: Difficulty[] = ['easy', 'medium', 'hard'];

interface ProblemListProps {
  onOpen: (scenarioId: string) => void;
  onSignIn: () => void;
}

export function ProblemList({ onOpen, onSignIn }: ProblemListProps) {
  const { user } = useAuth();
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [tracks, setTracks] = useState<TrackCard[]>([]);
  const [progress, setProgress] = useState<ScenarioProgress[]>([]);
  const [error, setError] = useState<string | null>(null);
  // Not in the URL: this is a view preference on the landing page, not a place you link
  // somebody to. `/s/<id>` is the shareable thing and it already carries the track.
  const [filter, setFilter] = useState<Track | 'all'>('all');

  useEffect(() => {
    listScenarios().then(setScenarios).catch((e) => setError(e.message));
    listTracks().then(setTracks).catch(() => undefined);
    // Best-effort: an empty table and a failed request both mean "nothing solved yet",
    // which is what a first run should look like anyway.
  }, []);

  // Progress is per account, so signing in or out has to refetch rather than leave the
  // previous answer on screen. Signed out the server returns an empty list by design.
  useEffect(() => {
    listProgress().then(setProgress).catch(() => setProgress([]));
  }, [user]);

  const byId = useMemo(() => new Map(progress.map((p) => [p.scenarioId, p])), [progress]);

  // Falls back to the id so a track the server knows about and this build does not still
  // renders something truthful.
  const trackLabel = (id: Track) => tracks.find((t) => t.id === id)?.label ?? id;

  // Everything below counts the filtered set, so the ring answers "how am I doing at the
  // thing I am looking at" rather than always answering for the whole catalogue.
  const visible = useMemo(
    () => scenarios.filter((s) => filter === 'all' || s.track === filter),
    [scenarios, filter],
  );

  const tally = useMemo(
    () =>
      ORDER.map((difficulty) => {
        const of = visible.filter((s) => s.difficulty === difficulty);
        return {
          difficulty,
          total: of.length,
          solved: of.filter((s) => byId.get(s.id)?.solved).length,
        };
      }),
    [visible, byId],
  );

  const solved = tally.reduce((n, t) => n + t.solved, 0);

  // The backend returns scenarios in the order they are declared, which is authoring
  // order rather than anything a reader wants. Grouping by difficulty puts the ramp at
  // the top and keeps a newly added Easy problem from landing below the Hard ones.
  const rows = useMemo(
    () => [...visible].sort((a, b) => ORDER.indexOf(a.difficulty) - ORDER.indexOf(b.difficulty)),
    [visible],
  );

  return (
    <div className="mx-auto grid max-w-5xl grid-cols-1 gap-x-10 gap-y-6 px-6 py-8 lg:grid-cols-[1fr_15rem]">
      <div className="lg:col-span-2">
        <h1 className="text-xl font-semibold">Problems</h1>
        <p className="mt-1 max-w-prose text-base text-ink-dim">
          Draw the architecture, then work the five interview stages. The critic grades one
          stage at a time against the rubric for the level you pick.
        </p>
      </div>

      <section>
        {tracks.length > 1 && (
          <div className="mb-4">
            <Segmented
              label="Interview"
              hideLabel
              value={filter}
              onChange={setFilter}
              options={[
                { value: 'all' as const, label: 'All', hint: String(scenarios.length) },
                ...tracks.map((t) => ({
                  value: t.id,
                  label: t.label,
                  hint: String(scenarios.filter((s) => s.track === t.id).length),
                })),
              ]}
            />
          </div>
        )}

        {error && (
          <p className="mb-4 rounded border border-hard/40 bg-hard/5 px-3 py-2 text-sm text-hard-ink">
            Could not reach the backend: {error}. Start it with ./dev and reload.
          </p>
        )}

        <table className="w-full border-collapse text-base">
          <thead>
            <tr className="border-b border-line text-left text-xs text-ink-dim">
              <th className="w-9 py-2 font-medium" scope="col">
                <span className="sr-only">Status</span>
              </th>
              <th className="py-2 font-medium" scope="col">Problem</th>
              <th className="w-32 py-2 font-medium" scope="col">Interview</th>
              <th className="w-24 py-2 font-medium" scope="col">Difficulty</th>
              <th className="w-20 py-2 text-right font-medium" scope="col">Attempts</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((s, i) => {
              const p = byId.get(s.id);
              return (
                <tr
                  key={s.id}
                  tabIndex={0}
                  role="link"
                  onClick={() => onOpen(s.id)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      onOpen(s.id);
                    }
                  }}
                  /* The violet edge is the row saying it is clickable. Status keeps the
                     assessment colours and never borrows the interaction one, so a
                     highlighted row cannot be misread as a solved row. */
                  className={`group cursor-pointer border-b border-line/70 hover:bg-surface ${
                    i % 2 === 1 ? 'bg-surface/50' : ''
                  }`}
                >
                  <td className="relative py-2.5 text-center">
                    <span className="absolute inset-y-0 left-0 w-0.5 scale-y-0 bg-accent transition-transform group-hover:scale-y-100 group-focus:scale-y-100" />
                    <Status progress={p} />
                  </td>
                  <td className="py-2.5 text-ink">{s.title}</td>
                  {/* Sorting is by difficulty alone, so the two interviews interleave.
                      Without this column an Easy ML problem sits next to an Easy classic
                      one and the first thing telling you which you opened is the stage
                      count in the panel. */}
                  <td className="py-2.5 text-sm text-ink-dim">
                    {trackLabel(s.track)}
                  </td>
                  <td className={`py-2.5 text-sm capitalize ${DIFFICULTY_TEXT[s.difficulty]}`}>
                    {s.difficulty}
                  </td>
                  <td className="tnum py-2.5 text-right font-mono text-sm text-ink-dim">
                    {p ? p.attempts : '–'}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>

      <aside>
        <ProgressRing solved={solved} total={visible.length} tally={tally} />

        <dl className="mt-6 space-y-2 text-sm">
          {tally.map((t) => (
            <div key={t.difficulty} className="flex items-baseline justify-between gap-3">
              <dt className={`capitalize ${DIFFICULTY_TEXT[t.difficulty]}`}>{t.difficulty}</dt>
              <dd className="tnum font-mono text-ink-dim">
                {t.solved}
                <span className="text-ink-faint">/{t.total}</span>
              </dd>
            </div>
          ))}
        </dl>

        <p className="mt-6 border-t border-line pt-4 text-xs leading-relaxed text-ink-dim">
          {user ? (
            <>
              A problem counts as solved once you have worked every stage of its
              interview. It tracks that you went the distance, not that the design was good.
            </>
          ) : (
            <>
              <button onClick={onSignIn} className="text-accent underline underline-offset-2">
                Sign in
              </button>{' '}
              to track which problems you have worked. Browsing and drawing do not need an
              account.
            </>
          )}
        </p>
      </aside>
    </div>
  );
}

/**
 * Shape as well as colour. Someone who cannot separate the teal from the amber still gets
 * a filled disc, a half disc and a ring — and the title attribute says it in words.
 */
function Status({ progress }: { progress?: ScenarioProgress }) {
  if (progress?.solved) {
    return (
      <span className="text-easy-ink" title="Solved — all five stages walked">
        ✓
      </span>
    );
  }
  if (progress) {
    return (
      <span className="text-medium-ink" title="Started, not finished">
        ◑
      </span>
    );
  }
  return (
    <span className="text-ink-faint/60" title="Not started">
      ○
    </span>
  );
}

/**
 * The one piece of the page allowed to be loud. Three arcs, one per difficulty, each
 * sized by how many problems carry that difficulty and filled by how many are solved —
 * so the ring reads as overall progress and as where the gaps are, at once.
 */
function ProgressRing({
  solved,
  total,
  tally,
}: {
  solved: number;
  total: number;
  tally: { difficulty: Difficulty; solved: number; total: number }[];
}) {
  const R = 52;
  const C = 2 * Math.PI * R;
  const GAP = 18;

  let offset = 0;
  const arcs = tally.map((t) => {
    const share = total ? (t.total / total) * C : 0;
    const arc = { ...t, start: offset, span: Math.max(share - GAP, 0) };
    offset += share;
    return arc;
  });

  return (
    <figure className="relative mx-auto h-40 w-40">
      <svg
        viewBox="0 0 128 128"
        className="h-full w-full -rotate-90"
        role="img"
        aria-label={`${solved} of ${total} problems solved`}
      >
        {arcs.map((a) => {
          const filled = a.total ? (a.solved / a.total) * a.span : 0;
          return (
            <g key={a.difficulty}>
              <circle
                cx="64"
                cy="64"
                r={R}
                fill="none"
                strokeWidth="8"
                strokeLinecap="round"
                stroke="var(--color-line)"
                strokeDasharray={`${a.span} ${C - a.span}`}
                strokeDashoffset={-a.start}
              />
              {/* A round cap paints a dot even at zero length, so an untouched
                  difficulty has to be skipped rather than drawn empty. */}
              {filled > 0 && (
                <circle
                  cx="64"
                  cy="64"
                  r={R}
                  fill="none"
                  strokeWidth="8"
                  strokeLinecap="round"
                  stroke={DIFFICULTY_RING[a.difficulty]}
                  strokeDasharray={`${filled} ${C - filled}`}
                  strokeDashoffset={-a.start}
                />
              )}
            </g>
          );
        })}
      </svg>

      <figcaption className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="tnum font-mono text-2xl leading-none text-ink">{solved}</span>
        <span className="mt-1.5 text-xs text-ink-dim">of {total} solved</span>
      </figcaption>
    </figure>
  );
}
