import { useCallback, useEffect, useState } from 'react';
import {
  createComment,
  createPost,
  deleteComment,
  deletePost,
  listPosts,
  listScenarios,
  loadPost,
  Unauthorized,
  votePost,
} from './api';
import { ago } from './time';
import type { Post, PostComment, PostSummary, Scenario } from './types';
import { useAuth } from './useAuth';

interface DiscussProps {
  /** Null on the index, a post id on a thread. */
  postId: number | null;
  onOpen: (id: number | null) => void;
  onBack: () => void;
  onOpenScenario: (id: string) => void;
  onSignIn: () => void;
}

export function Discuss({ postId, onOpen, onBack, onOpenScenario, onSignIn }: DiscussProps) {
  const { user } = useAuth();
  const [scenarios, setScenarios] = useState<Scenario[]>([]);

  // The workspace links here as `/discuss?scenario=<id>` in a new tab, so the filter has
  // to survive a cold load. `useRoute` tracks the path only, which is why this reads the
  // query string itself rather than being handed one.
  const [filter, setFilter] = useState(
    () => new URLSearchParams(window.location.search).get('scenario') ?? '',
  );

  useEffect(() => {
    listScenarios().then(setScenarios).catch(() => undefined);
  }, []);

  const title = useCallback(
    (id: string) => scenarios.find((s) => s.id === id)?.title ?? id,
    [scenarios],
  );

  const changeFilter = (next: string) => {
    setFilter(next);
    // replaceState rather than navigate(): the filter should be in a shareable URL, but
    // routing through `useRoute` would set its path to one carrying a query string and
    // stop `/discuss` matching.
    window.history.replaceState(null, '', next ? `/discuss?scenario=${next}` : '/discuss');
  };

  return (
    <div className="min-h-full bg-page">
      <header className="border-b border-line">
        <div className="mx-auto flex max-w-3xl items-baseline gap-3 px-6 py-4">
          <button onClick={onBack} className="font-mono text-[15px] font-medium tracking-tight">
            syspractice
          </button>
          <span className="text-[13px] text-ink-dim">Discussion</span>
          {user && <span className="ml-auto text-[13px] text-ink-dim">{user.displayName}</span>}
        </div>
      </header>

      <main className="mx-auto max-w-3xl px-6 py-8">
        {postId === null ? (
          <Index
            filter={filter}
            scenarios={scenarios}
            scenarioTitle={title}
            signedIn={!!user}
            onFilter={changeFilter}
            onOpen={onOpen}
            onSignIn={onSignIn}
          />
        ) : (
          <Thread
            postId={postId}
            scenarioTitle={title}
            onBack={() => onOpen(null)}
            onOpenScenario={onOpenScenario}
            onSignIn={onSignIn}
          />
        )}
      </main>
    </div>
  );
}

function Index({
  filter,
  scenarios,
  scenarioTitle,
  signedIn,
  onFilter,
  onOpen,
  onSignIn,
}: {
  filter: string;
  scenarios: Scenario[];
  scenarioTitle: (id: string) => string;
  signedIn: boolean;
  onFilter: (id: string) => void;
  onOpen: (id: number) => void;
  onSignIn: () => void;
}) {
  const [posts, setPosts] = useState<PostSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);

  const refresh = useCallback(() => {
    listPosts(filter || undefined)
      .then((p) => {
        setPosts(p);
        setError(null);
      })
      .catch((e) => setError(e.message));
  }, [filter]);

  useEffect(refresh, [refresh]);

  return (
    <>
      <div className="flex flex-wrap items-baseline gap-3">
        <h1 className="text-[20px] font-semibold">Discussion</h1>
        <select
          value={filter}
          onChange={(e) => onFilter(e.target.value)}
          className="rounded border border-line bg-page px-2 py-1 text-[13px] text-ink-dim"
          aria-label="Filter by problem"
        >
          <option value="">All problems</option>
          {scenarios.map((s) => (
            <option key={s.id} value={s.id}>
              {s.title}
            </option>
          ))}
        </select>

        {signedIn ? (
          <button
            onClick={() => setComposing((c) => !c)}
            className="ml-auto rounded border border-line px-2.5 py-1 text-[13px] hover:border-ink-faint"
          >
            {composing ? 'Cancel' : 'New post'}
          </button>
        ) : (
          <button onClick={onSignIn} className="ml-auto text-[13px] text-easy-ink">
            Sign in to post
          </button>
        )}
      </div>

      {composing && (
        <Composer
          scenarios={scenarios}
          initialScenario={filter}
          onSignIn={onSignIn}
          onPosted={(id) => {
            setComposing(false);
            onOpen(id);
          }}
        />
      )}

      {error && (
        <p className="mt-4 rounded border border-hard/40 bg-hard/5 px-3 py-2 text-[13px] text-hard-ink">
          {error}
        </p>
      )}

      {posts.length === 0 && !error && (
        <p className="mt-6 text-[14px] text-ink-dim">
          Nothing here yet.{' '}
          {filter ? 'No threads on this problem — start one.' : 'Start the first thread.'}
        </p>
      )}

      <ol className="mt-6 border-t border-line">
        {posts.map((p) => (
          <li key={p.id} className="flex items-start gap-3 border-b border-line py-3">
            <Vote
              postId={p.id}
              score={p.score}
              myVote={p.myVote}
              signedIn={signedIn}
              onSignIn={onSignIn}
            />
            <div className="min-w-0 flex-1">
              <button onClick={() => onOpen(p.id)} className="block text-left text-[14px] text-ink hover:underline">
                {p.title}
              </button>
              <p className="mt-0.5 text-[12px] text-ink-dim">
                {p.author} · {ago(p.createdAt)} · {p.comments}{' '}
                {p.comments === 1 ? 'comment' : 'comments'}
                {p.scenarioId && (
                  <>
                    {' · '}
                    <span className="text-easy-ink">{scenarioTitle(p.scenarioId)}</span>
                  </>
                )}
              </p>
            </div>
          </li>
        ))}
      </ol>
    </>
  );
}

const MAX_TITLE = 200;
const MAX_POST = 10_000;
const MAX_COMMENT = 4_000;

function Composer({
  scenarios,
  initialScenario,
  onPosted,
  onSignIn,
}: {
  scenarios: Scenario[];
  initialScenario: string;
  onPosted: (id: number) => void;
  onSignIn: () => void;
}) {
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [scenario, setScenario] = useState(initialScenario);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const { id } = await createPost(title, body, scenario || null);
      onPosted(id);
    } catch (err) {
      setError((err as Error).message);
      // A 401 here means the cookie expired while the form was open; sending the user to
      // sign in beats showing them "sign in to continue" over a draft they cannot submit.
      if (err instanceof Unauthorized) onSignIn();
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="mt-4 rounded border border-line p-4">
      <input
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        maxLength={MAX_TITLE}
        required
        placeholder="Title"
        className="w-full border-b border-line bg-transparent pb-2 text-[15px] outline-none"
      />
      <textarea
        value={body}
        onChange={(e) => setBody(e.target.value)}
        maxLength={MAX_POST}
        required
        rows={6}
        placeholder="What did you run into?"
        className="mt-3 w-full resize-y bg-transparent text-[14px] leading-relaxed outline-none"
      />
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <select
          value={scenario}
          onChange={(e) => setScenario(e.target.value)}
          className="rounded border border-line bg-page px-2 py-1 text-[13px] text-ink-dim"
          aria-label="Tag a problem"
        >
          <option value="">No problem tag</option>
          {scenarios.map((s) => (
            <option key={s.id} value={s.id}>
              {s.title}
            </option>
          ))}
        </select>
        <button
          type="submit"
          disabled={busy}
          className="ml-auto rounded bg-ink px-3 py-1.5 text-[13px] text-page disabled:opacity-50"
        >
          {busy ? 'Posting…' : 'Post'}
        </button>
      </div>
      {error && <p className="mt-2 text-[13px] text-hard-ink">{error}</p>}
    </form>
  );
}

/**
 * Optimistic, and it withdraws on a second click of the same arrow. The server returns
 * the recomputed score, so a stale local guess is corrected by the answer rather than
 * left to drift.
 */
function Vote({
  postId,
  score,
  myVote,
  signedIn,
  onSignIn,
}: {
  postId: number;
  score: number;
  myVote: number;
  signedIn: boolean;
  onSignIn: () => void;
}) {
  const [state, setState] = useState({ score, myVote });

  // A refetched list carries new numbers; without this the component keeps the first
  // ones it ever saw.
  useEffect(() => setState({ score, myVote }), [score, myVote]);

  const cast = async (value: number) => {
    if (!signedIn) return onSignIn();
    const next = state.myVote === value ? 0 : value;
    const before = state;
    setState({ score: state.score - state.myVote + next, myVote: next });
    try {
      const res = await votePost(postId, next);
      setState({ score: res.score, myVote: res.myVote });
    } catch {
      setState(before);
    }
  };

  const arrow = (value: number, glyph: string, label: string) => (
    <button
      onClick={() => void cast(value)}
      aria-label={label}
      aria-pressed={state.myVote === value}
      className={`leading-none ${state.myVote === value ? 'text-easy-ink' : 'text-ink-faint hover:text-ink-dim'}`}
    >
      {glyph}
    </button>
  );

  return (
    <div className="flex w-8 shrink-0 flex-col items-center gap-0.5 pt-0.5 text-[13px]">
      {arrow(1, '▲', 'Upvote')}
      <span className="font-mono text-[12px] text-ink-dim">{state.score}</span>
      {arrow(-1, '▼', 'Downvote')}
    </div>
  );
}

function Thread({
  postId,
  scenarioTitle,
  onBack,
  onOpenScenario,
  onSignIn,
}: {
  postId: number;
  scenarioTitle: (id: string) => string;
  onBack: () => void;
  onOpenScenario: (id: string) => void;
  onSignIn: () => void;
}) {
  const { user } = useAuth();
  const [post, setPost] = useState<Post | null>(null);
  const [comments, setComments] = useState<PostComment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    loadPost(postId)
      .then(({ post, comments }) => {
        setPost(post);
        setComments(comments);
      })
      .catch((e) => setError(e.message));
  }, [postId]);

  useEffect(refresh, [refresh]);

  // Author or moderator. The server decides the same thing in SQL — this only controls
  // whether the button is worth showing.
  const canRemove = (authorId: number) => !!user && (user.id === authorId || user.isAdmin);

  const remove = async (run: () => Promise<unknown>, after: () => void) => {
    try {
      await run();
      after();
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const comment = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await createComment(postId, draft);
      setDraft('');
      refresh();
    } catch (err) {
      setError((err as Error).message);
      if (err instanceof Unauthorized) onSignIn();
    } finally {
      setBusy(false);
    }
  };

  if (error && !post) return <p className="text-[13px] text-hard-ink">{error}</p>;
  if (!post) return <p className="text-[13px] text-ink-dim">Loading…</p>;

  return (
    <article>
      <button onClick={onBack} className="text-[12px] text-ink-dim hover:text-ink">
        All discussion
      </button>

      <div className="mt-2 flex items-start gap-3">
        <Vote
          postId={post.id}
          score={post.score}
          myVote={post.myVote}
          signedIn={!!user}
          onSignIn={onSignIn}
        />
        <div className="min-w-0 flex-1">
          <h1 className="text-[20px] font-semibold">{post.title}</h1>
          <p className="mt-0.5 text-[12px] text-ink-dim">
            {post.author} · {ago(post.createdAt)}
            {post.scenarioId && (
              <>
                {' · '}
                <button
                  onClick={() => onOpenScenario(post.scenarioId as string)}
                  className="text-easy-ink underline underline-offset-2"
                >
                  {scenarioTitle(post.scenarioId)}
                </button>
              </>
            )}
            {canRemove(post.authorId) && (
              <>
                {' · '}
                <button
                  onClick={() => void remove(() => deletePost(post.id), onBack)}
                  className="text-hard-ink"
                >
                  Delete
                </button>
              </>
            )}
          </p>
          {/* Rendered as text, never as markup. The body is user input, and a renderer
              here would buy a dependency and an injection surface at once — the same
              reasoning that keeps the concept articles structured data. */}
          <p className="mt-4 whitespace-pre-wrap text-[14px] leading-[1.7] text-ink">{post.body}</p>
        </div>
      </div>

      <section className="mt-8 border-t border-line pt-5">
        <h2 className="text-[15px] font-semibold">
          {comments.length} {comments.length === 1 ? 'comment' : 'comments'}
        </h2>

        {user ? (
          <form onSubmit={comment} className="mt-3">
            <textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              maxLength={MAX_COMMENT}
              required
              rows={3}
              placeholder="Add a comment"
              className="w-full resize-y rounded border border-line bg-transparent p-2.5 text-[14px] leading-relaxed outline-none focus:border-ink-faint"
            />
            <button
              type="submit"
              disabled={busy}
              className="mt-2 rounded bg-ink px-3 py-1.5 text-[13px] text-page disabled:opacity-50"
            >
              {busy ? 'Posting…' : 'Comment'}
            </button>
          </form>
        ) : (
          <p className="mt-3 text-[13px] text-ink-dim">
            <button onClick={onSignIn} className="text-easy-ink underline underline-offset-2">
              Sign in
            </button>{' '}
            to join the thread.
          </p>
        )}

        {error && <p className="mt-3 text-[13px] text-hard-ink">{error}</p>}

        <ul className="mt-6 space-y-5">
          {comments.map((c) => (
            <li key={c.id} className="border-l-2 border-line pl-3">
              <p className="text-[12px] text-ink-dim">
                {c.author} · {ago(c.createdAt)}
                {canRemove(c.authorId) && (
                  <>
                    {' · '}
                    <button
                      onClick={() => void remove(() => deleteComment(c.id), refresh)}
                      className="text-hard-ink"
                    >
                      Delete
                    </button>
                  </>
                )}
              </p>
              <p className="mt-1 whitespace-pre-wrap text-[14px] leading-relaxed text-ink">
                {c.body}
              </p>
            </li>
          ))}
        </ul>
      </section>
    </article>
  );
}
