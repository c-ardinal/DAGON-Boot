// Define-mode session: the document being edited, its history, the selected
// item and the check result. Edits go through `apply`, which keeps undo.
import { supportedBoards } from '../../wasm/dagon.ts';
import { ANNOUNCE } from '../copy.ts';
import { DESK_DEMO, DESK_DEMO_YAML, type Config } from '../sample.ts';
import { emptyDoc, fromYaml, place, placeMcu, removeLink, removeNode, toConfig, toYaml, type Doc, type Topo } from './model.ts';
import { problems, type Problem } from './problems.ts';

/** Undo depth; older steps fall off. */
const HISTORY = 100;

/**
 * Whether 運用 can be entered (§5.2): a 構成 with no problems in a shape
 * updating covers. `empty` and `problems` say M69, `unsupported` M68.
 */
export type Operable = 'ok' | 'empty' | 'problems' | 'unsupported';

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

  /** Nothing placed yet (E1) is not a problem to list. */
  readonly problems: Problem[] = $derived(this.revision >= 0 && this.doc.topo.nodes.length ? problems(this.doc) : []);
  readonly errors = $derived(this.problems.filter((p) => p.severity === 'error').length);
  readonly warnings = $derived(this.problems.length - this.errors);
  readonly config: Config = $derived(toConfig(this.doc, supportedBoards()));
  readonly operable: Operable = $derived(
    this.doc.topo.nodes.length === 0
      ? 'empty'
      : this.problems.some((p) => p.severity === 'error' && p.rule !== 'V-23')
        ? 'problems'
        : this.problems.some((p) => p.rule === 'V-23')
          ? 'unsupported'
          : 'ok',
  );
  get canUndo(): boolean {
    return this.revision >= 0 && this.past.length > 0;
  }
  get canRedo(): boolean {
    return this.revision >= 0 && this.future.length > 0;
  }

  /** One edit, undoable. */
  apply(next: Doc): void {
    if (next === this.doc) return;
    this.past = [...this.past.slice(-(HISTORY - 1)), $state.snapshot(this.doc) as Doc];
    this.future = [];
    this.doc = next;
    this.revision++;
  }

  undo(): void {
    const prev = this.past.pop();
    if (!prev) return;
    this.future.push($state.snapshot(this.doc) as Doc);
    this.doc = prev;
    this.keepSelection();
    this.revision++;
  }

  redo(): void {
    const next = this.future.pop();
    if (!next) return;
    this.past.push($state.snapshot(this.doc) as Doc);
    this.doc = next;
    this.keepSelection();
    this.revision++;
  }

  /** "見本を置く": the desk demo with its names (undoable). */
  placeSample(): void {
    this.apply(fromYaml(DESK_DEMO_YAML, Object.fromEntries(DESK_DEMO.nodes.map((n) => [n.id, n.name]))));
    this.selected = null;
  }

  /** "置く" from the node list (A01). */
  place(board: string, product: string): void {
    const r = place(this.doc, board);
    if (!r.id) return;
    this.apply(r.doc);
    this.selected = `node:${r.id}`;
    this.status = ANNOUNCE.A01(product, r.doc.topo.nodes.find((n) => n.id === r.id)!.role === 'coordinator');
  }

  /** "置く" by MCU (§4): a node without a board; its flash size is the developer's to fill. */
  placeMcu(mcu: string, name: string): void {
    const r = placeMcu(this.doc, mcu);
    if (!r.id) return;
    this.apply(r.doc);
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
      this.apply(removeNode(this.doc, id));
      this.status = ANNOUNCE.A03(name);
    } else if (s.startsWith('link:')) {
      const i = (this.doc.topo.links ?? []).findIndex((l) => l.to === s.slice(5));
      if (i >= 0) this.apply(removeLink(this.doc, i));
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
    return { name: `${this.doc.topo.name || 'topology'}.yaml`, text: toYaml(this.doc.topo) };
  }

  /** "読み込む": a YAML file; C09 when it does not parse. */
  importYaml(text: string): boolean {
    try {
      this.apply(fromYaml(text));
    } catch {
      this.unreadable = true;
      return false;
    }
    this.unreadable = false;
    this.selected = null;
    return true;
  }

  /** A project's 構成 as opened (no undo back past it). */
  load(doc: Doc): void {
    this.past = [];
    this.future = [];
    this.doc = doc;
    this.selected = null;
    this.unreadable = false;
    this.revision++;
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
  importRelease(topo: Topo): void {
    const t = { buses: [], links: [], recovery: {}, ...topo };
    const ids = new Set(t.nodes.map((n) => n.id));
    const keep = <T>(r: Record<string, T> | undefined) => Object.fromEntries(Object.entries(r ?? {}).filter(([id]) => ids.has(id)));
    this.apply({ topo: t, names: keep(this.doc.names), boards: keep(this.doc.boards) });
    this.selected = null;
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
