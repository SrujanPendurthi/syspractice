import { useState } from "react";
import { useAuth } from "./useAuth";

interface AuthPageProps {
  onDone: () => void;
  /**
   * Only passed when this is the sign-in overlay in the workspace. On its own route the
   * app bar is still above it, so there is nothing for a second back control to do.
   */
  onBack?: () => void;
}

export function AuthPage({ onDone, onBack }: AuthPageProps) {
  const { signIn, signUp } = useAuth();
  const [mode, setMode] = useState<"in" | "up">("in");
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const registering = mode === "up";

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (registering) await signUp(email, displayName, password);
      else await signIn(email, password);
      onDone();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const field =
    "w-full rounded border border-line bg-page px-3 py-2 text-base text-ink placeholder:text-ink-faint";

  return (
    <div className="mx-auto max-w-sm px-6 py-14">
      {onBack && (
        <button
          onClick={onBack}
          className="mb-6 text-sm text-ink-dim hover:text-ink"
        >
          Back to the canvas
        </button>
      )}
      <h1 className="text-xl font-semibold">
        {registering ? "Create an account" : "Sign in"}
      </h1>
      <p className="mt-1.5 text-sm leading-relaxed text-ink-dim">
        {registering
          ? "An account saves your attempts and tracks which problems you have worked."
          : "Your saved attempts and progress are waiting."}
      </p>

      <form onSubmit={submit} className="mt-6 space-y-3">
        <label className="block">
          <span className="mb-1 block text-sm text-ink-dim">Email</span>
          <input
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
            autoComplete="email"
            className={field}
          />
        </label>

        {registering && (
          <label className="block">
            <span className="mb-1 block text-sm text-ink-dim">
              Display name
            </span>
            <input
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              required
              maxLength={40}
              autoComplete="nickname"
              className={field}
            />
          </label>
        )}

        <label className="block">
          <span className="mb-1 block text-sm text-ink-dim">Password</span>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            // Stated up front rather than after a rejected submission. The browser
            // enforces the same number the server does.
            minLength={registering ? 10 : undefined}
            autoComplete={registering ? "new-password" : "current-password"}
            className={field}
          />
          {registering && (
            <span className="mt-1 block text-sm text-ink-dim">
              At least 10 characters. A phrase you can remember beats a short
              scramble.
            </span>
          )}
        </label>

        {error && (
          <p className="rounded border border-hard/30 bg-hard/5 px-3 py-2 text-sm text-hard-ink">
            {error}
          </p>
        )}

        <button
          type="submit"
          disabled={busy}
          className="w-full rounded bg-accent px-3 py-2 text-base font-medium text-accent-ink hover:brightness-110 disabled:opacity-60"
        >
          {busy ? "Working…" : registering ? "Create account" : "Sign in"}
        </button>
      </form>

      <p className="mt-5 text-sm text-ink-dim">
        {registering ? "Already have an account?" : "New here?"}{" "}
        <button
          onClick={() => {
            setMode(registering ? "in" : "up");
            setError(null);
          }}
          className="text-accent underline underline-offset-2"
        >
          {registering ? "Sign in" : "Create one"}
        </button>
      </p>
    </div>
  );
}
