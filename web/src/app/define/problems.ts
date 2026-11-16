// The problem list: topo_check's issues with the page's words. The
// check itself is dagon-topology's; this file only names each rule,
// says how to fix it and finds the item and field it points at.
import { topo_check } from '../../wasm/dagon.ts';
import { topo_cut } from '../../wasm/pkg/dagon_wasm.js';
import { INSPECT, RULE_FIX, RULE_SHORT, type FixWords } from '../copy.ts';
import { busNodes, definedPins, i2cHzs, linkKey, maxNodes, romI2cPairs, sides, type Doc } from './model.ts';

export interface Issue {
  rule: string;
  /** `info`: a valid part updating skips, never a problem to fix. */
  severity: 'error' | 'warning' | 'info';
  /** YAML path such as `links[0].ends[1].addr`. */
  path: string;
  message: string;
}

export interface Problem extends Issue {
  /** S07 short name. */
  short: string;
  /** How to fix it, one sentence. */
  sentence: string;
  /** The item to select (`node:<id>`, `link:<node it is shown under>`, `bus:<i>`) or null. */
  select: string | null;
  /** The inspector field to focus: the path below the item (`addr`, `control.nrst`, `id`). */
  field: string;
}

interface Where {
  node?: string;
  link?: number;
  /** The link end the path names, if it names one. */
  end?: number;
  bus?: number;
  rest: string;
}

/** An end's field as the inspector names it: `nrst` and `boot` are its reset lines, `port` follows the kind. */
const END_FIELD: Record<string, string> = { nrst: 'control.nrst', boot: 'control.boot', port: 'kind', node: '' };

/** `nodes[1].tag` → node 1, field `tag`; `links[0].ends[1].addr` → link 0, end 1, `addr`. */
function where(path: string, doc: Doc): Where {
  const m = /^(nodes|links|buses)\[(\d+)\]\.?(.*)$/.exec(path);
  if (m) {
    const i = Number(m[2]);
    if (m[1] === 'nodes') return { node: doc.topo.nodes[i]?.id, rest: m[3] };
    if (m[1] === 'links') {
      const e = /^ends(?:\[(\d)\])?\.?(.*)$/.exec(m[3]);
      if (!e) return { link: i, rest: m[3] };
      const head = e[2].split('.')[0];
      return { link: i, end: e[1] === undefined ? undefined : Number(e[1]), rest: head in END_FIELD ? END_FIELD[head] : e[2] };
    }
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
  const s = link ? sides(t, link) : undefined;
  const end = link && w.end !== undefined ? link.ends[w.end] : undefined;
  const node = (id: string | undefined) => t.nodes.find((n) => n.id === id);
  const to = nameOf(doc, s?.down.node ?? w.node);
  const board = t.nodes.find((n) => n.id === w.node)?.board;
  const fill: FixWords = {
    to,
    board: board ?? to,
    id: w.node ?? '',
    many: t.nodes.length > maxNodes(),
    max: maxNodes(),
    skipped: (s?.down.node ?? w.node ?? '') in skipped,
    // The pin's own node: the end the path names, else the node the link is drawn from.
    pins: definedPins(node(end?.node ?? s?.up.node ?? w.node)).map((n) => `GPIO${n}`).join('・') || undefined,
    pairs: romI2cPairs(node(end?.node ?? s?.down.node)).map((p) => `${p.sda}・${p.scl}`).join('、') || undefined,
    hzs: w.bus !== undefined && t.buses?.[w.bus] ? i2cHzs(busNodes(t, t.buses[w.bus].id)).map(INSPECT.hz).join('・') || undefined : undefined,
  };
  return RULE_FIX[i.rule]?.(fill) ?? '';
}

function select(w: Where, doc: Doc): string | null {
  if (w.node) return `node:${w.node}`;
  if (w.link !== undefined) {
    const l = doc.topo.links?.[w.link];
    return l ? linkKey(doc.topo, l) : null;
  }
  if (w.bus !== undefined) return `bus:${w.bus}`;
  return null;
}

const RANK: Record<Issue['severity'], number> = { error: 0, warning: 1, info: 2 };

/** topo_check on the document (as JSON, which the YAML parser reads): errors, then cautions, then what is not updatable yet. */
export function problems(doc: Doc): Problem[] {
  const text = JSON.stringify(doc.topo);
  const report = JSON.parse(topo_check(text)) as { issues: Issue[] };
  const skipped = (topo_cut(text) as { skipped: Record<string, string> } | null)?.skipped ?? {};
  return report.issues
    .map((i) => {
      const w = where(i.path, doc);
      return { ...i, short: RULE_SHORT[i.rule] ?? i.rule, sentence: sentence(i, w, doc, skipped), select: select(w, doc), field: w.rest };
    })
    .sort((a, b) => RANK[a.severity] - RANK[b.severity]);
}
