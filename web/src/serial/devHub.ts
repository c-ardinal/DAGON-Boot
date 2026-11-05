import type { LinkClock } from './hostLink.ts';
import { MockTransport, type DeviceKey } from './mock.ts';
import { simulator } from './simHub.ts';
import type { Transport } from './transport.ts';

/** Simulated ms per wall ms for the stand-in simulator: real time. */
const REAL_PACE = 1;

/** A stand-in hub, the clock dagon-host runs on with it, and the project files dagon serve would hand out. */
export interface DevHub {
  transport: Transport;
  clock?: LinkClock;
  latest?: () => Promise<Uint8Array | null>;
  bootcore?: (id: string) => Promise<Uint8Array | null>;
}

/**
 * Development builds and the public demo page (src/app/site.ts) carry the repository's fixtures; no other build does.
 * Written out in full so every other build folds it to false and leaves the fixture files out.
 */
const FIXTURED = import.meta.env.DEV || import.meta.env.MODE === 'demo';

/** A repository fixture served with the page (development builds and the demo page only). */
async function fixture(url: string | null): Promise<Uint8Array | null> {
  if (!FIXTURED || !url) return null;
  const r = await fetch(url);
  return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
}

/** The repository's test-key release and F401 boot core, standing in for the project's through dagon serve. */
const FIXTURES = {
  latest: async () => fixture(FIXTURED ? (await import('../../../crates/dagon-sim/fixtures/sample-full-1.1.0.dgb?url')).default : null),
  bootcore: async (id: string) =>
    fixture(FIXTURED && id === 'seg' ? (await import('../../../crates/dagon-wasm/fixtures/f401-bootcore-1.0.0.img?url')).default : null),
};

/**
 * Stand-in hub picked by the page URL instead of Web Serial, or null for the
 * real device (development builds only).
 * - `?mock`: the in-memory hub (`?mock=silent`: a device that never
 *   answers; `&quiet=uart,i2c:66`: nodes that do not answer; `&away`: after
 *   its REBOOT the hub stays away until chosen again; `&factory=uart`: nodes
 *   answering as their factory agent).
 * - `?sim`: the simulator (dagon-sim through dagon-wasm, ROM loader
 *   included) on its own clock; `&wipe` erases the F401 first. Both take the
 *   repository's test-key release and boot core as the project's files.
 */
export function devHub(q: URLSearchParams): DevHub | null {
  if (q.has('sim')) {
    // On the wall clock: a write runs about as long as on hardware.
    const s = simulator(1, (d) => q.has('wipe') && d.wipe(0x42), REAL_PACE);
    if (!s) return null;
    return { transport: s.transport, clock: s.clock, ...FIXTURES };
  }
  if (!q.has('mock')) return null;
  const keys = (k: string) => (q.get(k) ?? '').split(',').filter(Boolean) as DeviceKey[];
  const transport = new MockTransport({
    silent: q.get('mock') === 'silent',
    delayMs: 150,
    quiet: keys('quiet'),
    factory: keys('factory'),
    hubAway: q.has('away') ? Infinity : undefined,
  });
  return { transport, ...FIXTURES };
}
