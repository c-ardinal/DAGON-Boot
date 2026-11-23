// The lock table on the page: which operations are stopped now, the one check every button,
// menu, key and command goes through, and the reason shown where a stopped one was pressed.
import { untrack } from 'svelte';
import { LOCK } from '../copy.ts';
import { lockAria } from '../lockAria.ts';
import { activeStates, lockOf, type Control, type LockInput } from '../locks.ts';

export interface Why {
  text: string;
  x: number;
  y: number;
  /** A refused operation (red, role=alert); otherwise information (role=status). */
  failed: boolean;
}

/** How long a reason stays where it was shown. */
const WHY_MS = 4000;
/** The same refusal again this soon (a port drag repeats it) is not shown again. */
const REFUSE_AGAIN_MS = 600;

export class LockGate {
  /** The lock table's states now, leftmost first. */
  readonly states = $derived(activeStates(this.input()));
  /** Operations stopped now: dimmed through `data-locks` on the frame (base.css). */
  readonly locksNow = $derived((Object.keys(LOCK.controls) as Control[]).filter((c) => this.lockedBy(c) !== null).join(' '));
  /** The reason of a stopped operation, shown where it was pressed. */
  why = $state<Why | null>(null);

  private whyTimer: ReturnType<typeof setTimeout> | undefined;
  private refuseAt = 0;

  /** `note` records each reason in 記録. */
  constructor(
    private readonly input: () => LockInput,
    private readonly note: (text: string) => void,
  ) {}

  lockedBy(c: Control): string | null {
    return lockOf(c, this.states);
  }

  /** Shows `text` under `at` (the middle of the window without one). */
  showWhy(text: string, at?: Element | null, failed = false): void {
    const r = at?.getBoundingClientRect();
    this.why = { text, x: r ? r.left : innerWidth / 2, y: r ? r.bottom + 6 : innerHeight / 2, failed };
    this.note(text);
    clearTimeout(this.whyTimer);
    this.whyTimer = setTimeout(() => (this.why = null), WHY_MS);
  }

  /** True (and the reason shown) when `c` is stopped now. */
  stopped(c: Control, at?: Element | null): boolean {
    const r = this.lockedBy(c);
    if (r) this.showWhy(r, at);
    return r !== null;
  }

  /** A refused port drag: the reason in red at the pointer (it is a refusal). */
  refuse(text: string, x: number, y: number): void {
    const now = Date.now();
    if (this.why?.text === text && now - this.refuseAt < REFUSE_AGAIN_MS) return;
    this.refuseAt = now;
    this.showWhy(text, null, true);
    this.why = this.why && { ...this.why, x, y };
  }

  /**
   * Stops every control marked data-control while its operation is stopped, and marks it so for
   * assistive technology (WCAG 4.1.2). Call it where an effect may be made (a component's script).
   */
  install(): void {
    // Every control marked data-control: pressing a stopped one shows why and does nothing else.
    const click = (e: MouseEvent) => {
      const el = (e.target as Element | null)?.closest?.<HTMLElement>('[data-control]');
      if (!el || !this.stopped(el.dataset.control as Control, el)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    // Typing into a field of a stopped part (the inspector, the table) is stopped the same way.
    const keys = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (!/^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName) || ['Tab', 'Escape', 'Shift'].includes(e.key)) return;
      const el = t.closest<HTMLElement>('[data-control]');
      if (!el || !this.stopped(el.dataset.control as Control, t)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    // On the window in the capture phase: ahead of every handler on the page (Svelte delegates clicks to its root).
    $effect(() => {
      addEventListener('click', click, true);
      addEventListener('keydown', keys, true);
      return () => {
        removeEventListener('click', click, true);
        removeEventListener('keydown', keys, true);
      };
    });
    // A stopped control is disabled for assistive technology too, not only dimmed, and gives its reason on hover.
    let sync: (() => void) | undefined;
    $effect(() => {
      // Made once: its first sync reads the locks, which must not tie this effect to them (the next one keeps it in step).
      const l = untrack(() => lockAria(document.body, (c) => this.lockedBy(c as Control)));
      sync = l.sync;
      return l.stop;
    });
    $effect(() => {
      void this.locksNow;
      sync?.();
    });
  }
}
