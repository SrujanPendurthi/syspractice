import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';

/**
 * Appearance and accessibility settings.
 *
 * Four independent axes rather than one list of themes. Contrast, motion and text
 * size are not variations of light and dark — someone can want high contrast in
 * dark mode, or larger text in light mode, and folding them into a single
 * dropdown would force them to choose. The OS already models these separately;
 * so does this.
 *
 * Each defaults to `system`, which follows the matching media query live. Picking
 * an explicit value is a deliberate override of the machine, and it sticks.
 */
export type Theme = 'system' | 'light' | 'dark';
export type Contrast = 'system' | 'normal' | 'high';
export type Motion = 'system' | 'full' | 'reduced';
export type TextSize = 'normal' | 'large';

interface Settings {
  theme: Theme;
  contrast: Contrast;
  motion: Motion;
  text: TextSize;
  /** What `theme: 'system'` currently resolves to. The menu shows this, not the word "system". */
  resolvedTheme: 'light' | 'dark';
  setTheme: (t: Theme) => void;
  setContrast: (c: Contrast) => void;
  setMotion: (m: Motion) => void;
  setText: (t: TextSize) => void;
}

const SettingsContext = createContext<Settings | null>(null);

const KEY = (name: string) => `syspractice.${name}`;

/**
 * Storage throws outright in some locked-down browsers, and a setting that cannot
 * be saved is a smaller problem than a page that will not render.
 */
function load<T extends string>(name: string, fallback: T): T {
  try {
    return (localStorage.getItem(KEY(name)) as T) || fallback;
  } catch {
    return fallback;
  }
}

function save(name: string, value: string) {
  try {
    localStorage.setItem(KEY(name), value);
  } catch {
    // Nothing to do, and nothing worth telling the user about.
  }
}

/** Subscribes to a media query and re-renders when it changes. */
function useMedia(query: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const onChange = () => setMatches(mq.matches);
    mq.addEventListener('change', onChange);
    // The query can have changed between the initial read and this effect.
    onChange();
    return () => mq.removeEventListener('change', onChange);
  }, [query]);
  return matches;
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => load('theme', 'system'));
  const [contrast, setContrastState] = useState<Contrast>(() => load('contrast', 'system'));
  const [motion, setMotionState] = useState<Motion>(() => load('motion', 'system'));
  const [text, setTextState] = useState<TextSize>(() => load('text', 'normal'));

  const systemDark = useMedia('(prefers-color-scheme: dark)');
  const systemContrast = useMedia('(prefers-contrast: more)');
  const systemReduced = useMedia('(prefers-reduced-motion: reduce)');

  const resolvedTheme = theme === 'system' ? (systemDark ? 'dark' : 'light') : theme;
  const resolvedContrast =
    contrast === 'system' ? (systemContrast ? 'high' : 'normal') : contrast;
  const resolvedMotion = motion === 'system' ? (systemReduced ? 'reduced' : 'full') : motion;

  // The same attributes the inline script in index.html sets before first paint.
  // React takes ownership of them from here; the values agree because both read
  // the same keys the same way.
  useEffect(() => {
    const d = document.documentElement;
    d.dataset.theme = resolvedTheme;
    d.dataset.contrast = resolvedContrast;
    d.dataset.motion = resolvedMotion;
    d.dataset.text = text;
    // Tells the browser which way form controls, scrollbars and the caret should
    // go. Without it they stay light against a dark page.
    d.style.colorScheme = resolvedTheme;
  }, [resolvedTheme, resolvedContrast, resolvedMotion, text]);

  const value = useMemo<Settings>(
    () => ({
      theme,
      contrast,
      motion,
      text,
      resolvedTheme,
      setTheme: (t) => {
        setThemeState(t);
        save('theme', t);
      },
      setContrast: (c) => {
        setContrastState(c);
        save('contrast', c);
      },
      setMotion: (m) => {
        setMotionState(m);
        save('motion', m);
      },
      setText: (t) => {
        setTextState(t);
        save('text', t);
      },
    }),
    [theme, contrast, motion, text, resolvedTheme],
  );

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings(): Settings {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error('useSettings must be used inside SettingsProvider');
  return ctx;
}

/**
 * Closes a popover on an outside click or Escape, and hands focus back to the
 * control that opened it.
 *
 * Kept here rather than in the menu because the workspace will want the same
 * behaviour the moment anything else opens over the canvas.
 */
export function useDismissable(open: boolean, close: () => void) {
  const onKey = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    },
    [close],
  );

  useEffect(() => {
    if (!open) return;
    document.addEventListener('keydown', onKey);
    // `click` rather than `mousedown`: a mousedown listener fires before the
    // button's own click handler and the toggle would immediately reopen it.
    const onClick = () => close();
    document.addEventListener('click', onClick);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('click', onClick);
    };
  }, [open, onKey, close]);
}
