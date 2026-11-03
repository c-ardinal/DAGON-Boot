// §7.6: how the 構成 a release was made for differs from the one on the
// page — nodes only on one side, a different MCU, board or role, and links of
// another kind, address or pins. Rows for the SC-O4 table and the graph chips.
import { DIFF, INSPECT } from '../copy.ts';
import { KIND_LABEL, type TLink, type TNode, type Topo } from './model.ts';

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

function linkText(l: TLink | undefined): string {
  if (!l) return DIFF.none;
  const pins = (a?: number, b?: number) => (a === undefined && b === undefined ? '' : `${a ?? '—'}/${b ?? '—'}`);
  const parts =
    l.kind === 'i2c'
      ? ['I2C', hex(l.addr)]
      : l.kind === 'uart'
        ? ['UART', String(l.baud ?? ''), pins(l.pins?.tx, l.pins?.rx)]
        : [KIND_LABEL[l.kind]];
  const control = pins(l.control?.nrst, l.control?.boot);
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
  const to = [...new Set([...(here.links ?? []).map((l) => l.to), ...(file.links ?? []).map((l) => l.to)])];
  for (const t of to) {
    const la = (here.links ?? []).find((l) => l.to === t);
    const lb = (file.links ?? []).find((l) => l.to === t);
    const a = linkText(la);
    const b = linkText(lb);
    const from = la?.from ?? lb?.from ?? '';
    if (a !== b || la?.from !== lb?.from) rows.push({ what: `${name(from)} → ${name(t)}`, here: a, file: b, nodes: [t] });
  }
  return rows;
}
