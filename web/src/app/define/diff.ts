// How the 構成 a release was made for differs from the one on the
// page — nodes only on one side, a different MCU, board or role, and links of
// another kind, address or pins. Rows for the file check's table and the graph chips.
import { DIFF, INSPECT } from '../copy.ts';
import { kindLabel, LinkKind, linkIndexOf, shownLinks, sides, type TLink, type TNode, type Topo } from './model.ts';

export interface DiffRow {
  /** The node (表示名) or link ("メイン → 7セグ") the row is about. */
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
    l.kind === LinkKind.I2c
      ? [kindLabel(l.kind), hex(down.addr)]
      : l.kind === LinkKind.Uart
        ? [kindLabel(l.kind), String(l.baud ?? ''), pins(up.pins?.tx, up.pins?.rx)]
        : [kindLabel(l.kind)];
  const control = pins(up.nrst, up.boot);
  return [...parts, control && `${INSPECT.reset}/${INSPECT.boot} ${control}`].filter(Boolean).join(' · ');
}

/** Rows where `file` differs from `here`; `names` are the page's 表示名. */
export function topoDiff(here: Topo, file: Topo, names: Record<string, string>): DiffRow[] {
  const name = (id: string) => names[id] ?? id;
  const ids = [...new Set([...here.nodes.map((n) => n.id), ...file.nodes.map((n) => n.id)])];
  const rows: DiffRow[] = [];
  for (const id of ids) {
    const a = nodeText(here.nodes.find((n) => n.id === id));
    const b = nodeText(file.nodes.find((n) => n.id === id));
    if (a !== b) rows.push({ what: name(id), here: a, file: b, nodes: [id] });
  }
  // A node's primary link by the node it stands under, on each side.
  const under = (topo: Topo) => shownLinks(topo).filter((x) => x.primary).map((x) => x.down.node);
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
  // Each further link (a star's, a ring's) by the pair of nodes it joins.
  const more = (topo: Topo) => shownLinks(topo).flatMap((x, i) => (x.primary ? [] : [{ at: `${x.up.node}\n${x.down.node}`, from: x.up.node, to: x.down.node, l: (topo.links ?? [])[i] }]));
  const [ma, mb] = [more(here), more(file)];
  for (const at of new Set([...ma, ...mb].map((x) => x.at))) {
    const [xa, xb] = [ma.find((x) => x.at === at), mb.find((x) => x.at === at)];
    const a = linkText(here, xa?.l);
    const b = linkText(file, xb?.l);
    const x = (xa ?? xb)!;
    if (a !== b) rows.push({ what: `${name(x.from)} → ${name(x.to)}`, here: a, file: b, nodes: [x.to] });
  }
  return rows;
}
