// The Console's keys on the window: Ctrl+J the drawer, / DAGON, Escape the top thing, F and
// Ctrl+± the canvas's view, Ctrl+Z / Ctrl+Shift+Z, Delete and L in 定義.
import type { Define } from '../define/define.svelte.ts';
import type { Control } from '../locks.ts';

export interface KeyContext {
  /** A project is open (the work screen, not the start screen). */
  work: boolean;
  mode: 'operate' | 'define';
  d: Define;
  /** DAGON can be talked to (linked, or the demo's own). */
  dagon: boolean;
  bubbleOpen: boolean;
  /** The menu or the help is open: Escape is theirs. */
  popoverOpen: boolean;
  stopped: (c: Control) => boolean;
  toggleDrawer(): void;
  openDagon(): void;
  closeBubble(): void;
  /** Escape with nothing above the canvas: the reason shown goes, and the sheet closes. */
  closeSheet(): void;
  fit(): void;
  zoomBy(f: number): void;
  zoomStep: number;
}

const FIELD = /^(INPUT|SELECT|TEXTAREA)$/;
/** What takes Escape before the bubble and the sheet: a dialog, the version list or a notice. */
const ABOVE = 'dialog[open], .split .list, [data-stop-notice]';

export function consoleKeys(e: KeyboardEvent, k: KeyContext): void {
  const t = e.target as HTMLElement;
  const ctrl = e.ctrlKey || e.metaKey;
  const key = e.key.toLowerCase();
  if (k.work && key === 'j' && ctrl) {
    e.preventDefault();
    return k.toggleDrawer();
  }
  const field = FIELD.test(t.tagName);
  // A single-character key acts only with the focus on the canvas (WCAG 2.1.4): spoken words typed into the page elsewhere press nothing.
  const onCanvas = !!t.closest?.('#canvas') && !field;
  if (k.work && k.dagon && e.key === '/' && onCanvas) {
    e.preventDefault();
    return k.openDagon();
  }
  // Escape closes the top thing only: a dialog, the version list or a notice take it first; then the bubble, the menu, the help, the sheet.
  if (k.work && e.key === 'Escape' && !document.querySelector(ABOVE)) {
    if (k.bubbleOpen) {
      e.preventDefault();
      return k.closeBubble();
    }
    if (!k.popoverOpen) return k.closeSheet();
  }
  if (k.work && !field) {
    if (key === 'f' && !ctrl && onCanvas) return k.fit();
    if (ctrl && (e.key === '+' || e.key === '=' || e.key === '-')) {
      e.preventDefault();
      return k.zoomBy(e.key === '-' ? 1 / k.zoomStep : k.zoomStep);
    }
  }
  if (k.mode !== 'define') return;
  if (key === 'z' && ctrl && !field) {
    e.preventDefault();
    if (k.stopped('edit')) return;
    return void (e.shiftKey ? k.d.redo() : k.d.undo());
  }
  if (field || !t.closest('#canvas')) return;
  if (e.key === 'Delete') {
    e.preventDefault();
    if (k.stopped('edit')) return;
    k.d.requestRemove();
  } else if (key === 'l') {
    k.d.focusField = 'connect';
    k.d.focusRequest++;
  }
}
