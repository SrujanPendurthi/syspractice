import { useCallback, useEffect, useRef, useState } from 'react';
import { gradeCard, listCards, loadConcept, Unauthorized } from './api';
import type { Concept, Flashcard } from './types';
import { useAuth } from './useAuth';

interface FlashcardsProps {
  onSignIn: () => void;
}

/** Keys pressed inside these belong to the control, not to the deck. */
const OWNS_KEYS = 'button, a, input, textarea, select, [role=dialog]';

/**
 * One card at a time: the probe on the front, the concept it came from on the back.
 * Signed out you can flip through the whole deck; grading is what writes a schedule, so
 * that is the part that needs an account.
 */
export function Flashcards({ onSignIn }: FlashcardsProps) {
  const { user, loading } = useAuth();
  const [cards, setCards] = useState<Flashcard[] | null>(null);
  const [index, setIndex] = useState(0);
  const [flipped, setFlipped] = useState(false);
  const [tally, setTally] = useState({ got: 0, missed: 0 });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [back, setBack] = useState<Concept | null>(null);

  // Several cards share a concept, so its body is fetched once per visit, not per card.
  const concepts = useRef(new Map<string, Promise<Concept>>());
  // Only the newest deck request may land: a sign-out mid-load must not be overwritten
  // by the signed-in schedule that was already in flight.
  const request = useRef(0);
  // Flip and grade unmount the focused button; this says where focus goes instead.
  const focusTo = useRef<'front' | 'back' | null>(null);
  const frontRef = useRef<HTMLParagraphElement>(null);
  const backRef = useRef<HTMLHeadingElement>(null);

  const load = useCallback(() => {
    const mine = ++request.current;
    setCards(null);
    setIndex(0);
    setFlipped(false);
    setTally({ got: 0, missed: 0 });
    setError(null);
    listCards()
      .then((c) => mine === request.current && setCards(c))
      .catch((e) => mine === request.current && setError(e.message));
  }, []);

  // The deck a signed-in user sees is their schedule, so signing in or out reloads it —
  // but not before auth has resolved, or a cold load fetches the deck twice.
  useEffect(() => {
    if (!loading) load();
  }, [load, loading, user]);

  const card = cards?.[index] ?? null;

  useEffect(() => {
    if (!flipped || !card) return;
    const id = card.conceptId;
    let p = concepts.current.get(id);
    if (!p) {
      p = loadConcept(id);
      concepts.current.set(id, p);
      p.catch(() => concepts.current.delete(id));
    }
    let live = true;
    p.then((c) => live && setBack(c)).catch((e) => live && setError(e.message));
    return () => {
      live = false;
    };
  }, [flipped, card]);

  useEffect(() => {
    const target = focusTo.current === 'back' ? backRef.current : frontRef.current;
    if (focusTo.current && target) target.focus();
    focusTo.current = null;
  });

  const flip = useCallback(() => {
    focusTo.current = 'back';
    setFlipped(true);
  }, []);

  const next = useCallback(() => {
    focusTo.current = 'front';
    setFlipped(false);
    setIndex((i) => i + 1);
  }, []);

  const grade = useCallback(
    async (correct: boolean) => {
      if (!card || busy) return;
      setBusy(true);
      setError(null);
      try {
        await gradeCard(card.id, correct);
        setTally((t) => (correct ? { ...t, got: t.got + 1 } : { ...t, missed: t.missed + 1 }));
        next();
      } catch (e) {
        if (e instanceof Unauthorized) onSignIn();
        else setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [card, busy, next, onSignIn],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!card || e.defaultPrevented || e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
      // A focused control answers its own keys (the flip button flips through its click),
      // and the appearance menu uses the arrows.
      if ((e.target as Element).closest?.(OWNS_KEYS)) return;
      if (!flipped && (e.key === ' ' || e.key === 'Enter')) {
        e.preventDefault();
        flip();
      } else if (flipped && user && e.key === '1') {
        grade(true);
      } else if (flipped && user && e.key === '2') {
        grade(false);
      } else if (!user && e.key === 'ArrowRight') {
        next();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [card, flipped, user, grade, next, flip]);

  const shown = flipped && back?.id === card?.conceptId ? back : null;

  return (
    <div className="mx-auto max-w-3xl px-6 py-8">
      <div className="flex items-baseline gap-3">
        <h1 className="text-xl font-semibold">Flashcards</h1>
        {cards && card && (
          <span className="tnum ml-auto font-mono text-sm text-ink-dim">
            {index + 1}
            <span className="text-ink-faint">/{cards.length}</span>
          </span>
        )}
      </div>
      <p className="mt-1.5 max-w-prose text-base leading-relaxed text-ink-dim">
        The follow-up questions an interviewer asks once you have named a technique. Answer out
        loud, then check yourself against the concept it comes from.
        {!loading && !user && (
          <>
            {' '}
            <button onClick={onSignIn} className="text-accent underline underline-offset-2">
              Sign in
            </button>{' '}
            to grade cards and have the ones you miss come back sooner.
          </>
        )}
      </p>

      <p role="status" className="sr-only">
        {flipped ? 'Answer shown' : ''}
      </p>

      {error && (
        <p className="mt-4 rounded border border-hard/40 bg-hard/5 px-3 py-2 text-sm text-hard-ink">
          {error}
        </p>
      )}

      {!cards && !error && <p className="mt-6 text-sm text-ink-dim">Loading…</p>}

      {cards && !card && (
        <div className="mt-6 border-t border-line pt-5">
          {cards.length === 0 ? (
            <p className="text-base text-ink">Nothing is due. Come back tomorrow.</p>
          ) : (
            <p className="text-base text-ink">
              Done.
              {user && (
                <>
                  {' '}
                  <span className="text-easy-ink">{tally.got} got</span>,{' '}
                  <span className="text-hard-ink">{tally.missed} missed</span>.
                </>
              )}
            </p>
          )}
          <button
            onClick={load}
            className="mt-3 rounded border border-line px-3 py-1.5 text-sm text-ink hover:border-ink-faint"
          >
            {user ? 'Check for more' : 'Start over'}
          </button>
        </div>
      )}

      {card && (
        <div className="mt-6">
          <div className="rounded border border-line bg-surface px-5 py-6">
            <p className="text-sm text-ink-dim">{card.conceptTitle}</p>
            <p
              ref={frontRef}
              tabIndex={-1}
              className="mt-2 text-lg leading-relaxed text-ink outline-none"
            >
              {card.front}
            </p>
          </div>

          <div className="mt-4 flex flex-wrap items-center gap-2">
            {!flipped ? (
              <button
                onClick={flip}
                aria-keyshortcuts="Space"
                className="rounded bg-accent px-3 py-1.5 text-sm font-medium text-accent-ink"
              >
                Show answer <Key>Space</Key>
              </button>
            ) : user ? (
              <>
                <button
                  onClick={() => grade(true)}
                  disabled={busy}
                  aria-keyshortcuts="1"
                  className="rounded bg-accent px-3 py-1.5 text-sm font-medium text-accent-ink disabled:opacity-50"
                >
                  Got it <Key>1</Key>
                </button>
                <button
                  onClick={() => grade(false)}
                  disabled={busy}
                  aria-keyshortcuts="2"
                  className="rounded border border-line px-3 py-1.5 text-sm text-ink hover:border-ink-faint disabled:opacity-50"
                >
                  Missed it <Key>2</Key>
                </button>
              </>
            ) : (
              <button
                onClick={next}
                aria-keyshortcuts="ArrowRight"
                className="rounded bg-accent px-3 py-1.5 text-sm font-medium text-accent-ink"
              >
                Next <Key>→</Key>
              </button>
            )}
          </div>

          {flipped && (
            <div className="mt-6 border-t border-line pt-5">
              <h2 ref={backRef} tabIndex={-1} className="text-lg font-semibold outline-none">
                {card.conceptTitle}
              </h2>
              {!shown ? (
                <p className="mt-2 text-sm text-ink-dim">Loading…</p>
              ) : (
                <>
                  <p className="mt-2 max-w-prose text-base text-ink-dim">{shown.summary}</p>
                  {shown.sections.map((s) => (
                    <section key={s.heading} className="mt-5">
                      <h3 className="text-base font-semibold">{s.heading}</h3>
                      <p className="mt-1 max-w-prose text-base leading-[1.7] text-ink">{s.body}</p>
                    </section>
                  ))}
                </>
              )}
              {/* A new tab, so the deck keeps its place. */}
              <a
                href={`/learn/${encodeURIComponent(card.conceptId)}`}
                target="_blank"
                className="mt-5 inline-block text-sm text-accent underline underline-offset-2"
              >
                Read {card.conceptTitle}
              </a>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function Key({ children }: { children: string }) {
  return (
    <kbd aria-hidden="true" className="ml-1 font-mono text-xs opacity-70">
      {children}
    </kbd>
  );
}
