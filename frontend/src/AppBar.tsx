import { useState } from 'react';
import type { ReactNode } from 'react';
import { useAuth } from './useAuth';
import { useDismissable, useSettings } from './useTheme';
import type { Contrast, Motion, TextSize, Theme } from './useTheme';

/**
 * The one bar, on every page.
 *
 * Its slots are fixed — brand, sections, account, settings, in that order — and
 * a page that does not want a slot gets an empty one rather than a rearranged
 * bar. The workspace drops the section links because it is a full-screen tool
 * and its own sidebar already says where you are; everything else about the bar
 * is identical there. Nothing ever moves, which is the entire point: the bar is
 * the part of the app you are supposed to stop looking at.
 */
interface AppBarProps {
  path: string;
  navigate: (to: string) => void;
  /** False in the workspace: the section links are removed, nothing else changes. */
  sections?: boolean;
}

const SECTIONS = [
  { href: '/', label: 'Problems' },
  { href: '/learn', label: 'Concepts' },
  { href: '/discuss', label: 'Discuss' },
  { href: '/versus', label: '1v1' },
];

/** `/` only matches itself; every other section owns its subtree. */
const isActive = (path: string, href: string) =>
  href === '/' ? path === '/' || path.startsWith('/s/') : path === href || path.startsWith(`${href}/`);

export function AppBar({ path, navigate, sections = true }: AppBarProps) {
  const { user, signOut } = useAuth();

  return (
    <header className="sticky top-0 z-40 shrink-0 border-b border-line bg-page">
      <div className="flex h-12 items-stretch gap-1 px-4">
        <a
          href="/"
          onClick={(e) => {
            e.preventDefault();
            navigate('/');
          }}
          className="flex shrink-0 items-center gap-2 pr-3 text-ink"
        >
          <Mark />
          <span className="font-mono text-base font-semibold tracking-tight">syspractice</span>
        </a>

        <nav className="flex min-w-0 items-stretch gap-0.5 overflow-x-auto" aria-label="Sections">
          {sections &&
            SECTIONS.map((s) => {
              const active = isActive(path, s.href);
              return (
                <a
                  key={s.href}
                  href={s.href}
                  onClick={(e) => {
                    e.preventDefault();
                    navigate(s.href);
                  }}
                  aria-current={active ? 'page' : undefined}
                  className={`relative flex shrink-0 items-center px-3 text-base transition-colors ${
                    active ? 'text-ink' : 'text-ink-dim hover:text-ink'
                  }`}
                >
                  {s.label}
                  {/* The only place the bar shows state. A 2px rule sitting on the
                      bar's own bottom border, so the indicator is part of the
                      chrome rather than a pill floating inside it. */}
                  {active && (
                    <span className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-accent" />
                  )}
                </a>
              );
            })}
        </nav>

        <div className="ml-auto flex shrink-0 items-center gap-1 pl-3">
          {user ? (
            <>
              <span className="hidden text-sm text-ink-dim sm:inline">{user.displayName}</span>
              <button
                onClick={() => void signOut()}
                className="rounded px-2 py-1 text-sm text-ink-dim hover:bg-surface hover:text-ink"
              >
                Sign out
              </button>
            </>
          ) : (
            <button
              onClick={() => navigate('/login')}
              className="rounded px-2 py-1 text-sm font-medium text-accent hover:bg-accent-soft"
            >
              Sign in
            </button>
          )}
          <SettingsMenu />
        </div>
      </div>
    </header>
  );
}

/**
 * Three nodes and two edges. The whole app routes through a serialized topology,
 * so the mark is the thing the product is actually about rather than a glyph
 * chosen for looking technical.
 */
function Mark() {
  return (
    <svg viewBox="0 0 20 20" className="h-[18px] w-[18px] text-accent" aria-hidden="true">
      <path
        d="M4.5 5.5h11M15.5 5.5 9.5 14.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        opacity="0.55"
      />
      <circle cx="4.5" cy="5.5" r="2.6" fill="currentColor" />
      <circle cx="15.5" cy="5.5" r="2.6" fill="currentColor" />
      <circle cx="9.5" cy="14.5" r="2.6" fill="currentColor" />
    </svg>
  );
}

function SettingsMenu() {
  const [open, setOpen] = useState(false);
  const s = useSettings();
  useDismissable(open, () => setOpen(false));

  return (
    // The click that opens this also reaches the document listener that closes
    // it, so the whole control stops propagation rather than the listener
    // trying to work out which click was which.
    <div className="relative" onClick={(e) => e.stopPropagation()}>
      <button
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label="Appearance and accessibility"
        className={`flex h-8 w-8 items-center justify-center rounded hover:bg-surface ${
          open ? 'bg-surface text-ink' : 'text-ink-dim'
        }`}
      >
        {/* Half-filled circle: the universal contrast glyph, and this menu is
            mostly about contrast. */}
        <svg viewBox="0 0 20 20" className="h-[18px] w-[18px]" aria-hidden="true">
          <circle cx="10" cy="10" r="7.25" fill="none" stroke="currentColor" strokeWidth="1.5" />
          <path d="M10 2.75a7.25 7.25 0 0 1 0 14.5z" fill="currentColor" />
        </svg>
      </button>

      {open && (
        <div
          role="dialog"
          aria-label="Appearance and accessibility"
          className="absolute right-0 top-10 z-50 w-72 rounded-lg border border-line bg-raised p-3 shadow-lg"
        >
          <Choice
            label="Appearance"
            value={s.theme}
            onChange={s.setTheme}
            options={
              [
                ['system', 'System'],
                ['light', 'Light'],
                ['dark', 'Dark'],
              ] as [Theme, string][]
            }
          />
          <Choice
            label="Contrast"
            value={s.contrast}
            onChange={s.setContrast}
            options={
              [
                ['system', 'System'],
                ['normal', 'Normal'],
                ['high', 'High'],
              ] as [Contrast, string][]
            }
          />
          <Choice
            label="Motion"
            value={s.motion}
            onChange={s.setMotion}
            options={
              [
                ['system', 'System'],
                ['full', 'Full'],
                ['reduced', 'Reduced'],
              ] as [Motion, string][]
            }
          />
          <Choice
            label="Text size"
            value={s.text}
            onChange={s.setText}
            options={
              [
                ['normal', 'Normal'],
                ['large', 'Large'],
              ] as [TextSize, string][]
            }
          />
          <p className="mt-3 border-t border-line pt-2.5 text-xs leading-relaxed text-ink-dim">
            Saved in this browser. System follows your device, currently {s.resolvedTheme}.
          </p>
        </div>
      )}
    </div>
  );
}

/** A segmented control. Exclusive choice, so it is a radiogroup and not a set of toggles. */
function Choice<T extends string>({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: T;
  onChange: (v: T) => void;
  options: [T, string][];
}): ReactNode {
  return (
    <div className="mb-3 last:mb-0">
      <p className="mb-1.5 text-sm font-medium text-ink">{label}</p>
      <div role="radiogroup" aria-label={label} className="flex gap-1">
        {options.map(([v, text]) => {
          const on = v === value;
          return (
            <button
              key={v}
              role="radio"
              aria-checked={on}
              onClick={() => onChange(v)}
              className={`flex-1 rounded border px-2 py-1 text-xs ${
                on
                  ? 'border-accent-line bg-accent-soft font-medium text-accent'
                  : 'border-line text-ink-dim hover:border-line-strong hover:text-ink'
              }`}
            >
              {text}
            </button>
          );
        })}
      </div>
    </div>
  );
}
