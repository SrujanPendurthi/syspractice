import { useRef } from 'react';

/**
 * An exclusive choice, rendered as a row of joined buttons.
 *
 * A radiogroup rather than a set of toggles, because exactly one is always on — a screen
 * reader announces "2 of 3" from this and would announce three independent pressed states
 * from `aria-pressed` buttons.
 *
 * Lifted out of the appearance menu when the problem list needed the same control. Two
 * copies of this is two chances to get half of it right, which is roughly what happened:
 * the original had the ARIA and not the keyboard behaviour a radiogroup is supposed to
 * have, and nobody noticed because there was only one of them.
 */
export function Segmented<T extends string>({
  label,
  hideLabel,
  stretch,
  value,
  onChange,
  options,
}: {
  label: string;
  /** The label still reaches assistive tech; it just is not drawn. */
  hideLabel?: boolean;
  /** Fill the container, so stacked groups line up. Off for a row sized by its content. */
  stretch?: boolean;
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string; hint?: string }[];
}) {
  const group = useRef<HTMLDivElement>(null);

  /**
   * Roving tabindex: one stop for the whole group, arrows move between options and select
   * as they go. That is what a radiogroup promises, and a group of plain buttons that
   * takes four tab stops and ignores arrow keys is lying about its role.
   */
  const onKeyDown = (e: React.KeyboardEvent) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const i = options.findIndex((o) => o.value === value);
    // Wraps, which is what a radiogroup does and what the arrow keys imply.
    const next = options[(i + step + options.length) % options.length];
    onChange(next.value);
    // The DOM has not re-rendered yet, so focus by position rather than by looking for
    // the button that is now checked.
    const buttons = group.current?.querySelectorAll('button');
    buttons?.[(i + step + options.length) % options.length]?.focus();
  };

  return (
    <div>
      <p className={hideLabel ? 'sr-only' : 'mb-1.5 text-sm font-medium text-ink'}>{label}</p>
      <div
        ref={group}
        role="radiogroup"
        aria-label={label}
        onKeyDown={onKeyDown}
        className="flex gap-1"
      >
        {options.map((o) => {
          const on = o.value === value;
          return (
            <button
              key={o.value}
              role="radio"
              aria-checked={on}
              tabIndex={on ? 0 : -1}
              onClick={() => onChange(o.value)}
              className={`${stretch ? 'flex-1' : ''} rounded border px-2.5 py-1 text-xs ${
                on
                  ? 'border-accent-line bg-accent-soft font-medium text-accent'
                  : 'border-line text-ink-dim hover:border-line-strong hover:text-ink'
              }`}
            >
              {o.label}
              {o.hint && <span className="tnum ml-1.5 font-mono text-ink-faint">{o.hint}</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}
