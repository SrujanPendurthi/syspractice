import { useEffect, useState } from 'react';
import { listProgress, listScenarios } from './api';
import type { Difficulty, Scenario, ScenarioProgress } from './types';

const DIFFICULTY_STYLE: Record<Difficulty, string> = {
  easy: 'text-emerald-400',
  medium: 'text-amber-400',
  hard: 'text-rose-400',
};

interface ProblemListProps {
  onOpen: (scenarioId: string) => void;
}

export function ProblemList({ onOpen }: ProblemListProps) {
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [progress, setProgress] = useState<ScenarioProgress[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listScenarios().then(setScenarios).catch((e) => setError(e.message));
    // Progress is best-effort: an empty table is indistinguishable from never having
    // attempted anything, which is exactly what a first run should look like.
    listProgress().then(setProgress).catch(() => undefined);
  }, []);

  const solved = progress.filter((p) => p.solved).length;
  const attempted = progress.length - solved;

  return (
    <div className="h-screen w-screen overflow-y-auto bg-slate-950 text-slate-100">
      <div className="mx-auto max-w-3xl px-6 py-10">
        <h1 className="text-lg font-semibold">syspractice</h1>
        <p className="mt-1 text-xs text-slate-500">
          {solved} solved · {attempted} attempted · {scenarios.length} total
        </p>

        {error && (
          <p className="mt-4 rounded border border-red-800 bg-red-950 px-3 py-2 text-xs text-red-300">
            {error}
          </p>
        )}

        <table className="mt-6 w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-slate-800 text-left text-[11px] uppercase tracking-wide text-slate-500">
              <th className="w-10 py-2 font-medium">•</th>
              <th className="py-2 font-medium">Problem</th>
              <th className="w-24 py-2 font-medium">Difficulty</th>
              <th className="w-28 py-2 text-right font-medium">Attempts</th>
            </tr>
          </thead>
          <tbody>
            {scenarios.map((s) => {
              const p = progress.find((x) => x.scenarioId === s.id);
              return (
                <tr
                  key={s.id}
                  onClick={() => onOpen(s.id)}
                  className="cursor-pointer border-b border-slate-900 hover:bg-slate-900"
                >
                  <td className="py-2.5 text-center text-sm">
                    {p?.solved ? (
                      <span className="text-emerald-400" title="Walked all five stages">
                        ✓
                      </span>
                    ) : p ? (
                      <span className="text-amber-400" title="Attempted">
                        ◑
                      </span>
                    ) : (
                      <span className="text-slate-700">○</span>
                    )}
                  </td>
                  <td className="py-2.5 text-slate-200">{s.title}</td>
                  <td className={`py-2.5 text-xs capitalize ${DIFFICULTY_STYLE[s.difficulty]}`}>
                    {s.difficulty}
                  </td>
                  <td className="py-2.5 text-right text-xs text-slate-500">
                    {p ? p.attempts : '—'}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        <p className="mt-6 text-[11px] leading-relaxed text-slate-600">
          Solved means you walked all five interview stages. It tracks that you worked the
          problem end to end, not that the design was any good.
        </p>
      </div>
    </div>
  );
}
