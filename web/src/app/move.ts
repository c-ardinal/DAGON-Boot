// Moving a card or the DAGON mark without a drag (WCAG 2.1.1, 2.5.7): Alt + an arrow key
// steps it, and a click places it after the menu's move item.

/** One Alt + arrow step (px), and the step with Shift as well. */
export const STEP_PX = 16;
export const STEP_FAR_PX = 4 * STEP_PX;

const ARROWS: Record<string, [number, number]> = { ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0] };

/** The step an Alt + arrow key asks for, or null for any other key (a plain arrow moves the focus instead). */
export function stepOf(e: Pick<KeyboardEvent, 'key' | 'altKey' | 'shiftKey' | 'ctrlKey' | 'metaKey'>): { x: number; y: number } | null {
  const d = ARROWS[e.key];
  if (!d || !e.altKey || e.ctrlKey || e.metaKey) return null;
  const px = e.shiftKey ? STEP_FAR_PX : STEP_PX;
  return { x: d[0] * px, y: d[1] * px };
}

/**
 * Waits for the one press that places what is moving: a press inside `area` gives its client
 * point to `onplace`; a press outside it or Escape ends the move with `oncancel`. That press and
 * its click reach nothing else. Returns the function that stops waiting.
 */
export function placeByClick(area: () => DOMRect | undefined, onplace: (x: number, y: number) => void, oncancel: () => void): () => void {
  const swallow = (e: Event) => (e.preventDefault(), e.stopPropagation());
  const down = (e: PointerEvent) => {
    swallow(e);
    stop();
    // The click that follows this press opens or selects nothing.
    // A click comes in the same task as its pointerup, or not at all.
    addEventListener('click', swallow, { capture: true, once: true });
    addEventListener('pointerup', () => setTimeout(() => removeEventListener('click', swallow, { capture: true }), 0), { capture: true, once: true });
    const r = area();
    if (r && e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom) onplace(e.clientX, e.clientY);
    else oncancel();
  };
  const key = (e: KeyboardEvent) => {
    if (e.key !== 'Escape') return;
    swallow(e);
    stop();
    oncancel();
  };
  function stop() {
    removeEventListener('pointerdown', down, { capture: true });
    removeEventListener('keydown', key, { capture: true });
  }
  addEventListener('pointerdown', down, { capture: true });
  addEventListener('keydown', key, { capture: true });
  return stop;
}
