// How a link is drawn on the canvas: its strands along the cable, the ties round them, and its chip beside the run.
import { LINK } from '../app/copy.ts';
import { kindLabel } from '../app/define/model.ts';
import type { Config, Link } from '../app/sample.ts';
import { along, runLengths, strandPath } from './cable.ts';
import { tieLengths, wireOffsets, type Bezier, type Pt } from './geometry.ts';

/** Between a bundle's outer strand and its chip, and the chip's height (px). */
const CHIP_CLEAR = 12;
const CHIP_H = 24;

/** How far from a leaning run a chip stands so the run passes clear over its whole height. */
function clearOf(width: number, dir: Pt): number {
  const down = Math.max(0.2, Math.abs(dir.y));
  return width / 2 / down + (CHIP_H / 2) * (Math.abs(dir.x) / down) + CHIP_CLEAR;
}

/** The bus and its value: the strands show their count, and the words for it are in the name and the sheet. */
function bundleLabel(l: Link, config: Config | null): string {
  const value = l.addr || config?.nodes.find((n) => n.id === l.to)?.via.split(' ')[1] || '';
  return [kindLabel(l.kind), value].filter(Boolean).join(' ');
}

export interface Bundle {
  l: Link;
  key: string;
  open: boolean;
  /** The side the cable leans out to: the chip stands there (1 right, -1 left). */
  outward: 1 | -1;
  /** How far from the run its chip stands. */
  clear: number;
  /** A run that lies level where its chip stands: the chip stands over it (-1) or, where the cable bows down, under it (1); 0 for a leaning run. */
  level: 1 | -1 | 0;
  /** The link's end on the node nearer the PC. */
  start: Pt;
  strands: string[];
  ties: { p: Pt; deg: number; len: number }[];
  mid: Pt;
  label: string;
  /** What assistive technology says for the chip. */
  name: string;
  /** The node it leads to is not reached. */
  skipped: boolean;
}

/** Link `l` drawn along `points` (the cable as it is now) over its resting `route`; open when it is selected. */
export function bundleOf(l: Link, config: Config | null, route: Bezier, points: Pt[], open: boolean): Bundle {
  const nameOf = (id: string) => config?.nodes.find((n) => n.id === id)?.name ?? id;
  const lengths = runLengths(points);
  const total = lengths[lengths.length - 1];
  const gap = open ? 8 : 4;
  const count = Math.max(1, l.wires.length);
  const offsets = wireOffsets(count, gap);
  const width = (count - 1) * gap;
  const ties = open
    ? []
    : tieLengths(total).map((s) => {
        const { p, dir } = along(points, lengths, s);
        return { p, deg: (Math.atan2(dir.y, dir.x) * 180) / Math.PI - 90, len: width + 4 };
      });
  const { p: mid, dir: midDir } = along(points, lengths, total * 0.5);
  // A route between two sides leaves level (sideLink); one round below enters from below (underLink).
  const level = route[1].y === route[0].y ? -1 : route[2].y > route[3].y ? 1 : 0;
  const label = bundleLabel(l, config);
  const kind = kindLabel(l.kind);
  return {
    l,
    key: l.key,
    open,
    outward: route[3].x < route[0].x ? -1 : 1,
    clear: level ? width / 2 + CHIP_H / 2 + CHIP_CLEAR : clearOf(width, midDir),
    level,
    start: route[0],
    strands: offsets.map((o) => strandPath(points, o)),
    ties,
    mid,
    label,
    name: LINK.name(LINK.bundle(kind, l.wires.length, l.wires.filter((w) => w.shared).length, label.slice(kind.length + 1)), nameOf(l.from), nameOf(l.to), l.addr),
    skipped: !!config?.nodes.find((n) => n.id === l.to)?.skip,
  };
}
