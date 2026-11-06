import { describe, expect, test } from 'vitest';
import { along, CABLE, CableField, FrameLoop, restPoints, runLengths, strandPath } from './cable.ts';
import { verticalLink, type Bezier } from './geometry.ts';

const FRAME = 1000 / 60;
const route = (x: number): Bezier => verticalLink({ x: 0, y: 0 }, { x, y: 300 });

/** A field with one bundle, its lower port moved from x = 0 to `x`. */
function moved(x: number): CableField {
  const f = new CableField();
  f.route({ k: route(0) });
  f.route({ k: route(x) });
  return f;
}

function run(f: CableField, ms: number, each?: () => void): void {
  for (let t = 0; t < ms; t += FRAME) {
    f.step(FRAME);
    each?.();
  }
}

describe('cables', () => {
  test('a new bundle starts at rest on its route', () => {
    const f = new CableField();
    f.route({ k: route(100) });
    expect(f.atRest).toBe(true);
    expect(f.shapes().k).toEqual(restPoints(route(100)));
  });

  test('a moved port leaves the middle trailing behind', () => {
    const f = moved(200);
    expect(f.atRest).toBe(false);
    f.step(FRAME);
    const mid = f.shapes().k[CABLE.segments / 2];
    expect(mid.x).toBeLessThan(restPoints(route(200))[CABLE.segments / 2].x / 2);
  });

  test('settles to the exact rest shape in about a second', () => {
    const f = moved(200);
    let settledAt = 0;
    for (let t = 0; t < 3000 && !f.atRest; t += FRAME) {
      f.step(FRAME);
      settledAt = t;
    }
    expect(f.atRest).toBe(true);
    expect(settledAt).toBeLessThan(1500);
    expect(f.shapes().k).toEqual(restPoints(route(200)));
  });

  test('no ringing: the middle does not swing past its rest place by more than a pixel', () => {
    const f = moved(200);
    const target = restPoints(route(200))[CABLE.segments / 2].x;
    let furthest = -Infinity;
    run(f, 2000, () => (furthest = Math.max(furthest, f.shapes().k[CABLE.segments / 2].x)));
    expect(furthest).toBeLessThan(target + 1);
  });

  test('both ends stay on their ports in every frame', () => {
    const f = moved(200);
    run(f, 1000, () => {
      const k = f.shapes().k;
      expect(k[0]).toEqual({ x: 0, y: 0 });
      expect(k[CABLE.segments]).toEqual({ x: 200, y: 300 });
    });
  });

  test('the same at any frame rate', () => {
    const [a, b] = [moved(150), moved(150)];
    for (let t = 0; t < 12; t++) a.step(1000 / 60);
    for (let t = 0; t < 24; t++) b.step(1000 / 120);
    const [pa, pb] = [a.shapes().k[6], b.shapes().k[6]];
    expect(Math.hypot(pa.x - pb.x, pa.y - pb.y)).toBeLessThan(1);
  });

  test('stable: many bundles under wild moves stay finite and settle', () => {
    const f = new CableField();
    const keys = Array.from({ length: 64 }, (_, i) => `k${i}`);
    f.route(Object.fromEntries(keys.map((k) => [k, route(0)])));
    for (let i = 0; i < 60; i++) {
      f.route(Object.fromEntries(keys.map((k, j) => [k, route(((i * 37 + j * 11) % 2000) - 1000)])));
      f.step(FRAME);
    }
    for (const p of Object.values(f.shapes()).flat()) expect(Number.isFinite(p.x + p.y)).toBe(true);
    run(f, 2500);
    expect(f.atRest).toBe(true);
  });

  test('settle() is the reduced-motion path: the rest shape at once', () => {
    const f = moved(200);
    f.settle();
    expect(f.atRest).toBe(true);
    expect(f.shapes().k).toEqual(restPoints(route(200)));
  });

  test('a route that is no place leaves the cable where it is and at rest', () => {
    const f = new CableField();
    f.route({ k: route(100) });
    f.route({ k: verticalLink({ x: NaN, y: 0 }, { x: 0, y: Infinity }) });
    expect(f.atRest).toBe(true);
    expect(f.shapes().k).toEqual(restPoints(route(100)));
    f.route({ fresh: verticalLink({ x: NaN, y: 0 }, { x: 0, y: 1 }) });
    expect(f.atRest).toBe(true);
    expect(f.shapes().fresh).toBeUndefined();
  });

  test('a link gone is gone', () => {
    const f = moved(100);
    f.route({});
    expect(f.shapes()).toEqual({});
    expect(f.atRest).toBe(true);
  });
});

describe('frames', () => {
  function fake() {
    let queued: ((now: number) => void) | null = null;
    let requests = 0;
    return {
      request: (cb: (now: number) => void) => {
        queued = cb;
        return ++requests;
      },
      cancel: () => (queued = null),
      fire(now: number) {
        const q = queued;
        queued = null;
        q?.(now);
      },
      get requests() {
        return requests;
      },
      get waiting() {
        return queued !== null;
      },
    };
  }

  test('asks for frames only while the model is moving, and none at rest', () => {
    const r = fake();
    const f = moved(200);
    const loop = new FrameLoop(
      (ms) => {
        f.step(ms);
        return !f.atRest;
      },
      r.request,
      r.cancel,
    );
    loop.kick();
    let now = 0;
    for (let i = 0; i < 400 && r.waiting; i++) r.fire((now += FRAME));
    expect(f.atRest).toBe(true);
    expect(r.waiting).toBe(false);
    expect(loop.running).toBe(false);
    const asked = r.requests;
    r.fire((now += FRAME));
    expect(r.requests).toBe(asked);
  });

  test('a second kick does not double the frames', () => {
    const r = fake();
    const loop = new FrameLoop(() => true, r.request, r.cancel);
    loop.kick();
    loop.kick();
    expect(r.requests).toBe(1);
    loop.stop();
    expect(loop.running).toBe(false);
  });
});

describe('strands', () => {
  const line = restPoints(verticalLink({ x: 0, y: 0 }, { x: 0, y: 300 }));
  const starts = (d: string) => d.match(/^M(-?[\d.]+) /)![1];
  const middle = (d: string) => Number(d.split('C')[6].split(' ').slice(-2)[0]);

  test('parallel in the run, spread at the port', () => {
    const d = strandPath(line, 4);
    expect(Math.abs(Number(starts(d)))).toBeCloseTo(4 * CABLE.fanWide, 5);
    expect(Math.abs(middle(d))).toBeCloseTo(4, 5);
  });

  test('the centre strand is the centre line', () => {
    expect(starts(strandPath(line, 0))).toBe('0');
  });

  test('a point along the run', () => {
    const lengths = runLengths(line);
    const { p, dir } = along(line, lengths, 150);
    expect(p.y).toBeCloseTo(150, 0);
    expect(dir.y).toBeCloseTo(1, 1);
  });
});
