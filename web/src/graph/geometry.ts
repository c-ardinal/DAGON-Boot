// Link geometry: vertical cubic Béziers from the bottom port of the node nearer the PC
// to the top port of the node at the other end of the link (a cable's rest shape, see cable.ts), and where the wire
// bundle's strands and ties sit.

export interface Pt {
  x: number;
  y: number;
}

export type Bezier = readonly [Pt, Pt, Pt, Pt];

/** Leaves the upper port straight down and enters the lower port from straight above. */
export function verticalLink(from: Pt, to: Pt): Bezier {
  const k = (to.y - from.y) / 2;
  return [from, { x: from.x, y: from.y + k }, { x: to.x, y: to.y - k }, to];
}

/** A card as it stands on the canvas. */
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Between two cards of one row: leaves the side of `from` that faces `to` level, and enters the facing side of `to` level. */
export function sideLink(from: Box, to: Box): Bezier {
  const right = to.x >= from.x;
  const a = { x: from.x + (right ? from.w : 0), y: from.y + from.h / 2 };
  const b = { x: to.x + (right ? 0 : to.w), y: to.y + to.h / 2 };
  const k = Math.max(24, Math.abs(b.x - a.x) / 2) * (right ? 1 : -1);
  return [a, { x: a.x + k, y: a.y }, { x: b.x - k, y: b.y }, b];
}

/** Between two lower edges, round below what stands between them: leaves `from` straight down and enters `to` from straight below. */
export function underLink(from: Pt, to: Pt): Bezier {
  const k = 48 + Math.abs(to.x - from.x) * 0.12;
  return [from, { x: from.x, y: from.y + k }, { x: to.x, y: to.y + k }, to];
}

/** Whether a card of `others` stands in the gap between the facing sides of `from` and `to`, at their height. */
export function between(from: Box, to: Box, others: readonly Box[]): boolean {
  const [l, r] = to.x >= from.x ? [from.x + from.w, to.x] : [to.x + to.w, from.x];
  const [top, bottom] = [Math.min(from.y, to.y), Math.max(from.y + from.h, to.y + to.h)];
  return others.some((o) => o.x < r && o.x + o.w > l && o.y < bottom && o.y + o.h > top);
}

export function pointAt([p0, p1, p2, p3]: Bezier, t: number): Pt {
  const u = 1 - t;
  const a = u * u * u;
  const b = 3 * u * u * t;
  const c = 3 * u * t * t;
  const d = t * t * t;
  return { x: a * p0.x + b * p1.x + c * p2.x + d * p3.x, y: a * p0.y + b * p1.y + c * p2.y + d * p3.y };
}

export function pathD([p0, p1, p2, p3]: Bezier): string {
  return `M${p0.x} ${p0.y}C${p1.x} ${p1.y} ${p2.x} ${p2.y} ${p3.x} ${p3.y}`;
}

export const TIE_FROM_PORT = 32;
const SHORT_LINK = 160;
const MIN_TIE_GAP = 96;
const SHORT_TIE_FRACTION = 0.2;

/** Arc lengths of the two cable ties: 32 px from each port, or 20 % / 80 % on short links. */
export function tieLengths(total: number): [number, number] {
  if (total < SHORT_LINK && total - 2 * TIE_FROM_PORT < MIN_TIE_GAP) {
    return [total * SHORT_TIE_FRACTION, total * (1 - SHORT_TIE_FRACTION)];
  }
  return [TIE_FROM_PORT, total - TIE_FROM_PORT];
}

/** Offsets of n wires spaced `gap` apart, centred on the link. */
export function wireOffsets(n: number, gap: number): number[] {
  return Array.from({ length: n }, (_, i) => (i - (n - 1) / 2) * gap);
}

/** Next node from `from` in an arrow direction: nearest centre inside a 90° cone. */
export function neighbour(from: Pt, others: readonly Pt[], dir: 'up' | 'down' | 'left' | 'right'): number {
  const v = { up: { x: 0, y: -1 }, down: { x: 0, y: 1 }, left: { x: -1, y: 0 }, right: { x: 1, y: 0 } }[dir];
  let best = -1;
  let bestD = Infinity;
  others.forEach((p, i) => {
    const dx = p.x - from.x;
    const dy = p.y - from.y;
    const along = dx * v.x + dy * v.y;
    const across = Math.abs(dx * v.y - dy * v.x);
    if (along <= 0 || across > along) return;
    const d = Math.hypot(dx, dy);
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  });
  return best;
}
