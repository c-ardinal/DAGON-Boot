// What the Console tells the user without stopping them: the notice (toast), the 記録 log and
// the bottom drawer's open tab.
import type { Notice } from '../../screens/Toast.svelte';

export interface Note {
  at: string;
  text: string;
}

/** The latest records kept in 記録. */
const NOTES_KEPT = 200;

export class Feedback {
  /** The non-blocking notice on screen (bottom-right), or null. */
  notice = $state<Notice | null>(null);
  /** 記録: what the page did and why something was stopped, newest last (the drawer's log). */
  notes = $state<Note[]>([]);
  /** The bottom drawer's open tab, null while closed. */
  drawerTab = $state<string | null>(null);

  note(text: string): void {
    this.notes = [...this.notes, { at: new Date().toTimeString().slice(0, 8), text }].slice(-NOTES_KEPT);
  }

  /** A tab's button: it opens the tab, or closes the drawer when the tab is open. */
  pickTab(id: string): void {
    this.drawerTab = this.drawerTab === id ? null : id;
  }

  /** 記録 and the page's address, to paste elsewhere; a refused clipboard says so. */
  async copy(text: string, said: { done: string; failed: string }): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.notice = { word: said.done };
    } catch {
      this.notice = { word: said.failed, error: true };
    }
  }
}
