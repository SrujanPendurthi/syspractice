import { useEffect, useState } from 'react';
import { listConcepts, listReads, listScenarios, loadConcept, markRead, Unauthorized } from './api';
import type { Concept, ConceptCard, Scenario } from './types';
import { useAuth } from './useAuth';

interface LearnProps {
  /** Null on the index, a concept id on an article. */
  conceptId: string | null;
  onOpen: (id: string) => void;
  onBack: () => void;
  onOpenScenario: (id: string) => void;
  onSignIn: () => void;
}

export function Learn({ conceptId, onOpen, onBack, onOpenScenario, onSignIn }: LearnProps) {
  const { user } = useAuth();
  const [cards, setCards] = useState<ConceptCard[]>([]);
  const [reads, setReads] = useState<string[]>([]);
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listConcepts().then(setCards).catch((e) => setError(e.message));
    listScenarios().then(setScenarios).catch(() => undefined);
  }, []);

  // Read marks belong to an account, so signing in or out refetches rather than leaving
  // the previous answer on screen.
  useEffect(() => {
    listReads().then(setReads).catch(() => setReads([]));
  }, [user]);

  const toggleRead = async (id: string, read: boolean) => {
    // Optimistic: the mark is a checkbox, and waiting a round trip to tick it feels broken.
    setReads((prev) => (read ? [...prev, id] : prev.filter((r) => r !== id)));
    try {
      await markRead(id, read);
    } catch (e) {
      setReads((prev) => (read ? prev.filter((r) => r !== id) : [...prev, id]));
      if (e instanceof Unauthorized) onSignIn();
    }
  };

  const title = (id: string) => scenarios.find((s) => s.id === id)?.title ?? id;

  return (
    <div className="min-h-full bg-page">
      <header className="border-b border-line">
        <div className="mx-auto flex max-w-3xl items-baseline gap-3 px-6 py-4">
          <button onClick={onBack} className="font-mono text-[15px] font-medium tracking-tight">
            syspractice
          </button>
          <span className="text-[13px] text-ink-dim">Concepts</span>
          {user && (
            <span className="ml-auto font-mono text-[13px] text-ink-dim">
              {reads.length}
              <span className="text-ink-faint">/{cards.length} read</span>
            </span>
          )}
        </div>
      </header>

      <main className="mx-auto max-w-3xl px-6 py-8">
        {error && (
          <p className="mb-4 rounded border border-hard/40 bg-hard/5 px-3 py-2 text-[13px] text-hard-ink">
            Could not reach the backend: {error}. Start it with ./dev and reload.
          </p>
        )}

        {conceptId ? (
          <Article
            conceptId={conceptId}
            read={reads.includes(conceptId)}
            signedIn={!!user}
            onToggleRead={toggleRead}
            onSignIn={onSignIn}
            onOpenScenario={onOpenScenario}
            scenarioTitle={title}
            onBack={() => onOpen('')}
          />
        ) : (
          <Index
            cards={cards}
            reads={reads}
            signedIn={!!user}
            onOpen={onOpen}
            onSignIn={onSignIn}
          />
        )}
      </main>
    </div>
  );
}

function Index({
  cards,
  reads,
  signedIn,
  onOpen,
  onSignIn,
}: {
  cards: ConceptCard[];
  reads: string[];
  signedIn: boolean;
  onOpen: (id: string) => void;
  onSignIn: () => void;
}) {
  return (
    <>
      <h1 className="text-[20px] font-semibold">What the critic is grading you on</h1>
      <p className="mt-1.5 max-w-prose text-[14px] leading-relaxed text-ink-dim">
        These are the ideas the rubrics keep asking for, written as the follow-up questions
        an interviewer actually asks. Each one names the problems where it comes up.
        {!signedIn && (
          <>
            {' '}
            <button onClick={onSignIn} className="text-easy-ink underline underline-offset-2">
              Sign in
            </button>{' '}
            to keep track of what you have read.
          </>
        )}
      </p>

      <ol className="mt-6 border-t border-line">
        {cards.map((c) => (
          <li key={c.id} className="border-b border-line">
            <button
              onClick={() => onOpen(c.id)}
              className="flex w-full items-baseline gap-3 py-3 text-left hover:bg-surface"
            >
              <span className="w-4 shrink-0 text-center text-[13px]">
                {reads.includes(c.id) ? (
                  <span className="text-easy-ink" title="Read">
                    ✓
                  </span>
                ) : (
                  <span className="text-ink-faint/60">○</span>
                )}
              </span>
              <span className="min-w-0">
                <span className="text-[14px] text-ink">{c.title}</span>
                <span className="mt-0.5 block text-[13px] leading-relaxed text-ink-dim">
                  {c.summary}
                </span>
              </span>
            </button>
          </li>
        ))}
      </ol>
    </>
  );
}

function Article({
  conceptId,
  read,
  signedIn,
  onToggleRead,
  onSignIn,
  onOpenScenario,
  scenarioTitle,
  onBack,
}: {
  conceptId: string;
  read: boolean;
  signedIn: boolean;
  onToggleRead: (id: string, read: boolean) => void;
  onSignIn: () => void;
  onOpenScenario: (id: string) => void;
  scenarioTitle: (id: string) => string;
  onBack: () => void;
}) {
  const [concept, setConcept] = useState<Concept | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setConcept(null);
    setError(null);
    loadConcept(conceptId)
      .then(setConcept)
      .catch((e) => setError(e.message));
  }, [conceptId]);

  if (error) return <p className="text-[13px] text-hard-ink">{error}</p>;
  if (!concept) return <p className="text-[13px] text-ink-dim">Loading…</p>;

  return (
    <article>
      <button onClick={onBack} className="text-[12px] text-ink-dim hover:text-ink">
        All concepts
      </button>

      <h1 className="mt-2 text-[22px] font-semibold">{concept.title}</h1>
      <p className="mt-1 max-w-prose text-[14px] text-ink-dim">{concept.summary}</p>

      {concept.sections.map((s) => (
        <section key={s.heading} className="mt-6">
          <h2 className="text-[15px] font-semibold">{s.heading}</h2>
          {/* max-w-prose keeps lines under about 70 characters; long-form body text at the
              full width of this column is genuinely harder to read. */}
          <p className="mt-1.5 max-w-prose text-[14px] leading-[1.7] text-ink">{s.body}</p>
        </section>
      ))}

      <section className="mt-8 border-t border-line pt-5">
        <h2 className="text-[15px] font-semibold">What you get asked next</h2>
        <ul className="mt-2 max-w-prose space-y-1.5">
          {concept.probes.map((p) => (
            <li key={p} className="text-[14px] leading-relaxed text-ink">
              {p}
            </li>
          ))}
        </ul>
      </section>

      {concept.scenarios.length > 0 && (
        <section className="mt-7">
          <h2 className="text-[15px] font-semibold">Practise it</h2>
          <div className="mt-2 flex flex-wrap gap-2">
            {concept.scenarios.map((id) => (
              <button
                key={id}
                onClick={() => onOpenScenario(id)}
                className="rounded border border-line px-2.5 py-1 text-[13px] text-ink hover:border-ink-faint"
              >
                {scenarioTitle(id)}
              </button>
            ))}
          </div>
        </section>
      )}

      <div className="mt-8 border-t border-line pt-4">
        {signedIn ? (
          <label className="flex items-center gap-2 text-[13px] text-ink-dim">
            <input
              type="checkbox"
              checked={read}
              onChange={(e) => onToggleRead(concept.id, e.target.checked)}
              className="h-3.5 w-3.5 accent-easy"
            />
            Mark as read
          </label>
        ) : (
          <p className="text-[13px] text-ink-dim">
            <button onClick={onSignIn} className="text-easy-ink underline underline-offset-2">
              Sign in
            </button>{' '}
            to keep track of what you have read.
          </p>
        )}
      </div>
    </article>
  );
}
