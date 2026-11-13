// How the 構成 a release was made for differs from the one on the
// page — nodes only on one side, a different MCU, board or role, and links of
// another kind, address or pins. Rows for the file check's table and the graph chips.
import { DIFF, INSPECT } from '../copy.ts';
import { KIND_LABEL, linkIndexOf, sides, type TLink, type TNode, type Topo } from './model.ts';

export interface DiffRow {
  /** The node (呼び名) or link ("メイン → 7セグ") the row is about. */
  what: string;
  here: string;
  file: string;
  /** Node ids the row marks on the graph. */
  nodes: string[];
}

const hex = (a: number | undefined) => (a === undefined ? '' : `0x${a.toString(16).padStart(2, '0')}`);

function nodeText(n: TNode | undefined): string {
  if (!n) return DIFF.none;
  return [INSPECT.roles[n.role], n.mcu, n.board].filter(Boolean).join(' · ');
}

function linkText(t: Topo, l: TLink | undefined): string {
  if (!l) return DIFF.none;
  const { up, down } = sides(t, l);
  const pins = (a?: number, b?: number) => (a === undefined && b === undefined ? '' : `${a ?? '—'}/${b ?? '—'}`);
  const parts =
    l.kind === 'i2c'
      ? ['I2C', hex(down.addr)]
      : l.kind === 'uart'
        ? ['UART', String(l.baud ?? ''), pins(up.pins?.tx, up.pins?.rx)]
        : [KIND_LABEL[l.kind]];
  const control = pins(up.nrst, up.boot);
  return [...parts, control && `${INSPECT.reset}/${INSPECT.boot} ${control}`].filter(Boolean).join(' · ');
}

/** Rows where `file` differs from `here`; `names` are the page's 呼び名. */
export function topoDiff(here: Topo, file: Topo, names: Record<string, string>): DiffRow[] {
  const name = (id: string) => names[id] ?? id;
  const ids = [...new Set([...here.nodes.map((n) => n.id), ...file.nodes.map((n) => n.id)])];
  const rows: DiffRow[] = [];
  for (const id of ids) {
    const a = nodeText(here.nodes.find((n) => n.id === id));
    const b = nodeText(file.nodes.find((n) => n.id === id));
    if (a !== b) rows.push({ what: name(id), here: a, file: b, nodes: [id] });
  }
  // A link by the node it is shown under, on each side.
  const under = (topo: Topo) => (topo.links ?? []).map((l) => sides(topo, l).down.node);
  const to = [...new Set([...under(here), ...under(file)])];
  for (const t of to) {
    const la = (here.links ?? [])[linkIndexOf(here, t)];
    const lb = (file.links ?? [])[linkIndexOf(file, t)];
    const a = linkText(here, la);
    const b = linkText(file, lb);
    const fa = la && sides(here, la).up.node;
    const fb = lb && sides(file, lb).up.node;
    const from = fa ?? fb ?? '';
    if (a !== b || fa !== fb) rows.push({ what: `${name(from)} → ${name(t)}`, here: a, file: b, nodes: [t] });
  }
  return rows;
}
