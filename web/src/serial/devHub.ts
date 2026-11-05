import type { LinkClock } from './hostLink.ts';
import { MockTransport, type DeviceKey } from './mock.ts';
import { simulator } from './simHub.ts';
import type { Transport } from './transport.ts';

/** A stand-in hub, the clock dagon-host runs on with it, and the project files dagon serve would hand out. */
export interface DevHub {
  transport: Transport;
  clock?: LinkClock;
  latest?: () => Promise<Uint8Array | null>;
  bootcore?: (id: string) => Promise<Uint8Array | null>;
}

/** A repository fixture served by the dev server (development builds only). */
async function fixture(url: string | null): Promise<Uint8Array | null> {
  if (!import.meta.env.DEV || !url) return null;
  const r = await fetch(url);
  return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
}

/** The repository's test-key release and F401 boot core, standing in for the project's through dagon serve. */
const FIXTURES = {
  latest: async () => fixture(import.meta.env.DEV ? (await import('../../../crates/dagon-sim/fixtures/sample-1.1.0.dgb?url')).default : null),
  bootcore: async (id: string) =>
    fixture(import.meta.env.DEV && id === 'seg' ? (await import('../../../crates/dagon-wasm/fixtures/f401-bootcore-1.0.0.img?url')).default : null),
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
    const s = simulator(1, (d) => q.has('wipe') && d.wipe(0x42));
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
