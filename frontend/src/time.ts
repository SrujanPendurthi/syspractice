/**
 * SQLite writes `datetime('now')` as `YYYY-MM-DD HH:MM:SS` with no zone marker, and it is
 * UTC. `new Date` reads a string in that shape as *local* time, so every timestamp the
 * backend sends is silently wrong by the reader's offset — which looks like a clock bug
 * rather than a parsing one, and on a countdown it is the difference between having eight
 * minutes left and having finished five hours ago.
 */
export const parseSqlTime = (sql: string): number =>
  new Date(`${sql.replace(' ', 'T')}Z`).getTime();

export function ago(sql: string): string {
  const mins = Math.round((Date.now() - parseSqlTime(sql)) / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  if (mins < 60 * 24) return `${Math.round(mins / 60)}h ago`;
  return `${Math.round(mins / 1440)}d ago`;
}

/** `m:ss` remaining, floored at zero. */
export function countdown(msLeft: number): string {
  const total = Math.max(0, Math.floor(msLeft / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}
