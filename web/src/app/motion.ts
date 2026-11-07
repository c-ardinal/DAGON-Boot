// The Console's enter and leave transitions: Svelte runs them (so they reverse
// from where they are when interrupted), and every value is read from the
// tokens in styles/tokens.css, where reduced motion changes them too.
import type { TransitionConfig } from 'svelte/transition';

const token = (name: string): string => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** A duration token in milliseconds. */
export function tokenMs(name: string): number {
  const v = token(name);
  return parseFloat(v) * (v.endsWith('ms') ? 1 : 1000);
}

/** A `cubic-bezier()` token as a function of time, for Svelte's sampling (the curve's x is solved by bisection). */
function curve(name: string): (t: number) => number {
  const [x1, y1, x2, y2] = token(name).slice('cubic-bezier('.length, -1).split(',').map(Number);
  const at = (a: number, b: number, s: number) => 3 * a * s * (1 - s) ** 2 + 3 * b * s ** 2 * (1 - s) + s ** 3;
  return (t) => {
    let lo = 0;
    let hi = 1;
    for (let i = 0; i < 20; i++) {
      const s = (lo + hi) / 2;
      if (at(x1, x2, s) < t) lo = s;
      else hi = s;
    }
    return at(y1, y2, (lo + hi) / 2);
  };
}

/** The motion system's ease for what enters, leaves or settles. */
export const easeOut = (): ((t: number) => number) => curve('--ease-out');

/** A unitless token (`--zoom-default`). */
export const tokenNumber = (name: string): number => parseFloat(token(name));

/** False under reduced motion: what would travel goes to its end at once. */
export const moves = (): boolean => Number(token('--move')) > 0;

interface Timing {
  /** Which duration token: small things take `--dur-quick`. */
  dur?: string;
}

const eased = (dur: string) => ({ duration: tokenMs(dur), easing: curve('--ease-out') });

/** A surface entering or leaving (toast, floating window, card, popover): it fades and rises a little. */
/** A `.segmented` radio group's sliding thumb (base.css): stands under the checked choice, wherever that is. */
export function thumb(group: HTMLElement): { destroy: () => void } {
  const place = () => {
    const on = group.querySelector<HTMLElement>('[aria-checked="true"]');
    if (!on) return;
    group.style.setProperty('--thumb-x', `${on.offsetLeft}px`);
    group.style.setProperty('--thumb-y', `${on.offsetTop}px`);
    group.style.setProperty('--thumb-w', `${on.offsetWidth}px`);
    group.style.setProperty('--thumb-h', `${on.offsetHeight}px`);
  };
  place();
  const checked = new MutationObserver(place);
  checked.observe(group, { subtree: true, attributeFilter: ['aria-checked'] });
  const sized = new ResizeObserver(place);
  sized.observe(group);
  return { destroy: () => (checked.disconnect(), sized.disconnect()) };
}

export function rise(_el: Element, { dur = '--dur-ui' }: Timing = {}): TransitionConfig {
  const shift = parseFloat(token('--enter-shift'));
  const scale = parseFloat(token('--enter-scale'));
  return { ...eased(dur), css: (t, u) => `opacity: ${t}; translate: 0 ${u * shift}px; scale: ${1 - u * (1 - scale)}` };
}

/** A screen or a small part that stays in its place: only its opacity changes. */
export function fade(_el: Element, { dur = '--dur-ui' }: Timing = {}): TransitionConfig {
  return { ...eased(dur), css: (t) => `opacity: ${t}` };
}

/** A link appearing or leaving: it is drawn from the parent down to the child. */
export function draw(_el: Element): TransitionConfig {
  const move = Number(token('--move'));
  return { ...eased('--dur-line'), css: (t, u) => `opacity: ${t}; clip-path: inset(0 0 ${u * 100 * move}% 0) stroke-box` };
}

/** What stays readable through most of a card's settling and goes at its end; `shown` is the % it holds meanwhile. */
export function lateFade(el: HTMLElement, { shown }: { shown?: number } = {}): TransitionConfig {
  if (shown !== undefined) {
    el.style.transition = 'none';
    el.style.setProperty('--shown', String(shown));
  }
  return { duration: tokenMs('--dur-settle'), css: (t) => `opacity: ${Math.min(1, t * 2)}` };
}

/** A node's water as it stops being written: up to the top when the node confirmed, away when it did not. */
export function waterEnd(el: HTMLElement, { confirmed }: { confirmed: boolean }): TransitionConfig {
  const from = parseFloat(/translateY\(([-\d.]+)%\)/.exec(el.style.transform)?.[1] ?? '100');
  const to = confirmed ? 0 : 100;
  const move = Number(token('--move'));
  return { ...eased('--dur-settle'), css: (_t, u) => `transform: translateY(${from + u * (to - from) * move}%)` };
}

/**
 * Keeps a dialog on the page while base.css plays its opening or closing. The
 * dialog closes when its leave starts, so the page takes input at once; the
 * `close` event this causes is not the user closing it, so its handler stays
 * out of it. Interrupted, it opens again. Used `|global`: a dialog leaving with
 * the screen under it must free the page just the same.
 */
export function modal(el: HTMLDialogElement): TransitionConfig {
  const quiet = (e: Event) => e.stopImmediatePropagation();
  let left = false;
  el.addEventListener('outrostart', () => {
    left = true;
    el.addEventListener('close', quiet, true);
    el.close();
  });
  el.addEventListener('introstart', () => {
    if (!left) return;
    left = false;
    el.removeEventListener('close', quiet, true);
    el.showModal();
  });
  return { duration: tokenMs('--dur-ui') };
}
