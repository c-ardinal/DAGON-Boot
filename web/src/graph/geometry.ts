// Link geometry: vertical cubic Béziers from a parent's bottom port to a
// child's top port (a cable's rest shape, see cable.ts), and where the wire
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
