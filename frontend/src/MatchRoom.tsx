import { useEffect, useState } from 'react';
import { loadMatch } from './api';
import type { Match } from './types';
import { Workspace } from './Workspace';

/**
 * Resolves a match id to the problem it is about, then hands off to the ordinary
 * workspace. The canvas, the palette and the scenario sidebar are the same ones as
 * practice — a race is still practice, and a second canvas would be a second thing to
 * keep in step with the first.
 */
export function MatchRoom({ matchId, onBack }: { matchId: number; onBack: () => void }) {
  const [match, setMatch] = useState<Match | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadMatch(matchId).then(setMatch).catch((e) => setError(e.message));
  }, [matchId]);

  if (error) {
    return (
      <div className="p-8">
        <p className="text-sm text-hard-ink">{error}</p>
        <button onClick={onBack} className="mt-2 text-sm text-ink-dim hover:text-ink">
          All matches
        </button>
      </div>
    );
  }
  if (!match) return <p className="p-8 text-sm text-ink-dim">Loading match…</p>;

  return (
    <Workspace
      scenarioId={match.scenarioId}
      matchId={matchId}
      // Fixed by the match: both designs have to be graded against the same rubric, so
      // the level is not the player's to change here.
      level={match.level}
      onBack={onBack}
    />
  );
}
