// A popover closes when the user presses outside it or presses Escape.

/** Svelte action: `use:dismiss={onclose}` on the popover and whatever opened it. */
export function dismiss(el: HTMLElement, onclose: () => void): { update(next: () => void): void; destroy(): void } {
  let close = onclose;
  const press = (e: PointerEvent) => {
    if (!el.contains(e.target as Node)) close();
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === 'Escape') close();
  };
  addEventListener('pointerdown', press, true);
  addEventListener('keydown', key);
  return {
    update(next) {
      close = next;
    },
    destroy() {
      removeEventListener('pointerdown', press, true);
      removeEventListener('keydown', key);
    },
  };
}
