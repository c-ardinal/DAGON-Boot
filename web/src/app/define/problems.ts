// The problem list (SC-D3): topo_check's issues with the page's words. The
// check itself is dagon-topology's; this file only names each rule (S07),
// says how to fix it (§11.2) and finds the item and field it points at.
import { topo_check } from '../../wasm/dagon.ts';
import { RULE_FIX, RULE_SHORT, type FixWords } from '../copy.ts';
import { boardMissing, flashTooSmall, MAX_NODES, type Doc } from './model.ts';

export interface Issue {
  rule: string;
  severity: 'error' | 'warning';
  /** YAML path such as `links[0].control.nrst`. */
  path: string;
  message: string;
}

export interface Problem extends Issue {
  /** S07 short name. */
  short: string;
  /** §11.2: how to fix it, one sentence. */
  sentence: string;
  /** The item to select (`node:<id>`, `link:<to>`, `bus:<i>`) or null. */
  select: string | null;
  /** The inspector field to focus: the path below the item (`addr`, `control.nrst`, `id`). */
  field: string;
}

interface Where {
  node?: string;
  link?: number;
  bus?: number;
  rest: string;
}

/** `nodes[1].tag` → node 1, field `tag`; `links[0].control.nrst` → link 0, `control.nrst`. */
function where(path: string, doc: Doc): Where {
  const m = /^(nodes|links|buses)\[(\d+)\]\.?(.*)$/.exec(path);
  if (m) {
    const i = Number(m[2]);
    if (m[1] === 'nodes') return { node: doc.topo.nodes[i]?.id, rest: m[3] };
    if (m[1] === 'links') return { link: i, rest: m[3] };
    return { bus: i, rest: m[3] };
  }
  const r = /^recovery\.([a-z0-9-]+)/.exec(path);
  if (r) return { node: r[1], rest: 'recovery' };
  return { rest: path };
}

const nameOf = (doc: Doc, id: string | undefined) => (id ? (doc.names[id] ?? id) : '');

/** §11.2, filled with the names of the nodes and links the issue is about. */
function sentence(i: Issue, w: Where, doc: Doc): string {
  const t = doc.topo;
  const link = w.link !== undefined ? t.links?.[w.link] : undefined;
  const to = nameOf(doc, link?.to ?? w.node);
  const board = t.nodes.find((n) => n.id === w.node)?.board;
  const fill: FixWords = { to, board: board ?? to, id: w.node ?? '', many: t.nodes.length > MAX_NODES, size: i.rule === 'B-02' ? i.message : undefined };
  return RULE_FIX[i.rule]?.(fill) ?? '';
}

function select(w: Where, doc: Doc): string | null {
  if (w.node) return `node:${w.node}`;
  if (w.link !== undefined) {
    const l = doc.topo.links?.[w.link];
    return l ? `link:${l.to}` : null;
  }
  if (w.bus !== undefined) return `bus:${w.bus}`;
  return null;
}

const kib = (b: number) => (b >= 1 << 20 ? `${b >> 20} MiB` : `${b >> 10} KiB`);

/** The board fields of nodes chosen by MCU (§4.3): missing (B-01, before making the project) or too small (B-02). */
function boardIssues(doc: Doc): Issue[] {
  return doc.topo.nodes.flatMap((n, i): Issue[] => {
    const need = flashTooSmall(doc, n.id);
    if (need) return [{ rule: 'B-02', severity: 'error', path: `nodes[${i}].board.flash`, message: kib(need) }];
    return boardMissing(doc, n.id) ? [{ rule: 'B-01', severity: 'warning', path: `nodes[${i}].board.flash`, message: '' }] : [];
  });
}

/** topo_check on the document (as JSON, which the YAML parser reads), errors first. */
export function problems(doc: Doc): Problem[] {
  const report = JSON.parse(topo_check(JSON.stringify(doc.topo))) as { issues: Issue[] };
  return [...report.issues, ...boardIssues(doc)]
    .map((i) => {
      const w = where(i.path, doc);
      return { ...i, short: RULE_SHORT[i.rule] ?? i.rule, sentence: sentence(i, w, doc), select: select(w, doc), field: w.rest };
    })
    .sort((a, b) => (a.severity === b.severity ? 0 : a.severity === 'error' ? -1 : 1));
}
