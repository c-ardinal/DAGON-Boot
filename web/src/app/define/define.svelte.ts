// Define-mode session: the document being edited, its history, the selected
// item and the check result. Edits go through `apply`, which keeps undo.
import { supportedBoards } from '../../wasm/dagon.ts';
import { ANNOUNCE, LINK_WHY } from '../copy.ts';
import { DESK_DEMO, DESK_DEMO_YAML, type Config } from '../sample.ts';
import { connect, emptyDoc, fromYaml, place, placeMcu, removeLink, removeNode, toConfig, toYaml, type Doc, type Topo } from './model.ts';
import type { SaveResult } from './cli.svelte.ts';
import { keepComments } from './comments.ts';
import { problems, type Problem } from './problems.ts';

/** Undo depth; older steps fall off. */
const HISTORY = 100;

/**
 * Whether 運用 can be entered (§5.2): a 構成 with no problems. Parts
 * updating does not reach yet (V-23) are valid and do not stop it; they
 * are skipped there. `empty` and `problems` say M69.
 */
export type Operable = 'ok' | 'empty' | 'problems';

export class Define {
  doc = $state<Doc>(emptyDoc());
  /** `node:<id>`, `link:<to>`, `bus:<i>`, or null (the problem list shows). */
  selected = $state<string | null>(null);
  /** Graph or table (SC-T) in the canvas place. */
  view = $state<'graph' | 'table'>('graph');
  status = $state('');
  alert = $state('');
  /** The last file read did not parse (C09). */
  unreadable = $state(false);
  /** Bumped to ask the page to focus the inspector field `focusField`. */
  focusRequest = $state(0);
  focusField = $state('');

  private past: Doc[] = [];
  private future: Doc[] = [];
  /** Bumped by every change, so the derived check and view follow. */
  revision = $state(0);
  /** The revision last in the project's YAML (opened or saved); it never goes back (C-377). */
  savedAt = $state(0);
  /** The YAML as opened or read (its hand-written comments are kept on save, C-383), and the page's layout of it. */
  private source: { text: string; as: string } | null = null;
  /** Why the last save did not go through; the edits stay on the page (null after a save). */
  saveFailure = $state<Exclude<SaveResult, 'saved'> | null>(null);

  /** Nothing placed yet (E1) is not a problem to list. */
  readonly problems: Problem[] = $derived(this.revision >= 0 && this.doc.topo.nodes.length ? problems(this.doc) : []);
  readonly errors = $derived(this.problems.filter((p) => p.severity === 'error').length);
  readonly warnings = $derived(this.problems.filter((p) => p.severity === 'warning').length);
  readonly config: Config = $derived(toConfig(this.doc, supportedBoards()));
  /** Nodes updating skips for now (V-23): shown as まだ更新できない, never a problem. */
  readonly notYet = $derived(this.config.nodes.filter((n) => n.notYet).length);
  readonly operable: Operable = $derived(
    this.doc.topo.nodes.length === 0
      ? 'empty'
      : this.errors > 0
        ? 'problems'
        : 'ok',
  );
  /** Edits on the page that the project's YAML does not have yet. */
  get unsaved(): boolean {
    return this.revision !== this.savedAt;
  }

  /** The answer to saving revision `at`: the document stays as it is either way. */
  saved(at: number, r: SaveResult): void {
    if (r === 'saved') {
      // Saves can answer out of order: an older one never marks newer edits saved again.
      this.savedAt = Math.max(this.savedAt, at);
      this.saveFailure = null;
    } else {
      this.saveFailure = r;
    }
  }

  get canUndo(): boolean {
    return this.revision >= 0 && this.past.length > 0;
  }
  get canRedo(): boolean {
    return this.revision >= 0 && this.future.length > 0;
  }

  /**
   * The lock table's check for editing (DES-UX3-182), set by the page: true when editing is
   * stopped now (the page has shown why). Every edit goes through apply, undo or redo, so a
   * key, a double-click, a drag or any later path is stopped the same way.
   */
  guard: (kind: 'edit') => boolean = () => false;

  /** One edit, undoable; false when nothing changed or editing is stopped now. */
  apply(next: Doc): boolean {
    if (next === this.doc) return false;
    if (this.guard('edit')) return false;
    this.past = [...this.past.slice(-(HISTORY - 1)), $state.snapshot(this.doc) as Doc];
    this.future = [];
    this.doc = next;
    this.revision++;
    return true;
  }

  undo(): void {
    if (!this.past.length || this.guard('edit')) return;
    const prev = this.past.pop();
    if (!prev) return;
    this.future.push($state.snapshot(this.doc) as Doc);
    this.doc = prev;
    this.keepSelection();
    this.revision++;
  }

  redo(): void {
    if (!this.future.length || this.guard('edit')) return;
    const next = this.future.pop();
    if (!next) return;
    this.past.push($state.snapshot(this.doc) as Doc);
    this.doc = next;
    this.keepSelection();
    this.revision++;
  }

  /** "見本を置く": the desk demo with its names (undoable). */
  placeSample(): void {
    if (!this.apply(fromYaml(DESK_DEMO_YAML, Object.fromEntries(DESK_DEMO.nodes.map((n) => [n.id, n.name]))))) return;
    this.selected = null;
  }

  /** "置く" from the node list (A01). */
  place(board: string, product: string): void {
    const r = place(this.doc, board);
    if (!r.id || !this.apply(r.doc)) return;
    this.selected = `node:${r.id}`;
    this.status = ANNOUNCE.A01(product, r.doc.topo.nodes.find((n) => n.id === r.id)!.role === 'coordinator');
  }

  /**
   * A part dropped on the canvas (DES-UX3-030): placed as "置く" places it (DEFS: the board decides
   * 親機 or 子機), then, when dropped just under a card, linked under it in the same one edit.
   */
  placeAt(part: { board: string } | { mcu: string }, name: string, parent: string | null): void {
    const r = 'board' in part ? place(this.doc, part.board) : placeMcu(this.doc, part.mcu);
    if (!r.id) return;
    const next = parent ? connect(r.doc, r.id, parent) : r.doc;
    if (!this.apply(next)) return;
    this.selected = `node:${r.id}`;
    this.status = ANNOUNCE.A01(name, next.topo.nodes.find((n) => n.id === r.id)!.role === 'coordinator');
  }

  /** A port drag (DES-UX3-031): node `id` under `parent`, with the link "つなぐ先" would make. */
  link(id: string, parent: string): void {
    this.apply(connect(this.doc, id, parent));
  }

  /** Why `id` cannot go under `parent` by a port drag, or null when it can. */
  linkRefusal(parent: string, id: string): string | null {
    const links = this.doc.topo.links ?? [];
    const up = links.find((l) => l.to === id)?.from;
    if (up !== undefined) {
      const p = this.doc.topo.nodes.find((n) => n.id === up);
      return p?.role === 'coordinator' ? LINK_WHY.taken : LINK_WHY.under(this.doc.names[up] ?? up);
    }
    // Under its own descendant: the link would close a loop.
    for (let at: string | undefined = parent, i = 0; at !== undefined && i < 64; at = links.find((l) => l.to === at)?.from, i++) {
      if (at === id) return LINK_WHY.loop;
    }
    return null;
  }

  /** "置く" by MCU (§4): a node without a board; its flash size is the developer's to fill. */
  placeMcu(mcu: string, name: string): void {
    const r = placeMcu(this.doc, mcu);
    if (!r.id || !this.apply(r.doc)) return;
    this.selected = `node:${r.id}`;
    this.status = ANNOUNCE.A01(name, r.doc.topo.nodes.find((n) => n.id === r.id)!.role === 'coordinator');
  }

  /** Delete on the selected node or link (A03). */
  removeSelected(): void {
    const s = this.selected;
    if (!s) return;
    if (s.startsWith('node:')) {
      const id = s.slice(5);
      const name = this.doc.names[id] ?? id;
      if (!this.apply(removeNode(this.doc, id))) return;
      this.status = ANNOUNCE.A03(name);
    } else if (s.startsWith('link:')) {
      const i = (this.doc.topo.links ?? []).findIndex((l) => l.to === s.slice(5));
      if (i < 0 || !this.apply(removeLink(this.doc, i))) return;
    }
    this.selected = null;
  }

  /** "直す" on a problem: select its item and focus its field. */
  fix(p: Problem): void {
    this.selected = p.select;
    this.focusField = p.field;
    this.focusRequest++;
  }

  /** "書き出す": the YAML text and a file name. */
  exportYaml(): { name: string; text: string } {
    const text = toYaml(this.doc.topo);
    const s = this.source;
    // Unchanged: the text as written; changed: the page's layout with the comments of the lines that did not change.
    const kept = !s ? text : text === s.as ? s.text : keepComments(s.text, text);
    return { name: `${this.doc.topo.name || 'topology'}.yaml`, text: kept };
  }

  /** "読み込む": a YAML file; C09 when it does not parse. */
  importYaml(text: string): boolean {
    try {
      const doc = fromYaml(text);
      if (!this.apply(doc)) return false;
      this.source = { text, as: toYaml(doc.topo) };
    } catch {
      this.unreadable = true;
      return false;
    }
    this.unreadable = false;
    this.selected = null;
    return true;
  }

  /** A project's 構成 as opened (no undo back past it); `text` is its YAML as written. */
  load(doc: Doc, text?: string): void {
    this.source = text === undefined ? null : { text, as: toYaml(doc.topo) };
    this.past = [];
    this.future = [];
    this.doc = doc;
    this.selected = null;
    this.unreadable = false;
    this.revision++;
    this.savedAt = this.revision;
    this.saveFailure = null;
  }

  /** "最初からやり直す": nothing placed and no history (the project's files stay). */
  reset(): void {
    this.load(emptyDoc());
    this.status = '';
    this.alert = '';
    this.view = 'graph';
  }

  /**
   * "ファイルの構成を取り込む" (§7.6): the 構成 a release was made for
   * replaces this one, keeping the names and board fields of the nodes it
   * shares; undoable.
   */
  importRelease(topo: Topo): boolean {
    const t = { buses: [], links: [], recovery: {}, ...topo };
    const ids = new Set(t.nodes.map((n) => n.id));
    const keep = <T>(r: Record<string, T> | undefined) => Object.fromEntries(Object.entries(r ?? {}).filter(([id]) => ids.has(id)));
    if (!this.apply({ topo: t, names: keep(this.doc.names), boards: keep(this.doc.boards) })) return false;
    this.selected = null;
    return true;
  }

  /** Undo or redo can remove what was selected. */
  private keepSelection(): void {
    const s = this.selected;
    if (!s) return;
    const t = this.doc.topo;
    const there = s.startsWith('node:') ? t.nodes.some((n) => `node:${n.id}` === s) : s.startsWith('link:') ? (t.links ?? []).some((l) => `link:${l.to}` === s) : true;
    if (!there) this.selected = null;
  }
}
