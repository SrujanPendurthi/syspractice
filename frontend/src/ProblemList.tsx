import { useEffect, useMemo, useState } from 'react';
import { listProgress, listScenarios } from './api';
import type { Difficulty, Scenario, ScenarioProgress } from './types';

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
}

export function ProblemList({ onOpen }: ProblemListProps) {
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [progress, setProgress] = useState<ScenarioProgress[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listScenarios().then(setScenarios).catch((e) => setError(e.message));
    // Best-effort: an empty table and a failed request both mean "nothing solved yet",
    // which is what a first run should look like anyway.
    listProgress().then(setProgress).catch(() => undefined);
  }, []);

  const byId = useMemo(
    () => new Map(progress.map((p) => [p.scenarioId, p])),
    [progress],
  );

  const tally = useMemo(
    () =>
      ORDER.map((difficulty) => {
        const of = scenarios.filter((s) => s.difficulty === difficulty);
        return {
          difficulty,
          total: of.length,
          solved: of.filter((s) => byId.get(s.id)?.solved).length,
        };
      }),
    [scenarios, byId],
  );

  const solved = tally.reduce((n, t) => n + t.solved, 0);

  // The backend returns scenarios in the order they are declared, which is authoring
  // order rather than anything a reader wants. Grouping by difficulty puts the ramp at
  // the top and keeps a newly added Easy problem from landing below the Hard ones.
  const rows = useMemo(
    () =>
      [...scenarios].sort(
        (a, b) => ORDER.indexOf(a.difficulty) - ORDER.indexOf(b.difficulty),
      ),
    [scenarios],
  );

  return (
    <div className="min-h-full bg-page">
      <header className="border-b border-line">
        <div className="mx-auto flex max-w-5xl items-baseline gap-3 px-6 py-4">
          <span className="font-mono text-[15px] font-medium tracking-tight text-ink">
            syspractice
          </span>
          <span className="text-[13px] text-ink-dim">System design interview drills</span>
        </div>
      </header>

      <main className="mx-auto grid max-w-5xl grid-cols-1 gap-10 px-6 py-8 lg:grid-cols-[1fr_15rem]">
        <section>
          {error && (
            <p className="mb-4 rounded border border-hard/40 bg-hard/5 px-3 py-2 text-[13px] text-hard-ink">
              Could not reach the backend: {error}. Start it with ./dev and reload.
            </p>
          )}

          <table className="w-full border-collapse text-[14px]">
            <thead>
              <tr className="border-b border-line text-left text-[12px] text-ink-dim">
                <th className="w-9 py-2 font-medium" scope="col">
                  <span className="sr-only">Status</span>
                </th>
                <th className="py-2 font-medium" scope="col">Problem</th>
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
                    className={`cursor-pointer border-b border-line/70 hover:bg-surface ${
                      i % 2 === 1 ? 'bg-surface/50' : ''
                    }`}
                  >
                    <td className="py-2.5 text-center">
                      {p?.solved ? (
                        <span className="text-easy-ink" title="All five stages walked">✓</span>
                      ) : p ? (
                        <span className="text-medium-ink" title="Started, not finished">◑</span>
                      ) : (
                        <span className="text-ink-faint/60">○</span>
                      )}
                    </td>
                    <td className="py-2.5 text-ink">{s.title}</td>
                    <td className={`py-2.5 text-[13px] capitalize ${DIFFICULTY_TEXT[s.difficulty]}`}>
                      {s.difficulty}
                    </td>
                    <td className="py-2.5 text-right font-mono text-[13px] text-ink-dim">
                      {p ? p.attempts : '–'}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </section>

        <aside className="lg:pt-10">
          <ProgressRing solved={solved} total={scenarios.length} tally={tally} />

          <dl className="mt-6 space-y-2 text-[13px]">
            {tally.map((t) => (
              <div key={t.difficulty} className="flex items-baseline justify-between gap-3">
                <dt className={`capitalize ${DIFFICULTY_TEXT[t.difficulty]}`}>{t.difficulty}</dt>
                <dd className="font-mono text-ink-dim">
                  {t.solved}<span className="text-ink-faint">/{t.total}</span>
                </dd>
              </div>
            ))}
          </dl>

          <p className="mt-6 border-t border-line pt-4 text-[12px] leading-relaxed text-ink-dim">
            A problem counts as solved once you have worked all five interview stages. It
            tracks that you went the distance, not that the design was good.
          </p>
        </aside>
      </main>
    </div>
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
      <svg viewBox="0 0 128 128" className="h-full w-full -rotate-90" role="img"
        aria-label={`${solved} of ${total} problems solved`}>
        {arcs.map((a) => {
          const filled = a.total ? (a.solved / a.total) * a.span : 0;
          return (
            <g key={a.difficulty}>
              <circle
                cx="64" cy="64" r={R} fill="none" strokeWidth="8" strokeLinecap="round"
                stroke="var(--color-line)"
                strokeDasharray={`${a.span} ${C - a.span}`}
                strokeDashoffset={-a.start}
              />
              {/* A round cap paints a dot even at zero length, so an untouched
                  difficulty has to be skipped rather than drawn empty. */}
              {filled > 0 && (
                <circle
                  cx="64" cy="64" r={R} fill="none" strokeWidth="8" strokeLinecap="round"
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
        <span className="font-mono text-[28px] leading-none text-ink">{solved}</span>
        <span className="mt-1.5 text-[12px] text-ink-dim">of {total} solved</span>
      </figcaption>
    </figure>
  );
}
