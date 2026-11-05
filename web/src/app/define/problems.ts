// The problem list: topo_check's issues with the page's words. The
// check itself is dagon-topology's; this file only names each rule,
// says how to fix it and finds the item and field it points at.
import { topo_check } from '../../wasm/dagon.ts';
import { topo_cut } from '../../wasm/pkg/dagon_wasm.js';
import { RULE_FIX, RULE_SHORT, type FixWords } from '../copy.ts';
import { boardMissing, flashTooSmall, MAX_NODES, type Doc } from './model.ts';

export interface Issue {
  rule: string;
  /** `info`: a valid part updating skips for now, never a problem to fix. */
  severity: 'error' | 'warning' | 'info';
  /** YAML path such as `links[0].control.nrst`. */
  path: string;
  message: string;
}

export interface Problem extends Issue {
  /** S07 short name. */
  short: string;
  /** How to fix it, one sentence. */
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

/** The chip of a problem: ! an error, ▲ a caution, ◇ a part not updatable yet. */
export function problemChip(p: Pick<Issue, 'rule' | 'severity'>): { mark: '!' | '▲' | '◇'; kind: 'error' | 'warning' | 'unsupported' } {
  // V-24 (a hop the firmware cannot do yet) is the same kind of "not yet" as V-23.
  if (p.rule === 'V-23' || p.rule === 'V-24') return { mark: '◇', kind: 'unsupported' };
  return p.severity === 'warning' ? { mark: '▲', kind: 'warning' } : { mark: '!', kind: 'error' };
}

const nameOf = (doc: Doc, id: string | undefined) => (id ? (doc.names[id] ?? id) : '');

/** The fix sentence, filled with the names of the nodes and links the issue is about. */
function sentence(i: Issue, w: Where, doc: Doc, skipped: Record<string, string>): string {
  const t = doc.topo;
  const link = w.link !== undefined ? t.links?.[w.link] : undefined;
  const to = nameOf(doc, link?.to ?? w.node);
  const board = t.nodes.find((n) => n.id === w.node)?.board;
  const fill: FixWords = {
    to,
    board: board ?? to,
    id: w.node ?? '',
    many: t.nodes.length > MAX_NODES,
    size: i.rule === 'B-02' ? i.message : undefined,
    skipped: (link?.to ?? w.node ?? '') in skipped,
  };
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

const RANK: Record<Issue['severity'], number> = { error: 0, warning: 1, info: 2 };

const kib = (b: number) => (b >= 1 << 20 ? `${b >> 20} MiB` : `${b >> 10} KiB`);

/** The board fields of nodes chosen by MCU: missing (B-01, before making the project) or too small. */
function boardIssues(doc: Doc): Issue[] {
  return doc.topo.nodes.flatMap((n, i): Issue[] => {
    const need = flashTooSmall(doc, n.id);
    if (need) return [{ rule: 'B-02', severity: 'error', path: `nodes[${i}].board.flash`, message: kib(need) }];
    return boardMissing(doc, n.id) ? [{ rule: 'B-01', severity: 'warning', path: `nodes[${i}].board.flash`, message: '' }] : [];
  });
}

/** topo_check on the document (as JSON, which the YAML parser reads): errors, then cautions, then what is not updatable yet. */
export function problems(doc: Doc): Problem[] {
  const text = JSON.stringify(doc.topo);
  const report = JSON.parse(topo_check(text)) as { issues: Issue[] };
  const skipped = (topo_cut(text) as { skipped: Record<string, string> } | null)?.skipped ?? {};
  return [...report.issues, ...boardIssues(doc)]
    .map((i) => {
      const w = where(i.path, doc);
      return { ...i, short: RULE_SHORT[i.rule] ?? i.rule, sentence: sentence(i, w, doc, skipped), select: select(w, doc), field: w.rest };
    })
    .sort((a, b) => RANK[a.severity] - RANK[b.severity]);
}
