// Links as weighted cables: each bundle's centre line is a few points with mass,
// pulled to the route geometry.ts draws (the rest shape) and to their neighbours,
// heavily damped so a move trails and settles without ringing. The ends sit on
// the ports exactly. Nothing here touches the view of the canvas.
import { pointAt, type Bezier, type Pt } from './geometry.ts';

/** Every tuning value of the cables, in canvas px and seconds. */
export const CABLE = {
  /** Segments of a bundle's centre line. */
  segments: 12,
  /** Pull towards the rest shape (1/s²): together with `damping`, about a second to settle. */
  stiffness: 64,
  /** Pull between neighbouring points (1/s²): how the move of a port travels down the cable. */
  tension: 300,
  /** Velocity loss (1/s): 2 √stiffness, the critical value, so the slowest motion does not swing back. */
  damping: 16,
  /** Physics step (s): small enough for the tension above to stay stable. */
  step: 1 / 120,
  /** Longest stretch one frame simulates (s): a stalled tab does not run away. */
  longestFrame: 1 / 15,
  /** Within this of the rest shape and slower than `restSpeed`, the bundle is at rest. */
  restPx: 0.2,
  restSpeed: 2,
  /** How far from a port the strands spread (px), and how wide they stand at the port. */
  fanPx: 32,
  fanWide: 1.75,
} as const;

/** The rest shape of a route: its Bézier sampled at the model's points. */
export function restPoints(route: Bezier): Pt[] {
  return Array.from({ length: CABLE.segments + 1 }, (_, i) => pointAt(route, i / CABLE.segments));
}

interface Bundle {
  rest: Pt[];
  at: Pt[];
  speed: Pt[];
}

/** All bundles of a canvas. `step` runs only while something is away from rest. */
export class CableField {
  private bundles = new Map<string, Bundle>();
  private leftover = 0;
  /** No bundle is moving: nothing needs a frame. */
  atRest = true;

  /** The routes now. A new bundle starts at rest; a known one keeps its shape and is pulled to the new route, with its ends on the ports at once. */
  route(routes: Readonly<Record<string, Bezier>>): void {
    for (const key of this.bundles.keys()) if (!(key in routes)) this.bundles.delete(key);
    for (const [key, r] of Object.entries(routes)) {
      const rest = restPoints(r);
      // A route that is no place (bad coordinates) never moves a cable, and so never keeps the frames running.
      if (!rest.every((p) => Number.isFinite(p.x) && Number.isFinite(p.y))) continue;
      const b = this.bundles.get(key);
      if (!b) {
        this.bundles.set(key, { rest, at: rest.map((p) => ({ ...p })), speed: rest.map(() => ({ x: 0, y: 0 })) });
        continue;
      }
      b.rest = rest;
      pin(b);
    }
    this.atRest = [...this.bundles.values()].every(still);
  }

  /** Advances the model by `ms`, in fixed steps so a run is the same at any frame rate. */
  step(ms: number): void {
    this.leftover += Math.min(ms / 1000, CABLE.longestFrame);
    while (this.leftover >= CABLE.step) {
      this.leftover -= CABLE.step;
      for (const b of this.bundles.values()) stepBundle(b);
    }
    let rest = true;
    for (const b of this.bundles.values()) {
      if (still(b)) snap(b);
      else rest = false;
    }
    this.atRest = rest;
  }

  /** Every bundle to its rest shape at once (reduced motion). */
  settle(): void {
    for (const b of this.bundles.values()) snap(b);
    this.atRest = true;
  }

  /** The bundles' points as they are now. */
  shapes(): Record<string, Pt[]> {
    return Object.fromEntries([...this.bundles].map(([key, b]) => [key, b.at.map((p) => ({ ...p }))]));
  }
}

function pin(b: Bundle): void {
  const last = b.at.length - 1;
  b.at[0] = { ...b.rest[0] };
  b.at[last] = { ...b.rest[last] };
  b.speed[0] = { x: 0, y: 0 };
  b.speed[last] = { x: 0, y: 0 };
}

function stepBundle(b: Bundle): void {
  const { stiffness, tension, damping, step } = CABLE;
  const before = b.at.map((p) => ({ ...p }));
  for (let i = 1; i < b.at.length - 1; i++) {
    const [p, r, up, down, v] = [before[i], b.rest[i], before[i - 1], before[i + 1], b.speed[i]];
    // The neighbours pull against their own rest offsets, so the rest shape itself feels no force.
    const bendX = up.x - b.rest[i - 1].x + (down.x - b.rest[i + 1].x) - 2 * (p.x - r.x);
    const bendY = up.y - b.rest[i - 1].y + (down.y - b.rest[i + 1].y) - 2 * (p.y - r.y);
    const vx = v.x + step * (stiffness * (r.x - p.x) + tension * bendX - damping * v.x);
    const vy = v.y + step * (stiffness * (r.y - p.y) + tension * bendY - damping * v.y);
    b.speed[i] = { x: vx, y: vy };
    b.at[i] = { x: p.x + step * vx, y: p.y + step * vy };
  }
}

function still(b: Bundle): boolean {
  return b.at.every((p, i) => Math.hypot(p.x - b.rest[i].x, p.y - b.rest[i].y) < CABLE.restPx && Math.hypot(b.speed[i].x, b.speed[i].y) < CABLE.restSpeed);
}

function snap(b: Bundle): void {
  b.at = b.rest.map((p) => ({ ...p }));
  b.speed = b.rest.map(() => ({ x: 0, y: 0 }));
}

/** Runs `frame(ms since the last frame)` on animation frames while it returns true; stops itself otherwise. */
export class FrameLoop {
  private handle: number | null = null;
  private last = 0;
  constructor(
    private readonly frame: (ms: number) => boolean,
    private readonly request: (cb: (now: number) => void) => number = (cb) => requestAnimationFrame(cb),
    private readonly cancelRequest: (handle: number) => void = (h) => cancelAnimationFrame(h),
  ) {}

  get running(): boolean {
    return this.handle !== null;
  }

  kick(): void {
    if (this.handle !== null) return;
    this.handle = this.request((now) => {
      this.last = now;
      this.handle = this.request(this.tick);
    });
  }

  stop(): void {
    if (this.handle !== null) this.cancelRequest(this.handle);
    this.handle = null;
  }

  private tick = (now: number): void => {
    const ms = now - this.last;
    this.last = now;
    this.handle = this.frame(ms) ? this.request(this.tick) : null;
  };
}

/** Lengths along a polyline, from its first point. */
export function runLengths(points: readonly Pt[]): number[] {
  const out = [0];
  for (let i = 1; i < points.length; i++) out.push(out[i - 1] + Math.hypot(points[i].x - points[i - 1].x, points[i].y - points[i - 1].y));
  return out;
}

/** The point at length `s` along a polyline and its direction there. */
export function along(points: readonly Pt[], lengths: readonly number[], s: number): { p: Pt; dir: Pt } {
  let i = 1;
  while (i < lengths.length - 1 && lengths[i] < s) i++;
  const [a, b] = [points[i - 1], points[i]];
  const span = lengths[i] - lengths[i - 1] || 1;
  const f = Math.min(1, Math.max(0, (s - lengths[i - 1]) / span));
  return { p: { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f }, dir: { x: (b.x - a.x) / span, y: (b.y - a.y) / span } };
}

/** One strand of a bundle: `offset` from the centre line, parallel in the run, spreading to `fanWide` at each port; a smooth path through its points. */
export function strandPath(points: readonly Pt[], offset: number): string {
  const lengths = runLengths(points);
  const total = lengths[lengths.length - 1];
  const at = points.map((p, i) => {
    const a = points[Math.max(0, i - 1)];
    const b = points[Math.min(points.length - 1, i + 1)];
    const n = Math.hypot(b.x - a.x, b.y - a.y) || 1;
    const near = Math.max(0, 1 - Math.min(lengths[i], total - lengths[i]) / CABLE.fanPx);
    const spread = 1 + (CABLE.fanWide - 1) * near * near * (3 - 2 * near);
    // Normal to the run, so strands keep their gap when the cable leans.
    return { x: p.x + ((b.y - a.y) / n) * offset * spread, y: p.y - ((b.x - a.x) / n) * offset * spread };
  });
  let d = `M${at[0].x} ${at[0].y}`;
  for (let i = 0; i < at.length - 1; i++) {
    const [p0, p1, p2, p3] = [at[Math.max(0, i - 1)], at[i], at[i + 1], at[Math.min(at.length - 1, i + 2)]];
    // Catmull-Rom through the points, written as cubic Béziers.
    d += `C${p1.x + (p2.x - p0.x) / 6} ${p1.y + (p2.y - p0.y) / 6} ${p2.x - (p3.x - p1.x) / 6} ${p2.y - (p3.y - p1.y) / 6} ${p2.x} ${p2.y}`;
  }
  return d;
}
